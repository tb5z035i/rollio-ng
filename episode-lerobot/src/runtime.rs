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
    action_key, observation_key, remove_episode_artifacts, stage_episode, ActionSample,
    EpisodeAssemblyInput, ObservationSample, StagedEpisode,
};
use crate::packets::RecordingStreamBuffer;
use clap::Args;
use iceoryx2::node::NodeWaitFailure;
use iceoryx2::prelude::*;
use rollio_bus::{
    CONTROL_EVENTS_SERVICE, EPISODE_READY_SERVICE, STATE_BUFFER, STATE_MAX_NODES,
    STATE_MAX_PUBLISHERS, STATE_MAX_SUBSCRIBERS, STREAM_CONFIG_HISTORY_SIZE,
};
use rollio_types::config::{
    AssemblerActionRuntimeConfigV2, AssemblerObservationRuntimeConfigV2, AssemblerRuntimeConfigV2,
    EpisodeFormat, RobotCommandKind, RobotStateKind,
};
use rollio_types::messages::{
    ControlEvent, EncodedPacketHeader, EncodedPacketKind, EpisodeReady, FixedString256,
    JointMitCommand15, JointVector15, ParallelMitCommand2, ParallelVector2, Pose7,
};
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
}

impl EpisodeManager {
    fn new(config: AssemblerRuntimeConfigV2) -> Self {
        let camera_channel_ids = config
            .cameras
            .iter()
            .map(|c| c.channel_id.clone())
            .collect();
        Self {
            config,
            active_episode_index: None,
            episodes: BTreeMap::new(),
            camera_channel_ids,
        }
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
    ) -> Result<bool, Box<dyn Error>> {
        let now_us = unix_timestamp_us();
        let (ready_indices, failed_indices, timed_out_indices) =
            self.ready_failed_and_timed_out(now_us);
        let mut changed = false;

        for episode_index in failed_indices {
            if let Some(episode) = self.episodes.remove(&episode_index) {
                eprintln!(
                    "rollio-episode-lerobot: discarding episode {episode_index} due to packet stream failure ({:?})",
                    episode.failed_cameras
                );
                remove_episode_artifacts(&self.config.staging_dir, episode_index);
                changed = true;
            }
        }

        for episode_index in timed_out_indices {
            if self.episodes.remove(&episode_index).is_some() {
                eprintln!(
                    "rollio-episode-lerobot: discarding episode {episode_index} after waiting {} ms for missing EndOfStream",
                    self.config.missing_eos_timeout_ms
                );
                remove_episode_artifacts(&self.config.staging_dir, episode_index);
                changed = true;
            }
        }

        for episode_index in ready_indices {
            let Some(episode) = self.episodes.remove(&episode_index) else {
                continue;
            };
            worker
                .send(WorkerCommand::Stage(episode.as_assembly_input()))
                .map_err(|error| -> Box<dyn Error> {
                    format!("staging worker disconnected: {error}").into()
                })?;
            changed = true;
        }

        Ok(changed)
    }
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
    let observation_subscribers = create_observation_subscribers(&node, &config)?;
    let action_subscribers = create_action_subscribers(&node, &config)?;

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
        made_progress |= drain_actions(&action_subscribers, &mut manager)?;
        made_progress |= manager.dispatch_ready_episodes(&worker_tx)?;

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

        if shutdown_requested && manager.episodes.is_empty() {
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
