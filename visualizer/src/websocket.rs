//! WebSocket server for the visualizer bridge.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::preview_config::RuntimePreviewConfig;
use crate::protocol;
use crate::stream_info::StreamInfoRegistry;

#[derive(Clone, Debug)]
pub enum BroadcastMessage {
    Binary(Arc<Vec<u8>>),
    Text(Arc<String>),
}

/// Side-channel handle the WS handler uses to forward
/// `set_preview_size` to every per-camera `PreviewControl` topic.
pub type PreviewControlSender = Arc<dyn Fn(u32, u32) + Send + Sync>;

pub async fn run_server(
    addr: SocketAddr,
    broadcast_tx: broadcast::Sender<BroadcastMessage>,
    stream_info: Arc<Mutex<StreamInfoRegistry>>,
    preview_config: Arc<RuntimePreviewConfig>,
    preview_control_sender: PreviewControlSender,
    cached_configs: Arc<Mutex<Vec<Vec<u8>>>>,
) {
    let listener = match TcpListener::bind(addr).await {
        Ok(l) => {
            log::info!("WebSocket server listening on {addr}");
            l
        }
        Err(e) => {
            log::error!("failed to bind WebSocket server on {addr}: {e}");
            return;
        }
    };

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                log::info!("new WebSocket connection from {peer}");
                let rx = broadcast_tx.subscribe();
                tokio::spawn(handle_client(
                    stream,
                    peer,
                    rx,
                    stream_info.clone(),
                    preview_config.clone(),
                    preview_control_sender.clone(),
                    cached_configs.clone(),
                ));
            }
            Err(e) => log::warn!("accept error: {e}"),
        }
    }
}

async fn handle_client(
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    mut broadcast_rx: broadcast::Receiver<BroadcastMessage>,
    stream_info: Arc<Mutex<StreamInfoRegistry>>,
    preview_config: Arc<RuntimePreviewConfig>,
    preview_control_sender: PreviewControlSender,
    cached_configs: Arc<Mutex<Vec<Vec<u8>>>>,
) {
    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            log::warn!("WebSocket handshake failed for {peer}: {e}");
            return;
        }
    };

    let (mut ws_sink, mut ws_source) = ws_stream.split();

    // Initial handshake: stream_info JSON + cached encoded configs
    // (one binary 0x02 per camera) so a freshly-connected WebCodecs
    // client can configure its decoder without waiting for the next
    // session restart.
    let initial_stream_info = {
        let info = stream_info.lock().expect("stream info mutex poisoned");
        protocol::encode_stream_info(&info.snapshot())
    };
    if let Err(e) = ws_sink
        .send(Message::Text(initial_stream_info.into()))
        .await
    {
        log::debug!("failed to send initial stream info to {peer}: {e}");
        let _ = ws_sink.close().await;
        return;
    }
    let cached: Vec<Vec<u8>> = cached_configs
        .lock()
        .expect("cached configs mutex poisoned")
        .clone();
    for bytes in cached {
        if ws_sink.send(Message::Binary(bytes.into())).await.is_err() {
            let _ = ws_sink.close().await;
            return;
        }
    }
    preview_config.client_connected();

    loop {
        tokio::select! {
            incoming = ws_source.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(cmd) = protocol::decode_command(&text) {
                            log::info!("command from {peer}: {cmd:?}");
                            match cmd.action.as_str() {
                                "get_stream_info" => {
                                    let payload = {
                                        let info = stream_info.lock().expect("stream info");
                                        protocol::encode_stream_info(&info.snapshot())
                                    };
                                    if ws_sink.send(Message::Text(payload.into())).await.is_err() {
                                        break;
                                    }
                                }
                                "set_preview_size" => {
                                    let Some(width) = cmd.width else {
                                        log::warn!("ignoring set_preview_size without width from {peer}");
                                        continue;
                                    };
                                    let Some(height) = cmd.height else {
                                        log::warn!("ignoring set_preview_size without height from {peer}");
                                        continue;
                                    };
                                    let active = preview_config.set_requested_size(width, height);
                                    {
                                        let mut info = stream_info.lock().expect("stream info");
                                        info.set_active_preview_bounds(active.width, active.height);
                                    }
                                    // Forward upstream so every per-camera preview
                                    // encoder restarts at the new dims.
                                    preview_control_sender(active.width, active.height);
                                    let payload = {
                                        let info = stream_info.lock().expect("stream info");
                                        protocol::encode_stream_info(&info.snapshot())
                                    };
                                    if ws_sink.send(Message::Text(payload.into())).await.is_err() {
                                        break;
                                    }
                                }
                                _ => log::debug!("visualizer ignoring action {} from {peer}", cmd.action),
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => break,
                    Some(Err(e)) => {
                        log::debug!("read error from {peer}: {e}");
                        break;
                    }
                    Some(_) => {}
                    None => break,
                }
            }
            broadcast_msg = broadcast_rx.recv() => {
                match broadcast_msg {
                    Ok(msg) => {
                        let ws_msg = match msg {
                            BroadcastMessage::Binary(data) => Message::Binary((*data).clone().into()),
                            BroadcastMessage::Text(text) => Message::Text((*text).clone().into()),
                        };
                        if ws_sink.send(ws_msg).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        log::debug!("client {peer} lagged, skipped {n} messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    if let Some(default_preview) = preview_config.client_disconnected() {
        if let Ok(mut info) = stream_info.lock() {
            info.set_active_preview_bounds(default_preview.width, default_preview.height);
        }
    }
    let _ = ws_sink.close().await;
    log::info!("client {peer} disconnected");
}
