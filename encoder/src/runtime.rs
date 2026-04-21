use crate::error::{map_iceoryx_error, EncoderError, Result};
use crate::media::{self, BuiltPreview, EncodedArtifact, OwnedFrame, PreviewBuilder};
use clap::Args;
use iceoryx2::prelude::*;
use rollio_bus::{BACKPRESSURE_SERVICE, CONTROL_EVENTS_SERVICE, VIDEO_READY_SERVICE};
use rollio_types::config::EncoderRuntimeConfigV2;
use rollio_types::messages::{
    BackpressureEvent, CameraFrameHeader, ControlEvent, FixedString256, FixedString64, PixelFormat,
    VideoReady,
};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

type PreviewPublisher = iceoryx2::port::publisher::Publisher<ipc::Service, [u8], CameraFrameHeader>;

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(long, value_name = "PATH", conflicts_with = "config_inline")]
    pub config: Option<PathBuf>,
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    pub config_inline: Option<String>,
}

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
    VideoReady(EncodedArtifact),
    Error(String),
    ShutdownComplete,
}

pub fn run(args: RunArgs) -> Result<()> {
    let config = load_runtime_config(&args)?;
    media::ensure_ffmpeg_initialized()?;

    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()
        .map_err(map_iceoryx_error)?;

    let frame_topic = frame_topic(&config);
    let frame_service_name: ServiceName =
        frame_topic.as_str().try_into().map_err(map_iceoryx_error)?;
    let frame_service = node
        .service_builder(&frame_service_name)
        .publish_subscribe::<[u8]>()
        .user_header::<CameraFrameHeader>()
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

    // Every encoder is a publisher on `VIDEO_READY_SERVICE` (the assembler
    // is the sole subscriber) and on `BACKPRESSURE_SERVICE` (the controller
    // is the subscriber). iceoryx2's default `max_publishers` is 2, so when
    // a project has more than two cameras the third encoder used to fail
    // with `PublisherCreateError::ExceedsMaxSupportedPublishers`. Match
    // `controller/src/collect.rs::ControllerIpc` (which sets the same caps
    // for BACKPRESSURE_SERVICE) so whichever process creates the service
    // first sets a quota large enough for every encoder + assembler in the
    // pipeline.
    let ready_service_name: ServiceName =
        VIDEO_READY_SERVICE.try_into().map_err(map_iceoryx_error)?;
    let ready_service = node
        .service_builder(&ready_service_name)
        .publish_subscribe::<VideoReady>()
        .max_publishers(16)
        .max_subscribers(8)
        .max_nodes(16)
        .open_or_create()
        .map_err(map_iceoryx_error)?;
    let ready_publisher = ready_service
        .publisher_builder()
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

    let (frame_tx, frame_rx) = mpsc::sync_channel(config.queue_size as usize);
    let (control_tx, control_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();
    let worker_config = config.clone();

    let worker = thread::Builder::new()
        .name("rollio-encoder-worker".to_string())
        .spawn(move || worker_main(worker_config, control_rx, frame_rx, event_tx))
        .map_err(|error| EncoderError::message(format!("failed to spawn worker: {error}")))?;

    let mut recording_active = false;
    let mut shutdown_requested = false;
    while !shutdown_requested {
        let mut request_stop = false;
        let mut request_shutdown = false;
        while let Ok(event) = event_rx.try_recv() {
            match event {
                WorkerEvent::VideoReady(artifact) => {
                    publish_video_ready(&ready_publisher, &config.process_id, &artifact)?
                }
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

        // The encoder is now always-on: it forwards every frame to the
        // worker so the preview tap can produce RGB24 previews even when
        // no episode is being recorded. Backpressure is still meaningful
        // only inside a recording session — preview-tap drops are not an
        // actionable signal for the controller, so the BackpressureEvent
        // is only emitted while `recording_active = true`.
        while let Some(sample) = frame_subscriber.receive().map_err(map_iceoryx_error)? {
            let owned = OwnedFrame {
                header: *sample.user_header(),
                payload: sample.payload().to_vec(),
            };
            match frame_tx.try_send(owned) {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(_frame)) => {
                    if recording_active {
                        publish_backpressure(&backpressure_publisher, &config.process_id)?;
                        control_tx
                            .send(WorkerControl::DroppedFrame)
                            .map_err(|error| {
                                EncoderError::message(format!(
                                    "failed to signal dropped frame: {error}"
                                ))
                            })?;
                    }
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

        thread::sleep(Duration::from_millis(2));
    }

    worker
        .join()
        .map_err(|_| EncoderError::message("encoder worker panicked"))??;
    Ok(())
}

/// Owns the iceoryx2 preview publisher and the `PreviewBuilder`. Both
/// live on the worker thread so we keep ffmpeg and iceoryx2 publisher
/// state single-threaded.
struct PreviewPipeline {
    publisher: PreviewPublisher,
    builder: PreviewBuilder,
    output_width: u32,
    output_height: u32,
}

impl PreviewPipeline {
    fn new(config: &EncoderRuntimeConfigV2, node: &Node<ipc::Service>) -> Result<Self> {
        let preview_service_name: ServiceName = config
            .preview_topic
            .as_str()
            .try_into()
            .map_err(map_iceoryx_error)?;
        let preview_service = node
            .service_builder(&preview_service_name)
            .publish_subscribe::<[u8]>()
            .user_header::<CameraFrameHeader>()
            .open_or_create()
            .map_err(map_iceoryx_error)?;
        let payload_capacity = (config.preview_width as usize)
            .checked_mul(config.preview_height as usize)
            .and_then(|pixels| pixels.checked_mul(3))
            .ok_or_else(|| EncoderError::message("preview slot size overflow"))?;
        let publisher = preview_service
            .publisher_builder()
            .initial_max_slice_len(payload_capacity)
            .allocation_strategy(AllocationStrategy::PowerOfTwo)
            .create()
            .map_err(map_iceoryx_error)?;
        let builder = PreviewBuilder::new(
            config.preview_width,
            config.preview_height,
            config.preview_fps,
        );
        Ok(Self {
            publisher,
            builder,
            output_width: config.preview_width,
            output_height: config.preview_height,
        })
    }

    /// Decode (if MJPG), downscale to RGB24, and publish on the preview
    /// service if the throttle allows. Errors during preview generation
    /// are logged but never propagated; the recording path must keep
    /// running even if a preview frame fails.
    fn publish_if_due(&mut self, frame: &OwnedFrame, process_id: &str) {
        match self.builder.build(frame) {
            Ok(Some(preview)) => {
                if let Err(error) = self.publish(preview) {
                    eprintln!(
                        "rollio-encoder: preview publish failed for process={process_id}: {error}"
                    );
                }
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!(
                    "rollio-encoder: preview build failed for process={process_id} \
                     (format={:?}, frame={}x{}): {error}",
                    frame.header.pixel_format, frame.header.width, frame.header.height,
                );
            }
        }
    }

    fn publish(&self, preview: BuiltPreview) -> Result<()> {
        let mut sample = self
            .publisher
            .loan_slice_uninit(preview.rgb.len())
            .map_err(map_iceoryx_error)?;
        *sample.user_header_mut() = CameraFrameHeader {
            timestamp_us: preview.timestamp_us,
            width: self.output_width,
            height: self.output_height,
            pixel_format: PixelFormat::Rgb24,
            frame_index: preview.frame_index,
        };
        let sample = sample.write_from_slice(&preview.rgb);
        sample.send().map_err(map_iceoryx_error)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct PendingEpisode {
    episode_index: u32,
    // Controller's wall-clock anchor (UNIX-epoch microseconds) — used as
    // the VFR PTS origin so each frame's PTS in the encoded MP4 is
    // exactly `frame.header.timestamp_us - controller_ts_us`.
    controller_ts_us: u64,
}

fn worker_main(
    config: EncoderRuntimeConfigV2,
    control_rx: mpsc::Receiver<WorkerControl>,
    frame_rx: mpsc::Receiver<OwnedFrame>,
    event_tx: mpsc::Sender<WorkerEvent>,
) -> Result<()> {
    // The preview pipeline (publisher + downscale-to-RGB24 builder) lives
    // on the worker thread so iceoryx2 publishers stay single-threaded
    // and the MJPG decoder isn't shipped across threads.
    let preview_node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()
        .map_err(map_iceoryx_error)?;
    let mut preview_pipeline = PreviewPipeline::new(&config, &preview_node)?;

    let mut pending_episode: Option<PendingEpisode> = None;
    let mut active_session = None;
    let mut stop_after_drain = false;

    loop {
        while let Ok(control) = control_rx.try_recv() {
            match control {
                WorkerControl::RecordingStart {
                    episode_index,
                    controller_ts_us,
                } => {
                    if let Some(session) = active_session.take() {
                        match media::finish_session(session) {
                            Ok(artifact) => {
                                let _ = event_tx.send(WorkerEvent::VideoReady(artifact));
                            }
                            Err(error) => {
                                let _ = event_tx.send(WorkerEvent::Error(error.to_string()));
                                return Err(error);
                            }
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
                    if let Some(session) = active_session.as_mut() {
                        media::record_dropped_frame(session);
                    }
                }
                WorkerControl::Shutdown => {
                    if let Some(session) = active_session.take() {
                        match media::finish_session(session) {
                            Ok(artifact) => {
                                let _ = event_tx.send(WorkerEvent::VideoReady(artifact));
                            }
                            Err(error) => {
                                let _ = event_tx.send(WorkerEvent::Error(error.to_string()));
                                return Err(error);
                            }
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
            handle_frame(
                &config,
                &frame,
                &mut active_session,
                &pending_episode,
                &mut preview_pipeline,
            )?;
        }

        if !processed_any_frame {
            match frame_rx.recv_timeout(Duration::from_millis(10)) {
                Ok(frame) => {
                    processed_any_frame = true;
                    handle_frame(
                        &config,
                        &frame,
                        &mut active_session,
                        &pending_episode,
                        &mut preview_pipeline,
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
                match media::finish_session(session) {
                    Ok(artifact) => {
                        let _ = event_tx.send(WorkerEvent::VideoReady(artifact));
                    }
                    Err(error) => {
                        let _ = event_tx.send(WorkerEvent::Error(error.to_string()));
                        return Err(error);
                    }
                }
            }
            stop_after_drain = false;
        }
    }
}

/// One-frame work shared between the `try_recv` and `recv_timeout`
/// branches of `worker_main`: always run the preview tap, then encode if
/// a recording session is active.
fn handle_frame(
    config: &EncoderRuntimeConfigV2,
    frame: &OwnedFrame,
    active_session: &mut Option<media::SessionEncoder>,
    pending_episode: &Option<PendingEpisode>,
    preview_pipeline: &mut PreviewPipeline,
) -> Result<()> {
    // Preview tap runs on every received frame, regardless of recording
    // state. The pipeline throttles internally to honor `preview_fps`.
    preview_pipeline.publish_if_due(frame, &config.process_id);

    if active_session.is_none() {
        let Some(episode) = pending_episode.as_ref() else {
            return Ok(());
        };
        *active_session = Some(media::open_session(
            config,
            episode.episode_index,
            episode.controller_ts_us,
            frame,
        )?);
    }
    if let Some(session) = active_session.as_mut() {
        media::encode_frame(session, frame)?;
    }
    Ok(())
}

fn load_runtime_config(args: &RunArgs) -> Result<EncoderRuntimeConfigV2> {
    if let Some(path) = &args.config {
        return EncoderRuntimeConfigV2::from_file(path).map_err(Into::into);
    }
    if let Some(inline) = &args.config_inline {
        return inline.parse::<EncoderRuntimeConfigV2>().map_err(Into::into);
    }
    Err(EncoderError::message(
        "run requires either --config or --config-inline",
    ))
}

fn frame_topic(config: &EncoderRuntimeConfigV2) -> String {
    config.frame_topic.clone()
}

fn publish_video_ready(
    publisher: &iceoryx2::port::publisher::Publisher<ipc::Service, VideoReady, ()>,
    process_id: &str,
    artifact: &EncodedArtifact,
) -> Result<()> {
    publisher
        .send_copy(VideoReady {
            process_id: FixedString64::new(process_id),
            episode_index: episode_index_from_path(&artifact.path),
            file_path: FixedString256::new(&artifact.path.to_string_lossy()),
        })
        .map_err(map_iceoryx_error)?;
    Ok(())
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

fn episode_index_from_path(path: &std::path::Path) -> u32 {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return 0;
    };
    let Some(index_fragment) = file_name
        .split("_episode_")
        .nth(1)
        .and_then(|part| part.split('.').next())
    else {
        return 0;
    };
    index_fragment.parse().unwrap_or(0)
}
