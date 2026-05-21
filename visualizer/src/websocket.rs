//! WebSocket server for the visualizer bridge.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::preview_config::RuntimePreviewConfig;
use crate::protocol;
use crate::stream_info::StreamInfoRegistry;

const WEBSOCKET_STATS_LOG_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BroadcastKind {
    JpegFrame,
    EncodedConfig,
    EncodedPacket,
    RobotState,
}

#[derive(Clone, Debug)]
struct BroadcastDiagnostics {
    kind: BroadcastKind,
    camera: Option<Arc<str>>,
    source_timestamp_us: u64,
    sequence: Option<u64>,
    payload_bytes: usize,
    enqueued_at: Instant,
}

#[derive(Clone, Debug)]
enum BroadcastPayload {
    Binary(Arc<Vec<u8>>),
    Text(Arc<String>),
}

#[derive(Clone, Debug)]
pub struct BroadcastMessage {
    payload: BroadcastPayload,
    diagnostics: BroadcastDiagnostics,
}

impl BroadcastMessage {
    pub fn jpeg_frame(
        data: Arc<Vec<u8>>,
        camera: &str,
        source_timestamp_us: u64,
        frame_index: u64,
        payload_bytes: usize,
    ) -> Self {
        Self::binary(
            data,
            BroadcastKind::JpegFrame,
            Some(camera),
            source_timestamp_us,
            Some(frame_index),
            payload_bytes,
        )
    }

    pub fn encoded_config(data: Arc<Vec<u8>>, camera: &str, payload_bytes: usize) -> Self {
        Self::binary(
            data,
            BroadcastKind::EncodedConfig,
            Some(camera),
            0,
            None,
            payload_bytes,
        )
    }

    pub fn encoded_packet(
        data: Arc<Vec<u8>>,
        camera: &str,
        source_timestamp_us: u64,
        sequence: u64,
        payload_bytes: usize,
    ) -> Self {
        Self::binary(
            data,
            BroadcastKind::EncodedPacket,
            Some(camera),
            source_timestamp_us,
            Some(sequence),
            payload_bytes,
        )
    }

    pub fn robot_state(text: Arc<String>) -> Self {
        Self {
            payload: BroadcastPayload::Text(text),
            diagnostics: BroadcastDiagnostics {
                kind: BroadcastKind::RobotState,
                camera: None,
                source_timestamp_us: 0,
                sequence: None,
                payload_bytes: 0,
                enqueued_at: Instant::now(),
            },
        }
    }

    fn binary(
        data: Arc<Vec<u8>>,
        kind: BroadcastKind,
        camera: Option<&str>,
        source_timestamp_us: u64,
        sequence: Option<u64>,
        payload_bytes: usize,
    ) -> Self {
        Self {
            payload: BroadcastPayload::Binary(data),
            diagnostics: BroadcastDiagnostics {
                kind,
                camera: camera.map(Arc::<str>::from),
                source_timestamp_us,
                sequence,
                payload_bytes,
                enqueued_at: Instant::now(),
            },
        }
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
}

#[derive(Default)]
struct WebSocketCameraStats {
    frames: u64,
    configs: u64,
    bytes: u64,
    last_sequence: Option<u64>,
    payload_bytes: MetricStats,
    enqueue_to_send_ms: MetricStats,
    ws_send_ms: MetricStats,
    source_age_at_send_ms: MetricStats,
}

struct WebSocketClientStats {
    interval_started_at: Instant,
    last_log_at: Instant,
    binary_messages: u64,
    text_messages: u64,
    bytes: u64,
    lagged_messages: u64,
    cameras: std::collections::HashMap<String, WebSocketCameraStats>,
    enqueue_to_send_ms: MetricStats,
    ws_send_ms: MetricStats,
}

impl WebSocketClientStats {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            interval_started_at: now,
            last_log_at: now,
            binary_messages: 0,
            text_messages: 0,
            bytes: 0,
            lagged_messages: 0,
            cameras: std::collections::HashMap::new(),
            enqueue_to_send_ms: MetricStats::default(),
            ws_send_ms: MetricStats::default(),
        }
    }

    fn record(&mut self, diagnostics: &BroadcastDiagnostics, data_len: usize, send_ms: f64) {
        match diagnostics.kind {
            BroadcastKind::RobotState => self.text_messages += 1,
            _ => self.binary_messages += 1,
        }
        self.bytes = self.bytes.saturating_add(data_len as u64);
        let queue_ms = diagnostics.enqueued_at.elapsed().as_secs_f64() * 1000.0;
        self.enqueue_to_send_ms.observe(queue_ms);
        self.ws_send_ms.observe(send_ms);

        let Some(camera) = diagnostics.camera.as_deref() else {
            return;
        };
        let stats = self.cameras.entry(camera.to_string()).or_default();
        match diagnostics.kind {
            BroadcastKind::EncodedConfig => stats.configs += 1,
            BroadcastKind::JpegFrame | BroadcastKind::EncodedPacket => stats.frames += 1,
            BroadcastKind::RobotState => {}
        }
        stats.bytes = stats.bytes.saturating_add(data_len as u64);
        stats
            .payload_bytes
            .observe(diagnostics.payload_bytes as f64);
        stats.enqueue_to_send_ms.observe(queue_ms);
        stats.ws_send_ms.observe(send_ms);
        if let Some(sequence) = diagnostics.sequence {
            stats.last_sequence = Some(sequence);
        }
        if diagnostics.source_timestamp_us != 0 {
            stats
                .source_age_at_send_ms
                .observe(source_age_ms(diagnostics.source_timestamp_us));
        }
    }

    fn record_lagged(&mut self, skipped: u64) {
        self.lagged_messages = self.lagged_messages.saturating_add(skipped);
    }

    fn maybe_log(&mut self, peer: SocketAddr) {
        if self.last_log_at.elapsed() < WEBSOCKET_STATS_LOG_INTERVAL {
            return;
        }
        let now = Instant::now();
        let elapsed_sec = now.duration_since(self.interval_started_at).as_secs_f64();
        for (name, stats) in &self.cameras {
            let fps = if elapsed_sec > 0.0 {
                stats.frames as f64 / elapsed_sec
            } else {
                0.0
            };
            log::info!(
                "visualizer websocket pipeline peer={} camera={} frames={} configs={} fps={:.1} \
                 bytes={} source_age_at_send_ms={} enqueue_to_send_ms={} ws_send_ms={} \
                 payload_bytes={} last_sequence={}",
                peer,
                name,
                stats.frames,
                stats.configs,
                fps,
                stats.bytes,
                stats.source_age_at_send_ms.summary(),
                stats.enqueue_to_send_ms.summary(),
                stats.ws_send_ms.summary(),
                stats.payload_bytes.summary(),
                stats
                    .last_sequence
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "n/a".to_string()),
            );
        }
        log::info!(
            "visualizer websocket pipeline summary peer={} binary={} text={} bytes={} \
             lagged={} enqueue_to_send_ms={} ws_send_ms={}",
            peer,
            self.binary_messages,
            self.text_messages,
            self.bytes,
            self.lagged_messages,
            self.enqueue_to_send_ms.summary(),
            self.ws_send_ms.summary(),
        );
        *self = Self::new();
    }
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
    advanced_logs: bool,
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
                    advanced_logs,
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
    advanced_logs: bool,
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
    let mut client_stats = WebSocketClientStats::new();

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
                                    let update = preview_config.set_requested_size(width, height);
                                    {
                                        let mut info = stream_info.lock().expect("stream info");
                                        info.set_active_preview_bounds(
                                            update.size.width,
                                            update.size.height,
                                        );
                                    }
                                    // Only forward upstream when the post-clamp dims
                                    // actually changed; otherwise the encoder would
                                    // tear down its working codec session for nothing.
                                    if update.changed {
                                        preview_control_sender(
                                            update.size.width,
                                            update.size.height,
                                        );
                                    }
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
                        let (ws_msg, data_len, diagnostics) = match msg {
                            BroadcastMessage { payload: BroadcastPayload::Binary(data), diagnostics } => {
                                let data_len = data.len();
                                (Message::Binary((*data).clone().into()), data_len, diagnostics)
                            }
                            BroadcastMessage { payload: BroadcastPayload::Text(text), diagnostics } => {
                                let data_len = text.len();
                                (Message::Text((*text).clone().into()), data_len, diagnostics)
                            }
                        };
                        let send_started = Instant::now();
                        if ws_sink.send(ws_msg).await.is_err() {
                            break;
                        }
                        if advanced_logs {
                            client_stats.record(
                                &diagnostics,
                                data_len,
                                send_started.elapsed().as_secs_f64() * 1000.0,
                            );
                            client_stats.maybe_log(peer);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        log::debug!("client {peer} lagged, skipped {n} messages");
                        if advanced_logs {
                            client_stats.record_lagged(n);
                        }
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

fn unix_now_us() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
}

fn source_age_ms(source_timestamp_us: u64) -> f64 {
    signed_delta_us(unix_now_us(), u128::from(source_timestamp_us)) as f64 / 1000.0
}

fn signed_delta_us(lhs: u128, rhs: u128) -> i128 {
    if lhs >= rhs {
        (lhs - rhs) as i128
    } else {
        -((rhs - lhs) as i128)
    }
}
