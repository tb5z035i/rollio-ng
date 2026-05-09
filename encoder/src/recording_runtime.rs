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

use crate::codec::{open_session, CodecSessionParams, EncoderSession, OwnedFrame};
use crate::error::{map_iceoryx_error, EncoderError, Result};
use crate::sink::IpcRecordingSink;
use iceoryx2::prelude::*;
use rollio_bus::{BACKPRESSURE_SERVICE, CAMERA_FRAMES_MAX_SUBSCRIBERS, CONTROL_EVENTS_SERVICE};
use rollio_types::config::EncoderRuntimeConfigV2;
use rollio_types::messages::{BackpressureEvent, CameraFrameHeader, ControlEvent, FixedString64};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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

    let (frame_tx, frame_rx) = mpsc::sync_channel(recording.queue_size as usize);
    let (control_tx, control_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();

    let worker_config = config.clone();
    let worker = thread::Builder::new()
        .name("rollio-encoder-recording".to_string())
        .spawn(move || worker_main(worker_config, control_rx, frame_rx, event_tx))
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
            if !recording_active {
                continue;
            }
            let owned = OwnedFrame {
                header: *sample.user_header(),
                payload: sample.payload().to_vec(),
            };
            match frame_tx.try_send(owned) {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(_frame)) => {
                    publish_backpressure(&backpressure_publisher, &config.process_id)?;
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

    loop {
        while let Ok(control) = control_rx.try_recv() {
            match control {
                WorkerControl::RecordingStart {
                    episode_index,
                    controller_ts_us,
                } => {
                    if let Some(session) = active_session.take() {
                        if let Err(error) = session.finish(&mut sink) {
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
                    if let Some(session) = active_session.as_mut() {
                        session.record_dropped();
                    }
                }
                WorkerControl::Shutdown => {
                    if let Some(session) = active_session.take() {
                        if let Err(error) = session.finish(&mut sink) {
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
            handle_frame(
                &config,
                recording,
                &frame,
                &mut active_session,
                &pending_episode,
                &mut sink,
            )?;
        }

        if !processed_any_frame {
            match frame_rx.recv_timeout(Duration::from_millis(10)) {
                Ok(frame) => {
                    processed_any_frame = true;
                    handle_frame(
                        &config,
                        recording,
                        &frame,
                        &mut active_session,
                        &pending_episode,
                        &mut sink,
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
                if let Err(error) = session.finish(&mut sink) {
                    let _ = event_tx.send(WorkerEvent::Error(error.to_string()));
                    return Err(error);
                }
            }
            stop_after_drain = false;
        }
    }
}

fn handle_frame(
    config: &EncoderRuntimeConfigV2,
    recording: &rollio_types::config::RecordingEncoderConfig,
    frame: &OwnedFrame,
    active_session: &mut Option<EncoderSession>,
    pending_episode: &Option<PendingEpisode>,
    sink: &mut IpcRecordingSink,
) -> Result<()> {
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
        *active_session = Some(open_session(params, frame)?);
    }
    if let Some(session) = active_session.as_mut() {
        session.encode(frame, sink)?;
    }
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
