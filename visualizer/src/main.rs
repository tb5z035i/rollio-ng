mod ipc;
mod jpeg;
mod preview_config;
mod preview_pipeline;
mod protocol;
mod stream_info;
mod websocket;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clap::Parser;
use iceoryx2::node::NodeWaitFailure;
use rollio_types::config::VisualizerRuntimeConfig;
use rollio_types::messages::EpisodeCommand;
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
    /// TOML file containing VisualizerRuntimeConfig
    #[arg(long, value_name = "PATH", conflicts_with = "config_inline")]
    config: Option<PathBuf>,

    /// Inline TOML containing VisualizerRuntimeConfig
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    config_inline: Option<String>,

    /// WebSocket server port
    #[arg(long)]
    port: Option<u16>,

    /// Comma-separated camera names to subscribe to
    #[arg(long)]
    cameras: Option<String>,

    /// Comma-separated robot names to subscribe to
    #[arg(long)]
    robots: Option<String>,

    /// Maximum preview width for JPEG downsampling
    #[arg(long)]
    max_preview_width: Option<u32>,

    /// Maximum preview height for JPEG downsampling
    #[arg(long)]
    max_preview_height: Option<u32>,

    /// JPEG quality (1-100)
    #[arg(long)]
    jpeg_quality: Option<i32>,

    /// Maximum preview frames per second per camera. Set to 0 to disable
    /// throttling.
    #[arg(long)]
    preview_fps: Option<u32>,

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

fn split_name_list(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn legacy_runtime_config(args: &Args) -> VisualizerRuntimeConfig {
    let defaults = VisualizerRuntimeConfig::default();
    VisualizerRuntimeConfig {
        port: args.port.unwrap_or(defaults.port),
        cameras: args
            .cameras
            .as_deref()
            .map(split_name_list)
            .unwrap_or_else(|| split_name_list("camera_0,camera_1")),
        robots: args
            .robots
            .as_deref()
            .map(split_name_list)
            .unwrap_or_else(|| split_name_list("robot_0")),
        max_preview_width: args.max_preview_width.unwrap_or(defaults.max_preview_width),
        max_preview_height: args
            .max_preview_height
            .unwrap_or(defaults.max_preview_height),
        jpeg_quality: args.jpeg_quality.unwrap_or(defaults.jpeg_quality),
        preview_fps: args.preview_fps.unwrap_or(defaults.preview_fps),
        preview_workers: args.preview_workers.or(defaults.preview_workers),
    }
}

fn load_runtime_config(args: &Args) -> Result<VisualizerRuntimeConfig, Box<dyn std::error::Error>> {
    let mut config = if let Some(config_path) = &args.config {
        std::fs::read_to_string(config_path)?.parse::<VisualizerRuntimeConfig>()?
    } else if let Some(config_inline) = &args.config_inline {
        config_inline.parse::<VisualizerRuntimeConfig>()?
    } else {
        legacy_runtime_config(args)
    };

    if let Some(port) = args.port {
        config.port = port;
    }
    if let Some(cameras) = args.cameras.as_deref() {
        config.cameras = split_name_list(cameras);
    }
    if let Some(robots) = args.robots.as_deref() {
        config.robots = split_name_list(robots);
    }
    if let Some(max_preview_width) = args.max_preview_width {
        config.max_preview_width = max_preview_width;
    }
    if let Some(max_preview_height) = args.max_preview_height {
        config.max_preview_height = max_preview_height;
    }
    if let Some(jpeg_quality) = args.jpeg_quality {
        config.jpeg_quality = jpeg_quality;
    }
    if let Some(preview_fps) = args.preview_fps {
        config.preview_fps = preview_fps;
    }
    if let Some(preview_workers) = args.preview_workers {
        config.preview_workers = Some(preview_workers);
    }

    config.validate()?;
    Ok(config)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();
    let runtime_config = load_runtime_config(&args)?;

    let camera_names = runtime_config.cameras.clone();
    let robot_names = runtime_config.robots.clone();
    let preview_workers = runtime_config
        .preview_workers
        .unwrap_or_else(|| default_preview_workers(camera_names.len()))
        .max(1);

    log::info!(
        "cameras: {:?}, robots: {:?}, port: {}, max_preview={}x{}, jpeg_quality: {}, preview_fps: {}, preview_workers: {}",
        camera_names,
        robot_names,
        runtime_config.port,
        runtime_config.max_preview_width,
        runtime_config.max_preview_height,
        runtime_config.jpeg_quality,
        runtime_config.preview_fps,
        preview_workers,
    );

    // Broadcast channel: small capacity so slow consumers skip frames
    let (broadcast_tx, _) = broadcast::channel::<BroadcastMessage>(16);
    let (episode_command_tx, episode_command_rx) = mpsc::channel::<EpisodeCommand>();
    let stream_info = Arc::new(Mutex::new(StreamInfoRegistry::new(
        &camera_names,
        &robot_names,
        runtime_config.preview_fps,
        runtime_config.max_preview_width,
        runtime_config.max_preview_height,
        preview_workers,
        runtime_config.jpeg_quality,
    )));
    let preview_config = Arc::new(RuntimePreviewConfig::new(
        runtime_config.max_preview_width,
        runtime_config.max_preview_height,
    ));
    let latest_episode_status = Arc::new(Mutex::new(None::<String>));

    // Start WebSocket server
    let ws_addr: SocketAddr = ([0, 0, 0, 0], runtime_config.port).into();
    let ws_broadcast_tx = broadcast_tx.clone();
    let ws_stream_info = stream_info.clone();
    let ws_preview_config = preview_config.clone();
    let ws_episode_command_tx = episode_command_tx.clone();
    let ws_latest_episode_status = latest_episode_status.clone();
    tokio::spawn(async move {
        websocket::run_server(
            ws_addr,
            ws_broadcast_tx,
            ws_stream_info,
            ws_preview_config,
            ws_episode_command_tx,
            ws_latest_episode_status,
        )
        .await;
    });

    // Shared shutdown flag
    let shutdown = Arc::new(AtomicBool::new(false));

    // Run the iceoryx2 poll loop on a dedicated OS thread instead of
    // `spawn_blocking()`. Tokio waits for blocking tasks during runtime
    // shutdown, which can make Ctrl+C appear stuck if the poll loop is inside
    // a blocking iceoryx wait.
    let ipc_config = IpcPollConfig {
        jpeg_quality: runtime_config.jpeg_quality,
        preview_fps: runtime_config.preview_fps,
        preview_workers,
    };
    let ipc_broadcast_tx = broadcast_tx.clone();
    let ipc_shutdown = shutdown.clone();
    let ipc_stream_info = stream_info.clone();
    let ipc_preview_config = preview_config.clone();
    let ipc_latest_episode_status = latest_episode_status.clone();

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
                ipc_latest_episode_status,
                episode_command_rx,
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
    latest_episode_status: Arc<Mutex<Option<String>>>,
    episode_command_rx: mpsc::Receiver<EpisodeCommand>,
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
        while let Ok(command) = episode_command_rx.try_recv() {
            poller.publish_episode_command(command)?;
        }
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
                IpcMessage::EpisodeStatusMsg { status } => {
                    let json = protocol::encode_episode_status(&status);
                    if let Ok(mut latest) = latest_episode_status.lock() {
                        *latest = Some(json.clone());
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_args() -> Args {
        Args {
            config: None,
            config_inline: None,
            port: None,
            cameras: None,
            robots: None,
            max_preview_width: None,
            max_preview_height: None,
            jpeg_quality: None,
            preview_fps: None,
            preview_workers: None,
        }
    }

    #[test]
    fn legacy_runtime_defaults_match_previous_cli_behavior() {
        let config = load_runtime_config(&empty_args()).expect("legacy runtime config should load");
        assert_eq!(config.port, 9090);
        assert_eq!(
            config.cameras,
            vec!["camera_0".to_string(), "camera_1".to_string()]
        );
        assert_eq!(config.robots, vec!["robot_0".to_string()]);
    }

    #[test]
    fn config_inline_runtime_overrides_legacy_lists() {
        let mut args = empty_args();
        args.config_inline = Some(
            r#"
port = 9910
cameras = ["camera_top"]
robots = ["leader_arm", "follower_arm"]
max_preview_width = 160
max_preview_height = 90
jpeg_quality = 45
preview_fps = 15
"#
            .to_string(),
        );

        let config = load_runtime_config(&args).expect("inline runtime config should load");
        assert_eq!(config.port, 9910);
        assert_eq!(config.cameras, vec!["camera_top".to_string()]);
        assert_eq!(
            config.robots,
            vec!["leader_arm".to_string(), "follower_arm".to_string()]
        );
        assert_eq!(config.max_preview_width, 160);
        assert_eq!(config.max_preview_height, 90);
        assert_eq!(config.jpeg_quality, 45);
        assert_eq!(config.preview_fps, 15);
    }
}
