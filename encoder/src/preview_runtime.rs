//! Preview-role encoder runtime.
//!
//! Always-on. Subscribes to the per-camera frame topic + the shared
//! `ControlEvent` service (only honours `Shutdown`) + the per-channel
//! `…/preview-control` topic (`PreviewControl::SetSize`).
//!
//! Output mode (jpeg vs encoded) is decided once at startup from
//! [`rollio_types::config::PreviewEncoderConfig`]:
//!
//! * **JPEG mode**: every received frame goes through
//!   [`crate::preview::PreviewBuilder`] (decode/copy → swscale to RGB24
//!   at the configured preview dims), then through
//!   [`crate::preview::JpegCompressor`] (turbojpeg), then is published
//!   on the per-channel `…/preview-jpeg` topic via
//!   [`crate::sink::IpcPreviewJpegSink`]. The visualizer's existing
//!   `CameraFrameHeader`-based plumbing handles the rest.
//!
//! * **Encoded mode**: each frame is fed into a long-lived
//!   [`crate::codec::EncoderSession`] (H.264 by default for color, RVL
//!   for depth) and published on `…/preview-config` +
//!   `…/preview-packets` via [`crate::sink::IpcPreviewPacketSink`]. On
//!   `PreviewControl::SetSize`, the session is closed (with EOS) and
//!   reopened at the new dims so the very next packet is a fresh
//!   keyframe.

use crate::codec::{open_session, CodecSessionParams, EncoderSession, OwnedFrame};
use crate::error::{map_iceoryx_error, EncoderError, Result};
use crate::preview::{JpegCompressor, PreviewBuilder};
use crate::sink::{IpcPreviewJpegSink, IpcPreviewPacketSink};
use iceoryx2::node::NodeWaitFailure;
use iceoryx2::prelude::*;
use rollio_bus::{
    CAMERA_FRAMES_MAX_SUBSCRIBERS, CONTROL_EVENTS_MAX_NODES, CONTROL_EVENTS_MAX_PUBLISHERS,
    CONTROL_EVENTS_MAX_SUBSCRIBERS, CONTROL_EVENTS_SERVICE,
};
use rollio_types::config::{
    EncoderRuntimeConfigV2, PreviewEncoderConfig, PreviewOutputMode, PreviewResizePolicy,
};
use rollio_types::messages::{CameraFrameHeader, ControlEvent, PixelFormat, PreviewControl};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Mirrors `visualizer::preview_config::MIN_PREVIEW_DIMENSION`. H.264
/// NVENC's documented per-codec minimum width is ~145 on Turing+ (and
/// AV1's is 160 on Ada+); after 16-byte alignment 160 is the smallest
/// value that works on every NVENC path we ship. Reject smaller dims
/// here so a bogus `SetSize` cannot crash the codec session at open
/// time.
const MIN_PREVIEW_DIM: u32 = 160;
/// Mirrors `visualizer::preview_config::PREVIEW_DIMENSION_ALIGNMENT`.
const PREVIEW_DIM_ALIGNMENT: u32 = 16;
const PREVIEW_STATS_LOG_INTERVAL: Duration = Duration::from_secs(10);

fn is_valid_preview_dim(value: u32) -> bool {
    value >= MIN_PREVIEW_DIM && value.is_multiple_of(PREVIEW_DIM_ALIGNMENT)
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

struct PreviewRuntimeStats {
    interval_started_at: Instant,
    last_log_at: Instant,
    last_receive_at: Option<Instant>,
    last_source_timestamp_us: Option<u64>,
    frames_received: u64,
    frames_processed: u64,
    frames_collapsed: u64,
    frames_order_preserved: u64,
    errors: u64,
    accepted_resizes: u64,
    rejected_resizes: u64,
    payload_bytes: MetricStats,
    receive_gap_ms: MetricStats,
    source_gap_ms: MetricStats,
    source_age_ms: MetricStats,
    handle_ms: MetricStats,
    drain_batch: MetricStats,
}

impl PreviewRuntimeStats {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            interval_started_at: now,
            last_log_at: now,
            last_receive_at: None,
            last_source_timestamp_us: None,
            frames_received: 0,
            frames_processed: 0,
            frames_collapsed: 0,
            frames_order_preserved: 0,
            errors: 0,
            accepted_resizes: 0,
            rejected_resizes: 0,
            payload_bytes: MetricStats::default(),
            receive_gap_ms: MetricStats::default(),
            source_gap_ms: MetricStats::default(),
            source_age_ms: MetricStats::default(),
            handle_ms: MetricStats::default(),
            drain_batch: MetricStats::default(),
        }
    }

    fn record_received(&mut self, frame: &OwnedFrame, receive_at: Instant, order_preserved: bool) {
        self.frames_received += 1;
        if order_preserved {
            self.frames_order_preserved += 1;
        }
        self.payload_bytes.observe(frame.payload.len() as f64);
        if let Some(last) = self.last_receive_at.replace(receive_at) {
            self.receive_gap_ms
                .observe(receive_at.duration_since(last).as_secs_f64() * 1000.0);
        }
        if frame.header.timestamp_us != 0 {
            self.source_age_ms
                .observe(source_age_ms(frame.header.timestamp_us));
            if let Some(last) = self
                .last_source_timestamp_us
                .replace(frame.header.timestamp_us)
            {
                self.source_gap_ms
                    .observe(timestamp_delta_ms(frame.header.timestamp_us, last));
            }
        }
    }

    fn record_processed(&mut self, elapsed: Duration, ok: bool) {
        self.frames_processed += 1;
        if !ok {
            self.errors += 1;
        }
        self.handle_ms.observe(elapsed.as_secs_f64() * 1000.0);
    }

    fn record_collapsed(&mut self) {
        self.frames_collapsed += 1;
    }

    fn record_batch(&mut self, frames: usize) {
        if frames > 0 {
            self.drain_batch.observe(frames as f64);
        }
    }

    fn record_resize(&mut self, accepted: bool) {
        if accepted {
            self.accepted_resizes += 1;
        } else {
            self.rejected_resizes += 1;
        }
    }

    fn maybe_log(
        &mut self,
        config: &EncoderRuntimeConfigV2,
        preview: &PreviewEncoderConfig,
        dims: (u32, u32),
        advanced: bool,
    ) {
        if self.last_log_at.elapsed() < PREVIEW_STATS_LOG_INTERVAL {
            return;
        }
        let now = Instant::now();
        let elapsed_sec = now.duration_since(self.interval_started_at).as_secs_f64();
        let recv_fps = rate(self.frames_received, elapsed_sec);
        let processed_fps = rate(self.frames_processed, elapsed_sec);
        if advanced {
            eprintln!(
                "rollio-encoder: preview pipeline process={} channel={} mode={} dims={}x{} \
                 recv={} processed={} collapsed={} ordered={} errors={} resizes={}/{} \
                 recv_fps={:.1} processed_fps={:.1} payload_bytes={} source_age_ms={} \
                 source_gap_ms={} receive_gap_ms={} handle_ms={} drain_batch={}",
                config.process_id,
                config.channel_id,
                preview.output_mode.as_str(),
                dims.0,
                dims.1,
                self.frames_received,
                self.frames_processed,
                self.frames_collapsed,
                self.frames_order_preserved,
                self.errors,
                self.accepted_resizes,
                self.rejected_resizes,
                recv_fps,
                processed_fps,
                self.payload_bytes.summary(),
                self.source_age_ms.summary(),
                self.source_gap_ms.summary(),
                self.receive_gap_ms.summary(),
                self.handle_ms.summary(),
                self.drain_batch.summary(),
            );
        } else {
            eprintln!(
                "rollio-encoder: preview summary process={} channel={} mode={} dims={}x{} \
                 recv={} processed={} errors={} recv_fps={:.1} processed_fps={:.1} \
                 source_age_ms={} handle_ms={}",
                config.process_id,
                config.channel_id,
                preview.output_mode.as_str(),
                dims.0,
                dims.1,
                self.frames_received,
                self.frames_processed,
                self.errors,
                recv_fps,
                processed_fps,
                self.source_age_ms.avg_max_summary(),
                self.handle_ms.avg_max_summary(),
            );
        }
        let last_receive_at = self.last_receive_at;
        let last_source_timestamp_us = self.last_source_timestamp_us;
        *self = Self::new();
        self.last_receive_at = last_receive_at;
        self.last_source_timestamp_us = last_source_timestamp_us;
        self.interval_started_at = now;
        self.last_log_at = now;
    }
}

fn rate(count: u64, elapsed_sec: f64) -> f64 {
    if elapsed_sec > 0.0 {
        count as f64 / elapsed_sec
    } else {
        0.0
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

fn advanced_pipeline_logs_enabled() -> bool {
    rollio_types::config::RuntimeConfig::advanced_pipeline_logs_enabled()
}

pub fn run(config: EncoderRuntimeConfigV2) -> Result<()> {
    let preview = config
        .preview
        .clone()
        .ok_or_else(|| EncoderError::message("preview-role config missing [preview] block"))?;

    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()
        .map_err(map_iceoryx_error)?;

    let frame_service_name: ServiceName = config
        .frame_topic
        .as_str()
        .try_into()
        .map_err(map_iceoryx_error)?;
    let frame_service = node
        .service_builder(&frame_service_name)
        .publish_subscribe::<[u8]>()
        .user_header::<CameraFrameHeader>()
        .max_subscribers(CAMERA_FRAMES_MAX_SUBSCRIBERS)
        .open_or_create()
        .map_err(map_iceoryx_error)?;
    let frame_subscriber = frame_service
        .subscriber_builder()
        .create()
        .map_err(map_iceoryx_error)?;

    let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE
        .try_into()
        .map_err(map_iceoryx_error)?;
    let control_service = node
        .service_builder(&control_service_name)
        .publish_subscribe::<ControlEvent>()
        .max_publishers(CONTROL_EVENTS_MAX_PUBLISHERS)
        .max_subscribers(CONTROL_EVENTS_MAX_SUBSCRIBERS)
        .max_nodes(CONTROL_EVENTS_MAX_NODES)
        .open_or_create()
        .map_err(map_iceoryx_error)?;
    let control_subscriber = control_service
        .subscriber_builder()
        .create()
        .map_err(map_iceoryx_error)?;

    let preview_control_service_name: ServiceName = preview
        .control_topic
        .as_str()
        .try_into()
        .map_err(map_iceoryx_error)?;
    let preview_control_service = node
        .service_builder(&preview_control_service_name)
        .publish_subscribe::<PreviewControl>()
        .open_or_create()
        .map_err(map_iceoryx_error)?;
    let preview_control_subscriber = preview_control_service
        .subscriber_builder()
        .create()
        .map_err(map_iceoryx_error)?;

    let mut state = match preview.output_mode {
        PreviewOutputMode::Jpeg => PreviewState::open_jpeg(&node, &config, &preview)?,
        PreviewOutputMode::Encoded => PreviewState::open_encoded(&node, &config, &preview)?,
    };
    let mut stats = PreviewRuntimeStats::new();
    let advanced_logs = advanced_pipeline_logs_enabled();

    let mut shutdown = false;
    let mut last_error_message: Option<String> = None;
    while !shutdown {
        // Drain control-plane events.
        while let Some(sample) = control_subscriber.receive().map_err(map_iceoryx_error)? {
            if matches!(sample.payload(), ControlEvent::Shutdown) {
                shutdown = true;
            }
        }

        // Drain preview-control events; on a size change, reopen the
        // current preview producer at the new dims. Bad dims (below the
        // encoder floor, not 16-aligned) are rejected here so a noisy
        // SetSize stream — e.g. a UI publishing raw DOM CSS pixels
        // before layout settles — cannot tear down a working codec
        // session and crash on re-open.
        let mut new_dims: Option<(u32, u32)> = None;
        while let Some(sample) = preview_control_subscriber
            .receive()
            .map_err(map_iceoryx_error)?
        {
            let PreviewControl::SetSize { width, height } = *sample.payload();
            if preview.resize_policy == PreviewResizePolicy::FixedSource {
                continue;
            }
            if !is_valid_preview_dim(width) || !is_valid_preview_dim(height) {
                stats.record_resize(false);
                eprintln!(
                    "rollio-encoder: ignoring SetSize {}x{} on {} \
                     (each dim must be >= {} and a multiple of {})",
                    width, height, config.channel_id, MIN_PREVIEW_DIM, PREVIEW_DIM_ALIGNMENT,
                );
                continue;
            }
            if (width, height) == state.current_dims() {
                continue;
            }
            stats.record_resize(true);
            new_dims = Some((width, height));
        }
        if let Some((w, h)) = new_dims {
            state.resize(&config, &preview, w, h)?;
        }

        // Process whatever frames are queued. Most preview modes are
        // best-effort, so one polling pass keeps only the latest frame.
        // H.264 Annex-B passthrough is different: delta frames depend on
        // earlier frames, so dropping queued P-frames can freeze the browser
        // decoder until the next IDR. Preserve queue order for that case.
        let mut latest: Option<OwnedFrame> = None;
        let mut drained_frames = 0usize;
        while let Some(sample) = frame_subscriber.receive().map_err(map_iceoryx_error)? {
            let owned = OwnedFrame {
                header: *sample.user_header(),
                payload: sample.payload().to_vec(),
            };
            drained_frames += 1;
            let order_preserved = state.preserves_queue_order_for(&owned);
            stats.record_received(&owned, Instant::now(), order_preserved);
            if order_preserved {
                if let Some(frame) = latest.take() {
                    process_preview_frame(
                        &mut state,
                        &config,
                        &preview,
                        &frame,
                        &mut last_error_message,
                        &mut stats,
                    );
                }
                process_preview_frame(
                    &mut state,
                    &config,
                    &preview,
                    &owned,
                    &mut last_error_message,
                    &mut stats,
                );
            } else {
                if latest.is_some() {
                    stats.record_collapsed();
                }
                latest = Some(owned);
            }
        }
        if let Some(frame) = latest {
            process_preview_frame(
                &mut state,
                &config,
                &preview,
                &frame,
                &mut last_error_message,
                &mut stats,
            );
        }
        stats.record_batch(drained_frames);
        stats.maybe_log(&config, &preview, state.current_dims(), advanced_logs);

        // Event-driven wait: blocks until *any* of the subscribed
        // services (frames, control, preview-control) has a new sample,
        // or the timeout elapses. Replaces the previous 2 ms busy-poll
        // which woke up ~500x/s even though a 30 fps camera produces a
        // frame every ~33 ms — the wasted wakeups were eating ~10-15%
        // of one core in `recv()` syscalls and iceoryx2 polling. The
        // 33 ms cap matches the camera's frame interval, so an idle
        // stream still drains control events at least once per frame.
        match node.wait(Duration::from_millis(33)) {
            Ok(()) => {}
            Err(NodeWaitFailure::Interrupt | NodeWaitFailure::TerminationRequest) => {
                break;
            }
        }
    }

    state.shutdown();
    Ok(())
}

fn process_preview_frame(
    state: &mut PreviewState,
    config: &EncoderRuntimeConfigV2,
    preview: &PreviewEncoderConfig,
    frame: &OwnedFrame,
    last_error_message: &mut Option<String>,
    stats: &mut PreviewRuntimeStats,
) {
    let started_at = Instant::now();
    let ok = match state.handle_frame(config, preview, frame) {
        Ok(()) => {
            *last_error_message = None;
            true
        }
        Err(error) => {
            let msg = format!(
                "rollio-encoder: preview frame failed for process={} channel={}: {error}",
                config.process_id, config.channel_id
            );
            if last_error_message.as_deref() != Some(msg.as_str()) {
                eprintln!("{msg}");
                *last_error_message = Some(msg);
            }
            false
        }
    };
    stats.record_processed(started_at.elapsed(), ok);
}

/// One of the two preview output modes; bundles the active producer
/// pieces (jpeg vs encoded) so the polling loop above is uniform.
enum PreviewState {
    Jpeg {
        builder: PreviewBuilder,
        compressor: JpegCompressor,
        sink: IpcPreviewJpegSink,
    },
    Encoded {
        sink: IpcPreviewPacketSink,
        session: Option<EncoderSession>,
        /// Owned mutable copy of the preview block so `set_preview_size`
        /// can update `width`/`height` and the next `handle_frame`
        /// reopens the session at the new dims.
        preview: PreviewEncoderConfig,
    },
}

impl PreviewState {
    fn preserves_queue_order_for(&self, frame: &OwnedFrame) -> bool {
        matches!(self, Self::Encoded { .. }) && frame.header.pixel_format == PixelFormat::H264AnnexB
    }

    /// Currently configured `(width, height)` of the producer. Used by
    /// the run loop to drop noop `SetSize` requests before they tear
    /// down a working codec session.
    fn current_dims(&self) -> (u32, u32) {
        match self {
            Self::Jpeg { builder, .. } => (builder.output_width(), builder.output_height()),
            Self::Encoded { preview, .. } => (preview.width, preview.height),
        }
    }

    fn open_jpeg(
        node: &Node<ipc::Service>,
        _config: &EncoderRuntimeConfigV2,
        preview: &PreviewEncoderConfig,
    ) -> Result<Self> {
        let topic = preview
            .jpeg_topic
            .as_deref()
            .ok_or_else(|| EncoderError::message("preview jpeg mode requires jpeg_topic"))?;
        let sink = IpcPreviewJpegSink::open(node, topic, 8 * 1024 * 1024)?;
        let builder = PreviewBuilder::new(preview.width, preview.height, preview.fps);
        let compressor = JpegCompressor::new(preview.jpeg_quality)?;
        Ok(Self::Jpeg {
            builder,
            compressor,
            sink,
        })
    }

    fn open_encoded(
        node: &Node<ipc::Service>,
        _config: &EncoderRuntimeConfigV2,
        preview: &PreviewEncoderConfig,
    ) -> Result<Self> {
        let config_topic = preview
            .config_topic
            .as_deref()
            .ok_or_else(|| EncoderError::message("preview encoded mode requires config_topic"))?;
        let packet_topic = preview
            .packet_topic
            .as_deref()
            .ok_or_else(|| EncoderError::message("preview encoded mode requires packet_topic"))?;
        let sink = IpcPreviewPacketSink::open(node, config_topic, packet_topic, 8 * 1024 * 1024)?;
        Ok(Self::Encoded {
            sink,
            session: None,
            preview: preview.clone(),
        })
    }

    fn handle_frame(
        &mut self,
        config: &EncoderRuntimeConfigV2,
        _preview_block: &PreviewEncoderConfig,
        frame: &OwnedFrame,
    ) -> Result<()> {
        match self {
            Self::Jpeg {
                builder,
                compressor,
                sink,
            } => {
                let Some(built) = builder.build(frame)? else {
                    return Ok(());
                };
                let compressed = compressor.compress(
                    &built.rgb,
                    built.width,
                    built.height,
                    built.width,
                    built.height,
                )?;
                use crate::codec::EncodedPacketSink;
                use rollio_types::messages::{
                    EncodedCodecId, EncodedPacketHeader, EncodedPacketKind,
                };
                let header = EncodedPacketHeader {
                    kind: EncodedPacketKind::Packet,
                    codec: EncodedCodecId::Mjpg,
                    flags: rollio_types::messages::ENCODED_PACKET_FLAG_KEYFRAME,
                    width: compressed.width,
                    height: compressed.height,
                    pixel_format: PixelFormat::Mjpeg,
                    _reserved0: 0,
                    time_base_num: 1,
                    time_base_den: 1_000_000,
                    pts_us: 0,
                    dts_us: 0,
                    duration_us: 0,
                    sequence_number: 0,
                    source_timestamp_us: built.timestamp_us,
                    source_frame_index: built.frame_index,
                    episode_index: 0,
                    payload_len: compressed.jpeg_data.len() as u32,
                };
                sink.write_packet(header, compressed.jpeg_data)?;
                Ok(())
            }
            Self::Encoded {
                sink,
                session,
                preview,
            } => {
                if session.is_none() {
                    let codec = if frame.header.pixel_format == PixelFormat::Depth16 {
                        preview.depth_codec
                    } else {
                        preview.color_codec
                    };
                    // The libav preview session opens at the preview
                    // dims and downscales arbitrary camera-native
                    // source dims internally via swscale (see
                    // `LibavCodecSession::ensure_scaler`). RVL keeps
                    // the strict-dim contract — depth previews must
                    // already match preview dims, which the depth
                    // pipeline guarantees.
                    let params = CodecSessionParams {
                        codec,
                        backend: preview.backend,
                        fps: preview.fps,
                        crf: preview.crf,
                        preset: None,
                        tune: None,
                        bit_depth: 8,
                        chroma_subsampling: rollio_types::config::ChromaSubsampling::S420,
                        color_space: rollio_types::config::EncoderColorSpace::Auto,
                        process_id: &config.process_id,
                        episode_index: 0,
                        recording_start_us: frame.header.timestamp_us,
                        output_width: preview.width,
                        output_height: preview.height,
                        allow_rescale: codec != rollio_types::config::EncoderCodec::Rvl,
                    };
                    *session = Some(open_session(params, frame)?);
                }
                if let Some(s) = session.as_mut() {
                    s.encode(frame, sink)?;
                }
                Ok(())
            }
        }
    }

    fn resize(
        &mut self,
        _config: &EncoderRuntimeConfigV2,
        _preview_block: &PreviewEncoderConfig,
        width: u32,
        height: u32,
    ) -> Result<()> {
        match self {
            Self::Jpeg { builder, .. } => {
                builder.set_output_dims(width, height);
                Ok(())
            }
            Self::Encoded {
                sink,
                session,
                preview,
            } => {
                if preview.resize_policy == PreviewResizePolicy::FixedSource {
                    return Ok(());
                }
                preview.width = width;
                preview.height = height;
                if let Some(s) = session.take() {
                    let _ = s.finish(sink);
                }
                Ok(())
            }
        }
    }

    fn shutdown(self) {
        match self {
            Self::Jpeg { .. } => {}
            Self::Encoded {
                mut sink, session, ..
            } => {
                if let Some(s) = session {
                    let _ = s.finish(&mut sink);
                }
            }
        }
    }
}
