//! `rollio-episode-lerobot` runtime — packets-only assembler.
//!
//! Subscribes to:
//! - `CONTROL_EVENTS_SERVICE` for episode lifecycle
//! - per-camera `…/recording-config` (history=1) + `…/recording-packets`
//! - per-observation state topic
//! - per-action command topic
//! - `EPISODE_READY_SERVICE` (publisher)
//!
//! Maintains per-`(channel, episode)` packet buffers, validates
//! sequence numbers as packets arrive, and stages the episode after
//! every camera has emitted `EndOfStream`. A bounded
//! `missing_eos_timeout_ms` removes stuck episodes when the encoder
//! crashes mid-recording.

use crate::dataset::{
    action_key, observation_key, remove_episode_artifacts, sensor_observation_key, stage_episode,
    ActionSample, EpisodeAssemblyInput, ObservationSample, SensorSample, StagedEpisode,
};
use crate::packets::RecordingStreamBuffer;
use clap::Args;
use iceoryx2::node::NodeWaitFailure;
use iceoryx2::prelude::*;
use rollio_bus::{
    BACKPRESSURE_SERVICE, CONTROL_EVENTS_SERVICE, EPISODE_DROPPED_SERVICE, EPISODE_READY_SERVICE,
    EPISODE_STORED_SERVICE, SAMPLE_BUFFER, SAMPLE_MAX_NODES, SAMPLE_MAX_PUBLISHERS,
    SAMPLE_MAX_SUBSCRIBERS, STATE_BUFFER, STATE_MAX_NODES, STATE_MAX_PUBLISHERS,
    STATE_MAX_SUBSCRIBERS, STREAM_CONFIG_HISTORY_SIZE,
};
use rollio_types::config::{
    AssemblerActionRuntimeConfigV2, AssemblerObservationRuntimeConfigV2, AssemblerRuntimeConfigV2,
    AssemblerSensorObservationRuntimeConfigV2, EpisodeFormat, RobotCommandKind, RobotStateKind,
    SensorStateKind,
};
use rollio_types::messages::{
    BackpressureEvent, ControlEvent, EncodedPacketHeader, EncodedPacketKind, EpisodeDropped,
    EpisodeReady, EpisodeStored, FixedString256, FixedString64, JointMitCommand15, JointVector15,
    ParallelMitCommand2, ParallelVector2, Pose7, SensorDType, SensorFrameHeader,
};
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(long, value_name = "PATH", conflicts_with = "config_inline")]
    pub config: Option<PathBuf>,
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    pub config_inline: Option<String>,
}

// ---------------------------------------------------------------------------
// Per-channel packet subscribers
// ---------------------------------------------------------------------------

type PacketSubscriber =
    iceoryx2::port::subscriber::Subscriber<ipc::Service, [u8], EncodedPacketHeader>;

struct CameraSubscriber {
    channel_id: String,
    config_subscriber: PacketSubscriber,
    packet_subscriber: PacketSubscriber,
}

// ---------------------------------------------------------------------------
// Robot state / action subscribers (unchanged from the file-mode
// implementation; state/action plumbing did not change)
// ---------------------------------------------------------------------------

struct ObservationSubscriber {
    config: AssemblerObservationRuntimeConfigV2,
    subscriber: ObservationSubscriberKind,
}

/// Sensor sample subscriber. Dynamic payload + `SensorFrameHeader` user
/// header; the runtime decodes payload bytes per `dtype` into `Vec<f32>`.
struct SensorSubscriber {
    config: AssemblerSensorObservationRuntimeConfigV2,
    subscriber:
        iceoryx2::port::subscriber::Subscriber<ipc::Service, [u8], SensorFrameHeader>,
}

enum ObservationSubscriberKind {
    JointVector15(iceoryx2::port::subscriber::Subscriber<ipc::Service, JointVector15, ()>),
    ParallelVector2(iceoryx2::port::subscriber::Subscriber<ipc::Service, ParallelVector2, ()>),
    Pose7(iceoryx2::port::subscriber::Subscriber<ipc::Service, Pose7, ()>),
}

struct ActionSubscriber {
    config: AssemblerActionRuntimeConfigV2,
    subscriber: ActionSubscriberKind,
}

enum ActionSubscriberKind {
    JointVector15(iceoryx2::port::subscriber::Subscriber<ipc::Service, JointVector15, ()>),
    JointMitCommand15(iceoryx2::port::subscriber::Subscriber<ipc::Service, JointMitCommand15, ()>),
    ParallelVector2(iceoryx2::port::subscriber::Subscriber<ipc::Service, ParallelVector2, ()>),
    ParallelMitCommand2(
        iceoryx2::port::subscriber::Subscriber<ipc::Service, ParallelMitCommand2, ()>,
    ),
    Pose7(iceoryx2::port::subscriber::Subscriber<ipc::Service, Pose7, ()>),
}

// ---------------------------------------------------------------------------
// Pending episode aggregation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct PendingEpisode {
    episode_index: u32,
    start_time_us: u64,
    stop_time_us: Option<u64>,
    keep_requested: bool,
    ready_wait_started_us: Option<u64>,
    observation_samples: BTreeMap<String, Vec<ObservationSample>>,
    sensor_samples: BTreeMap<String, Vec<SensorSample>>,
    action_samples: BTreeMap<String, Vec<ActionSample>>,
    /// Per-channel packet streams. Populated as packets arrive and
    /// finalized when each camera observes `EndOfStream`.
    camera_streams: BTreeMap<String, RecordingStreamBuffer>,
    /// Channels that have reported a sequence error or other failure
    /// while accumulating packets. The episode is removed on the next
    /// `dispatch_ready_episodes` tick.
    failed_cameras: Vec<String>,
}

impl PendingEpisode {
    fn new(episode_index: u32, start_time_us: u64, channel_ids: &[String]) -> Self {
        let mut camera_streams = BTreeMap::new();
        for channel_id in channel_ids {
            camera_streams.insert(channel_id.clone(), RecordingStreamBuffer::default());
        }
        Self {
            episode_index,
            start_time_us,
            stop_time_us: None,
            keep_requested: false,
            ready_wait_started_us: None,
            observation_samples: BTreeMap::new(),
            sensor_samples: BTreeMap::new(),
            action_samples: BTreeMap::new(),
            camera_streams,
            failed_cameras: Vec::new(),
        }
    }

    fn as_assembly_input(&self) -> EpisodeAssemblyInput {
        EpisodeAssemblyInput {
            episode_index: self.episode_index,
            start_time_us: self.start_time_us,
            stop_time_us: self.stop_time_us.unwrap_or(self.start_time_us),
            observation_samples: self.observation_samples.clone(),
            sensor_samples: self.sensor_samples.clone(),
            action_samples: self.action_samples.clone(),
            camera_streams: self.camera_streams.clone(),
        }
    }

    fn all_cameras_eos(&self, expected: usize) -> bool {
        self.camera_streams
            .values()
            .filter(|s| s.eos_received)
            .count()
            == expected
    }
}

struct EpisodeManager {
    config: AssemblerRuntimeConfigV2,
    active_episode_index: Option<u32>,
    episodes: BTreeMap<u32, PendingEpisode>,
    camera_channel_ids: Vec<String>,
    /// Episodes the assembler has handed to the stage worker but not yet
    /// observed `EpisodeStored` for. Each entry holds the dispatch
    /// timestamp so a stalled storage process (or lost `EpisodeStored`)
    /// can be detected by `sweep_stale_slots` instead of leaking the
    /// slot forever.
    in_flight: BTreeMap<u32, Instant>,
    staging_slots: usize,
    stale_after: Duration,
}

impl EpisodeManager {
    fn new(config: AssemblerRuntimeConfigV2) -> Self {
        let camera_channel_ids = config
            .cameras
            .iter()
            .map(|c| c.channel_id.clone())
            .collect();
        let staging_slots = config.staging_slots as usize;
        let stale_after =
            Duration::from_millis(config.missing_eos_timeout_ms.saturating_mul(2));
        Self {
            config,
            active_episode_index: None,
            episodes: BTreeMap::new(),
            camera_channel_ids,
            in_flight: BTreeMap::new(),
            staging_slots,
            stale_after,
        }
    }

    fn release_slot(&mut self, episode_index: u32) -> bool {
        self.in_flight.remove(&episode_index).is_some()
    }

    fn sweep_stale_slots(&mut self, now: Instant) -> Vec<u32> {
        let stale: Vec<u32> = self
            .in_flight
            .iter()
            .filter(|(_, started)| now.duration_since(**started) > self.stale_after)
            .map(|(idx, _)| *idx)
            .collect();
        for idx in &stale {
            self.in_flight.remove(idx);
        }
        stale
    }

    fn on_control_event(&mut self, event: ControlEvent) -> bool {
        match event {
            ControlEvent::RecordingStart {
                episode_index,
                controller_ts_us,
            } => {
                self.episodes.insert(
                    episode_index,
                    PendingEpisode::new(episode_index, controller_ts_us, &self.camera_channel_ids),
                );
                self.active_episode_index = Some(episode_index);
                false
            }
            ControlEvent::RecordingStop {
                episode_index,
                controller_ts_us,
            } => {
                if let Some(episode) = self.episodes.get_mut(&episode_index) {
                    episode.stop_time_us = Some(controller_ts_us);
                    episode.ready_wait_started_us = Some(unix_timestamp_us());
                }
                if self.active_episode_index == Some(episode_index) {
                    self.active_episode_index = None;
                }
                false
            }
            ControlEvent::EpisodeKeep { episode_index } => {
                if let Some(episode) = self.episodes.get_mut(&episode_index) {
                    episode.keep_requested = true;
                    episode
                        .ready_wait_started_us
                        .get_or_insert_with(unix_timestamp_us);
                }
                false
            }
            ControlEvent::EpisodeDiscard { episode_index } => {
                if self.episodes.remove(&episode_index).is_some() {
                    remove_episode_artifacts(&self.config.staging_dir, episode_index);
                }
                if self.active_episode_index == Some(episode_index) {
                    self.active_episode_index = None;
                }
                false
            }
            ControlEvent::Shutdown => true,
            ControlEvent::ModeSwitch { .. } => false,
        }
    }

    fn on_observation(
        &mut self,
        channel_id: &str,
        state_kind: RobotStateKind,
        timestamp_us: u64,
        values: Vec<f64>,
    ) {
        let Some(episode_index) = self.active_episode_index else {
            return;
        };
        let Some(episode) = self.episodes.get_mut(&episode_index) else {
            return;
        };
        episode
            .observation_samples
            .entry(observation_key(channel_id, state_kind))
            .or_default()
            .push(ObservationSample {
                timestamp_us,
                values,
            });
    }

    fn on_sensor_sample(
        &mut self,
        channel_id: &str,
        sensor_kind: SensorStateKind,
        timestamp_us: u64,
        values: Vec<f32>,
    ) {
        let Some(episode_index) = self.active_episode_index else {
            return;
        };
        let Some(episode) = self.episodes.get_mut(&episode_index) else {
            return;
        };
        episode
            .sensor_samples
            .entry(sensor_observation_key(channel_id, sensor_kind))
            .or_default()
            .push(SensorSample {
                timestamp_us,
                values,
            });
    }

    fn on_action(
        &mut self,
        channel_id: &str,
        command_kind: RobotCommandKind,
        timestamp_us: u64,
        values: Vec<f64>,
    ) {
        let Some(episode_index) = self.active_episode_index else {
            return;
        };
        let Some(episode) = self.episodes.get_mut(&episode_index) else {
            return;
        };
        episode
            .action_samples
            .entry(action_key(channel_id, command_kind))
            .or_default()
            .push(ActionSample {
                timestamp_us,
                values,
            });
    }

    fn on_packet(&mut self, channel_id: &str, header: &EncodedPacketHeader, payload: &[u8]) {
        let episode_index = header.episode_index;
        let Some(episode) = self.episodes.get_mut(&episode_index) else {
            // Late packet for an unknown episode (or one we already
            // dispatched): drop. Late join across an episode boundary
            // is an encoder bug, not an assembler one.
            return;
        };
        let Some(buffer) = episode.camera_streams.get_mut(channel_id) else {
            return;
        };
        match header.kind {
            EncodedPacketKind::Config => buffer.observe_config(header, payload),
            EncodedPacketKind::Packet => buffer.observe_packet(header, payload),
            EncodedPacketKind::EndOfStream => buffer.observe_eos(header),
        }
        if buffer.failed.is_some() && !episode.failed_cameras.iter().any(|id| id == channel_id) {
            episode.failed_cameras.push(channel_id.to_string());
        }
    }

    fn ready_failed_and_timed_out(&self, now_us: u64) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
        let mut ready = Vec::new();
        let mut failed = Vec::new();
        let mut timed_out = Vec::new();
        let timeout_us = self.config.missing_eos_timeout_ms.saturating_mul(1_000);
        let expected_cameras = self.config.cameras.len();

        for (episode_index, episode) in &self.episodes {
            if !episode.failed_cameras.is_empty() {
                failed.push(*episode_index);
                continue;
            }
            if !episode.keep_requested || episode.stop_time_us.is_none() {
                continue;
            }
            if episode.all_cameras_eos(expected_cameras) {
                ready.push(*episode_index);
                continue;
            }
            if let Some(wait_started_us) = episode.ready_wait_started_us {
                if now_us.saturating_sub(wait_started_us) > timeout_us {
                    timed_out.push(*episode_index);
                }
            }
        }
        (ready, failed, timed_out)
    }

    fn dispatch_ready_episodes(
        &mut self,
        worker: &mpsc::Sender<WorkerCommand>,
    ) -> Result<DispatchOutcome, Box<dyn Error>> {
        let now_us = unix_timestamp_us();
        let (ready_indices, failed_indices, timed_out_indices) =
            self.ready_failed_and_timed_out(now_us);
        let mut outcome = DispatchOutcome::default();

        for episode_index in failed_indices {
            if let Some(episode) = self.episodes.remove(&episode_index) {
                eprintln!(
                    "rollio-episode-lerobot: discarding episode {episode_index} due to packet stream failure ({:?})",
                    episode.failed_cameras
                );
                remove_episode_artifacts(&self.config.staging_dir, episode_index);
                outcome.dropped.push(DropNotice {
                    episode_index,
                    reason: "packet_stream_failure",
                    backpressure: false,
                });
                outcome.changed = true;
            }
        }

        for episode_index in timed_out_indices {
            if self.episodes.remove(&episode_index).is_some() {
                eprintln!(
                    "rollio-episode-lerobot: discarding episode {episode_index} after waiting {} ms for missing EndOfStream",
                    self.config.missing_eos_timeout_ms
                );
                remove_episode_artifacts(&self.config.staging_dir, episode_index);
                outcome.dropped.push(DropNotice {
                    episode_index,
                    reason: "missing_end_of_stream",
                    backpressure: false,
                });
                outcome.changed = true;
            }
        }

        for episode_index in ready_indices {
            let Some(episode) = self.episodes.remove(&episode_index) else {
                continue;
            };
            if self.in_flight.len() >= self.staging_slots {
                eprintln!(
                    "rollio-episode-lerobot: dropping ready episode {episode_index}: staging_slots exhausted ({} in flight)",
                    self.in_flight.len()
                );
                remove_episode_artifacts(&self.config.staging_dir, episode_index);
                outcome.dropped.push(DropNotice {
                    episode_index,
                    reason: "staging_slots_full",
                    backpressure: true,
                });
                outcome.changed = true;
                continue;
            }
            worker
                .send(WorkerCommand::Stage(episode.as_assembly_input()))
                .map_err(|error| -> Box<dyn Error> {
                    format!("staging worker disconnected: {error}").into()
                })?;
            self.in_flight.insert(episode_index, Instant::now());
            outcome.changed = true;
        }

        Ok(outcome)
    }
}

#[derive(Default)]
struct DispatchOutcome {
    changed: bool,
    dropped: Vec<DropNotice>,
}

#[derive(Debug, Clone, Copy)]
struct DropNotice {
    episode_index: u32,
    /// Short, machine-grep-able reason. Surfaces in `EpisodeDropped.reason`
    /// and (for `backpressure: true`) in the `BackpressureEvent.queue_name`.
    reason: &'static str,
    /// True when the drop is caused by the assembler running out of
    /// staging slots and should also block new `RecordingStart` commands
    /// at the controller via `BackpressureEvent`.
    backpressure: bool,
}

// ---------------------------------------------------------------------------
// Staging worker
// ---------------------------------------------------------------------------

enum WorkerCommand {
    Stage(EpisodeAssemblyInput),
    Shutdown,
}

enum WorkerEvent {
    Staged(StagedEpisode),
    Error(String),
    ShutdownComplete,
}

type StageWorkerHandles = (
    mpsc::Sender<WorkerCommand>,
    mpsc::Receiver<WorkerEvent>,
    thread::JoinHandle<()>,
);

fn spawn_stage_worker(
    config: AssemblerRuntimeConfigV2,
) -> Result<StageWorkerHandles, Box<dyn Error>> {
    let (cmd_tx, cmd_rx) = mpsc::channel::<WorkerCommand>();
    let (evt_tx, evt_rx) = mpsc::channel::<WorkerEvent>();
    let handle = thread::Builder::new()
        .name("rollio-lerobot-staging-worker".into())
        .spawn(move || stage_worker_main(config, cmd_rx, evt_tx))?;
    Ok((cmd_tx, evt_rx, handle))
}

fn stage_worker_main(
    config: AssemblerRuntimeConfigV2,
    cmd_rx: mpsc::Receiver<WorkerCommand>,
    evt_tx: mpsc::Sender<WorkerEvent>,
) {
    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            WorkerCommand::Stage(input) => match stage_episode(&config, &input) {
                Ok(staged) => {
                    let _ = evt_tx.send(WorkerEvent::Staged(staged));
                }
                Err(error) => {
                    let _ = evt_tx.send(WorkerEvent::Error(error.to_string()));
                }
            },
            WorkerCommand::Shutdown => break,
        }
    }
    let _ = evt_tx.send(WorkerEvent::ShutdownComplete);
}

// ---------------------------------------------------------------------------
// Top-level run
// ---------------------------------------------------------------------------

pub fn run(args: RunArgs) -> Result<(), Box<dyn Error>> {
    let config = load_runtime_config(&args)?;
    run_with_config(config)
}

fn load_runtime_config(args: &RunArgs) -> Result<AssemblerRuntimeConfigV2, Box<dyn Error>> {
    match (&args.config, &args.config_inline) {
        (Some(path), None) => Ok(AssemblerRuntimeConfigV2::from_file(path)?),
        (None, Some(inline)) => Ok(inline.parse::<AssemblerRuntimeConfigV2>()?),
        (None, None) => Err("rollio-episode-lerobot requires --config or --config-inline".into()),
        (Some(_), Some(_)) => {
            Err("rollio-episode-lerobot config flags are mutually exclusive".into())
        }
    }
}

pub fn run_with_config(config: AssemblerRuntimeConfigV2) -> Result<(), Box<dyn Error>> {
    if config.format != EpisodeFormat::LeRobotV2_1 {
        return Err(format!(
            "rollio-episode-lerobot supports format=lerobot-v2.1 only, got {:?}",
            config.format
        )
        .into());
    }

    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;

    let control_subscriber = create_control_subscriber(&node)?;
    let camera_subscribers = create_camera_subscribers(&node, &config)?;
    let episode_ready_publisher = create_episode_ready_publisher(&node)?;
    let backpressure_publisher = create_backpressure_publisher(&node)?;
    let episode_dropped_publisher = create_episode_dropped_publisher(&node)?;
    let episode_stored_subscriber = create_episode_stored_subscriber(&node)?;
    let observation_subscribers = create_observation_subscribers(&node, &config)?;
    let sensor_subscribers = create_sensor_subscribers(&node, &config)?;
    let action_subscribers = create_action_subscribers(&node, &config)?;

    let process_id = config.process_id.clone();
    let (worker_tx, worker_rx, worker_handle) = spawn_stage_worker(config.clone())?;
    let mut manager = EpisodeManager::new(config);

    let mut shutdown_requested = false;
    'main_loop: loop {
        let mut made_progress = false;
        if drain_control_events(&control_subscriber, &mut manager)? {
            shutdown_requested = true;
        }
        made_progress |= drain_camera_packets(&camera_subscribers, &mut manager)?;
        made_progress |= drain_observations(&observation_subscribers, &mut manager)?;
        made_progress |= drain_sensor_samples(&sensor_subscribers, &mut manager)?;
        made_progress |= drain_actions(&action_subscribers, &mut manager)?;

        let outcome = manager.dispatch_ready_episodes(&worker_tx)?;
        made_progress |= outcome.changed;
        for notice in outcome.dropped {
            publish_drop(
                &backpressure_publisher,
                &episode_dropped_publisher,
                &process_id,
                notice,
            )?;
        }

        while let Some(sample) = episode_stored_subscriber.receive()? {
            if manager.release_slot(sample.payload().episode_index) {
                made_progress = true;
            }
        }

        for leaked in manager.sweep_stale_slots(Instant::now()) {
            eprintln!(
                "rollio-episode-lerobot: staging slot for episode {leaked} appears leaked after {}ms; releasing",
                manager.stale_after.as_millis()
            );
            publish_drop(
                &backpressure_publisher,
                &episode_dropped_publisher,
                &process_id,
                DropNotice {
                    episode_index: leaked,
                    reason: "staging_slot_leak",
                    backpressure: true,
                },
            )?;
            made_progress = true;
        }

        while let Ok(event) = worker_rx.try_recv() {
            match event {
                WorkerEvent::Staged(staged) => {
                    episode_ready_publisher.send_copy(EpisodeReady {
                        episode_index: staged.episode_index,
                        staging_dir: FixedString256::new(&staged.staging_dir.to_string_lossy()),
                    })?;
                    made_progress = true;
                }
                WorkerEvent::Error(message) => {
                    eprintln!("rollio-episode-lerobot: staging worker error: {message}");
                }
                WorkerEvent::ShutdownComplete => break 'main_loop,
            }
        }

        if shutdown_requested && manager.episodes.is_empty() && manager.in_flight.is_empty() {
            let _ = worker_tx.send(WorkerCommand::Shutdown);
            shutdown_requested = false;
        }

        if made_progress {
            continue;
        }
        match node.wait(Duration::from_millis(2)) {
            Ok(()) => {}
            Err(NodeWaitFailure::Interrupt | NodeWaitFailure::TerminationRequest) => break,
        }
    }

    drop(worker_tx);
    let _ = worker_handle.join();
    Ok(())
}

fn publish_drop(
    backpressure_publisher: &iceoryx2::port::publisher::Publisher<
        ipc::Service,
        BackpressureEvent,
        (),
    >,
    episode_dropped_publisher: &iceoryx2::port::publisher::Publisher<
        ipc::Service,
        EpisodeDropped,
        (),
    >,
    process_id: &str,
    notice: DropNotice,
) -> Result<(), Box<dyn Error>> {
    if notice.backpressure {
        backpressure_publisher.send_copy(BackpressureEvent {
            process_id: FixedString64::new(process_id),
            queue_name: FixedString64::new(notice.reason),
        })?;
    }
    episode_dropped_publisher.send_copy(EpisodeDropped {
        episode_index: notice.episode_index,
        reason: FixedString64::new(notice.reason),
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Subscriber factories
// ---------------------------------------------------------------------------

fn create_control_subscriber(
    node: &Node<ipc::Service>,
) -> Result<iceoryx2::port::subscriber::Subscriber<ipc::Service, ControlEvent, ()>, Box<dyn Error>>
{
    let service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<ControlEvent>()
        .open_or_create()?;
    Ok(service.subscriber_builder().create()?)
}

fn create_episode_ready_publisher(
    node: &Node<ipc::Service>,
) -> Result<iceoryx2::port::publisher::Publisher<ipc::Service, EpisodeReady, ()>, Box<dyn Error>> {
    let service_name: ServiceName = EPISODE_READY_SERVICE.try_into()?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<EpisodeReady>()
        .open_or_create()?;
    Ok(service.publisher_builder().create()?)
}

fn create_backpressure_publisher(
    node: &Node<ipc::Service>,
) -> Result<
    iceoryx2::port::publisher::Publisher<ipc::Service, BackpressureEvent, ()>,
    Box<dyn Error>,
> {
    let service_name: ServiceName = BACKPRESSURE_SERVICE.try_into()?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<BackpressureEvent>()
        .open_or_create()?;
    Ok(service.publisher_builder().create()?)
}

fn create_episode_dropped_publisher(
    node: &Node<ipc::Service>,
) -> Result<iceoryx2::port::publisher::Publisher<ipc::Service, EpisodeDropped, ()>, Box<dyn Error>>
{
    let service_name: ServiceName = EPISODE_DROPPED_SERVICE.try_into()?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<EpisodeDropped>()
        .open_or_create()?;
    Ok(service.publisher_builder().create()?)
}

fn create_episode_stored_subscriber(
    node: &Node<ipc::Service>,
) -> Result<iceoryx2::port::subscriber::Subscriber<ipc::Service, EpisodeStored, ()>, Box<dyn Error>>
{
    let service_name: ServiceName = EPISODE_STORED_SERVICE.try_into()?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<EpisodeStored>()
        .open_or_create()?;
    Ok(service.subscriber_builder().create()?)
}

fn create_camera_subscribers(
    node: &Node<ipc::Service>,
    config: &AssemblerRuntimeConfigV2,
) -> Result<Vec<CameraSubscriber>, Box<dyn Error>> {
    let mut subs = Vec::with_capacity(config.cameras.len());
    for camera in &config.cameras {
        let config_service_name: ServiceName = camera.recording_config_topic.as_str().try_into()?;
        let config_service = node
            .service_builder(&config_service_name)
            .publish_subscribe::<[u8]>()
            .user_header::<EncodedPacketHeader>()
            .history_size(STREAM_CONFIG_HISTORY_SIZE)
            .subscriber_max_buffer_size(STREAM_CONFIG_HISTORY_SIZE.max(2))
            .max_publishers(16)
            .max_subscribers(16)
            .max_nodes(16)
            .open_or_create()?;
        let config_subscriber = config_service.subscriber_builder().create()?;

        let packet_service_name: ServiceName = camera.recording_packet_topic.as_str().try_into()?;
        let packet_service = node
            .service_builder(&packet_service_name)
            .publish_subscribe::<[u8]>()
            .user_header::<EncodedPacketHeader>()
            .enable_safe_overflow(false)
            .subscriber_max_buffer_size(rollio_bus::RECORDING_PACKET_BUFFER)
            .max_publishers(16)
            .max_subscribers(16)
            .max_nodes(16)
            .open_or_create()?;
        let packet_subscriber = packet_service.subscriber_builder().create()?;

        subs.push(CameraSubscriber {
            channel_id: camera.channel_id.clone(),
            config_subscriber,
            packet_subscriber,
        });
    }
    Ok(subs)
}

fn create_observation_subscribers(
    node: &Node<ipc::Service>,
    config: &AssemblerRuntimeConfigV2,
) -> Result<Vec<ObservationSubscriber>, Box<dyn Error>> {
    config
        .observations
        .iter()
        .map(|observation| {
            let service_name: ServiceName = observation.state_topic.as_str().try_into()?;
            let subscriber = match observation.state_kind {
                RobotStateKind::EndEffectorPose => {
                    let service = node
                        .service_builder(&service_name)
                        .publish_subscribe::<Pose7>()
                        .subscriber_max_buffer_size(STATE_BUFFER)
                        .history_size(STATE_BUFFER)
                        .max_publishers(STATE_MAX_PUBLISHERS)
                        .max_subscribers(STATE_MAX_SUBSCRIBERS)
                        .max_nodes(STATE_MAX_NODES)
                        .open_or_create()?;
                    ObservationSubscriberKind::Pose7(service.subscriber_builder().create()?)
                }
                RobotStateKind::ParallelPosition
                | RobotStateKind::ParallelVelocity
                | RobotStateKind::ParallelEffort => {
                    let service = node
                        .service_builder(&service_name)
                        .publish_subscribe::<ParallelVector2>()
                        .subscriber_max_buffer_size(STATE_BUFFER)
                        .history_size(STATE_BUFFER)
                        .max_publishers(STATE_MAX_PUBLISHERS)
                        .max_subscribers(STATE_MAX_SUBSCRIBERS)
                        .max_nodes(STATE_MAX_NODES)
                        .open_or_create()?;
                    ObservationSubscriberKind::ParallelVector2(
                        service.subscriber_builder().create()?,
                    )
                }
                _ => {
                    let service = node
                        .service_builder(&service_name)
                        .publish_subscribe::<JointVector15>()
                        .subscriber_max_buffer_size(STATE_BUFFER)
                        .history_size(STATE_BUFFER)
                        .max_publishers(STATE_MAX_PUBLISHERS)
                        .max_subscribers(STATE_MAX_SUBSCRIBERS)
                        .max_nodes(STATE_MAX_NODES)
                        .open_or_create()?;
                    ObservationSubscriberKind::JointVector15(service.subscriber_builder().create()?)
                }
            };
            Ok(ObservationSubscriber {
                config: observation.clone(),
                subscriber,
            })
        })
        .collect()
}

fn create_sensor_subscribers(
    node: &Node<ipc::Service>,
    config: &AssemblerRuntimeConfigV2,
) -> Result<Vec<SensorSubscriber>, Box<dyn Error>> {
    config
        .sensor_observations
        .iter()
        .map(|sensor| {
            let service_name: ServiceName = sensor.sample_topic.as_str().try_into()?;
            let service = node
                .service_builder(&service_name)
                .publish_subscribe::<[u8]>()
                .user_header::<SensorFrameHeader>()
                .subscriber_max_buffer_size(SAMPLE_BUFFER)
                .history_size(SAMPLE_BUFFER)
                .max_publishers(SAMPLE_MAX_PUBLISHERS)
                .max_subscribers(SAMPLE_MAX_SUBSCRIBERS)
                .max_nodes(SAMPLE_MAX_NODES)
                .open_or_create()?;
            let subscriber = service.subscriber_builder().create()?;
            Ok(SensorSubscriber {
                config: sensor.clone(),
                subscriber,
            })
        })
        .collect()
}

fn decode_sensor_payload(header: &SensorFrameHeader, bytes: &[u8]) -> Vec<f32> {
    let elements = header.element_count();
    let mut out = Vec::with_capacity(elements);
    let byte_size = header.dtype.byte_size();
    if elements == 0 || byte_size == 0 || bytes.len() < elements * byte_size {
        return out;
    }
    match header.dtype {
        SensorDType::F32 => {
            for i in 0..elements {
                let off = i * 4;
                let v = f32::from_le_bytes([
                    bytes[off],
                    bytes[off + 1],
                    bytes[off + 2],
                    bytes[off + 3],
                ]);
                out.push(v);
            }
        }
        SensorDType::F64 => {
            for i in 0..elements {
                let off = i * 8;
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&bytes[off..off + 8]);
                out.push(f64::from_le_bytes(buf) as f32);
            }
        }
        SensorDType::I32 => {
            for i in 0..elements {
                let off = i * 4;
                let v = i32::from_le_bytes([
                    bytes[off],
                    bytes[off + 1],
                    bytes[off + 2],
                    bytes[off + 3],
                ]) as f32;
                out.push(v);
            }
        }
        SensorDType::U32 => {
            for i in 0..elements {
                let off = i * 4;
                let v = u32::from_le_bytes([
                    bytes[off],
                    bytes[off + 1],
                    bytes[off + 2],
                    bytes[off + 3],
                ]) as f32;
                out.push(v);
            }
        }
        SensorDType::I16 => {
            for i in 0..elements {
                let off = i * 2;
                let v = i16::from_le_bytes([bytes[off], bytes[off + 1]]) as f32;
                out.push(v);
            }
        }
        SensorDType::U16 => {
            for i in 0..elements {
                let off = i * 2;
                let v = u16::from_le_bytes([bytes[off], bytes[off + 1]]) as f32;
                out.push(v);
            }
        }
        SensorDType::I8 => {
            for &b in &bytes[..elements] {
                out.push(i8::from_le_bytes([b]) as f32);
            }
        }
        SensorDType::U8 => {
            for &b in &bytes[..elements] {
                out.push(b as f32);
            }
        }
    }
    out
}

fn create_action_subscribers(
    node: &Node<ipc::Service>,
    config: &AssemblerRuntimeConfigV2,
) -> Result<Vec<ActionSubscriber>, Box<dyn Error>> {
    config
        .actions
        .iter()
        .map(|action| {
            let service_name: ServiceName = action.command_topic.as_str().try_into()?;
            let subscriber = match action.command_kind {
                RobotCommandKind::JointPosition => {
                    let service = node
                        .service_builder(&service_name)
                        .publish_subscribe::<JointVector15>()
                        .subscriber_max_buffer_size(STATE_BUFFER)
                        .history_size(STATE_BUFFER)
                        .max_publishers(STATE_MAX_PUBLISHERS)
                        .max_subscribers(STATE_MAX_SUBSCRIBERS)
                        .max_nodes(STATE_MAX_NODES)
                        .open_or_create()?;
                    ActionSubscriberKind::JointVector15(service.subscriber_builder().create()?)
                }
                RobotCommandKind::JointMit => {
                    let service = node
                        .service_builder(&service_name)
                        .publish_subscribe::<JointMitCommand15>()
                        .subscriber_max_buffer_size(STATE_BUFFER)
                        .history_size(STATE_BUFFER)
                        .max_publishers(STATE_MAX_PUBLISHERS)
                        .max_subscribers(STATE_MAX_SUBSCRIBERS)
                        .max_nodes(STATE_MAX_NODES)
                        .open_or_create()?;
                    ActionSubscriberKind::JointMitCommand15(service.subscriber_builder().create()?)
                }
                RobotCommandKind::ParallelPosition => {
                    let service = node
                        .service_builder(&service_name)
                        .publish_subscribe::<ParallelVector2>()
                        .subscriber_max_buffer_size(STATE_BUFFER)
                        .history_size(STATE_BUFFER)
                        .max_publishers(STATE_MAX_PUBLISHERS)
                        .max_subscribers(STATE_MAX_SUBSCRIBERS)
                        .max_nodes(STATE_MAX_NODES)
                        .open_or_create()?;
                    ActionSubscriberKind::ParallelVector2(service.subscriber_builder().create()?)
                }
                RobotCommandKind::ParallelMit => {
                    let service = node
                        .service_builder(&service_name)
                        .publish_subscribe::<ParallelMitCommand2>()
                        .subscriber_max_buffer_size(STATE_BUFFER)
                        .history_size(STATE_BUFFER)
                        .max_publishers(STATE_MAX_PUBLISHERS)
                        .max_subscribers(STATE_MAX_SUBSCRIBERS)
                        .max_nodes(STATE_MAX_NODES)
                        .open_or_create()?;
                    ActionSubscriberKind::ParallelMitCommand2(
                        service.subscriber_builder().create()?,
                    )
                }
                RobotCommandKind::EndPose => {
                    let service = node
                        .service_builder(&service_name)
                        .publish_subscribe::<Pose7>()
                        .subscriber_max_buffer_size(STATE_BUFFER)
                        .history_size(STATE_BUFFER)
                        .max_publishers(STATE_MAX_PUBLISHERS)
                        .max_subscribers(STATE_MAX_SUBSCRIBERS)
                        .max_nodes(STATE_MAX_NODES)
                        .open_or_create()?;
                    ActionSubscriberKind::Pose7(service.subscriber_builder().create()?)
                }
            };
            Ok(ActionSubscriber {
                config: action.clone(),
                subscriber,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Drain helpers
// ---------------------------------------------------------------------------

fn drain_control_events(
    subscriber: &iceoryx2::port::subscriber::Subscriber<ipc::Service, ControlEvent, ()>,
    manager: &mut EpisodeManager,
) -> Result<bool, Box<dyn Error>> {
    loop {
        let Some(sample) = subscriber.receive()? else {
            return Ok(false);
        };
        if manager.on_control_event(*sample.payload()) {
            return Ok(true);
        }
    }
}

fn drain_camera_packets(
    subscribers: &[CameraSubscriber],
    manager: &mut EpisodeManager,
) -> Result<bool, Box<dyn Error>> {
    let mut changed = false;
    for cam in subscribers {
        loop {
            let Some(sample) = cam.config_subscriber.receive()? else {
                break;
            };
            manager.on_packet(&cam.channel_id, sample.user_header(), sample.payload());
            changed = true;
        }
        loop {
            let Some(sample) = cam.packet_subscriber.receive()? else {
                break;
            };
            manager.on_packet(&cam.channel_id, sample.user_header(), sample.payload());
            changed = true;
        }
    }
    Ok(changed)
}

fn drain_observations(
    subscribers: &[ObservationSubscriber],
    manager: &mut EpisodeManager,
) -> Result<bool, Box<dyn Error>> {
    let mut changed = false;
    for observation in subscribers {
        match &observation.subscriber {
            ObservationSubscriberKind::JointVector15(subscriber) => loop {
                let Some(sample) = subscriber.receive()? else {
                    break;
                };
                let payload = *sample.payload();
                manager.on_observation(
                    &observation.config.channel_id,
                    observation.config.state_kind,
                    payload.timestamp_us,
                    payload.values[..payload.len as usize].to_vec(),
                );
                changed = true;
            },
            ObservationSubscriberKind::ParallelVector2(subscriber) => loop {
                let Some(sample) = subscriber.receive()? else {
                    break;
                };
                let payload = *sample.payload();
                manager.on_observation(
                    &observation.config.channel_id,
                    observation.config.state_kind,
                    payload.timestamp_us,
                    payload.values[..payload.len as usize].to_vec(),
                );
                changed = true;
            },
            ObservationSubscriberKind::Pose7(subscriber) => loop {
                let Some(sample) = subscriber.receive()? else {
                    break;
                };
                let payload = *sample.payload();
                manager.on_observation(
                    &observation.config.channel_id,
                    observation.config.state_kind,
                    payload.timestamp_us,
                    payload.values.to_vec(),
                );
                changed = true;
            },
        }
    }
    Ok(changed)
}

fn drain_sensor_samples(
    subscribers: &[SensorSubscriber],
    manager: &mut EpisodeManager,
) -> Result<bool, Box<dyn Error>> {
    let mut changed = false;
    for sensor in subscribers {
        loop {
            let Some(sample) = sensor.subscriber.receive()? else {
                break;
            };
            let header = *sample.user_header();
            let payload = sample.payload();
            let values = decode_sensor_payload(&header, payload);
            manager.on_sensor_sample(
                &sensor.config.channel_id,
                sensor.config.sensor_kind,
                header.timestamp_us,
                values,
            );
            changed = true;
        }
    }
    Ok(changed)
}

fn drain_actions(
    subscribers: &[ActionSubscriber],
    manager: &mut EpisodeManager,
) -> Result<bool, Box<dyn Error>> {
    let mut changed = false;
    for action in subscribers {
        match &action.subscriber {
            ActionSubscriberKind::JointVector15(subscriber) => loop {
                let Some(sample) = subscriber.receive()? else {
                    break;
                };
                let payload = *sample.payload();
                manager.on_action(
                    &action.config.channel_id,
                    action.config.command_kind,
                    payload.timestamp_us,
                    payload.values[..payload.len as usize].to_vec(),
                );
                changed = true;
            },
            ActionSubscriberKind::JointMitCommand15(subscriber) => loop {
                let Some(sample) = subscriber.receive()? else {
                    break;
                };
                let payload = *sample.payload();
                manager.on_action(
                    &action.config.channel_id,
                    action.config.command_kind,
                    payload.timestamp_us,
                    payload.position[..payload.len as usize].to_vec(),
                );
                changed = true;
            },
            ActionSubscriberKind::ParallelVector2(subscriber) => loop {
                let Some(sample) = subscriber.receive()? else {
                    break;
                };
                let payload = *sample.payload();
                manager.on_action(
                    &action.config.channel_id,
                    action.config.command_kind,
                    payload.timestamp_us,
                    payload.values[..payload.len as usize].to_vec(),
                );
                changed = true;
            },
            ActionSubscriberKind::ParallelMitCommand2(subscriber) => loop {
                let Some(sample) = subscriber.receive()? else {
                    break;
                };
                let payload = *sample.payload();
                manager.on_action(
                    &action.config.channel_id,
                    action.config.command_kind,
                    payload.timestamp_us,
                    payload.position[..payload.len as usize].to_vec(),
                );
                changed = true;
            },
            ActionSubscriberKind::Pose7(subscriber) => loop {
                let Some(sample) = subscriber.receive()? else {
                    break;
                };
                let payload = *sample.payload();
                manager.on_action(
                    &action.config.channel_id,
                    action.config.command_kind,
                    payload.timestamp_us,
                    payload.values.to_vec(),
                );
                changed = true;
            },
        }
    }
    Ok(changed)
}

fn unix_timestamp_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

// Suppress unused-import warning for `HashMap` (kept here for the
// sequence-buffer growth metrics that will land with the smoke tests).
#[allow(dead_code)]
fn _unused_hashmap() -> HashMap<u32, ()> {
    HashMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollio_types::config::EpisodeFormat;

    fn manager_with_slots(staging_slots: u32) -> EpisodeManager {
        let config = AssemblerRuntimeConfigV2 {
            process_id: "episode-lerobot-test".into(),
            format: EpisodeFormat::LeRobotV2_1,
            fps: 30,
            chunk_size: 1000,
            missing_eos_timeout_ms: 5000,
            staging_dir: std::env::temp_dir()
                .join("rollio-slot-test")
                .to_string_lossy()
                .into_owned(),
            staging_slots,
            cameras: Vec::new(),
            observations: Vec::new(),
            actions: Vec::new(),
            embedded_config_toml: "stub".into(),
        };
        EpisodeManager::new(config)
    }

    fn push_ready_episode(manager: &mut EpisodeManager, episode_index: u32) {
        let mut episode = PendingEpisode::new(episode_index, 1_000_000 + episode_index as u64, &[]);
        episode.stop_time_us = Some(episode.start_time_us + 1_000_000);
        episode.keep_requested = true;
        manager.episodes.insert(episode_index, episode);
    }

    #[test]
    fn dispatch_drops_episode_when_staging_slots_exhausted() {
        let mut manager = manager_with_slots(1);
        push_ready_episode(&mut manager, 0);
        push_ready_episode(&mut manager, 1);

        let (worker_tx, worker_rx) = mpsc::channel::<WorkerCommand>();
        let outcome = manager
            .dispatch_ready_episodes(&worker_tx)
            .expect("dispatch should succeed");

        let staged: Vec<_> = worker_rx.try_iter().collect();
        assert_eq!(
            staged.len(),
            1,
            "exactly one episode should be staged when staging_slots = 1",
        );
        assert!(matches!(staged[0], WorkerCommand::Stage(_)));

        assert_eq!(outcome.dropped.len(), 1, "the second episode must be dropped");
        let drop = outcome.dropped[0];
        assert_eq!(drop.episode_index, 1);
        assert_eq!(drop.reason, "staging_slots_full");
        assert!(drop.backpressure, "slot-full drop must also raise backpressure");

        assert_eq!(manager.in_flight.len(), 1);
        assert!(manager.in_flight.contains_key(&0));
        assert!(
            manager.episodes.is_empty(),
            "both ready episodes (staged + dropped) leave the manager",
        );
    }

    #[test]
    fn release_slot_frees_in_flight_entry() {
        let mut manager = manager_with_slots(2);
        push_ready_episode(&mut manager, 7);
        let (worker_tx, _worker_rx) = mpsc::channel::<WorkerCommand>();
        manager
            .dispatch_ready_episodes(&worker_tx)
            .expect("dispatch should succeed");
        assert_eq!(manager.in_flight.len(), 1);
        assert!(manager.release_slot(7));
        assert!(!manager.release_slot(7), "second release is a no-op");
        assert!(manager.in_flight.is_empty());
    }

    #[test]
    fn sweep_stale_slots_releases_entries_older_than_threshold() {
        let mut manager = manager_with_slots(2);
        manager.stale_after = Duration::from_millis(0);
        push_ready_episode(&mut manager, 3);
        let (worker_tx, _worker_rx) = mpsc::channel::<WorkerCommand>();
        manager
            .dispatch_ready_episodes(&worker_tx)
            .expect("dispatch should succeed");
        // Force the sample to look older than `stale_after`.
        std::thread::sleep(Duration::from_millis(2));
        let leaked = manager.sweep_stale_slots(Instant::now());
        assert_eq!(leaked, vec![3]);
        assert!(manager.in_flight.is_empty());
    }
}
