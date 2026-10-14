//! Native TCP Port Forwarding & SSH Tunneling via DataChannel

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;

pub struct PortForwarder;

impl PortForwarder {
    /// 将 DataChannel 桥接到本地指定 TCP 端口（默认 127.0.0.1:22 SSH）
    pub async fn bridge_tcp(
        target_addr: &str,
        channel: Arc<RTCDataChannel>,
    ) -> Result<(), String> {
        let stream = TcpStream::connect(target_addr)
            .await
            .map_err(|e| format!("Failed to connect to {}: {}", target_addr, e))?;

        log::info!("Connected to local service {}", target_addr);
        let (mut tcp_read, mut tcp_write) = stream.into_split();
        let channel_for_write = channel.clone();

        // 协程 1: 从本地 TCP (如 SSH) 读取数据并写入 DataChannel
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            loop {
                match tcp_read.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let bytes = bytes::Bytes::copy_from_slice(&buf[..n]);
                        if let Err(e) = channel_for_write.send(&bytes).await {
                            log::warn!("DataChannel send failed: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        log::warn!("TCP read error: {}", e);
                        break;
                    }
                }
            }
        });

        // 协程 2: 绑定 DataChannel on_message 写入本地 TCP
        let tcp_write = Arc::new(Mutex::new(tcp_write));
        let tcp_write_clone = tcp_write.clone();

        channel.on_message(Box::new(move |msg: DataChannelMessage| {
            let tcp = tcp_write_clone.clone();
            Box::pin(async move {
                let mut lock = tcp.lock().await;
                let _ = lock.write_all(&msg.data).await;
            })
        }));

        Ok(())
    }
}
