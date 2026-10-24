// Modified by 0bCyan for the Ubuntu fork, 2026-10-24. See NOTICE.md and docs/UBUNTU_FORK.md.
//! Per-peer video budget. The writer sits below NACK, so repairs share the same
//! wire budget as new pictures; audio/control keep their own low-latency path.
use std::{sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering}}, time::Duration};
use async_trait::async_trait;
use webrtc::interceptor::{self, Attributes, Interceptor, InterceptorBuilder, RTPWriter, RTPReader, RTCPWriter, RTCPReader, stream_info::StreamInfo};

type IResult<T> = std::result::Result<T, interceptor::Error>;

const START_BPS: u32 = 6_000_000;
const BURST_BYTES: f64 = 3600.0;
#[derive(Debug)]
struct Controller { last: tokio::time::Instant, sent: u64, nacks: u64, bytes: u64, bad: u8, healthy: u8, fraction: u8, report_sequence: Option<u32> }
impl Controller {
    fn sample(&mut self, sent: u64, nacks: u64, bytes: u64, cap: u32, ceiling: u32) -> Option<(u32, u64)> {
        let packets = sent.saturating_sub(self.sent);
        if self.last.elapsed() < Duration::from_secs(2) || packets < 80 { return None; }
        let repair = nacks.saturating_sub(self.nacks).saturating_mul(1000) / packets;
        let loss = self.fraction;
        // Idle traffic establishes no spare capacity: only probe upward under load.
        let active = bytes.saturating_sub(self.bytes) as f64 * 8.0 / self.last.elapsed().as_secs_f64() >= f64::from(cap) * 0.5;
        self.bytes = bytes;
        self.last = tokio::time::Instant::now(); self.sent = sent; self.nacks = nacks; self.fraction = 0;
        let floor = 500_000.min(ceiling);
        let next = if repair >= 100 || loss >= 26 {
            self.bad = 0; self.healthy = 0; (cap * 3 / 4).max(floor.min(cap))
        } else if repair >= 30 || loss >= 13 {
            self.bad += 1; self.healthy = 0;
            if self.bad >= 2 { self.bad = 0; ((u64::from(cap) * 85 / 100) as u32).max(floor.min(cap)) } else { cap }
        } else if repair < 5 && loss < 3 {
            self.bad = 0; self.healthy = if active { self.healthy + 1 } else { 0 };
            if self.healthy >= 6 { self.healthy = 0; cap.saturating_add((cap / 20).max(50_000)).min(ceiling) } else { cap }
        } else { self.bad = 0; self.healthy = 0; cap };
        (next != cap).then_some((next.min(ceiling), repair))
    }
}
#[derive(Debug)]
struct Bucket { at: tokio::time::Instant, credit: f64 }
#[derive(Debug)]
pub(crate) struct VideoNetwork {
    pub(crate) pacing: AtomicBool,
    adaptive: bool,
    cap: AtomicU32,
    ceiling: AtomicU32,
    ssrc: AtomicU32,
    sent: AtomicU64,
    nacks: AtomicU64,
    bytes: AtomicU64,
    controller: Mutex<Controller>,
    bucket: tokio::sync::Mutex<Bucket>,
}
impl VideoNetwork {
    pub(crate) fn new() -> Self {
        Self::configured(std::env::var("UU_VIDEO_PACING").map_or(true, |v| v != "0"),
            std::env::var("UU_VIDEO_ADAPTIVE").map_or(true, |v| v != "0"))
    }
    fn configured(pacing: bool, adaptive: bool) -> Self {
        let now = tokio::time::Instant::now();
        Self { pacing: AtomicBool::new(pacing), adaptive, cap: AtomicU32::new(START_BPS), ceiling: AtomicU32::new(30_000_000), ssrc: AtomicU32::new(0),
            sent: AtomicU64::new(0), nacks: AtomicU64::new(0), bytes: AtomicU64::new(0),
            controller: Mutex::new(Controller { last: now, sent: 0, nacks: 0, bytes: 0, bad: 0, healthy: 0, fraction: 0, report_sequence: None }),
            bucket: tokio::sync::Mutex::new(Bucket { at: now, credit: BURST_BYTES }) }
    }
    pub(crate) fn set_ceiling(&self, bps: u32) {
        let bps = bps.clamp(1000, 100_000_000);
        self.ceiling.store(bps, Ordering::Relaxed);
        self.cap.fetch_min(bps, Ordering::Relaxed);
    }
    pub(crate) fn encoder_bps(&self) -> u32 {
        if self.adaptive { self.cap.load(Ordering::Relaxed).min(self.ceiling.load(Ordering::Relaxed)) }
        else { self.ceiling.load(Ordering::Relaxed) }
    }
    pub(crate) fn feedback(&self, packet: &dyn std::any::Any) {
        use webrtc::rtcp::{transport_feedbacks::transport_layer_nack::TransportLayerNack, receiver_report::ReceiverReport};
        let mut state = self.controller.lock().unwrap();
        let ssrc = self.ssrc.load(Ordering::Relaxed);
        if let Some(nack) = packet.downcast_ref::<TransportLayerNack>() {
            if ssrc != 0 && nack.media_ssrc != ssrc { return; }
            self.nacks.fetch_add(nack.nacks.iter().map(|p| 1 + u64::from(p.lost_packets.count_ones())).sum::<u64>(), Ordering::Relaxed);
        }
        if let Some(rr) = packet.downcast_ref::<ReceiverReport>() {
            for report in &rr.reports {
                if ssrc != 0 && report.ssrc != ssrc { continue; }
                if let Some(previous) = state.report_sequence.replace(report.last_sequence_number) {
                    let count=report.last_sequence_number.wrapping_sub(previous);
                    if (80..0x8000_0000).contains(&count) { state.fraction=state.fraction.max(report.fraction_lost); }
                }
            }
        }
        if !self.adaptive { return; }
        let cap = self.encoder_bps();
        if let Some((next, repair)) = state.sample(self.sent.load(Ordering::Relaxed), self.nacks.load(Ordering::Relaxed), self.bytes.load(Ordering::Relaxed), cap, self.ceiling.load(Ordering::Relaxed)) {
            self.cap.store(next, Ordering::Relaxed);
            log::info!("Video network budget: encoder_kbps={} repair_per_mille={} (repair demand, not unrecovered loss)", next / 1000, repair);
        }
    }
}
struct PacedWriter { network: Arc<VideoNetwork>, next: Arc<dyn RTPWriter + Send + Sync> }
#[async_trait]
impl RTPWriter for PacedWriter {
    async fn write(&self, packet: &webrtc::rtp::packet::Packet, attrs: &Attributes) -> IResult<usize> {
        // Includes RTP extensions, SRTP authentication, UDP/IP and TURN allowance.
        let bytes = packet.payload.len() as f64 + 80.0;
        let mut bucket = self.network.bucket.lock().await;
        if self.network.pacing.load(Ordering::Relaxed) {
            let bytes_per_second = f64::from(self.network.encoder_bps().max(64_000)) * 1.15 / 8.0;
            let now = tokio::time::Instant::now();
            bucket.credit = (bucket.credit + now.duration_since(bucket.at).as_secs_f64() * bytes_per_second).min(BURST_BYTES);
            bucket.at = now;
            if bucket.credit < bytes {
                tokio::time::sleep(Duration::from_secs_f64((bytes - bucket.credit) / bytes_per_second)).await;
                let now = tokio::time::Instant::now();
                bucket.credit = (bucket.credit + now.duration_since(bucket.at).as_secs_f64() * bytes_per_second).min(BURST_BYTES);
                bucket.at = now;
            }
            bucket.credit = (bucket.credit - bytes).max(0.0);
        } else { bucket.at = tokio::time::Instant::now(); bucket.credit = BURST_BYTES; }
        // Keep the lock through the write: concurrent repair tasks cannot bypass pacing.
        let result = self.next.write(packet, attrs).await;
        // Interceptors may accept a packet and return zero (e.g. the mock writer).
        if result.is_ok() { self.network.sent.fetch_add(1, Ordering::Relaxed); self.network.bytes.fetch_add(bytes as u64, Ordering::Relaxed); }
        result
    }
}
pub(crate) struct PacingBuilder(pub Arc<VideoNetwork>);
impl InterceptorBuilder for PacingBuilder {
    fn build(&self, _: &str) -> IResult<Arc<dyn Interceptor + Send + Sync>> { Ok(Arc::new(PacingBuilder(self.0.clone()))) }
}
#[async_trait]
impl Interceptor for PacingBuilder {
    async fn bind_local_stream(&self, info: &StreamInfo, writer: Arc<dyn RTPWriter + Send + Sync>) -> Arc<dyn RTPWriter + Send + Sync> {
        if info.mime_type.to_ascii_lowercase().starts_with("video/") {
            let mut state = self.0.controller.lock().unwrap();
            if self.0.ssrc.swap(info.ssrc, Ordering::Relaxed) != info.ssrc { state.report_sequence = None; }
            Arc::new(PacedWriter { network: self.0.clone(), next: writer })
        } else { writer }
    }
    async fn bind_rtcp_reader(&self, r: Arc<dyn RTCPReader + Send + Sync>) -> Arc<dyn RTCPReader + Send + Sync> { r }
    async fn bind_rtcp_writer(&self, w: Arc<dyn RTCPWriter + Send + Sync>) -> Arc<dyn RTCPWriter + Send + Sync> { w }
    async fn bind_remote_stream(&self, _: &StreamInfo, r: Arc<dyn RTPReader + Send + Sync>) -> Arc<dyn RTPReader + Send + Sync> { r }
    async fn unbind_local_stream(&self, _: &StreamInfo) {}
    async fn unbind_remote_stream(&self, _: &StreamInfo) {}
    async fn close(&self) -> IResult<()> { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn window(state: &mut Controller, sent: u64, nack: u64, cap: u32, ceiling: u32) -> Option<(u32,u64)> {
        state.last = tokio::time::Instant::now() - Duration::from_secs(3);
        state.sample(sent, nack, sent*1180, cap, ceiling)
    }
    #[test]
    fn budget_backs_off_on_repairs_and_recovers_slowly_within_viewer_ceiling() {
        let n = VideoNetwork::configured(true, true); let mut s = n.controller.lock().unwrap();
        assert_eq!(window(&mut s, 1000, 150, START_BPS, 20_000_000), Some((4_500_000,150)));
        for i in 1..6 { assert!(window(&mut s, 1000+i*1000,150,4_500_000,20_000_000).is_none()); }
        assert_eq!(window(&mut s, 7000,150,4_500_000,20_000_000),Some((4_725_000,0)));
        assert_eq!(window(&mut s, 8000,1150,500_000,20_000_000),None);
        n.set_ceiling(1_000_000); assert_eq!(n.encoder_bps(),1_000_000);
        n.set_ceiling(20_000_000); assert_eq!(n.encoder_bps(),1_000_000);
    }
    #[test]
    fn sparse_idle_reports_do_not_invent_congestion() {
        let n = VideoNetwork::configured(true,true); let mut s=n.controller.lock().unwrap();
        s.fraction=170;
        assert!(window(&mut s,4,2,START_BPS,20_000_000).is_none());
        assert_eq!(s.sent,0);
    }
    #[test]
    fn idle_traffic_never_probes_upward() {
        let n=VideoNetwork::configured(true,true);let mut s=n.controller.lock().unwrap();
        for i in 1..20 {
            s.last=tokio::time::Instant::now()-Duration::from_secs(3);
            assert!(s.sample(i*100,0,i*10000,START_BPS,30_000_000).is_none());
        }
        assert_eq!(s.healthy,0);
    }
    #[test]
    fn feedback_ignores_other_streams_and_sparse_loss_reports() {
        use webrtc::rtcp::{receiver_report::ReceiverReport, reception_report::ReceptionReport,
            transport_feedbacks::transport_layer_nack::{TransportLayerNack,NackPair}};
        let n=VideoNetwork::configured(true,true); n.ssrc.store(1,Ordering::Relaxed);
        let rr=|ssrc,sequence| ReceiverReport {reports:vec![ReceptionReport {ssrc,last_sequence_number:sequence,fraction_lost:255,..Default::default()}],..Default::default()};
        n.feedback(&rr(1,1)); n.feedback(&rr(1,5));
        n.feedback(&rr(2,5000)); n.feedback(&rr(2,6000));
        n.feedback(&TransportLayerNack {media_ssrc:2,nacks:vec![NackPair {packet_id:0,lost_packets:0xffff}],..Default::default()});
        { let s=n.controller.lock().unwrap(); assert_eq!(s.fraction,0); assert_eq!(s.report_sequence,Some(5)); }
        assert_eq!(n.nacks.load(Ordering::Relaxed),0);
        n.sent.store(100,Ordering::Relaxed);
        n.controller.lock().unwrap().last=tokio::time::Instant::now()-Duration::from_secs(3);
        n.feedback(&rr(1,105)); assert_eq!(n.encoder_bps(),4_500_000);
    }
    #[tokio::test]
    async fn nack_responder_repairs_cannot_bypass_the_transport_budget() {
        use webrtc::interceptor::{registry::Registry, nack::responder::Responder, mock::mock_stream::MockStream, stream_info::RTCPFeedback};
        use webrtc::rtcp::transport_feedbacks::transport_layer_nack::{NackPair,TransportLayerNack};
        let network=Arc::new(VideoNetwork::configured(true,false)); network.set_ceiling(1_000_000);
        let mut registry=Registry::new();
        registry.add(Box::new(PacingBuilder(network.clone())));
        registry.add(Box::new(Responder::builder()));
        let stream=MockStream::new(&StreamInfo {ssrc:1,mime_type:"video/H264".into(),
            rtcp_feedback:vec![RTCPFeedback{typ:"nack".into(),parameter:String::new()}],..Default::default()},registry.build("").unwrap()).await;
        for sequence_number in 0..12 {
            stream.write_rtp(&webrtc::rtp::packet::Packet {header:webrtc::rtp::header::Header {ssrc:1,sequence_number,..Default::default()},payload:vec![1;1100].into()}).await.unwrap();
            stream.written_rtp().await.unwrap();
        }
        let start=tokio::time::Instant::now();
        stream.receive_rtcp(vec![Box::new(TransportLayerNack {media_ssrc:1,sender_ssrc:2,nacks:vec![NackPair{packet_id:0,lost_packets:0x7ff}]} )]).await;
        for sequence in 0..12 {
            let packet=tokio::time::timeout(Duration::from_secs(2),stream.written_rtp()).await.unwrap().unwrap();
            assert_eq!(packet.header.sequence_number,sequence);
        }
        assert!(start.elapsed()>=Duration::from_millis(60));
        assert_eq!(network.sent.load(Ordering::Relaxed),24);
        stream.close().await.unwrap();
    }
    #[tokio::test]
    async fn original_and_concurrent_repair_writers_share_budget() {
        use webrtc::interceptor::RTPWriterFn;
        let times=Arc::new(Mutex::new(Vec::new())); let captured=times.clone();
        let sink=Arc::new(RTPWriterFn(Box::new(move |p,_| {
            let captured=captured.clone(); let seq=p.header.sequence_number;
            Box::pin(async move {captured.lock().unwrap().push((tokio::time::Instant::now(),seq));Ok(1100)})
        })));
        let network=Arc::new(VideoNetwork::configured(true,false)); network.set_ceiling(2_000_000);
        let writer=Arc::new(PacedWriter{network,next:sink}); let mut jobs=vec![];
        for i in 0..24 { let w=writer.clone();jobs.push(tokio::spawn(async move {
            let packet=webrtc::rtp::packet::Packet{header:webrtc::rtp::header::Header{sequence_number:i%12,..Default::default()},payload:vec![1;1100].into()};
            w.write(&packet,&Attributes::new()).await.unwrap();
        })); }
        for job in jobs {job.await.unwrap();}
        let times=times.lock().unwrap();assert_eq!(times.len(),24);
        assert!(times.last().unwrap().0.duration_since(times[0].0)>=Duration::from_millis(70));
        for pair in times.windows(4) {assert!(pair[3].0.duration_since(pair[0].0)>=Duration::from_millis(1));}
    }
}
