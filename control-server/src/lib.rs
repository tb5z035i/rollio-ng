//! Library entry points for the rollio control server.
//!
//! Exposes [`run`] so the binary and integration tests can drive the same
//! lifecycle. The server hosts a WebSocket on `127.0.0.1:<port>` and bridges
//! it to iceoryx2.

pub mod ipc;
pub mod protocol;
pub mod websocket;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use rollio_types::messages::EpisodeCommand;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// Direction-of-traffic role for the control server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlServerRole {
    /// Setup wizard: forwards `setup_command` and `setup_state`.
    Setup,
    /// Collect session: forwards `episode_command`, `episode_status`,
    /// and `backpressure`.
    Collect,
}

/// Inline TOML configuration accepted by the binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlServerConfig {
    pub port: u16,
    pub role: ControlServerRole,
}

/// One outbound JSON snapshot, broadcast to all connected clients.
#[derive(Debug, Clone)]
pub struct OutboundMessage {
    pub text: Arc<String>,
}

impl OutboundMessage {
    pub fn new(text: String) -> Self {
        Self {
            text: Arc::new(text),
        }
    }
}

/// A command from a UI client, queued for republication on iceoryx2.
#[derive(Debug, Clone)]
pub enum UiCommand {
    /// Raw JSON of a `setup_*` command to forward verbatim.
    Setup(String),
    /// A typed episode command.
    Episode(EpisodeCommand),
}

/// Run the control server until shutdown is requested.
pub async fn run(config: ControlServerConfig) -> Result<(), Box<dyn std::error::Error>> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let (outbound_tx, _) = broadcast::channel::<OutboundMessage>(64);
    let (command_tx, command_rx) = mpsc::channel::<UiCommand>();
    let latest_snapshot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let ws_addr: SocketAddr = ([127, 0, 0, 1], config.port).into();
    let ws_outbound_tx = outbound_tx.clone();
    let ws_command_tx = command_tx.clone();
    let ws_latest_snapshot = latest_snapshot.clone();
    tokio::spawn(async move {
        websocket::run_server(ws_addr, ws_outbound_tx, ws_command_tx, ws_latest_snapshot).await;
    });

    let role = config.role;
    let ipc_outbound_tx = outbound_tx.clone();
    let ipc_latest_snapshot = latest_snapshot.clone();
    let ipc_shutdown = shutdown.clone();
    let ipc_handle = std::thread::Builder::new()
        .name("rollio-control-server-ipc".into())
        .spawn(move || {
            if let Err(e) = ipc::run_poll_loop(
                role,
                command_rx,
                ipc_outbound_tx,
                ipc_latest_snapshot,
                ipc_shutdown,
            ) {
                log::error!("control server IPC poll loop failed: {e}");
            }
        })?;

    // The control server's lifecycle equals the entire setup/collect session.
    // We intentionally do NOT listen for ControlEvent::Shutdown on the bus —
    // that signal is used to stop per-swap preview runtimes. Instead we exit
    // when the controller sends SIGINT or SIGTERM.
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            if let Err(e) = result {
                log::warn!("control server failed to install Ctrl+C handler: {e}");
            }
            log::info!("control server received Ctrl+C");
        }
        _ = sigterm.recv() => {
            log::info!("control server received SIGTERM");
        }
    }

    shutdown.store(true, Ordering::Relaxed);
    if let Err(e) = ipc_handle.join() {
        log::warn!("control server IPC thread join failed: {e:?}");
    }
    Ok(())
}
