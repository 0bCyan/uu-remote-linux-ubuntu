//! Lightweight Native Engine.IO v4 / Socket.IO Signaling Client

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoacMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gzip_sdp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ice_id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp_mid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp_mline_index: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_control_id: Option<Value>,
}

pub enum SignalingEvent {
    Connected,
    RoomInfoAck(Value),
    DeviceInfoChanged(Value),
    BeControlled(Value),
    /// be-controlled with its binary app_data (ConnectOptions): capture type and the
    /// ice_id it announces, when present.
    ConnectRequest { capture_type: u64, ice_id: Option<String>, transport:crate::session_ice::SessionIce },
    Soac(SoacMessage),
    Released(Value),
    Left(Value),
    Disconnected,
}

pub struct SignalingClient {
    tx_outgoing: mpsc::Sender<String>,
}

impl SignalingClient {
    pub async fn connect(
        url: &str,
        room_token: &str,
        event_sender: mpsc::Sender<SignalingEvent>,
    ) -> Result<Self, String> {
        let ws_url = format!(
            "{}/socket.io/?transport=websocket&EIO=4",
            url.trim_end_matches('/')
        );

        let mut req = ws_url
            .clone()
            .into_client_request()
            .map_err(|e| format!("Invalid WS request: {}", e))?;

        req.headers_mut().insert(
            "X-NRD-AUTH",
            room_token.parse().map_err(|_| "Invalid token header")?,
        );

        let (ws_stream, _) = connect_async(req)
            .await
            .map_err(|e| format!("WS connect failed: {}", e))?;

        log::info!("Connected to Engine.IO / Socket.IO at {}", ws_url);
        let (mut write, mut read) = ws_stream.split();
        let (tx_outgoing, mut rx_outgoing) = mpsc::channel::<String>(32);

        // 发送协程
        tokio::spawn(async move {
            while let Some(msg) = rx_outgoing.recv().await {
                if let Err(e) = write.send(Message::Text(msg)).await {
                    log::warn!("WS send error: {}", e);
                    break;
                }
            }
        });

        let tx_out_clone = tx_outgoing.clone();

        // 接收与状态机协程
        tokio::spawn(async move {
            let mut req_counter = 10;
            // Socket.IO binary event awaiting attachments: (event name, header JSON).
            let mut pending_binary: Option<(String, Value)> = None;
            while let Some(msg_res) = read.next().await {
                match msg_res {
                    Ok(Message::Text(txt)) => {
                        // Engine.IO 报文处理
                        if txt.starts_with('0') {
                            // Open packet -> 发送 Socket.IO connect
                            let _ = tx_out_clone.send("40".into()).await;
                        } else if txt == "2" {
                            // Ping -> 回复 Pong
                            let _ = tx_out_clone.send("3".into()).await;
                        } else if txt.starts_with("40") {
                            // Socket.IO connected -> 发送 room_info 请求
                            req_counter += 1;
                            let room_req = format!("42{}[\"room_info\"]", req_counter);
                            let _ = tx_out_clone.send(room_req).await;
                            let _ = event_sender.send(SignalingEvent::Connected).await;
                        } else if txt.starts_with("43") {
                            // Socket.IO ack 回调
                            let payload = &txt[txt.find('[').unwrap_or(2)..];
                            if let Ok(v) = serde_json::from_str::<Value>(payload) {
                                let _ = event_sender.send(SignalingEvent::RoomInfoAck(v)).await;
                            }
                        } else if txt.starts_with("45") || txt.starts_with("46") {
                            // Socket.IO binary event/ack: header text, attachments follow as binary frames.
                            let body = txt.trim_start_matches(|c: char| c.is_ascii_digit() || c == '-');
                            let name = serde_json::from_str::<Value>(body).ok()
                                .and_then(|v| v.get(0).and_then(|n| n.as_str()).map(str::to_owned))
                                .unwrap_or_default();
                            let name: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').take(48).collect();
                            log::info!("Socket.IO binary packet: kind={} event={} header_bytes={}", &txt[..2], name, txt.len());
                            let header = serde_json::from_str::<Value>(body).ok().and_then(|v| v.get(1).cloned()).unwrap_or(Value::Null);
                            pending_binary = Some((name, header));
                        } else if txt.starts_with("42") {
                            // Socket.IO event
                            let payload = &txt[2..];
                            log::debug!("Socket.IO event received ({} bytes)", payload.len());
                            if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(payload) {
                                if arr.len() >= 2 {
                                    let event_name = arr[0].as_str().unwrap_or_default();
                                    let data = &arr[1];
                                    log::info!("Socket.IO event dispatch: name={}", event_name);

                                    match event_name {
                                        "bmsg_push" => {
                                            let _ = event_sender
                                                .send(SignalingEvent::DeviceInfoChanged(data.clone()))
                                                .await;
                                        }
                                        "be-controlled" => {
                                            let _ = event_sender
                                                .send(SignalingEvent::BeControlled(data.clone()))
                                                .await;
                                        }
                                        "soac" => {
                                            let (target_val, client_id_opt) = if let Some(inner) = data.get("data") {
                                                (inner.clone(), data.get("client_id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                                            } else {
                                                (data.clone(), None)
                                            };

                                            match serde_json::from_value::<SoacMessage>(target_val.clone()) {
                                                Ok(mut soac) => {
                                                    if soac.client_id.is_none() {
                                                        soac.client_id = client_id_opt;
                                                    }
                                                    let _ = event_sender
                                                        .send(SignalingEvent::Soac(soac))
                                                        .await;
                                                }
                                                Err(e) => {
                                                    log::error!("Failed to deserialize SoacMessage: error={}, raw={}", e, data);
                                                }
                                            }
                                        }
                                        "released" => {
                                            let _ = event_sender
                                                .send(SignalingEvent::Released(data.clone()))
                                                .await;
                                        }
                                        "left" | "leave" => {
                                            let ice_id = data.get("ice_id").and_then(|v| v.as_str()).unwrap_or("");
                                            if ice_id.is_empty() {
                                                log::warn!("Room expired from NetEase cloud (leave event). Triggering room renewal...");
                                                let _ = event_sender.send(SignalingEvent::Disconnected).await;
                                            } else {
                                                let _ = event_sender
                                                    .send(SignalingEvent::Left(data.clone()))
                                                    .await;
                                            }
                                        }
                                        _ => {
                                            let keys: Vec<String> = data.as_object().map(|o| o.keys().take(16).cloned().collect()).unwrap_or_default();
                                            log::info!("Unhandled Socket.IO event: {} keys={:?}", event_name, keys);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Ok(Message::Binary(data)) => {
                        // Engine.IO v4 sends attachments raw; tolerate a legacy 0x04 prefix.
                        let data: &[u8] = if data.first() == Some(&4) && crate::protocol::decode_wire(&data).is_err() { &data[1..] } else { &data };
                        let fields = crate::protocol::decode_wire(data).ok();
                        let shape: Vec<(u32, Option<u64>)> = fields.as_ref().map(|f| f.iter().map(|x| (x.number, match x.value {
                            crate::protocol::WireValue::Varint(v) => Some(v), _ => None })).collect()).unwrap_or_default();
                        log::info!("Socket.IO binary attachment: bytes={} protobuf_fields={:?}", data.len(), shape);
                        if let Some((name, header)) = pending_binary.take() {
                            if name == "be-controlled" {
                                let capture_type = shape.iter().find(|(n, _)| *n == 1).and_then(|(_, v)| *v).unwrap_or(0);
                                let keys: Vec<String> = header.as_object().map(|o| o.keys().take(24).cloned().collect()).unwrap_or_default();
                                let ice_id = ["ice_id", "iceId"].iter().find_map(|k| header.get(*k).or_else(|| header.get("data").and_then(|d| d.get(*k))))
                                    .and_then(|v| v.as_str()).map(str::to_owned);
                                log::info!("be-controlled connect request: capture_type={} header_keys={:?} ice_id_present={}", capture_type, keys, ice_id.is_some());
                                let _ = event_sender.send(SignalingEvent::ConnectRequest { capture_type, ice_id, transport:crate::session_ice::SessionIce::from_header(&header) }).await;
                            }
                        }
                    }
                    Ok(Message::Close(_)) | Err(_) => {
                        let _ = event_sender.send(SignalingEvent::Disconnected).await;
                        break;
                    }
                    _ => {}
                }
            }
        });

        Ok(Self { tx_outgoing })
    }

    pub async fn emit_event(&self, event: &str, payload: serde_json::Value) -> Result<(), String> {
        let packet = format!("42{}", serde_json::json!([event, payload]));
        log::info!("Emitting signaling event: {}", event);
        self.tx_outgoing.send(packet).await.map_err(|e| e.to_string())
    }

    /// 发送 SOAC 消息（Answer 或 Candidate）
    pub async fn emit_soac(&self, soac: SoacMessage) -> Result<(), String> {
        let payload = if let Some(cid) = &soac.client_id {
            serde_json::json!({
                "client_id": cid,
                "data": soac
            }).to_string()
        } else {
            serde_json::to_string(&soac).map_err(|e| e.to_string())?
        };
        let msg = format!("42[\"soac\",{}]", payload);
        log::info!("Emitting soac packet: type={}", soac.msg_type);
        self.tx_outgoing
            .send(msg)
            .await
            .map_err(|e| format!("Send soac error: {}", e))
    }
}
