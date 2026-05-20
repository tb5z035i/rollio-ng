//! Recording-role encoder runtime.
//!
//! Subscribes to the per-camera frame topic + the project's
//! ControlEvent service, opens a [`crate::codec::EncoderSession`] on
//! `RecordingStart`, ships the resulting packets through an
//! [`crate::sink::IpcRecordingSink`], and emits an `EndOfStream`
//! packet on `RecordingStop` (or when the encoder process is being
//! shut down).
//!
//! There is no longer any file output: the assembler subscribes to
//! the same `…/recording-config` + `…/recording-packets` topics this
//! runtime publishes and muxes the final video container itself.

use crate::codec::{
    open_session, CodecSessionParams, EncodedPacketSink, EncoderSession, OwnedFrame,
};
use crate::error::{map_iceoryx_error, EncoderError, Result};
use crate::sink::IpcRecordingSink;
use iceoryx2::prelude::*;
use rollio_bus::{BACKPRESSURE_SERVICE, CAMERA_FRAMES_MAX_SUBSCRIBERS, CONTROL_EVENTS_SERVICE};
use rollio_types::config::EncoderRuntimeConfigV2;
use rollio_types::messages::{
    BackpressureEvent, CameraFrameHeader, ControlEvent, EncodedPacketHeader, EncodedPacketKind,
    FixedString64,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const RECORDING_STATS_LOG_INTERVAL: Duration = Duration::from_secs(10);

enum WorkerControl {
    RecordingStart {
        episode_index: u32,
        controller_ts_us: u64,
    },
    RecordingStop,
    DroppedFrame,
    Shutdown,
}

enum WorkerEvent {
    Error(String),
    ShutdownComplete,
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

struct RecordingIngestStats {
    interval_started_at: Instant,
    last_log_at: Instant,
    last_receive_at: Option<Instant>,
    last_source_timestamp_us: Option<u64>,
    frames_received: u64,
    frames_inactive: u64,
    frames_queued: u64,
    frames_dropped: u64,
    total_queued: u64,
    queue_depth_high_watermark: u64,
    payload_bytes: MetricStats,
    receive_gap_ms: MetricStats,
    source_gap_ms: MetricStats,
    source_age_ms: MetricStats,
}

impl RecordingIngestStats {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            interval_started_at: now,
            last_log_at: now,
            last_receive_at: None,
            last_source_timestamp_us: None,
            frames_received: 0,
            frames_inactive: 0,
            frames_queued: 0,
            frames_dropped: 0,
            total_queued: 0,
            queue_depth_high_watermark: 0,
            payload_bytes: MetricStats::default(),
            receive_gap_ms: MetricStats::default(),
            source_gap_ms: MetricStats::default(),
            source_age_ms: MetricStats::default(),
        }
    }

    fn record_received(
        &mut self,
        header: &CameraFrameHeader,
        payload_len: usize,
        receive_at: Instant,
        recording_active: bool,
    ) {
        self.frames_received += 1;
        if !recording_active {
            self.frames_inactive += 1;
            return;
        }
        self.payload_bytes.observe(payload_len as f64);
        if let Some(last) = self.last_receive_at.replace(receive_at) {
            self.receive_gap_ms
                .observe(receive_at.duration_since(last).as_secs_f64() * 1000.0);
        }
        if header.timestamp_us != 0 {
            self.source_age_ms
                .observe(source_age_ms(header.timestamp_us));
            if let Some(last) = self.last_source_timestamp_us.replace(header.timestamp_us) {
                self.source_gap_ms
                    .observe(timestamp_delta_ms(header.timestamp_us, last));
            }
        }
    }

    fn record_queued(&mut self, dequeued_total: u64) {
        self.frames_queued += 1;
        self.total_queued += 1;
        self.observe_queue_depth(dequeued_total);
    }

    fn record_dropped(&mut self, dequeued_total: u64) {
        self.frames_dropped += 1;
        self.observe_queue_depth(dequeued_total);
    }

    fn observe_queue_depth(&mut self, dequeued_total: u64) {
        let depth = self.total_queued.saturating_sub(dequeued_total);
        self.queue_depth_high_watermark = self.queue_depth_high_watermark.max(depth);
    }

    fn maybe_log(
        &mut self,
        config: &EncoderRuntimeConfigV2,
        recording_active: bool,
        dequeued_total: u64,
    ) {
        if self.last_log_at.elapsed() < RECORDING_STATS_LOG_INTERVAL {
            return;
        }
        let now = Instant::now();
        let elapsed_sec = now.duration_since(self.interval_started_at).as_secs_f64();
        let queue_depth = self.total_queued.saturating_sub(dequeued_total);
        eprintln!(
            "rollio-encoder: recording ingest pipeline process={} channel={} active={} \
             recv={} inactive={} queued={} dropped={} recv_fps={:.1} queued_fps={:.1} \
             payload_bytes={} source_age_ms={} source_gap_ms={} receive_gap_ms={} \
             queue_depth={} queue_high={}",
            config.process_id,
            config.channel_id,
            recording_active,
            self.frames_received,
            self.frames_inactive,
            self.frames_queued,
            self.frames_dropped,
            rate(self.frames_received, elapsed_sec),
            rate(self.frames_queued, elapsed_sec),
            self.payload_bytes.summary(),
            self.source_age_ms.summary(),
            self.source_gap_ms.summary(),
            self.receive_gap_ms.summary(),
            queue_depth,
            self.queue_depth_high_watermark,
        );
        let last_receive_at = self.last_receive_at;
        let last_source_timestamp_us = self.last_source_timestamp_us;
        let total_queued = self.total_queued;
        *self = Self::new();
        self.last_receive_at = last_receive_at;
        self.last_source_timestamp_us = last_source_timestamp_us;
        self.total_queued = total_queued;
        self.queue_depth_high_watermark = queue_depth;
        self.interval_started_at = now;
        self.last_log_at = now;
    }
}

struct RecordingWorkerStats {
    interval_started_at: Instant,
    last_log_at: Instant,
    last_dequeue_at: Option<Instant>,
    last_source_timestamp_us: Option<u64>,
    frames_dequeued: u64,
    frames_handled: u64,
    errors: u64,
    session_starts: u64,
    session_finishes: u64,
    dropped_notifications: u64,
    config_packets: u64,
    data_packets: u64,
    eos_packets: u64,
    publish_errors: u64,
    input_bytes: MetricStats,
    encoded_bytes: MetricStats,
    source_age_ms: MetricStats,
    source_gap_ms: MetricStats,
    dequeue_gap_ms: MetricStats,
    handle_ms: MetricStats,
    session_open_ms: MetricStats,
    session_finish_ms: MetricStats,
    publish_ms: MetricStats,
}

impl RecordingWorkerStats {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            interval_started_at: now,
            last_log_at: now,
            last_dequeue_at: None,
            last_source_timestamp_us: None,
            frames_dequeued: 0,
            frames_handled: 0,
            errors: 0,
            session_starts: 0,
            session_finishes: 0,
            dropped_notifications: 0,
            config_packets: 0,
            data_packets: 0,
            eos_packets: 0,
            publish_errors: 0,
            input_bytes: MetricStats::default(),
            encoded_bytes: MetricStats::default(),
            source_age_ms: MetricStats::default(),
            source_gap_ms: MetricStats::default(),
            dequeue_gap_ms: MetricStats::default(),
            handle_ms: MetricStats::default(),
            session_open_ms: MetricStats::default(),
            session_finish_ms: MetricStats::default(),
            publish_ms: MetricStats::default(),
        }
    }

    fn record_dequeued(&mut self, frame: &OwnedFrame, receive_at: Instant) {
        self.frames_dequeued += 1;
        self.input_bytes.observe(frame.payload.len() as f64);
        if let Some(last) = self.last_dequeue_at.replace(receive_at) {
            self.dequeue_gap_ms
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

    fn record_frame_handled(&mut self, elapsed: Duration, ok: bool) {
        self.frames_handled += 1;
        if !ok {
            self.errors += 1;
        }
        self.handle_ms.observe(elapsed.as_secs_f64() * 1000.0);
    }

    fn record_session_open(&mut self, elapsed: Duration) {
        self.session_starts += 1;
        self.session_open_ms.observe(elapsed.as_secs_f64() * 1000.0);
    }

    fn record_session_finish(&mut self, elapsed: Duration, ok: bool) {
        self.session_finishes += 1;
        if !ok {
            self.errors += 1;
        }
        self.session_finish_ms
            .observe(elapsed.as_secs_f64() * 1000.0);
    }

    fn record_dropped_notification(&mut self) {
        self.dropped_notifications += 1;
    }

    fn record_publish(
        &mut self,
        header: &EncodedPacketHeader,
        payload_len: usize,
        elapsed: Duration,
        ok: bool,
    ) {
        if ok {
            match header.kind {
                EncodedPacketKind::Config => self.config_packets += 1,
                EncodedPacketKind::Packet => {
                    self.data_packets += 1;
                    self.encoded_bytes.observe(payload_len as f64);
                }
                EncodedPacketKind::EndOfStream => self.eos_packets += 1,
            }
        } else {
            self.publish_errors += 1;
        }
        self.publish_ms.observe(elapsed.as_secs_f64() * 1000.0);
    }

    fn maybe_log(&mut self, config: &EncoderRuntimeConfigV2) {
        if self.last_log_at.elapsed() < RECORDING_STATS_LOG_INTERVAL {
            return;
        }
        if !self.has_activity() {
            self.last_log_at = Instant::now();
            self.interval_started_at = self.last_log_at;
            return;
        }
        let now = Instant::now();
        let elapsed_sec = now.duration_since(self.interval_started_at).as_secs_f64();
        eprintln!(
            "rollio-encoder: recording worker pipeline process={} channel={} \
             dequeued={} handled={} errors={} dropped_signals={} sessions={}/{} \
             packets={}/{}/{} dequeue_fps={:.1} handled_fps={:.1} input_bytes={} \
             encoded_bytes={} source_age_ms={} source_gap_ms={} dequeue_gap_ms={} \
             handle_ms={} open_ms={} finish_ms={} publish_ms={} publish_errors={}",
            config.process_id,
            config.channel_id,
            self.frames_dequeued,
            self.frames_handled,
            self.errors,
            self.dropped_notifications,
            self.session_starts,
            self.session_finishes,
            self.config_packets,
            self.data_packets,
            self.eos_packets,
            rate(self.frames_dequeued, elapsed_sec),
            rate(self.frames_handled, elapsed_sec),
            self.input_bytes.summary(),
            self.encoded_bytes.summary(),
            self.source_age_ms.summary(),
            self.source_gap_ms.summary(),
            self.dequeue_gap_ms.summary(),
            self.handle_ms.summary(),
            self.session_open_ms.summary(),
            self.session_finish_ms.summary(),
            self.publish_ms.summary(),
            self.publish_errors,
        );
        let last_dequeue_at = self.last_dequeue_at;
        let last_source_timestamp_us = self.last_source_timestamp_us;
        *self = Self::new();
        self.last_dequeue_at = last_dequeue_at;
        self.last_source_timestamp_us = last_source_timestamp_us;
        self.interval_started_at = now;
        self.last_log_at = now;
    }

    fn has_activity(&self) -> bool {
        self.frames_dequeued > 0
            || self.frames_handled > 0
            || self.errors > 0
            || self.dropped_notifications > 0
            || self.config_packets > 0
            || self.data_packets > 0
            || self.eos_packets > 0
            || self.publish_errors > 0
    }
}

struct RecordingStatsSink<'a> {
    inner: &'a mut IpcRecordingSink,
    stats: &'a mut RecordingWorkerStats,
}

impl EncodedPacketSink for RecordingStatsSink<'_> {
    fn write_config(&mut self, header: EncodedPacketHeader, extradata: &[u8]) -> Result<()> {
        let started = Instant::now();
        let result = self.inner.write_config(header, extradata);
        self.stats
            .record_publish(&header, extradata.len(), started.elapsed(), result.is_ok());
        result
    }

    fn write_packet(&mut self, header: EncodedPacketHeader, payload: &[u8]) -> Result<()> {
        let started = Instant::now();
        let result = self.inner.write_packet(header, payload);
        self.stats
            .record_publish(&header, payload.len(), started.elapsed(), result.is_ok());
        result
    }

    fn write_eos(&mut self, header: EncodedPacketHeader) -> Result<()> {
        let started = Instant::now();
        let result = self.inner.write_eos(header);
        self.stats
            .record_publish(&header, 0, started.elapsed(), result.is_ok());
        result
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
    matches!(
        std::env::var("ROLLIO_ADVANCED_PIPELINE_LOGS").as_deref(),
        Ok(value)
            if !value.is_empty()
                && !matches!(value, "0" | "false" | "FALSE" | "off" | "OFF")
    )
}

pub fn run(config: EncoderRuntimeConfigV2) -> Result<()> {
    let recording = config
        .recording
        .clone()
        .ok_or_else(|| EncoderError::message("recording-role config missing [recording] block"))?;

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
        .open_or_create()
        .map_err(map_iceoryx_error)?;
    let control_subscriber = control_service
        .subscriber_builder()
        .create()
        .map_err(map_iceoryx_error)?;

    let backpressure_service_name: ServiceName =
        BACKPRESSURE_SERVICE.try_into().map_err(map_iceoryx_error)?;
    let backpressure_service = node
        .service_builder(&backpressure_service_name)
        .publish_subscribe::<BackpressureEvent>()
        .max_publishers(16)
        .max_subscribers(8)
        .max_nodes(16)
        .open_or_create()
        .map_err(map_iceoryx_error)?;
    let backpressure_publisher = backpressure_service
        .publisher_builder()
        .create()
        .map_err(map_iceoryx_error)?;

    let advanced_logs = advanced_pipeline_logs_enabled();
    let dequeued_frames = advanced_logs.then(|| Arc::new(AtomicU64::new(0)));
    let worker_dequeued_frames = dequeued_frames.clone();
    let mut ingest_stats = advanced_logs.then(RecordingIngestStats::new);

    let (frame_tx, frame_rx) = mpsc::sync_channel(recording.queue_size as usize);
    let (control_tx, control_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();

    let worker_config = config.clone();
    let worker = thread::Builder::new()
        .name("rollio-encoder-recording".to_string())
        .spawn(move || {
            worker_main(
                worker_config,
                control_rx,
                frame_rx,
                event_tx,
                advanced_logs,
                worker_dequeued_frames,
            )
        })
        .map_err(|error| EncoderError::message(format!("failed to spawn worker: {error}")))?;

    let mut recording_active = false;
    let mut shutdown_requested = false;
    while !shutdown_requested {
        let mut request_stop = false;
        let mut request_shutdown = false;

        while let Ok(event) = event_rx.try_recv() {
            match event {
                WorkerEvent::Error(message) => {
                    let _ = control_tx.send(WorkerControl::Shutdown);
                    let _ = worker.join();
                    return Err(EncoderError::message(message));
                }
                WorkerEvent::ShutdownComplete => shutdown_requested = true,
            }
        }

        while let Some(sample) = control_subscriber.receive().map_err(map_iceoryx_error)? {
            match sample.payload() {
                ControlEvent::RecordingStart {
                    episode_index,
                    controller_ts_us,
                } => {
                    control_tx
                        .send(WorkerControl::RecordingStart {
                            episode_index: *episode_index,
                            controller_ts_us: *controller_ts_us,
                        })
                        .map_err(|error| {
                            EncoderError::message(format!("failed to signal worker: {error}"))
                        })?;
                    recording_active = true;
                }
                ControlEvent::RecordingStop { .. } => {
                    request_stop = true;
                }
                ControlEvent::Shutdown => {
                    request_shutdown = true;
                }
                ControlEvent::EpisodeKeep { .. }
                | ControlEvent::EpisodeDiscard { .. }
                | ControlEvent::ModeSwitch { .. } => {}
            }
        }

        // Recording role only forwards frames during an active
        // recording. Outside a session the worker is idle.
        while let Some(sample) = frame_subscriber.receive().map_err(map_iceoryx_error)? {
            let receive_at = Instant::now();
            let header = *sample.user_header();
            let payload_len = sample.payload().len();
            if let Some(stats) = ingest_stats.as_mut() {
                stats.record_received(&header, payload_len, receive_at, recording_active);
            }
            if !recording_active {
                continue;
            }
            let owned = OwnedFrame {
                header,
                payload: sample.payload().to_vec(),
            };
            match frame_tx.try_send(owned) {
                Ok(()) => {
                    if let (Some(stats), Some(counter)) =
                        (ingest_stats.as_mut(), dequeued_frames.as_ref())
                    {
                        stats.record_queued(counter.load(Ordering::Relaxed));
                    }
                }
                Err(mpsc::TrySendError::Full(_frame)) => {
                    publish_backpressure(&backpressure_publisher, &config.process_id)?;
                    if let (Some(stats), Some(counter)) =
                        (ingest_stats.as_mut(), dequeued_frames.as_ref())
                    {
                        stats.record_dropped(counter.load(Ordering::Relaxed));
                    }
                    control_tx
                        .send(WorkerControl::DroppedFrame)
                        .map_err(|error| {
                            EncoderError::message(format!(
                                "failed to signal dropped frame: {error}"
                            ))
                        })?;
                }
                Err(mpsc::TrySendError::Disconnected(_frame)) => {
                    return Err(EncoderError::message(
                        "encoder worker disconnected while sending frame",
                    ));
                }
            }
        }

        if request_stop {
            control_tx
                .send(WorkerControl::RecordingStop)
                .map_err(|error| {
                    EncoderError::message(format!("failed to signal worker: {error}"))
                })?;
            recording_active = false;
        }

        if request_shutdown {
            control_tx.send(WorkerControl::Shutdown).map_err(|error| {
                EncoderError::message(format!("failed to signal worker: {error}"))
            })?;
            recording_active = false;
        }

        if let (Some(stats), Some(counter)) = (ingest_stats.as_mut(), dequeued_frames.as_ref()) {
            stats.maybe_log(&config, recording_active, counter.load(Ordering::Relaxed));
        }

        thread::sleep(Duration::from_millis(2));
    }

    worker
        .join()
        .map_err(|_| EncoderError::message("encoder worker panicked"))??;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct PendingEpisode {
    episode_index: u32,
    controller_ts_us: u64,
}

fn worker_main(
    config: EncoderRuntimeConfigV2,
    control_rx: mpsc::Receiver<WorkerControl>,
    frame_rx: mpsc::Receiver<OwnedFrame>,
    event_tx: mpsc::Sender<WorkerEvent>,
    advanced_logs: bool,
    dequeued_frames: Option<Arc<AtomicU64>>,
) -> Result<()> {
    let recording = config
        .recording
        .as_ref()
        .ok_or_else(|| EncoderError::message("recording-role worker missing [recording] block"))?;
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()
        .map_err(map_iceoryx_error)?;
    let mut sink = IpcRecordingSink::open(
        &node,
        &recording.config_topic,
        &recording.packet_topic,
        16 * 1024 * 1024,
    )?;

    let mut pending_episode: Option<PendingEpisode> = None;
    let mut active_session: Option<EncoderSession> = None;
    let mut stop_after_drain = false;
    let mut worker_stats = advanced_logs.then(RecordingWorkerStats::new);

    loop {
        while let Ok(control) = control_rx.try_recv() {
            match control {
                WorkerControl::RecordingStart {
                    episode_index,
                    controller_ts_us,
                } => {
                    if let Some(session) = active_session.take() {
                        if let Err(error) =
                            finish_session(session, &mut sink, worker_stats.as_mut())
                        {
                            let _ = event_tx.send(WorkerEvent::Error(error.to_string()));
                            return Err(error);
                        }
                    }
                    pending_episode = Some(PendingEpisode {
                        episode_index,
                        controller_ts_us,
                    });
                    stop_after_drain = false;
                }
                WorkerControl::RecordingStop => {
                    stop_after_drain = true;
                }
                WorkerControl::DroppedFrame => {
                    if let Some(stats) = worker_stats.as_mut() {
                        stats.record_dropped_notification();
                    }
                    if let Some(session) = active_session.as_mut() {
                        session.record_dropped();
                    }
                }
                WorkerControl::Shutdown => {
                    if let Some(session) = active_session.take() {
                        if let Err(error) =
                            finish_session(session, &mut sink, worker_stats.as_mut())
                        {
                            let _ = event_tx.send(WorkerEvent::Error(error.to_string()));
                            return Err(error);
                        }
                    }
                    let _ = event_tx.send(WorkerEvent::ShutdownComplete);
                    return Ok(());
                }
            }
        }

        let mut processed_any_frame = false;
        while let Ok(frame) = frame_rx.try_recv() {
            processed_any_frame = true;
            process_worker_frame(
                &config,
                recording,
                &frame,
                &mut active_session,
                &pending_episode,
                &mut sink,
                worker_stats.as_mut(),
                dequeued_frames.as_ref(),
            )?;
        }

        if !processed_any_frame {
            match frame_rx.recv_timeout(Duration::from_millis(10)) {
                Ok(frame) => {
                    processed_any_frame = true;
                    process_worker_frame(
                        &config,
                        recording,
                        &frame,
                        &mut active_session,
                        &pending_episode,
                        &mut sink,
                        worker_stats.as_mut(),
                        dequeued_frames.as_ref(),
                    )?;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = event_tx.send(WorkerEvent::ShutdownComplete);
                    return Ok(());
                }
            }
        }

        if stop_after_drain && !processed_any_frame {
            pending_episode = None;
            if let Some(session) = active_session.take() {
                if let Err(error) = finish_session(session, &mut sink, worker_stats.as_mut()) {
                    let _ = event_tx.send(WorkerEvent::Error(error.to_string()));
                    return Err(error);
                }
            }
            stop_after_drain = false;
        }

        if let Some(stats) = worker_stats.as_mut() {
            stats.maybe_log(&config);
        }
    }
}

fn process_worker_frame(
    config: &EncoderRuntimeConfigV2,
    recording: &rollio_types::config::RecordingEncoderConfig,
    frame: &OwnedFrame,
    active_session: &mut Option<EncoderSession>,
    pending_episode: &Option<PendingEpisode>,
    sink: &mut IpcRecordingSink,
    stats: Option<&mut RecordingWorkerStats>,
    dequeued_frames: Option<&Arc<AtomicU64>>,
) -> Result<()> {
    if let Some(counter) = dequeued_frames {
        counter.fetch_add(1, Ordering::Relaxed);
    }
    let mut stats = stats;
    if let Some(stats) = stats.as_deref_mut() {
        stats.record_dequeued(frame, Instant::now());
    }
    let started = Instant::now();
    let result = handle_frame(
        config,
        recording,
        frame,
        active_session,
        pending_episode,
        sink,
        stats.as_deref_mut(),
    );
    if let Some(stats) = stats {
        stats.record_frame_handled(started.elapsed(), result.is_ok());
    }
    result
}

fn handle_frame(
    config: &EncoderRuntimeConfigV2,
    recording: &rollio_types::config::RecordingEncoderConfig,
    frame: &OwnedFrame,
    active_session: &mut Option<EncoderSession>,
    pending_episode: &Option<PendingEpisode>,
    sink: &mut IpcRecordingSink,
    stats: Option<&mut RecordingWorkerStats>,
) -> Result<()> {
    let mut stats = stats;
    if active_session.is_none() {
        let Some(episode) = pending_episode.as_ref() else {
            return Ok(());
        };
        let params = CodecSessionParams::from_recording(
            recording,
            &config.process_id,
            episode.episode_index,
            episode.controller_ts_us,
            frame.header.width,
            frame.header.height,
        );
        let started = Instant::now();
        *active_session = Some(open_session(params, frame)?);
        if let Some(stats) = stats.as_deref_mut() {
            stats.record_session_open(started.elapsed());
        }
    }
    if let Some(session) = active_session.as_mut() {
        if let Some(stats) = stats {
            let mut stats_sink = RecordingStatsSink { inner: sink, stats };
            session.encode(frame, &mut stats_sink)?;
        } else {
            session.encode(frame, sink)?;
        }
    }
    Ok(())
}

fn finish_session(
    session: EncoderSession,
    sink: &mut IpcRecordingSink,
    stats: Option<&mut RecordingWorkerStats>,
) -> Result<()> {
    let started = Instant::now();
    let mut stats = stats;
    let result = if let Some(stats_ref) = stats.as_deref_mut() {
        let mut stats_sink = RecordingStatsSink {
            inner: sink,
            stats: stats_ref,
        };
        session.finish(&mut stats_sink)
    } else {
        session.finish(sink)
    };
    if let Some(stats) = stats {
        stats.record_session_finish(started.elapsed(), result.is_ok());
    }
    result
}

fn publish_backpressure(
    publisher: &iceoryx2::port::publisher::Publisher<ipc::Service, BackpressureEvent, ()>,
    process_id: &str,
) -> Result<()> {
    publisher
        .send_copy(BackpressureEvent {
            process_id: FixedString64::new(process_id),
            queue_name: FixedString64::new("frame_queue"),
        })
        .map_err(map_iceoryx_error)?;
    Ok(())
}
