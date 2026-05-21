mod ipc;
mod preview_config;
mod protocol;
mod stream_info;
mod websocket;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

const VISUALIZER_STATS_LOG_INTERVAL: Duration = Duration::from_secs(10);

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
        &runtime_config.camera_sources,
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

#[derive(Clone, Copy, Default)]
struct MetricStats {
    count: u64,
    sum: f64,
    min: f64,
    max: f64,
}

impl MetricStats {
    fn observe(&mut self, value: f64) {
        if self.count == 0 {
            self.min = value;
            self.max = value;
        } else {
            self.min = self.min.min(value);
            self.max = self.max.max(value);
        }
        self.sum += value;
        self.count += 1;
    }

    fn avg(self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum / self.count as f64
        }
    }

    fn summary(self) -> String {
        if self.count == 0 {
            "n/a".to_string()
        } else {
            format!("{:.1}/{:.1}/{:.1}", self.avg(), self.min, self.max)
        }
    }

    fn avg_max_summary(self) -> String {
        if self.count == 0 {
            "n/a".to_string()
        } else {
            format!("{:.1}/{:.1}", self.avg(), self.max)
        }
    }
}

#[derive(Default)]
struct VisualizerCameraStats {
    frames: u64,
    configs: u64,
    bytes: u64,
    last_source_timestamp_us: Option<u64>,
    source_age_ms: MetricStats,
    source_gap_ms: MetricStats,
    bridge_ms: MetricStats,
    payload_bytes: MetricStats,
}

struct VisualizerIpcStats {
    interval_started_at: Instant,
    last_log_at: Instant,
    cameras: std::collections::HashMap<String, VisualizerCameraStats>,
    robot_messages: u64,
    poll_batch: MetricStats,
}

impl VisualizerIpcStats {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            interval_started_at: now,
            last_log_at: now,
            cameras: std::collections::HashMap::new(),
            robot_messages: 0,
            poll_batch: MetricStats::default(),
        }
    }

    fn record_poll_batch(&mut self, messages: usize) {
        if messages > 0 {
            self.poll_batch.observe(messages as f64);
        }
    }

    fn record_config(&mut self, name: &str, payload_len: usize, bridge_elapsed: Duration) {
        let stats = self.cameras.entry(name.to_string()).or_default();
        stats.configs += 1;
        stats.bytes = stats.bytes.saturating_add(payload_len as u64);
        stats.payload_bytes.observe(payload_len as f64);
        stats
            .bridge_ms
            .observe(bridge_elapsed.as_secs_f64() * 1000.0);
    }

    fn record_frame(
        &mut self,
        name: &str,
        source_timestamp_us: u64,
        payload_len: usize,
        bridge_elapsed: Duration,
    ) {
        let stats = self.cameras.entry(name.to_string()).or_default();
        stats.frames += 1;
        stats.bytes = stats.bytes.saturating_add(payload_len as u64);
        stats.payload_bytes.observe(payload_len as f64);
        stats
            .bridge_ms
            .observe(bridge_elapsed.as_secs_f64() * 1000.0);
        if source_timestamp_us != 0 {
            stats
                .source_age_ms
                .observe(source_age_ms(source_timestamp_us));
            if let Some(previous) = stats.last_source_timestamp_us.replace(source_timestamp_us) {
                stats
                    .source_gap_ms
                    .observe(timestamp_delta_ms(source_timestamp_us, previous));
            }
        }
    }

    fn record_robot_message(&mut self) {
        self.robot_messages += 1;
    }

    fn maybe_log(&mut self, output_mode: PreviewOutputMode, advanced: bool) {
        if self.last_log_at.elapsed() < VISUALIZER_STATS_LOG_INTERVAL {
            return;
        }
        let now = Instant::now();
        let elapsed_sec = now.duration_since(self.interval_started_at).as_secs_f64();
        let mut total_frames = 0u64;
        let mut total_bytes = 0u64;
        for (name, stats) in &self.cameras {
            total_frames = total_frames.saturating_add(stats.frames);
            total_bytes = total_bytes.saturating_add(stats.bytes);
            let fps = if elapsed_sec > 0.0 {
                stats.frames as f64 / elapsed_sec
            } else {
                0.0
            };
            if advanced {
                log::info!(
                    "visualizer pipeline camera={} mode={} frames={} configs={} fps={:.1} \
                     bytes={} source_age_ms={} source_gap_ms={} bridge_ms={} payload_bytes={}",
                    name,
                    output_mode.as_str(),
                    stats.frames,
                    stats.configs,
                    fps,
                    stats.bytes,
                    stats.source_age_ms.summary(),
                    stats.source_gap_ms.summary(),
                    stats.bridge_ms.summary(),
                    stats.payload_bytes.summary(),
                );
            } else {
                log::info!(
                    "visualizer summary camera={} mode={} frames={} configs={} fps={:.1} \
                     bytes={} source_age_ms={} bridge_ms={}",
                    name,
                    output_mode.as_str(),
                    stats.frames,
                    stats.configs,
                    fps,
                    stats.bytes,
                    stats.source_age_ms.avg_max_summary(),
                    stats.bridge_ms.avg_max_summary(),
                );
            }
        }
        if self.robot_messages > 0 || self.poll_batch.count > 0 {
            if advanced {
                log::info!(
                    "visualizer pipeline summary mode={} cameras={} robot_msgs={} poll_batch={}",
                    output_mode.as_str(),
                    self.cameras.len(),
                    self.robot_messages,
                    self.poll_batch.summary(),
                );
            } else {
                let total_fps = if elapsed_sec > 0.0 {
                    total_frames as f64 / elapsed_sec
                } else {
                    0.0
                };
                log::info!(
                    "visualizer summary mode={} cameras={} frames={} fps={:.1} bytes={} robot_msgs={}",
                    output_mode.as_str(),
                    self.cameras.len(),
                    total_frames,
                    total_fps,
                    total_bytes,
                    self.robot_messages,
                );
            }
        }
        let last_source_timestamps = self
            .cameras
            .iter()
            .map(|(name, stats)| (name.clone(), stats.last_source_timestamp_us))
            .collect::<Vec<_>>();
        *self = Self::new();
        self.interval_started_at = now;
        self.last_log_at = now;
        for (name, last_source_timestamp_us) in last_source_timestamps {
            self.cameras
                .entry(name)
                .or_default()
                .last_source_timestamp_us = last_source_timestamp_us;
        }
    }
}

fn unix_now_us() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
}

fn source_age_ms(source_timestamp_us: u64) -> f64 {
    signed_delta_us(unix_now_us(), u128::from(source_timestamp_us)) as f64 / 1000.0
}

fn timestamp_delta_ms(current_us: u64, previous_us: u64) -> f64 {
    signed_delta_us(u128::from(current_us), u128::from(previous_us)) as f64 / 1000.0
}

fn signed_delta_us(lhs: u128, rhs: u128) -> i128 {
    if lhs >= rhs {
        (lhs - rhs) as i128
    } else {
        -((rhs - lhs) as i128)
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
    let mut ipc_stats = VisualizerIpcStats::new();
    let advanced_logs = advanced_pipeline_logs_enabled();

    log::info!("IPC poll loop started ({})", output_mode.as_str());
    while !shutdown.load(Ordering::Relaxed) {
        if poller.poll_shutdown() {
            shutdown.store(true, Ordering::Relaxed);
            break;
        }

        // Forward any pending preview-size requests to the
        // per-camera preview encoders. Fixed-source previews (H.264
        // passthrough) keep their native coded dimensions and are scaled
        // visually by the web UI instead.
        while let Ok((width, height)) = resize_rx.try_recv() {
            for entry in &preview_publishers {
                if !entry.resizable {
                    continue;
                }
                if let Err(e) = entry
                    .publisher
                    .send_copy(PreviewControl::SetSize { width, height })
                {
                    log::warn!("preview-control send for {} failed: {e}", entry.channel_id);
                }
            }
        }

        let messages = poller.poll();
        ipc_stats.record_poll_batch(messages.len());
        for msg in messages {
            match msg {
                IpcMessage::JpegFrame {
                    name,
                    header,
                    payload,
                } => {
                    let bridge_started = Instant::now();
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
                    ipc_stats.record_frame(
                        &name,
                        header.timestamp_us,
                        payload.len(),
                        bridge_started.elapsed(),
                    );
                }
                IpcMessage::EncodedConfig {
                    name,
                    header,
                    extradata,
                } => {
                    let bridge_started = Instant::now();
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
                    let payload_len = bytes.len();
                    cache_config(&cached_configs, &name, &bytes);
                    let _ = broadcast_tx.send(BroadcastMessage::Binary(Arc::new(bytes)));
                    ipc_stats.record_config(&name, payload_len, bridge_started.elapsed());
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
                    let bridge_started = Instant::now();
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
                    let payload_len = payload_bytes.len();
                    dump_visualizer_h264_packet(&name, &payload_bytes);
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
                    ipc_stats.record_frame(
                        &name,
                        header.source_timestamp_us,
                        payload_len,
                        bridge_started.elapsed(),
                    );
                }
                IpcMessage::RobotStateMsg {
                    name,
                    state_kind,
                    timestamp_us,
                    values,
                    value_min,
                    value_max,
                } => {
                    ipc_stats.record_robot_message();
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
        ipc_stats.maybe_log(output_mode, advanced_logs);

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

fn dump_visualizer_h264_packet(name: &str, payload: &[u8]) {
    let Some(dir) = visualizer_h264_dump_dir() else {
        return;
    };
    if let Err(error) = std::fs::create_dir_all(&dir) {
        log::warn!(
            "failed to create visualizer H.264 dump dir {}: {error}",
            dir.display()
        );
        return;
    }
    let path = dir.join(format!("{}.h264", name.replace('/', "_")));
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        log::warn!("failed to open visualizer H.264 dump {}", path.display());
        return;
    };
    use std::io::Write;
    if let Err(error) = file.write_all(payload) {
        log::warn!(
            "failed to write visualizer H.264 dump {}: {error}",
            path.display()
        );
    }
}

fn visualizer_h264_dump_dir() -> Option<PathBuf> {
    match std::env::var("ROLLIO_VISUALIZER_H264_DUMP_DIR") {
        Ok(dir) if !dir.is_empty() => return Some(PathBuf::from(dir)),
        _ => {}
    }
    if !env_is_enabled("ROLLIO_VISUALIZER_H264_DUMP") {
        return None;
    }
    let log_dir = std::env::var("ROLLIO_LOG_DIR").ok()?;
    if log_dir.is_empty() {
        return None;
    }
    Some(PathBuf::from(log_dir).join("h264-visualizer"))
}

fn env_is_enabled(name: &str) -> bool {
    matches!(
        std::env::var(name).as_deref(),
        Ok(value)
            if !value.is_empty()
                && !matches!(value, "0" | "false" | "FALSE" | "off" | "OFF")
    )
}

fn advanced_pipeline_logs_enabled() -> bool {
    rollio_types::config::RuntimeConfig::advanced_pipeline_logs_enabled()
}
