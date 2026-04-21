use crate::dataset::{
    action_key, observation_key, remove_episode_artifacts, stage_episode, ActionSample,
    EpisodeAssemblyInput, ObservationSample, StagedEpisode,
};
use clap::Args;
use iceoryx2::node::NodeWaitFailure;
use iceoryx2::prelude::*;
use rollio_bus::{
    CONTROL_EVENTS_SERVICE, EPISODE_READY_SERVICE, STATE_BUFFER, STATE_MAX_NODES,
    STATE_MAX_PUBLISHERS, STATE_MAX_SUBSCRIBERS, VIDEO_READY_SERVICE,
};
use rollio_types::config::{
    AssemblerActionRuntimeConfigV2, AssemblerObservationRuntimeConfigV2, AssemblerRuntimeConfigV2,
    EncodedHandoffMode, EpisodeFormat, RobotCommandKind, RobotStateKind,
};
use rollio_types::messages::{
    ControlEvent, EpisodeReady, FixedString256, JointMitCommand15, JointVector15,
    ParallelMitCommand2, ParallelVector2, Pose7, VideoReady,
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

#[derive(Debug, Clone)]
struct PendingEpisode {
    episode_index: u32,
    start_time_us: u64,
    stop_time_us: Option<u64>,
    keep_requested: bool,
    ready_wait_started_us: Option<u64>,
    observation_samples: BTreeMap<String, Vec<ObservationSample>>,
    action_samples: BTreeMap<String, Vec<ActionSample>>,
    video_paths: BTreeMap<String, PathBuf>,
}

impl PendingEpisode {
    fn new(episode_index: u32, start_time_us: u64) -> Self {
        Self {
            episode_index,
            start_time_us,
            stop_time_us: None,
            keep_requested: false,
            ready_wait_started_us: None,
            observation_samples: BTreeMap::new(),
            action_samples: BTreeMap::new(),
            video_paths: BTreeMap::new(),
        }
    }

    fn as_assembly_input(&self) -> EpisodeAssemblyInput {
        EpisodeAssemblyInput {
            episode_index: self.episode_index,
            start_time_us: self.start_time_us,
            stop_time_us: self.stop_time_us.unwrap_or(self.start_time_us),
            observation_samples: self.observation_samples.clone(),
            action_samples: self.action_samples.clone(),
            video_paths: self.video_paths.clone(),
        }
    }
}

struct EpisodeManager {
    config: AssemblerRuntimeConfigV2,
    active_episode_index: Option<u32>,
    episodes: BTreeMap<u32, PendingEpisode>,
    camera_by_process_id: HashMap<String, String>,
}

impl EpisodeManager {
    fn new(config: AssemblerRuntimeConfigV2) -> Self {
        let camera_by_process_id = config
            .cameras
            .iter()
            .map(|camera| (camera.encoder_process_id.clone(), camera.channel_id.clone()))
            .collect();
        Self {
            config,
            active_episode_index: None,
            episodes: BTreeMap::new(),
            camera_by_process_id,
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
                    PendingEpisode::new(episode_index, controller_ts_us),
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
                }
                if self.active_episode_index == Some(episode_index) {
                    self.active_episode_index = None;
                }
                false
            }
            ControlEvent::EpisodeKeep { episode_index } => {
                if let Some(episode) = self.episodes.get_mut(&episode_index) {
                    episode.keep_requested = true;
                    episode.ready_wait_started_us = Some(unix_timestamp_us());
                }
                false
            }
            ControlEvent::EpisodeDiscard { episode_index } => {
                if let Some(episode) = self.episodes.remove(&episode_index) {
                    remove_episode_artifacts(&episode.as_assembly_input());
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

    fn on_video_ready(&mut self, ready: VideoReady) {
        let Some(channel_id) = self.camera_by_process_id.get(ready.process_id.as_str()) else {
            eprintln!(
                "rollio-episode-assembler: ignoring video_ready from unknown process {}",
                ready.process_id.as_str()
            );
            return;
        };
        let Some(episode) = self.episodes.get_mut(&ready.episode_index) else {
            eprintln!(
                "rollio-episode-assembler: ignoring video_ready for unknown episode {}",
                ready.episode_index
            );
            return;
        };
        episode
            .video_paths
            .insert(channel_id.clone(), PathBuf::from(ready.file_path.as_str()));
    }

    fn ready_and_timed_out_episode_indices(&self, now_us: u64) -> (Vec<u32>, Vec<u32>) {
        let mut ready = Vec::new();
        let mut timed_out = Vec::new();
        let timeout_us = self.config.missing_video_timeout_ms.saturating_mul(1_000);

        for (episode_index, episode) in &self.episodes {
            if !episode.keep_requested || episode.stop_time_us.is_none() {
                continue;
            }
            if episode.video_paths.len() == self.config.cameras.len() {
                ready.push(*episode_index);
                continue;
            }
            if let Some(wait_started_us) = episode.ready_wait_started_us {
                if now_us.saturating_sub(wait_started_us) > timeout_us {
                    timed_out.push(*episode_index);
                }
            }
        }

        (ready, timed_out)
    }

    /// Hand off ready / timed-out episodes to the staging worker.
    ///
    /// Staging (parquet write + raw dump + video file moves) used to run
    /// inline on the main loop, which blocked the iceoryx2 subscriber drain
    /// for the full duration. With Phase 6b that work moves to a dedicated
    /// `rollio-assembler-worker` thread (see `spawn_stage_worker`) so the
    /// 250 Hz state subscribers keep getting drained even while a heavy
    /// stage is in flight.
    fn dispatch_ready_episodes(
        &mut self,
        worker: &mpsc::Sender<WorkerCommand>,
    ) -> Result<bool, Box<dyn Error>> {
        let now_us = unix_timestamp_us();
        let (ready_episode_indices, timed_out_episode_indices) =
            self.ready_and_timed_out_episode_indices(now_us);
        let mut changed = false;

        for episode_index in timed_out_episode_indices {
            if let Some(episode) = self.episodes.remove(&episode_index) {
                eprintln!(
                    "rollio-episode-assembler: discarding episode {} after waiting {} ms for missing videos",
                    episode_index, self.config.missing_video_timeout_ms
                );
                remove_episode_artifacts(&episode.as_assembly_input());
                changed = true;
            }
        }

        for episode_index in ready_episode_indices {
            let Some(episode) = self.episodes.remove(&episode_index) else {
                continue;
            };
            worker
                .send(WorkerCommand::Stage(episode.as_assembly_input()))
                .map_err(|error| -> Box<dyn Error> {
                    format!("episode assembler worker disconnected: {error}").into()
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

/// Handles returned by `spawn_stage_worker`: the command channel into the
/// worker, the event channel out of it, and the thread join handle.
type StageWorkerHandles = (
    mpsc::Sender<WorkerCommand>,
    mpsc::Receiver<WorkerEvent>,
    thread::JoinHandle<()>,
);

/// Spawn the dedicated staging thread.
///
/// The main loop sends `WorkerCommand::Stage(...)` for every episode that
/// should be persisted (or `WorkerCommand::Shutdown` on a clean exit) and
/// receives `WorkerEvent::Staged(...)` so it can publish `EpisodeReady`
/// without ever calling the synchronous `stage_episode` itself.
fn spawn_stage_worker(
    config: AssemblerRuntimeConfigV2,
) -> Result<StageWorkerHandles, Box<dyn Error>> {
    let (cmd_tx, cmd_rx) = mpsc::channel::<WorkerCommand>();
    let (evt_tx, evt_rx) = mpsc::channel::<WorkerEvent>();
    let handle = thread::Builder::new()
        .name("rollio-assembler-worker".into())
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

pub fn run(args: RunArgs) -> Result<(), Box<dyn Error>> {
    let config = load_runtime_config(&args)?;
    run_with_config(config)
}

fn load_runtime_config(args: &RunArgs) -> Result<AssemblerRuntimeConfigV2, Box<dyn Error>> {
    match (&args.config, &args.config_inline) {
        (Some(path), None) => Ok(AssemblerRuntimeConfigV2::from_file(path)?),
        (None, Some(inline)) => Ok(inline.parse::<AssemblerRuntimeConfigV2>()?),
        (None, None) => Err("episode assembler requires --config or --config-inline".into()),
        (Some(_), Some(_)) => Err("episode assembler config flags are mutually exclusive".into()),
    }
}

pub fn run_with_config(config: AssemblerRuntimeConfigV2) -> Result<(), Box<dyn Error>> {
    if config.encoded_handoff != EncodedHandoffMode::File {
        return Err("episode assembler currently supports encoded_handoff=file only".into());
    }
    if config.format != EpisodeFormat::LeRobotV2_1 {
        return Err("episode assembler currently supports format=lerobot-v2.1 only".into());
    }

    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;

    let control_subscriber = create_control_subscriber(&node)?;
    let video_ready_subscriber = create_video_ready_subscriber(&node)?;
    let episode_ready_publisher = create_episode_ready_publisher(&node)?;
    let observation_subscribers = create_observation_subscribers(&node, &config)?;
    let action_subscribers = create_action_subscribers(&node, &config)?;

    // Phase 6b: stage_episode + raw-dump persistence runs on a dedicated
    // worker thread. The main loop never blocks on disk I/O, so it can
    // keep draining the 250 Hz state subscribers as fast as iceoryx2 lets
    // it.
    let (worker_tx, worker_rx, worker_handle) = spawn_stage_worker(config.clone())?;
    let mut manager = EpisodeManager::new(config);

    let mut shutdown_requested = false;
    'main_loop: loop {
        let mut made_progress = false;
        if drain_control_events(&control_subscriber, &mut manager)? {
            shutdown_requested = true;
        }
        made_progress |= drain_video_ready(&video_ready_subscriber, &mut manager)?;
        made_progress |= drain_observations(&observation_subscribers, &mut manager)?;
        made_progress |= drain_actions(&action_subscribers, &mut manager)?;
        made_progress |= manager.dispatch_ready_episodes(&worker_tx)?;

        // Drain any worker-completion events ASAP so EpisodeReady is
        // published promptly.
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
                    eprintln!(
                        "rollio-episode-assembler: staging worker error: {}",
                        message
                    );
                }
                WorkerEvent::ShutdownComplete => {
                    break 'main_loop;
                }
            }
        }

        if shutdown_requested && manager.episodes.is_empty() {
            // No more outstanding work — request worker shutdown and drain
            // the final ShutdownComplete event before exiting.
            let _ = worker_tx.send(WorkerCommand::Shutdown);
            shutdown_requested = false;
        }

        if made_progress {
            continue;
        }

        // 2 ms idle wait keeps the maximum sample-loss window well under
        // the `STATE_BUFFER` ring depth (~4 s at 250 Hz). With 1024-slot
        // buffers and a 2 ms wakeup, the assembler tolerates pathological
        // worker stalls of up to ~4 s before any 250 Hz publisher would
        // overwrite an unread sample.
        match node.wait(Duration::from_millis(2)) {
            Ok(()) => {}
            Err(NodeWaitFailure::Interrupt | NodeWaitFailure::TerminationRequest) => break,
        }
    }

    // Best-effort worker join. If the worker is still busy at shutdown,
    // dropping the sender lets it observe the channel close and exit.
    drop(worker_tx);
    let _ = worker_handle.join();

    Ok(())
}

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

fn create_video_ready_subscriber(
    node: &Node<ipc::Service>,
) -> Result<iceoryx2::port::subscriber::Subscriber<ipc::Service, VideoReady, ()>, Box<dyn Error>> {
    let service_name: ServiceName = VIDEO_READY_SERVICE.try_into()?;
    // Match the encoder's quotas — see the comment in
    // `encoder::runtime::run`. Either the assembler or one of the encoders
    // can win the race to create this service, and `open_or_create` rejects
    // a service whose existing config doesn't satisfy the requested
    // `max_publishers`. Stating the same caps on both sides means whichever
    // creates first sets a quota that all encoders + the assembler can
    // share.
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<VideoReady>()
        .max_publishers(16)
        .max_subscribers(8)
        .max_nodes(16)
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

fn create_observation_subscribers(
    node: &Node<ipc::Service>,
    config: &AssemblerRuntimeConfigV2,
) -> Result<Vec<ObservationSubscriber>, Box<dyn Error>> {
    // The assembler is the consumer side of the loss-point-A fix: every
    // state subscription is opened with the same `STATE_BUFFER` ring depth
    // the producers ask for. iceoryx2's `open_or_create` requires both
    // sides to agree on these caps, so this MUST stay in sync with the
    // robot drivers' service builders.
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

fn drain_video_ready(
    subscriber: &iceoryx2::port::subscriber::Subscriber<ipc::Service, VideoReady, ()>,
    manager: &mut EpisodeManager,
) -> Result<bool, Box<dyn Error>> {
    let mut changed = false;
    loop {
        let Some(sample) = subscriber.receive()? else {
            return Ok(changed);
        };
        manager.on_video_ready(*sample.payload());
        changed = true;
    }
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
