mod ipc;
mod jpeg;
mod protocol;
mod websocket;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use iceoryx2::node::NodeWaitFailure;
use tokio::sync::broadcast;

use crate::ipc::{IpcMessage, IpcPoller};
use crate::jpeg::JpegCompressor;
use crate::websocket::BroadcastMessage;

#[derive(Parser, Debug)]
#[command(name = "rollio-visualizer")]
#[command(about = "iceoryx2 subscriber → WebSocket bridge with JPEG compression")]
struct Args {
    /// WebSocket server port
    #[arg(long, default_value_t = 9090)]
    port: u16,

    /// Comma-separated camera names to subscribe to
    #[arg(long, default_value = "camera_0,camera_1")]
    cameras: String,

    /// Comma-separated robot names to subscribe to
    #[arg(long, default_value = "robot_0")]
    robots: String,

    /// Maximum preview width for JPEG downsampling
    #[arg(long, default_value_t = 640)]
    max_preview_width: u32,

    /// JPEG quality (1-100)
    #[arg(long, default_value_t = 75)]
    jpeg_quality: i32,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    let camera_names: Vec<String> = args
        .cameras
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let robot_names: Vec<String> = args
        .robots
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    log::info!(
        "cameras: {:?}, robots: {:?}, port: {}, max_preview_width: {}, jpeg_quality: {}",
        camera_names,
        robot_names,
        args.port,
        args.max_preview_width,
        args.jpeg_quality,
    );

    // Broadcast channel: small capacity so slow consumers skip frames
    let (broadcast_tx, _) = broadcast::channel::<BroadcastMessage>(16);

    // Start WebSocket server
    let ws_addr: SocketAddr = ([0, 0, 0, 0], args.port).into();
    let ws_broadcast_tx = broadcast_tx.clone();
    tokio::spawn(async move {
        websocket::run_server(ws_addr, ws_broadcast_tx).await;
    });

    // Shared shutdown flag
    let shutdown = Arc::new(AtomicBool::new(false));

    // Run the iceoryx2 poll loop on a dedicated OS thread instead of
    // `spawn_blocking()`. Tokio waits for blocking tasks during runtime
    // shutdown, which can make Ctrl+C appear stuck if the poll loop is inside
    // a blocking iceoryx wait.
    let max_preview_width = args.max_preview_width;
    let jpeg_quality = args.jpeg_quality;
    let ipc_broadcast_tx = broadcast_tx.clone();
    let ipc_shutdown = shutdown.clone();

    std::thread::Builder::new()
        .name("rollio-visualizer-ipc".to_string())
        .spawn(move || {
            if let Err(e) = ipc_poll_loop(
                &camera_names,
                &robot_names,
                max_preview_width,
                jpeg_quality,
                ipc_broadcast_tx,
                &ipc_shutdown,
            ) {
                log::error!("IPC poll loop failed: {e}");
            }
        })?;

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await?;
    log::info!("shutting down");
    shutdown.store(true, Ordering::Relaxed);

    // Give the blocking thread a moment to exit
    tokio::time::sleep(Duration::from_millis(100)).await;

    Ok(())
}

/// Main iceoryx2 polling loop. Runs on a blocking thread.
///
/// Polls iceoryx2 subscribers, compresses camera frames to JPEG, encodes
/// protocol messages, and sends them to the broadcast channel.
fn ipc_poll_loop(
    camera_names: &[String],
    robot_names: &[String],
    max_preview_width: u32,
    jpeg_quality: i32,
    broadcast_tx: broadcast::Sender<BroadcastMessage>,
    shutdown: &AtomicBool,
) -> Result<(), Box<dyn std::error::Error>> {
    let poller = IpcPoller::new(camera_names, robot_names)?;
    let mut compressor = JpegCompressor::new(jpeg_quality)?;

    log::info!("IPC poll loop started");

    while !shutdown.load(Ordering::Relaxed) {
        let messages = poller.poll();

        for msg in messages {
            match msg {
                IpcMessage::CameraFrame { name, header, data } => {
                    // Compress to JPEG (with optional downsampling)
                    match compressor.compress(&data, header.width, header.height, max_preview_width)
                    {
                        Ok(jpeg_data) => {
                            let encoded = protocol::encode_camera_frame(
                                &name,
                                header.width,
                                header.height,
                                &jpeg_data,
                            );
                            // Broadcast to all WebSocket clients via Arc (no clone of payload)
                            let _ = broadcast_tx.send(BroadcastMessage::Binary(Arc::new(encoded)));
                        }
                        Err(e) => {
                            log::warn!("JPEG compression failed for {name}: {e}");
                        }
                    }
                }
                IpcMessage::RobotStateMsg { name, state } => {
                    let json = protocol::encode_robot_state(&name, &state);
                    let _ = broadcast_tx.send(BroadcastMessage::Text(Arc::new(json)));
                }
            }
        }

        // Wait briefly for more data (1ms — low latency, minimal CPU when idle).
        // Stop promptly if the sleep is interrupted by a termination signal.
        match poller.node().wait(Duration::from_millis(1)) {
            Ok(()) => {}
            Err(NodeWaitFailure::Interrupt | NodeWaitFailure::TerminationRequest) => {
                log::info!("IPC poll loop interrupted by shutdown signal");
                break;
            }
        }
    }

    log::info!("IPC poll loop stopped");
    Ok(())
}
