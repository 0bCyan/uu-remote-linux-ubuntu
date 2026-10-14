//! Address-free ICE path diagnostics. A host pair may still traverse a VPN;
//! `direct` means no ICE relay candidate, not proof of a shared LAN.
use std::sync::{atomic::{AtomicU64, Ordering}, Weak};
use std::time::Instant;
use webrtc::peer_connection::{RTCPeerConnection, peer_connection_state::RTCPeerConnectionState};
use webrtc::stats::StatsReportType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateSummary {
    pub protocol: &'static str,
    pub kind: &'static str,
}

fn protocol(value: &str) -> &'static str {
    if value.eq_ignore_ascii_case("udp") { "udp" }
    else if value.eq_ignore_ascii_case("tcp") { "tcp" }
    else { "unknown" }
}
fn kind(value: &str) -> &'static str {
    match value { "host" => "host", "srflx" => "srflx", "prflx" => "prflx", "relay" => "relay", _ => "unknown" }
}

/// Parse only protocol/type from an SDP candidate. Never return arbitrary input.
pub fn candidate_summary(sdp: &str) -> CandidateSummary {
    let fields: Vec<_> = sdp.split_whitespace().collect();
    CandidateSummary {
        protocol: fields.get(2).map_or("unknown", |v| protocol(v)),
        kind: if fields.get(6) == Some(&"typ") { fields.get(7).map_or("unknown", |v| kind(v)) } else { "unknown" },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathSummary {
    pub local: CandidateSummary,
    pub remote: CandidateSummary,
    pub route: &'static str,
}

/// webrtc 0.10 keeps RTCIceCandidatePair fields private. Its Display is
/// `(local) <protocol> <type> <endpoint> <-> (remote) ...`. Restrict output to
/// allowlisted enums; fail closed if upstream changes this representation.
fn selected_pair_summary(display: &str) -> Option<PathSummary> {
    let (local, remote) = display.strip_prefix("(local) ")?.split_once(" <-> (remote) ")?;
    fn side(value: &str) -> CandidateSummary {
        let mut fields = value.split_whitespace();
        CandidateSummary { protocol: protocol(fields.next().unwrap_or("")), kind: kind(fields.next().unwrap_or("")) }
    }
    let local = side(local);
    let remote = side(remote);
    let route = if local.kind == "relay" || remote.kind == "relay" { "relay" }
        else if local.kind == "unknown" || remote.kind == "unknown" || local.protocol == "unknown" || remote.protocol == "unknown" { "unknown" }
        else { "direct" };
    Some(PathSummary { local, remote, route })
}

pub async fn selected_path(pc: &RTCPeerConnection) -> Option<PathSummary> {
    let transport = pc.dtls_transport();
    let pair = transport.ice_transport().get_selected_candidate_pair().await?;
    Some(selected_pair_summary(&pair.to_string()).unwrap_or(PathSummary {
        local: CandidateSummary { protocol: "unknown", kind: "unknown" },
        remote: CandidateSummary { protocol: "unknown", kind: "unknown" },
        route: "unknown",
    }))
}

#[derive(Default)]
struct RateCounter { previous: Option<(Instant, u64, u64)> }
impl RateCounter {
    fn sample(&mut self, now: Instant, sent: u64, received: u64) -> Option<(u64, u64)> {
        let old = self.previous.replace((now, sent, received))?;
        let seconds = now.checked_duration_since(old.0)?.as_secs_f64();
        if seconds <= 0.0 || sent < old.1 || received < old.2 { return None; }
        Some((((sent - old.1) as f64 * 8.0 / seconds) as u64, ((received - old.2) as f64 * 8.0 / seconds) as u64))
    }
}

static SESSION_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Spawn once per peer. Weak ownership and closed-state exit prevent leaked
/// peer sessions. Reconnects reset counters. No IP, candidate ID or SDP is logged.
pub fn start(peer: Weak<RTCPeerConnection>) {
    let session = SESSION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut rate = RateCounter::default();
        let mut last_path = None;
        let mut ticks = 0;
        loop {
            interval.tick().await;
            let Some(pc) = peer.upgrade() else { break };
            let state = pc.connection_state();
            if state == RTCPeerConnectionState::Closed { break; }
            if state != RTCPeerConnectionState::Connected {
                rate = RateCounter::default();
                last_path = None;
                continue;
            }
            let Some(path) = selected_path(&pc).await else { continue };
            let report = pc.get_stats().await;
            let (mut sent, mut received) = (0u64, 0u64);
            for stats in report.reports.values() {
                // SCTPTransport repeats the same ICE totals; count Transport only.
                if let StatsReportType::Transport(stats) = stats {
                    sent = sent.saturating_add(stats.bytes_sent as u64);
                    received = received.saturating_add(stats.bytes_received as u64);
                }
            }
            let rates = rate.sample(Instant::now(), sent, received);
            ticks += 1;
            if last_path != Some(path) || ticks % 6 == 0 {
                log::info!("ICE path: session={} route={} local={}/{} remote={}/{} tx_bps={:?} rx_bps={:?} rtt=unavailable",
                    session, path.route, path.local.protocol, path.local.kind, path.remote.protocol, path.remote.kind,
                    rates.map(|r| r.0), rates.map(|r| r.1));
                // webrtc-ice 0.10 candidate-pair RTT/packet/byte fields are zero
                // placeholders, not measured metrics. Do not publish them.
                last_path = Some(path);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_real_pair_format_and_both_relay_sides() {
        use webrtc::ice_transport::{ice_candidate::RTCIceCandidate, ice_candidate_pair::RTCIceCandidatePair, ice_candidate_type::RTCIceCandidateType, ice_protocol::RTCIceProtocol};
        for (local, remote, expected) in [
            (RTCIceCandidateType::Host, RTCIceCandidateType::Srflx, "direct"),
            (RTCIceCandidateType::Relay, RTCIceCandidateType::Host, "relay"),
            (RTCIceCandidateType::Host, RTCIceCandidateType::Relay, "relay"),
            (RTCIceCandidateType::Unspecified, RTCIceCandidateType::Host, "unknown"),
        ] {
            let candidate = |typ| RTCIceCandidate { typ, protocol: RTCIceProtocol::Udp, address: "192.0.2.42".into(), port: 1234, ..Default::default() };
            let summary = selected_pair_summary(&RTCIceCandidatePair::new(candidate(local), candidate(remote)).to_string()).unwrap();
            assert_eq!(summary.route, expected);
            assert!(!format!("{summary:?}").contains("192.0.2.42"));
        }
    }
    #[test]
    fn candidate_log_is_allowlisted() {
        let summary = candidate_summary("candidate:x 1 UDP 123 192.0.2.5 4444 typ relay raddr 10.0.0.1 rport 123 ufrag secret");
        assert_eq!(summary, CandidateSummary { protocol: "udp", kind: "relay" });
        assert_eq!(candidate_summary("malformed secret"), CandidateSummary { protocol: "unknown", kind: "unknown" });
        assert!(selected_pair_summary("unexpected secret").is_none());
    }
    #[test]
    fn rates_reset_on_counter_restart() {
        let mut rates = RateCounter::default();
        let now = Instant::now();
        assert_eq!(rates.sample(now, 100, 200), None);
        assert_eq!(rates.sample(now + std::time::Duration::from_secs(2), 300, 300), Some((800, 400)));
        assert_eq!(rates.sample(now + std::time::Duration::from_secs(3), 0, 0), None);
    }

    /// Exercises actual ICE nomination and SCTP delivery between local peers, without
    /// login, public STUN, external endpoints or production configuration.
    #[tokio::test]
    #[ignore = "opens local UDP sockets; run explicitly for network acceptance"]
    async fn local_selected_pair_and_payload() {
        use std::sync::Arc;
        use webrtc::api::{APIBuilder, setting_engine::SettingEngine};
        use webrtc::peer_connection::configuration::RTCConfiguration;
        let mut settings = SettingEngine::default();
        // This crate excludes loopback addresses during ICE gathering. Both
        // peers still run in this process and use only gathered local addresses.
        settings.set_ice_multicast_dns_mode(webrtc::ice::mdns::MulticastDnsMode::Disabled);
        let mut media = webrtc::api::media_engine::MediaEngine::default();
        media.register_default_codecs().unwrap();
        let api = APIBuilder::new().with_media_engine(media).with_setting_engine(settings).build();
        let left = Arc::new(api.new_peer_connection(RTCConfiguration::default()).await.unwrap());
        let right = Arc::new(api.new_peer_connection(RTCConfiguration::default()).await.unwrap());
        for (label, peer) in [("left", &left), ("right", &right)] {
            peer.on_peer_connection_state_change(Box::new(move |state| {
                eprintln!("isolated ICE probe {label}: {state}");
                Box::pin(async {})
            }));
        }
        let (received_tx, mut received_rx) = tokio::sync::mpsc::channel(1);
        right.on_data_channel(Box::new(move |channel| {
            let tx = received_tx.clone();
            Box::pin(async move {
                channel.on_message(Box::new(move |message| {
                    let tx = tx.clone();
                    Box::pin(async move { let _ = tx.send(message.data).await; })
                }));
            })
        }));
        let channel = left.create_data_channel("network-acceptance", None).await.unwrap();
        let sender = Arc::downgrade(&channel);
        channel.on_open(Box::new(move || {
            let sender = sender.clone();
            Box::pin(async move {
                if let Some(channel) = sender.upgrade() {
                    channel.send(&bytes::Bytes::from_static(b"isolated-ice-probe")).await.unwrap();
                }
            })
        }));
        let acceptance = async {
            let mut gathered = left.gathering_complete_promise().await;
            left.set_local_description(left.create_offer(None).await.unwrap()).await.unwrap();
            let _ = gathered.recv().await;
            eprintln!("left SDP candidate count={}", left.local_description().await.unwrap().sdp.lines().filter(|s| s.starts_with("a=candidate:")).count());
            right.set_remote_description(left.local_description().await.unwrap()).await.unwrap();
            let mut gathered = right.gathering_complete_promise().await;
            right.set_local_description(right.create_answer(None).await.unwrap()).await.unwrap();
            let _ = gathered.recv().await;
            eprintln!("right SDP candidate count={}", right.local_description().await.unwrap().sdp.lines().filter(|s| s.starts_with("a=candidate:")).count());
            left.set_remote_description(right.local_description().await.unwrap()).await.unwrap();
            eprintln!("isolated ICE probe: descriptions exchanged");
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            eprintln!("isolated ICE probe: left_ice={} right_ice={} selected={:?}", left.ice_connection_state(), right.ice_connection_state(), selected_path(&left).await);
            assert_eq!(received_rx.recv().await.unwrap().as_ref(), b"isolated-ice-probe");
            let path = selected_path(&left).await.unwrap();
            assert_eq!(path.route, "direct");
            assert_eq!(path.local.kind, "host");
            assert_eq!(path.remote.kind, "host");
            let stats = left.get_stats().await;
            assert!(stats.reports.values().any(|v| matches!(v, StatsReportType::Transport(t) if t.bytes_sent > 0 && t.bytes_received > 0)));
        };
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(15), acceptance).await;
        left.close().await.unwrap();
        right.close().await.unwrap();
        outcome.expect("isolated local ICE/SCTP did not connect within 15 seconds");
    }
}
