//! WebSocket server for the control plane.
//!
//! Accepts connections on a configured loopback port, decodes inbound
//! `command` messages, and forwards them to the iceoryx2 poll loop. Outbound
//! state snapshots arrive on a broadcast channel and are fanned out to all
//! connected clients.

use std::net::SocketAddr;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::protocol;
use crate::{OutboundMessage, UiCommand};

pub async fn run_server(
    addr: SocketAddr,
    outbound_tx: broadcast::Sender<OutboundMessage>,
    command_tx: Sender<UiCommand>,
    latest_snapshot: Arc<Mutex<Option<String>>>,
) {
    let listener = match TcpListener::bind(addr).await {
        Ok(l) => {
            log::info!("control WebSocket listening on {addr}");
            l
        }
        Err(e) => {
            log::error!("failed to bind control WebSocket on {addr}: {e}");
            return;
        }
    };

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                log::info!("new control WebSocket client {peer}");
                let outbound_rx = outbound_tx.subscribe();
                let client_command_tx = command_tx.clone();
                let client_latest = latest_snapshot.clone();
                tokio::spawn(handle_client(
                    stream,
                    peer,
                    outbound_rx,
                    client_command_tx,
                    client_latest,
                ));
            }
            Err(e) => {
                log::warn!("control WebSocket accept error: {e}");
            }
        }
    }
}

async fn handle_client(
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    mut outbound_rx: broadcast::Receiver<OutboundMessage>,
    command_tx: Sender<UiCommand>,
    latest_snapshot: Arc<Mutex<Option<String>>>,
) {
    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            log::warn!("control WebSocket handshake failed for {peer}: {e}");
            return;
        }
    };

    let (mut sink, mut source) = ws_stream.split();

    let initial_snapshot = latest_snapshot
        .lock()
        .expect("control snapshot mutex poisoned")
        .clone();
    if let Some(snapshot_json) = initial_snapshot {
        if let Err(e) = sink.send(Message::Text(snapshot_json.into())).await {
            log::debug!("failed to send initial control snapshot to {peer}: {e}");
            let _ = sink.close().await;
            return;
        }
    }

    loop {
        tokio::select! {
            incoming = source.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(cmd) = protocol::decode_command(&text) {
                            log::info!("control command from {peer}: {}", cmd.action);
                            if cmd.action.starts_with("setup_") {
                                if let Err(e) = command_tx.send(UiCommand::Setup(text.to_string())) {
                                    log::warn!("failed to forward setup command from {peer}: {e}");
                                    break;
                                }
                            } else if let Some(episode) = protocol::decode_episode_command(&cmd.action) {
                                if let Err(e) = command_tx.send(UiCommand::Episode(episode)) {
                                    log::warn!("failed to forward episode command from {peer}: {e}");
                                    break;
                                }
                            } else {
                                log::debug!("ignoring unsupported control action from {peer}: {}", cmd.action);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        log::info!("control client {peer} sent close");
                        break;
                    }
                    Some(Err(e)) => {
                        log::debug!("control read error from {peer}: {e}");
                        break;
                    }
                    Some(_) => {}
                    None => break,
                }
            }
            outbound = outbound_rx.recv() => {
                match outbound {
                    Ok(msg) => {
                        if let Err(e) = sink.send(Message::Text((*msg.text).clone().into())).await {
                            log::debug!("control write error to {peer}: {e}");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        log::debug!("control client {peer} lagged, skipped {n} messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        log::info!("control broadcast closed, disconnecting {peer}");
                        break;
                    }
                }
            }
        }
    }

    let _ = sink.close().await;
    log::info!("control client {peer} disconnected");
}
