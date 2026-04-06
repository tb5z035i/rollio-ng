/// WebSocket server for the Visualizer.
///
/// Accepts WebSocket connections and broadcasts camera frames (binary) and
/// robot states (JSON text) to all connected clients. Uses `Arc` to share
/// message payloads across clients without copying.
///
/// Slow clients that fall behind on the broadcast channel simply skip frames
/// (lag) rather than causing backpressure.
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::preview_config::RuntimePreviewConfig;
use crate::protocol;
use crate::stream_info::StreamInfoRegistry;

/// A message to broadcast to all WebSocket clients.
#[derive(Clone, Debug)]
pub enum BroadcastMessage {
    /// Binary message (camera frame encoded per the WS protocol).
    Binary(Arc<Vec<u8>>),
    /// Text/JSON message (robot state, status, etc.).
    Text(Arc<String>),
}

/// Run the WebSocket server, accepting connections and broadcasting messages.
///
/// This function runs forever (until the task is cancelled).
pub async fn run_server(
    addr: SocketAddr,
    broadcast_tx: broadcast::Sender<BroadcastMessage>,
    stream_info: Arc<Mutex<StreamInfoRegistry>>,
    preview_config: Arc<RuntimePreviewConfig>,
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
                let client_stream_info = stream_info.clone();
                let client_preview_config = preview_config.clone();
                tokio::spawn(handle_client(
                    stream,
                    peer,
                    rx,
                    client_stream_info,
                    client_preview_config,
                ));
            }
            Err(e) => {
                log::warn!("accept error: {e}");
            }
        }
    }
}

/// Handle a single WebSocket client connection.
///
/// Reads from the broadcast channel and forwards to the WebSocket.
/// Also reads incoming messages from the client (commands).
async fn handle_client(
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    mut broadcast_rx: broadcast::Receiver<BroadcastMessage>,
    stream_info: Arc<Mutex<StreamInfoRegistry>>,
    preview_config: Arc<RuntimePreviewConfig>,
) {
    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            log::warn!("WebSocket handshake failed for {peer}: {e}");
            return;
        }
    };

    let (mut ws_sink, mut ws_source) = ws_stream.split();

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
    preview_config.client_connected();

    loop {
        tokio::select! {
            incoming = ws_source.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(cmd) = protocol::decode_command(&text) {
                            log::info!("command from {peer}: {:?}", cmd);
                            match cmd.action.as_str() {
                                "get_stream_info" => {
                                    let stream_info_payload = {
                                        let info = stream_info.lock().expect("stream info mutex poisoned");
                                        protocol::encode_stream_info(&info.snapshot())
                                    };
                                    if let Err(e) = ws_sink.send(Message::Text(stream_info_payload.into())).await {
                                        log::debug!("write error to {peer}: {e}");
                                        break;
                                    }
                                }
                                "set_preview_size" => {
                                    let Some(width) = cmd.width else {
                                        log::warn!("ignoring preview resize from {peer} without width");
                                        continue;
                                    };
                                    let Some(height) = cmd.height else {
                                        log::warn!("ignoring preview resize from {peer} without height");
                                        continue;
                                    };
                                    let active_preview = preview_config.set_requested_size(width, height);
                                    let stream_info_payload = {
                                        let mut info = stream_info.lock().expect("stream info mutex poisoned");
                                        info.set_active_preview_bounds(
                                            active_preview.width,
                                            active_preview.height,
                                        );
                                        protocol::encode_stream_info(&info.snapshot())
                                    };
                                    if let Err(e) = ws_sink.send(Message::Text(stream_info_payload.into())).await {
                                        log::debug!("write error to {peer}: {e}");
                                        break;
                                    }
                                }
                                _ => {
                                    log::debug!("ignoring unsupported command from {peer}: {}", cmd.action);
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        log::info!("client {peer} sent close");
                        break;
                    }
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
                        if let Err(e) = ws_sink.send(ws_msg).await {
                            log::debug!("write error to {peer}: {e}");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        log::debug!("client {peer} lagged, skipped {n} messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        log::info!("broadcast channel closed, disconnecting {peer}");
                        break;
                    }
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
