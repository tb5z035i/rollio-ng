//! `rollio-episode-mcap` runtime — MCAP+FlatBuffers episode assembler.
//!
//! Subscribes to:
//! - `CONTROL_EVENTS_SERVICE` for episode lifecycle
//! - per-camera `…/recording-config` (history=1) + `…/recording-packets`
//! - per-observation state topic
//! - per-action command topic
//! - `EPISODE_READY_SERVICE` (publisher)
//!
//! Maintains per-`(channel, episode)` packet buffers, validates
//! sequence numbers as packets arrive, and stages the episode as an
//! MCAP file after every camera has emitted `EndOfStream`.

use crate::encode;
use crate::mcap_writer::{us_to_ns, McapEpisodeWriter, SchemaType};
use clap::Args;
use iceoryx2::node::NodeWaitFailure;
use iceoryx2::prelude::*;
use rollio_bus::{
    BACKPRESSURE_SERVICE, CONTROL_EVENTS_SERVICE, EPISODE_DROPPED_SERVICE, EPISODE_READY_SERVICE,
    EPISODE_STORED_SERVICE, STATE_BUFFER, STATE_MAX_NODES, STATE_MAX_PUBLISHERS,
    STATE_MAX_SUBSCRIBERS, STREAM_CONFIG_HISTORY_SIZE,
};
use rollio_types::config::{
    AssemblerActionRuntimeConfigV2, AssemblerObservationRuntimeConfigV2, AssemblerRuntimeConfigV2,
    EpisodeFormat, RobotCommandKind, RobotStateKind,
};
use rollio_types::messages::{
    BackpressureEvent, ControlEvent, EncodedPacketHeader, EncodedPacketKind, EpisodeDropped,
    EpisodeReady, EpisodeStored, FixedString256, FixedString64, JointMitCommand15, JointVector15,
    ParallelMitCommand2, ParallelVector2, Pose7,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
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
// Robot state / action subscribers
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

/// A single observation or action sample with timestamp and values.
#[derive(Debug, Clone)]
pub struct Sample {
    pub timestamp_us: u64,
    pub values: Vec<f64>,
}

/// Per-camera recording stream buffer for MCAP output.
#[derive(Debug, Clone, Default)]
struct CameraStreamBuffer {
    /// Codec config packet (SPS/PPS for H.264).
    config_data: Option<Vec<u8>>,
    /// Accumulated video packets for this episode.
    packets: Vec<CameraPacket>,
    /// Whether EndOfStream has been received.
    eos_received: bool,
    /// Sequence validation.
    expected_seq: u64,
    /// Failure reason if sequence error detected.
    failed: Option<String>,
}

#[derive(Debug, Clone)]
struct CameraPacket {
    timestamp_us: u64,
    data: Vec<u8>,
}

impl CameraStreamBuffer {
    fn observe_config(&mut self, header: &EncodedPacketHeader, payload: &[u8]) {
        self.config_data = Some(payload.to_vec());
        self.expected_seq = header.sequence_number.wrapping_add(1);
    }

    fn observe_packet(&mut self, header: &EncodedPacketHeader, payload: &[u8]) {
        if self.failed.is_some() {
            return;
        }
        if header.sequence_number != self.expected_seq && self.expected_seq != 0 {
            self.failed = Some(format!(
                "sequence gap: expected {}, got {}",
                self.expected_seq, header.sequence_number
            ));
            return;
        }
        self.expected_seq = header.sequence_number.wrapping_add(1);
        self.packets.push(CameraPacket {
            timestamp_us: header.source_timestamp_us,
            data: payload.to_vec(),
        });
    }

    fn observe_eos(&mut self, _header: &EncodedPacketHeader) {
        self.eos_received = true;
    }
}

#[derive(Debug, Clone)]
struct PendingEpisode {
    episode_index: u32,
    start_time_us: u64,
    stop_time_us: Option<u64>,
    keep_requested: bool,
    ready_wait_started_us: Option<u64>,
    observation_samples: BTreeMap<String, Vec<Sample>>,
    action_samples: BTreeMap<String, Vec<Sample>>,
    camera_streams: BTreeMap<String, CameraStreamBuffer>,
    failed_cameras: Vec<String>,
}

impl PendingEpisode {
    fn new(episode_index: u32, start_time_us: u64, channel_ids: &[String]) -> Self {
        let mut camera_streams = BTreeMap::new();
        for channel_id in channel_ids {
            camera_streams.insert(channel_id.clone(), CameraStreamBuffer::default());
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

    fn all_cameras_eos(&self, expected: usize) -> bool {
        self.camera_streams
            .values()
            .filter(|s| s.eos_received)
            .count()
            == expected
    }
}

// ---------------------------------------------------------------------------
// Episode Manager
// ---------------------------------------------------------------------------

struct EpisodeManager {
    config: AssemblerRuntimeConfigV2,
    active_episode_index: Option<u32>,
    episodes: BTreeMap<u32, PendingEpisode>,
    camera_channel_ids: Vec<String>,
    /// Episodes the assembler has handed to the stage worker but not yet
    /// observed `EpisodeStored` for. Decremented when the matching
    /// `EpisodeStored` arrives or when the stale-slot sweep reclaims a
    /// leaked entry.
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
        let stale_after = Duration::from_millis(config.missing_eos_timeout_ms.saturating_mul(2));
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
                self.episodes.remove(&episode_index);
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
        let key = format!("{}/{}", channel_id, state_kind.topic_suffix());
        episode
            .observation_samples
            .entry(key)
            .or_default()
            .push(Sample {
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
        let key = format!("{}/{}", channel_id, command_kind.topic_suffix());
        episode.action_samples.entry(key).or_default().push(Sample {
            timestamp_us,
            values,
        });
    }

    fn on_packet(&mut self, channel_id: &str, header: &EncodedPacketHeader, payload: &[u8]) {
        let episode_index = header.episode_index;
        let Some(episode) = self.episodes.get_mut(&episode_index) else {
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

    fn dispatch_ready_episodes(
        &mut self,
        worker: &mpsc::Sender<WorkerCommand>,
    ) -> Result<DispatchOutcome, Box<dyn Error>> {
        let now_us = unix_timestamp_us();
        let timeout_us = self.config.missing_eos_timeout_ms.saturating_mul(1_000);
        let expected_cameras = self.config.cameras.len();

        let mut ready = Vec::new();
        let mut failed = Vec::new();
        let mut timed_out = Vec::new();

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

        let mut outcome = DispatchOutcome::default();

        for episode_index in failed {
            if self.episodes.remove(&episode_index).is_some() {
                log::warn!(
                    "rollio-episode-mcap: discarding episode {episode_index} due to packet stream failure"
                );
                remove_mcap_episode_artifacts(&self.config.staging_dir, episode_index);
                outcome.dropped.push(DropNotice {
                    episode_index,
                    reason: "packet_stream_failure",
                    backpressure: false,
                });
                outcome.changed = true;
            }
        }

        for episode_index in timed_out {
            if self.episodes.remove(&episode_index).is_some() {
                log::warn!(
                    "rollio-episode-mcap: discarding episode {episode_index} after EOS timeout"
                );
                remove_mcap_episode_artifacts(&self.config.staging_dir, episode_index);
                outcome.dropped.push(DropNotice {
                    episode_index,
                    reason: "missing_end_of_stream",
                    backpressure: false,
                });
                outcome.changed = true;
            }
        }

        for episode_index in ready {
            let Some(episode) = self.episodes.remove(&episode_index) else {
                continue;
            };
            if self.in_flight.len() >= self.staging_slots {
                log::warn!(
                    "rollio-episode-mcap: dropping ready episode {episode_index}: staging_slots exhausted ({} in flight)",
                    self.in_flight.len()
                );
                remove_mcap_episode_artifacts(&self.config.staging_dir, episode_index);
                outcome.dropped.push(DropNotice {
                    episode_index,
                    reason: "staging_slots_full",
                    backpressure: true,
                });
                outcome.changed = true;
                continue;
            }
            worker
                .send(WorkerCommand::Stage(episode))
                .map_err(|e| -> Box<dyn Error> {
                    format!("staging worker disconnected: {e}").into()
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
    reason: &'static str,
    backpressure: bool,
}

fn remove_mcap_episode_artifacts(staging_root: &str, episode_index: u32) {
    let dir = Path::new(staging_root).join(format!("episode_{episode_index:06}"));
    if dir.exists() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

// ---------------------------------------------------------------------------
// Staging worker
// ---------------------------------------------------------------------------

enum WorkerCommand {
    Stage(PendingEpisode),
    Shutdown,
}

struct StagedResult {
    episode_index: u32,
    staging_dir: PathBuf,
}

enum WorkerEvent {
    Staged(StagedResult),
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
    bfbs_dir: PathBuf,
) -> Result<StageWorkerHandles, Box<dyn Error>> {
    let (cmd_tx, cmd_rx) = mpsc::channel::<WorkerCommand>();
    let (evt_tx, evt_rx) = mpsc::channel::<WorkerEvent>();
    let handle = thread::Builder::new()
        .name("rollio-mcap-staging-worker".into())
        .spawn(move || stage_worker_main(config, bfbs_dir, cmd_rx, evt_tx))?;
    Ok((cmd_tx, evt_rx, handle))
}

fn stage_worker_main(
    config: AssemblerRuntimeConfigV2,
    bfbs_dir: PathBuf,
    cmd_rx: mpsc::Receiver<WorkerCommand>,
    evt_tx: mpsc::Sender<WorkerEvent>,
) {
    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            WorkerCommand::Stage(episode) => {
                match stage_episode_mcap(&config, &bfbs_dir, &episode) {
                    Ok(result) => {
                        let _ = evt_tx.send(WorkerEvent::Staged(result));
                    }
                    Err(error) => {
                        let _ = evt_tx.send(WorkerEvent::Error(error.to_string()));
                    }
                }
            }
            WorkerCommand::Shutdown => break,
        }
    }
    let _ = evt_tx.send(WorkerEvent::ShutdownComplete);
}

/// Write a pending episode to an MCAP file in the staging directory.
fn stage_episode_mcap(
    config: &AssemblerRuntimeConfigV2,
    bfbs_dir: &Path,
    episode: &PendingEpisode,
) -> Result<StagedResult, Box<dyn Error>> {
    let episode_dir =
        Path::new(&config.staging_dir).join(format!("episode_{:06}", episode.episode_index));
    std::fs::create_dir_all(&episode_dir)?;

    let mcap_path = episode_dir.join("episode.mcap");
    let mut writer = McapEpisodeWriter::new(&mcap_path, bfbs_dir)?;

    // Register camera channels and write video packets
    for camera_config in &config.cameras {
        let channel_idx = writer.add_channel(
            &format!("/camera/{}/video", camera_config.channel_id),
            SchemaType::CompressedVideo,
            bfbs_dir,
        )?;

        if let Some(stream) = episode.camera_streams.get(&camera_config.channel_id) {
            // Determine video format from codec enum
            let format = match camera_config.codec {
                rollio_types::config::EncoderCodec::H264 => "h264",
                rollio_types::config::EncoderCodec::H265 => "h265",
                rollio_types::config::EncoderCodec::Av1 => "av1",
                rollio_types::config::EncoderCodec::Mjpg => "mjpeg",
                rollio_types::config::EncoderCodec::Rvl => "rvl",
            };

            for packet in &stream.packets {
                let fb_data = encode::encode_compressed_video(
                    packet.timestamp_us,
                    &camera_config.channel_id,
                    format,
                    &packet.data,
                );
                writer.write_message(channel_idx, us_to_ns(packet.timestamp_us), &fb_data)?;
            }
        }
    }

    // Register observation channels and write samples
    for (key, samples) in &episode.observation_samples {
        let channel_idx = writer.add_channel(
            &format!("/observation/{key}"),
            SchemaType::JointStates,
            bfbs_dir,
        )?;
        for sample in samples {
            let fb_data = encode::encode_joint_states(sample.timestamp_us, &sample.values, None);
            writer.write_message(channel_idx, us_to_ns(sample.timestamp_us), &fb_data)?;
        }
    }

    // Register action channels and write samples
    for (key, samples) in &episode.action_samples {
        let channel_idx =
            writer.add_channel(&format!("/action/{key}"), SchemaType::JointStates, bfbs_dir)?;
        for sample in samples {
            let fb_data = encode::encode_joint_states(sample.timestamp_us, &sample.values, None);
            writer.write_message(channel_idx, us_to_ns(sample.timestamp_us), &fb_data)?;
        }
    }

    // Write episode metadata
    let mut meta = BTreeMap::new();
    meta.insert(
        "episode_index".to_string(),
        episode.episode_index.to_string(),
    );
    meta.insert(
        "start_time_us".to_string(),
        episode.start_time_us.to_string(),
    );
    if let Some(stop) = episode.stop_time_us {
        meta.insert("stop_time_us".to_string(), stop.to_string());
    }
    if !config.embedded_config_toml.is_empty() {
        meta.insert(
            "config_toml".to_string(),
            config.embedded_config_toml.clone(),
        );
    }
    writer.write_metadata("episode", meta)?;

    writer.finish()?;

    log::info!(
        "rollio-episode-mcap: staged episode {} -> {}",
        episode.episode_index,
        mcap_path.display()
    );

    Ok(StagedResult {
        episode_index: episode.episode_index,
        staging_dir: episode_dir,
    })
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
        (None, None) => Err("rollio-episode-mcap requires --config or --config-inline".into()),
        (Some(_), Some(_)) => Err("config flags are mutually exclusive".into()),
    }
}

pub fn run_with_config(config: AssemblerRuntimeConfigV2) -> Result<(), Box<dyn Error>> {
    if config.format != EpisodeFormat::Mcap {
        return Err(format!(
            "rollio-episode-mcap supports format=mcap only, got {:?}",
            config.format
        )
        .into());
    }

    // Resolve bfbs schema directory (relative to staging_dir or from env)
    let bfbs_dir = resolve_bfbs_dir(&config)?;

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
    let action_subscribers = create_action_subscribers(&node, &config)?;

    let process_id = config.process_id.clone();
    let (worker_tx, worker_rx, worker_handle) = spawn_stage_worker(config.clone(), bfbs_dir)?;
    let mut manager = EpisodeManager::new(config);

    log::info!("rollio-episode-mcap: runtime started");

    let mut shutdown_requested = false;
    'main_loop: loop {
        let mut made_progress = false;
        if drain_control_events(&control_subscriber, &mut manager)? {
            shutdown_requested = true;
        }
        made_progress |= drain_camera_packets(&camera_subscribers, &mut manager)?;
        made_progress |= drain_observations(&observation_subscribers, &mut manager)?;
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
            log::warn!(
                "rollio-episode-mcap: staging slot for episode {leaked} appears leaked after {}ms; releasing",
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
                WorkerEvent::Staged(result) => {
                    episode_ready_publisher.send_copy(EpisodeReady {
                        episode_index: result.episode_index,
                        staging_dir: FixedString256::new(&result.staging_dir.to_string_lossy()),
                    })?;
                    made_progress = true;
                }
                WorkerEvent::Error(message) => {
                    log::error!("rollio-episode-mcap: staging worker error: {message}");
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

/// Resolve the directory containing .bfbs schema files.
fn resolve_bfbs_dir(config: &AssemblerRuntimeConfigV2) -> Result<PathBuf, Box<dyn Error>> {
    // Check ROLLIO_BFBS_DIR env var first
    if let Ok(dir) = std::env::var("ROLLIO_BFBS_DIR") {
        let p = PathBuf::from(dir);
        if p.is_dir() {
            return Ok(p);
        }
    }
    // Fall back to a `bfbs/` directory next to the staging dir
    let candidate = Path::new(&config.staging_dir)
        .parent()
        .unwrap_or(Path::new("."))
        .join("bfbs");
    if candidate.is_dir() {
        return Ok(candidate);
    }
    // Fall back to /usr/share/rollio/bfbs (installed location)
    let installed = PathBuf::from("/usr/share/rollio/bfbs");
    if installed.is_dir() {
        return Ok(installed);
    }
    Err("Cannot find .bfbs schema directory. Set ROLLIO_BFBS_DIR or place schemas in /usr/share/rollio/bfbs".into())
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
) -> Result<iceoryx2::port::publisher::Publisher<ipc::Service, BackpressureEvent, ()>, Box<dyn Error>>
{
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
