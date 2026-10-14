//! UU 端口映射 (port mapping) — the controlled ("B") side.
//!
//! The viewer owns the rules (local port → target_host:target_port reachable from this host).
//! For every local TCP connection it opens a stream with Message field 27 `PortMappingFrame
//! {1 session_id, 2 rule_id, 3 stream_id, 4 type, 5 payload}` on FILE_DATA_CHANNEL (the official
//! host rejects frames from other channels). Frame types: SYN=0, FIN=1, DATA=2, SYN_ACK=3,
//! DATA_ACK=4. SYN payload is JSON `{"version", "target_host", "target_port"}`; the host
//! connects and answers SYN_ACK `{"version", "ok", "error"}`. DATA carries raw bytes both ways,
//! FIN closes a stream, DATA_ACK is ignored by both sides (official log strings). Mirrors
//! UURemoteServer `PortMappingHandler` (handleSyn / handleTargetState / flushTargetWrites /
//! closeStream). Payload bytes are never logged. `UU_PORT_MAPPING=0` disables it, like the
//! official controlled-side switch.

use crate::protocol::{decode_wire, WireField, WireValue};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use webrtc::data_channel::RTCDataChannel;

pub const MESSAGE_FIELD: u32 = 27;
const SYN: u64 = 0;
const FIN: u64 = 1;
const DATA: u64 = 2;
const SYN_ACK: u64 = 3;
const DATA_ACK: u64 = 4;
const PROTOCOL_VERSION: u64 = 1;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_CHUNK: usize = 16 * 1024;
/// Pending writes to one target before the stream is closed ("写缓冲超限").
const WRITE_QUEUE_FRAMES: usize = 256;
/// Pause reading targets while this much is queued on the data channel.
const MAX_BUFFERED: usize = 4 * 1024 * 1024;

pub fn enabled() -> bool { std::env::var("UU_PORT_MAPPING").map_or(true, |v| v != "0") }

/// A frame identity field echoed back exactly as received (varint or bytes).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Id { Varint(u64), Bytes(Vec<u8>), Missing }

impl Id {
    fn of(fields: &[WireField], number: u32) -> Self {
        match fields.iter().find(|f| f.number == number).map(|f| &f.value) {
            Some(WireValue::Varint(v)) => Id::Varint(*v),
            Some(WireValue::Bytes(b)) => Id::Bytes(b.to_vec()),
            _ => Id::Missing,
        }
    }
    fn encode(&self, number: u32, out: &mut Vec<u8>) {
        match self {
            Id::Varint(v) => { varint(u64::from(number) << 3, out); varint(*v, out); }
            Id::Bytes(b) => bytes(number, b, out),
            Id::Missing => {}
        }
    }
}

fn varint(mut v: u64, out: &mut Vec<u8>) { while v >= 128 { out.push((v as u8 & 127) | 128); v >>= 7; } out.push(v as u8); }
fn bytes(number: u32, v: &[u8], out: &mut Vec<u8>) { varint(u64::from(number) << 3 | 2, out); varint(v.len() as u64, out); out.extend_from_slice(v); }

/// One stream: (session_id, rule_id, stream_id) as sent by the viewer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key { session: Id, rule: Id, stream: Id }

fn frame(key: &Key, kind: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    key.session.encode(1, &mut out);
    key.rule.encode(2, &mut out);
    key.stream.encode(3, &mut out);
    if kind != 0 { varint(4 << 3, &mut out); varint(kind, &mut out); }
    if !payload.is_empty() { bytes(5, payload, &mut out); }
    out
}

/// SYN payload; accepts the official snake_case keys and camelCase for robustness.
fn parse_syn(payload: &[u8]) -> Result<(String, u16), String> {
    let value: serde_json::Value = serde_json::from_slice(payload).map_err(|_| "invalid SYN payload")?;
    let host = value.get("target_host").or_else(|| value.get("targetHost")).and_then(|v| v.as_str())
        .map(str::trim).filter(|h| !h.is_empty()).ok_or("missing target host")?;
    let port = value.get("target_port").or_else(|| value.get("targetPort")).and_then(|v| v.as_u64())
        .and_then(|p| u16::try_from(p).ok()).filter(|p| *p != 0).ok_or("invalid target port")?;
    Ok((host.to_owned(), port))
}

fn syn_ack(ok: bool, error: Option<&str>) -> Vec<u8> {
    let mut value = serde_json::json!({ "version": PROTOCOL_VERSION, "ok": ok });
    if let Some(error) = error { value["error"] = serde_json::Value::from(error); }
    value.to_string().into_bytes()
}

pub struct Session {
    outbound: mpsc::Sender<(Key, u64, Vec<u8>)>,
    /// Open streams: writer queue to the target and the task reading from it.
    streams: Mutex<HashMap<Key, (mpsc::Sender<Vec<u8>>, tokio::task::AbortHandle)>>,
}

impl Session {
    /// `channel` is the peer's FILE_DATA_CHANNEL slot (filled once the viewer opens it).
    pub fn new(channel: Arc<Mutex<Option<Arc<RTCDataChannel>>>>, sequence: Arc<AtomicU64>) -> Arc<Self> {
        let (outbound, mut queue) = mpsc::channel::<(Key, u64, Vec<u8>)>(64);
        tokio::spawn(async move {
            while let Some((key, kind, payload)) = queue.recv().await {
                let mut slot = channel.lock().unwrap().clone();
                for _ in 0..100 {
                    if slot.is_some() { break; }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    slot = channel.lock().unwrap().clone();
                }
                let Some(dc) = slot else { log::warn!("Port mapping frame dropped: no FILE_DATA_CHANNEL"); continue; };
                while dc.buffered_amount().await > MAX_BUFFERED {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                let message = crate::initialization::envelope(sequence.fetch_add(1, Ordering::Relaxed), timestamp,
                    MESSAGE_FIELD, &frame(&key, kind, &payload));
                if let Err(e) = dc.send(&bytes::Bytes::from(message)).await {
                    log::warn!("Port mapping send failed: {}", e); break;
                }
            }
        });
        Arc::new(Self { outbound, streams: Mutex::new(HashMap::new()) })
    }

    /// Handles Message field 27 if present; returns true when the message was a port-mapping frame.
    pub fn handle(self: &Arc<Self>, message: &[WireField], channel: &str) -> bool {
        let Some(WireValue::Bytes(raw)) = message.iter().find(|f| f.number == MESSAGE_FIELD).map(|f| &f.value) else { return false };
        if channel != "FILE_DATA_CHANNEL" {
            log::warn!("Port mapping frame ignored on {}", channel);
            return true;
        }
        let Ok(fields) = decode_wire(raw) else { log::warn!("Port mapping frame undecodable"); return true };
        let key = Key { session: Id::of(&fields, 1), rule: Id::of(&fields, 2), stream: Id::of(&fields, 3) };
        let kind = fields.iter().find_map(|f| match (f.number, &f.value) { (4, WireValue::Varint(v)) => Some(*v), _ => None }).unwrap_or(SYN);
        let payload = fields.iter().find_map(|f| match (f.number, &f.value) { (5, WireValue::Bytes(b)) => Some(*b), _ => None }).unwrap_or(&[]);
        match kind {
            SYN => self.syn(key, payload),
            DATA => {
                let writer = self.streams.lock().unwrap().get(&key).map(|s| s.0.clone());
                match writer {
                    Some(writer) => if writer.try_send(payload.to_vec()).is_err() {
                        log::warn!("Port mapping write buffer full; closing stream");
                        self.close(&key, true);
                    },
                    None => { let _ = self.outbound.try_send((key, FIN, Vec::new())); }
                }
            }
            FIN => self.close(&key, false),
            DATA_ACK => {}
            other => log::info!("Port mapping unsupported frame type {}", other),
        }
        true
    }

    fn syn(self: &Arc<Self>, key: Key, payload: &[u8]) {
        let request = if enabled() { parse_syn(payload) } else { Err("port mapping disabled on this host".into()) };
        let (host, port) = match request {
            Ok(target) => target,
            Err(error) => {
                log::info!("Port mapping SYN rejected: {}", error);
                let _ = self.outbound.try_send((key, SYN_ACK, syn_ack(false, Some(&error))));
                return;
            }
        };
        let session = self.clone();
        tokio::spawn(async move {
            log::info!("Port mapping connecting: port={} loopback={}", port, host == "localhost" || host.starts_with("127.") || host == "::1");
            let connected = tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::TcpStream::connect((host.as_str(), port))).await;
            let stream = match connected {
                Ok(Ok(stream)) => stream,
                Ok(Err(e)) => { session.reply_failure(key, &format!("connect failed: {}", e.kind())).await; return; }
                Err(_) => { session.reply_failure(key, "connect timeout").await; return; }
            };
            let _ = stream.set_nodelay(true);
            let (mut read, mut write) = stream.into_split();
            let (writer, mut writes) = mpsc::channel::<Vec<u8>>(WRITE_QUEUE_FRAMES);
            // SYN_ACK goes out before any DATA from the target (same ordered queue).
            if session.outbound.send((key.clone(), SYN_ACK, syn_ack(true, None))).await.is_err() { return; }
            log::info!("Port mapping stream open: port={}", port);
            tokio::spawn(async move {
                while let Some(data) = writes.recv().await {
                    if write.write_all(&data).await.is_err() { break; }
                }
                let _ = write.shutdown().await;
            });
            let reader_session = session.clone();
            let reader_key = key.clone();
            let (registered, wait_registered) = tokio::sync::oneshot::channel::<()>();
            let reader = tokio::spawn(async move {
                // Start reading only once the stream is registered, so an early close finds it.
                if wait_registered.await.is_err() { return; }
                let mut buf = vec![0u8; READ_CHUNK];
                loop {
                    match read.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => if reader_session.outbound.send((reader_key.clone(), DATA, buf[..n].to_vec())).await.is_err() { return; },
                    }
                }
                reader_session.close(&reader_key, true);
            });
            session.streams.lock().unwrap().insert(key, (writer, reader.abort_handle()));
            let _ = registered.send(());
        });
    }

    async fn reply_failure(&self, key: Key, error: &str) {
        log::info!("Port mapping connect failed: {}", error);
        let _ = self.outbound.send((key, SYN_ACK, syn_ack(false, Some(error)))).await;
    }

    /// Drops the stream: the writer task shuts the target down and the reader is aborted;
    /// optionally tells the viewer.
    fn close(&self, key: &Key, send_fin: bool) {
        let removed = self.streams.lock().unwrap().remove(key);
        if let Some((_, reader)) = removed {
            reader.abort();
            if send_fin { let _ = self.outbound.try_send((key.clone(), FIN, Vec::new())); }
        }
    }

    /// The peer connection ended: close every target connection.
    pub fn shutdown(&self) {
        let streams: Vec<_> = self.streams.lock().unwrap().drain().collect();
        if !streams.is_empty() { log::info!("Port mapping closed {} stream(s) with the connection", streams.len()); }
        for (_, (_, reader)) in streams { reader.abort(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syn_payload_uses_official_snake_case_keys() {
        assert_eq!(parse_syn(br#"{"version":1,"target_host":"127.0.0.1","target_port":22}"#).unwrap(), ("127.0.0.1".into(), 22));
        assert_eq!(parse_syn(br#"{"targetHost":"nas.local","targetPort":8080}"#).unwrap(), ("nas.local".into(), 8080));
        assert!(parse_syn(br#"{"target_host":"","target_port":22}"#).is_err());
        assert!(parse_syn(br#"{"target_host":"h","target_port":70000}"#).is_err());
        assert!(parse_syn(b"not json").is_err());
    }

    #[test]
    fn syn_ack_reports_ok_and_error() {
        let ok: serde_json::Value = serde_json::from_slice(&syn_ack(true, None)).unwrap();
        assert_eq!(ok, serde_json::json!({"version": 1, "ok": true}));
        let failed: serde_json::Value = serde_json::from_slice(&syn_ack(false, Some("connect timeout"))).unwrap();
        assert_eq!(failed["ok"], false);
        assert_eq!(failed["error"], "connect timeout");
    }

    #[test]
    fn frames_echo_identity_fields_with_their_wire_types() {
        let key = Key { session: Id::Bytes(b"s1".to_vec()), rule: Id::Varint(7), stream: Id::Varint(300) };
        let encoded = frame(&key, DATA, b"abc");
        let fields = decode_wire(&encoded).unwrap();
        assert_eq!(Id::of(&fields, 1), Id::Bytes(b"s1".to_vec()));
        assert_eq!(Id::of(&fields, 2), Id::Varint(7));
        assert_eq!(Id::of(&fields, 3), Id::Varint(300));
        assert!(fields.iter().any(|f| f.number == 4 && f.value == WireValue::Varint(DATA)));
        assert!(fields.iter().any(|f| f.number == 5 && f.value == WireValue::Bytes(b"abc")));
        // SYN is the proto3 default (0) and is omitted on the wire.
        let syn = frame(&key, SYN, b"");
        assert!(!decode_wire(&syn).unwrap().iter().any(|f| f.number == 4 || f.number == 5));
    }
}
