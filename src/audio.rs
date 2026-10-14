//! Desktop sound for the viewer's 声音 menu: the default output's monitor (PulseAudio or
//! PipeWire-pulse) is encoded to Opus by FFmpeg and sent on the negotiated audio m-line.
//! PipeWire monitors ignore the sink's mute by default, so 被控端静音运行 keeps the stream audible.
//! `UU_AUDIO=0` disables it; `UU_AUDIO_SOURCE` picks another Pulse source.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use webrtc::media::io::ogg_reader::OggReader;
use webrtc::media::Sample;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;

/// One Opus packet per Ogg page (FFmpeg -page_duration), 20 ms each.
const FRAME: Duration = Duration::from_millis(20);

pub fn enabled() -> bool { std::env::var("UU_AUDIO").map_or(true, |v| v != "0") }

struct Encoder(Child);
impl Drop for Encoder { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }

fn spawn_encoder() -> Result<Encoder, String> {
    let source = std::env::var("UU_AUDIO_SOURCE").unwrap_or_else(|_| "@DEFAULT_MONITOR@".into());
    let mut command = match crate::config::ffmpeg_root() {
        Some(root) => { let mut c = Command::new(root.join("bin/ffmpeg")); c.env("LD_LIBRARY_PATH", root.join("lib/x86_64-linux-gnu")); c }
        None => Command::new("ffmpeg"),
    };
    command.args(["-hide_banner", "-loglevel", "error", "-nostdin",
            "-f", "pulse", "-fragment_size", "1920", "-i", &source, "-vn",
            "-c:a", "libopus", "-ar", "48000", "-ac", "2", "-b:a", "128k", "-application", "lowdelay",
            "-frame_duration", "20", "-page_duration", "20000", "-flush_packets", "1", "-f", "ogg", "pipe:1"])
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::inherit());
    command.spawn().map(Encoder).map_err(|e| format!("Start audio encoder: {e}"))
}

/// Reads Opus packets until the encoder ends or `tx` closes. Blocking.
fn read_packets(stdout: impl Read, tx: &tokio::sync::mpsc::Sender<bytes::Bytes>) -> Result<(), String> {
    let (mut reader, _) = OggReader::new(stdout, false).map_err(|e| format!("Audio Ogg header: {e}"))?;
    loop {
        let (payload, _) = reader.parse_next_page().map_err(|e| format!("Audio Ogg page: {e}"))?;
        if payload.starts_with(b"OpusTags") { continue; }
        if tx.blocking_send(payload.freeze()).is_err() { return Ok(()); }
    }
}

fn closed(pc: &RTCPeerConnection) -> bool {
    matches!(pc.connection_state(), RTCPeerConnectionState::Closed | RTCPeerConnectionState::Failed)
}

/// Streams desktop sound to one viewer until its connection closes; restarts a failed encoder.
pub async fn stream_audio(pc: Arc<RTCPeerConnection>, track: Arc<TrackLocalStaticSample>) {
    while pc.connection_state() != RTCPeerConnectionState::Connected {
        if closed(&pc) { return; }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let mut failures = 0u32;
    while !closed(&pc) {
        let mut encoder = match spawn_encoder() {
            Ok(encoder) => encoder,
            Err(e) => { log::warn!("{e}"); return; }
        };
        let Some(stdout) = encoder.0.stdout.take() else { return };
        let (tx, mut rx) = tokio::sync::mpsc::channel::<bytes::Bytes>(25);
        let reader = tokio::task::spawn_blocking(move || read_packets(stdout, &tx));
        log::info!("Desktop audio started: opus 48k stereo");
        let mut packets = 0u64;
        let mut health = tokio::time::interval(Duration::from_millis(500));
        loop {
            tokio::select! {
                packet = rx.recv() => {
                    let Some(data) = packet else { break };
                    if let Err(e) = track.write_sample(&Sample { data, duration: FRAME, ..Default::default() }).await {
                        log::warn!("Audio send: {e}"); break;
                    }
                    packets += 1;
                },
                _ = health.tick() => if closed(&pc) { break },
            }
        }
        drop(rx);
        drop(encoder); // kills FFmpeg, which ends the blocking reader
        let result = reader.await.unwrap_or_else(|e| Err(e.to_string()));
        if closed(&pc) { break; }
        log::warn!("Desktop audio stopped after {packets} packets: {:?}", result.err());
        failures = if packets > 250 { 0 } else { failures + 1 };
        if failures >= 5 { log::warn!("Desktop audio disabled for this connection"); return; }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    log::info!("Desktop audio ended");
}
