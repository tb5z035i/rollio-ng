mod ipc;
mod preview_config;
mod protocol;
mod stream_info;
mod websocket;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use iceoryx2::node::NodeWaitFailure;
use rollio_types::config::{
    PreviewOutputMode, VisualizerCameraSourceConfig, VisualizerRobotSourceConfig,
    VisualizerRuntimeConfigV2,
};
use rollio_types::messages::{EncodedPacketKind, PreviewControl};
use tokio::sync::broadcast;

use crate::ipc::{IpcMessage, IpcPoller};
use crate::preview_config::RuntimePreviewConfig;
use crate::stream_info::StreamInfoRegistry;
use crate::websocket::BroadcastMessage;

#[derive(Parser, Debug)]
#[command(name = "rollio-visualizer")]
#[command(about = "iceoryx2 -> WebSocket bridge for the encoder's preview output")]
struct Args {
    #[arg(long, value_name = "PATH", conflicts_with = "config_inline")]
    config: Option<PathBuf>,
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    config_inline: Option<String>,
    #[arg(long)]
    port: Option<u16>,
}

fn legacy_runtime_config(args: &Args) -> VisualizerRuntimeConfigV2 {
    VisualizerRuntimeConfigV2 {
        port: args.port.unwrap_or(19090),
        camera_sources: Vec::new(),
        robot_sources: Vec::new(),
        preview_output_mode: PreviewOutputMode::default(),
    }
}

fn load_runtime_config(
    args: &Args,
) -> Result<VisualizerRuntimeConfigV2, Box<dyn std::error::Error>> {
    let mut config = if let Some(config_path) = &args.config {
        std::fs::read_to_string(config_path)?.parse::<VisualizerRuntimeConfigV2>()?
    } else if let Some(config_inline) = &args.config_inline {
        config_inline.parse::<VisualizerRuntimeConfigV2>()?
    } else {
        legacy_runtime_config(args)
    };
    if let Some(port) = args.port {
        config.port = port;
    }
    config.validate()?;
    Ok(config)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();
    let runtime_config = load_runtime_config(&args)?;
    let camera_names = runtime_config
        .camera_sources
        .iter()
        .map(|s| s.channel_id.clone())
        .collect::<Vec<_>>();
    let robot_names = {
        let mut seen = std::collections::HashSet::new();
        let mut names = Vec::new();
        for source in &runtime_config.robot_sources {
            if seen.insert(source.channel_id.clone()) {
                names.push(source.channel_id.clone());
            }
        }
        names
    };
    log::info!(
        "cameras: {:?}, robots: {:?}, port: {}, output_mode: {}",
        camera_names,
        robot_names,
        runtime_config.port,
        runtime_config.preview_output_mode.as_str(),
    );

    let (broadcast_tx, _) = broadcast::channel::<BroadcastMessage>(64);
    let stream_info = Arc::new(Mutex::new(StreamInfoRegistry::new(
        &camera_names,
        &robot_names,
        match runtime_config.preview_output_mode {
            PreviewOutputMode::Jpeg => "jpeg",
            PreviewOutputMode::Encoded => "encoded",
        },
        320,
        240,
    )));
    let preview_config = Arc::new(RuntimePreviewConfig::new(320, 240));

    // Cached encoded-config payloads (one per camera) so newly-connected
    // WS clients can configure their decoder without waiting for the
    // next session restart.
    let cached_configs: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));

    let ws_addr: SocketAddr = ([0, 0, 0, 0], runtime_config.port).into();
    let ws_broadcast_tx = broadcast_tx.clone();
    let ws_stream_info = stream_info.clone();
    let ws_preview_config = preview_config.clone();
    let ws_cached_configs = cached_configs.clone();

    // The IPC thread owns the per-camera PreviewControl publishers
    // (they are not Send). Route `set_preview_size` from the WS
    // handler back to the IPC thread via an mpsc channel; the IPC
    // thread drains the channel each tick and fans the request out
    // to every per-camera publisher.
    let (resize_tx, resize_rx) = std::sync::mpsc::channel::<(u32, u32)>();
    let resize_tx_for_ws = std::sync::Mutex::new(resize_tx);
    let preview_control_sender: crate::websocket::PreviewControlSender =
        Arc::new(move |width, height| {
            if let Err(e) = resize_tx_for_ws
                .lock()
                .expect("resize tx mutex poisoned")
                .send((width, height))
            {
                log::warn!("preview-control resize forward failed: {e}");
            }
        });

    tokio::spawn(async move {
        crate::websocket::run_server(
            ws_addr,
            ws_broadcast_tx,
            ws_stream_info,
            ws_preview_config,
            preview_control_sender,
            ws_cached_configs,
        )
        .await;
    });

    let shutdown = Arc::new(AtomicBool::new(false));
    let ipc_shutdown = shutdown.clone();
    let ipc_broadcast_tx = broadcast_tx.clone();
    let ipc_stream_info = stream_info.clone();
    let ipc_cached_configs = cached_configs.clone();
    let ipc_camera_sources = runtime_config.camera_sources.clone();
    let ipc_robot_sources = runtime_config.robot_sources.clone();
    let ipc_output_mode = runtime_config.preview_output_mode;

    std::thread::Builder::new()
        .name("rollio-visualizer-ipc".to_string())
        .spawn(move || {
            if let Err(e) = ipc_poll_loop(
                &ipc_camera_sources,
                &ipc_robot_sources,
                ipc_output_mode,
                ipc_broadcast_tx,
                ipc_stream_info,
                ipc_cached_configs,
                resize_rx,
                &ipc_shutdown,
            ) {
                log::error!("IPC poll loop failed: {e}");
            }
        })?;

    let shutdown_clone = shutdown.clone();
    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            if let Err(e) = result {
                log::warn!("ctrl_c handler failed: {e}");
            }
            log::info!("shutting down on Ctrl+C");
        }
        _ = wait_for_shutdown(shutdown_clone) => {
            log::info!("shutting down on ControlEvent::Shutdown");
        }
    }
    shutdown.store(true, Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(())
}

async fn wait_for_shutdown(shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[allow(clippy::too_many_arguments)]
fn ipc_poll_loop(
    camera_sources: &[VisualizerCameraSourceConfig],
    robot_sources: &[VisualizerRobotSourceConfig],
    output_mode: PreviewOutputMode,
    broadcast_tx: broadcast::Sender<BroadcastMessage>,
    stream_info: Arc<Mutex<StreamInfoRegistry>>,
    cached_configs: Arc<Mutex<Vec<Vec<u8>>>>,
    resize_rx: std::sync::mpsc::Receiver<(u32, u32)>,
    shutdown: &AtomicBool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut poller = IpcPoller::new(camera_sources, robot_sources, output_mode)?;
    let preview_publishers = poller.take_preview_publishers();

    // Per-camera Annex B SPS/PPS bytes, as captured from the encoder's
    // most-recent `EncodedConfig`. Prepended to every keyframe packet
    // we broadcast so WebCodecs (configured in Annex B mode — no
    // `description`) sees in-band parameter sets on every IDR. The
    // encoder still ships GLOBAL_HEADER (recording needs it for mp4
    // muxing), so without this prepend the bitstream would lack
    // SPS/PPS and the browser decoder would never produce output.
    let mut annex_b_params: std::collections::HashMap<String, Vec<u8>> =
        std::collections::HashMap::new();

    log::info!("IPC poll loop started ({})", output_mode.as_str());
    while !shutdown.load(Ordering::Relaxed) {
        if poller.poll_shutdown() {
            shutdown.store(true, Ordering::Relaxed);
            break;
        }

        // Forward any pending preview-size requests to the
        // per-camera preview encoders.
        while let Ok((width, height)) = resize_rx.try_recv() {
            for entry in &preview_publishers {
                if let Err(e) = entry
                    .publisher
                    .send_copy(PreviewControl::SetSize { width, height })
                {
                    log::warn!("preview-control send for {} failed: {e}", entry.channel_id);
                }
            }
        }

        for msg in poller.poll() {
            match msg {
                IpcMessage::JpegFrame {
                    name,
                    header,
                    payload,
                } => {
                    if let Ok(mut info) = stream_info.lock() {
                        info.observe_jpeg_frame(
                            &name,
                            header.width,
                            header.height,
                            header.timestamp_us,
                            header.frame_index,
                            payload.len(),
                        );
                    }
                    let bytes = protocol::encode_jpeg_frame(
                        &name,
                        header.timestamp_us,
                        header.frame_index,
                        header.width,
                        header.height,
                        &payload,
                    );
                    let _ = broadcast_tx.send(BroadcastMessage::Binary(Arc::new(bytes)));
                }
                IpcMessage::EncodedConfig {
                    name,
                    header,
                    extradata,
                } => {
                    // Annex B passthrough: ship the encoder's
                    // start-code-prefixed SPS/PPS bytes verbatim, and
                    // cache them so we can re-insert them ahead of
                    // every keyframe's slice NALUs below. The
                    // encoder still uses GLOBAL_HEADER (the
                    // recording role needs it for mp4 muxing), so
                    // these bytes are the only place SPS/PPS live
                    // until we splice them back in.
                    annex_b_params.insert(name.clone(), extradata.clone());
                    let codec_id = header.codec as u8;
                    let bytes = protocol::encode_stream_config(
                        &name,
                        codec_id,
                        header.width,
                        header.height,
                        &extradata,
                    );
                    cache_config(&cached_configs, &name, &bytes);
                    let _ = broadcast_tx.send(BroadcastMessage::Binary(Arc::new(bytes)));
                }
                IpcMessage::EncodedPacket {
                    name,
                    header,
                    payload,
                } => {
                    if matches!(header.kind, EncodedPacketKind::EndOfStream) {
                        // Don't broadcast EOS to UI clients; encoded
                        // mode just ignores it for now.
                        continue;
                    }
                    if let Ok(mut info) = stream_info.lock() {
                        info.observe_encoded_packet(&name, &header, payload.len());
                    }
                    let codec_id = header.codec as u8;
                    let flags = (header.is_keyframe() as u8) & 0x01;
                    // Annex B passthrough: ship the encoder's AU
                    // bytes to the browser verbatim. For keyframes,
                    // prepend the cached Annex B SPS/PPS so the
                    // WebCodecs decoder (configured without an AVCC
                    // `description`) finds in-band parameter sets on
                    // every IDR. The encoder still uses GLOBAL_HEADER
                    // for the recording role, so without this prepend
                    // the live bitstream would lack SPS/PPS entirely.
                    let payload_bytes: std::borrow::Cow<'_, [u8]> = if header.is_keyframe() {
                        match annex_b_params.get(&name) {
                            Some(extra) if !extra.is_empty() => {
                                let mut combined = Vec::with_capacity(extra.len() + payload.len());
                                combined.extend_from_slice(extra);
                                combined.extend_from_slice(&payload);
                                std::borrow::Cow::Owned(combined)
                            }
                            _ => std::borrow::Cow::Borrowed(&payload[..]),
                        }
                    } else {
                        std::borrow::Cow::Borrowed(&payload[..])
                    };
                    let bytes = protocol::encode_packet(
                        &name,
                        codec_id,
                        flags,
                        header.pts_us,
                        header.sequence_number,
                        header.source_timestamp_us,
                        &payload_bytes,
                    );
                    let _ = broadcast_tx.send(BroadcastMessage::Binary(Arc::new(bytes)));
                }
                IpcMessage::RobotStateMsg {
                    name,
                    state_kind,
                    timestamp_us,
                    values,
                    value_min,
                    value_max,
                } => {
                    let json = protocol::encode_robot_state(
                        &name,
                        timestamp_us,
                        &values,
                        state_kind.topic_suffix(),
                        &value_min,
                        &value_max,
                    );
                    let _ = broadcast_tx.send(BroadcastMessage::Text(Arc::new(json)));
                }
            }
        }

        match poller.node().wait(Duration::from_millis(1)) {
            Ok(()) => {}
            Err(NodeWaitFailure::Interrupt | NodeWaitFailure::TerminationRequest) => {
                log::info!("IPC poll loop interrupted");
                break;
            }
        }
    }

    log::info!("IPC poll loop stopped");
    Ok(())
}

fn cache_config(cache: &Arc<Mutex<Vec<Vec<u8>>>>, name: &str, bytes: &[u8]) {
    let mut guard = cache.lock().expect("cached configs mutex poisoned");
    let name_bytes = name.as_bytes();
    let entry_matches = |entry: &Vec<u8>| -> bool {
        if entry.len() < 3 + name_bytes.len() {
            return false;
        }
        let entry_name_len = u16::from_le_bytes([entry[1], entry[2]]) as usize;
        if entry_name_len != name_bytes.len() {
            return false;
        }
        &entry[3..3 + entry_name_len] == name_bytes
    };
    guard.retain(|entry| !entry_matches(entry));
    guard.push(bytes.to_vec());
}
