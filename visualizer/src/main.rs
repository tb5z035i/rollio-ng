mod ipc;
mod jpeg;
mod preview_config;
mod preview_pipeline;
mod protocol;
mod stream_info;
mod websocket;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clap::Parser;
use iceoryx2::node::NodeWaitFailure;
use tokio::sync::broadcast;

use crate::ipc::{IpcMessage, IpcPoller};
use crate::preview_config::RuntimePreviewConfig;
use crate::preview_pipeline::PreviewPipeline;
use crate::stream_info::StreamInfoRegistry;
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
    #[arg(long, default_value_t = 320)]
    max_preview_width: u32,

    /// Maximum preview height for JPEG downsampling
    #[arg(long, default_value_t = 240)]
    max_preview_height: u32,

    /// JPEG quality (1-100)
    #[arg(long, default_value_t = 30)]
    jpeg_quality: i32,

    /// Maximum preview frames per second per camera. Set to 0 to disable
    /// throttling.
    #[arg(long, default_value_t = 60)]
    preview_fps: u32,

    /// Number of preview worker threads used for JPEG compression.
    #[arg(long)]
    preview_workers: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
struct IpcPollConfig {
    jpeg_quality: i32,
    preview_fps: u32,
    preview_workers: usize,
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
    let preview_workers = args
        .preview_workers
        .unwrap_or_else(|| default_preview_workers(camera_names.len()))
        .max(1);

    log::info!(
        "cameras: {:?}, robots: {:?}, port: {}, max_preview={}x{}, jpeg_quality: {}, preview_fps: {}, preview_workers: {}",
        camera_names,
        robot_names,
        args.port,
        args.max_preview_width,
        args.max_preview_height,
        args.jpeg_quality,
        args.preview_fps,
        preview_workers,
    );

    // Broadcast channel: small capacity so slow consumers skip frames
    let (broadcast_tx, _) = broadcast::channel::<BroadcastMessage>(16);
    let stream_info = Arc::new(Mutex::new(StreamInfoRegistry::new(
        &camera_names,
        &robot_names,
        args.preview_fps,
        args.max_preview_width,
        args.max_preview_height,
        preview_workers,
        args.jpeg_quality,
    )));
    let preview_config = Arc::new(RuntimePreviewConfig::new(
        args.max_preview_width,
        args.max_preview_height,
    ));

    // Start WebSocket server
    let ws_addr: SocketAddr = ([0, 0, 0, 0], args.port).into();
    let ws_broadcast_tx = broadcast_tx.clone();
    let ws_stream_info = stream_info.clone();
    let ws_preview_config = preview_config.clone();
    tokio::spawn(async move {
        websocket::run_server(ws_addr, ws_broadcast_tx, ws_stream_info, ws_preview_config).await;
    });

    // Shared shutdown flag
    let shutdown = Arc::new(AtomicBool::new(false));

    // Run the iceoryx2 poll loop on a dedicated OS thread instead of
    // `spawn_blocking()`. Tokio waits for blocking tasks during runtime
    // shutdown, which can make Ctrl+C appear stuck if the poll loop is inside
    // a blocking iceoryx wait.
    let ipc_config = IpcPollConfig {
        jpeg_quality: args.jpeg_quality,
        preview_fps: args.preview_fps,
        preview_workers,
    };
    let ipc_broadcast_tx = broadcast_tx.clone();
    let ipc_shutdown = shutdown.clone();
    let ipc_stream_info = stream_info.clone();
    let ipc_preview_config = preview_config.clone();

    std::thread::Builder::new()
        .name("rollio-visualizer-ipc".to_string())
        .spawn(move || {
            if let Err(e) = ipc_poll_loop(
                &camera_names,
                &robot_names,
                ipc_config,
                ipc_broadcast_tx,
                ipc_stream_info,
                ipc_preview_config,
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
/// Polls iceoryx2 subscribers, forwards robot state immediately, and hands the
/// latest camera frames off to the preview worker pipeline.
fn ipc_poll_loop(
    camera_names: &[String],
    robot_names: &[String],
    config: IpcPollConfig,
    broadcast_tx: broadcast::Sender<BroadcastMessage>,
    stream_info: Arc<Mutex<StreamInfoRegistry>>,
    preview_config: Arc<RuntimePreviewConfig>,
    shutdown: &AtomicBool,
) -> Result<(), Box<dyn std::error::Error>> {
    let poller = IpcPoller::new(camera_names, robot_names)?;
    let preview_pipeline = PreviewPipeline::new(
        camera_names,
        config.preview_workers,
        preview_config,
        config.jpeg_quality,
        broadcast_tx.clone(),
        stream_info.clone(),
    )?;
    let preview_interval = if config.preview_fps == 0 {
        None
    } else {
        Some(Duration::from_secs_f64(1.0 / config.preview_fps as f64))
    };
    let mut next_preview_at: HashMap<String, Instant> = HashMap::new();

    log::info!("IPC poll loop started");

    while !shutdown.load(Ordering::Relaxed) {
        let messages = poller.poll();

        for msg in messages {
            match msg {
                IpcMessage::CameraFrame { name, header, data } => {
                    if let Ok(mut info) = stream_info.lock() {
                        info.observe_source_frame(&name, &header);
                    }

                    if let Some(interval) = preview_interval {
                        let now = Instant::now();
                        let next_due = next_preview_at.entry(name.clone()).or_insert(now);
                        if now < *next_due {
                            continue;
                        }

                        // Keep a stable cadence anchored to the previous due time
                        // instead of resetting the schedule to "now + interval" on
                        // every published frame. This reduces jitter-driven frame skips
                        // when the source and preview rates are close (for example 60 -> 60).
                        while *next_due <= now {
                            *next_due += interval;
                        }
                    }

                    preview_pipeline.submit_frame(name, header, data);
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

fn default_preview_workers(camera_count: usize) -> usize {
    let available_parallelism = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    camera_count.max(1).min(available_parallelism)
}
