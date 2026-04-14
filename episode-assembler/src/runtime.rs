use crate::dataset::{
    remove_episode_artifacts, stage_episode, ActionSample, EpisodeAssemblyInput,
    RobotObservationSample,
};
use clap::Args;
use iceoryx2::node::NodeWaitFailure;
use iceoryx2::prelude::*;
use rollio_bus::{CONTROL_EVENTS_SERVICE, EPISODE_READY_SERVICE, VIDEO_READY_SERVICE};
use rollio_types::config::{
    AssemblerActionRuntimeConfig, AssemblerRobotRuntimeConfig, AssemblerRuntimeConfig,
    EncodedHandoffMode, EpisodeFormat,
};
use rollio_types::messages::{
    ControlEvent, EpisodeReady, FixedString256, RobotCommand, RobotState, VideoReady,
};
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(long, value_name = "PATH", conflicts_with = "config_inline")]
    pub config: Option<PathBuf>,
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    pub config_inline: Option<String>,
}

struct RobotSubscriber {
    config: AssemblerRobotRuntimeConfig,
    subscriber: iceoryx2::port::subscriber::Subscriber<ipc::Service, RobotState, ()>,
}

struct ActionSubscriber {
    config: AssemblerActionRuntimeConfig,
    subscriber: iceoryx2::port::subscriber::Subscriber<ipc::Service, RobotCommand, ()>,
}

#[derive(Debug, Clone)]
struct PendingEpisode {
    episode_index: u32,
    start_time_ns: u64,
    stop_time_ns: Option<u64>,
    keep_requested: bool,
    ready_wait_started_ns: Option<u64>,
    robot_samples: BTreeMap<String, Vec<RobotObservationSample>>,
    action_samples: BTreeMap<String, Vec<ActionSample>>,
    video_paths: BTreeMap<String, PathBuf>,
}

impl PendingEpisode {
    fn new(episode_index: u32, start_time_ns: u64) -> Self {
        Self {
            episode_index,
            start_time_ns,
            stop_time_ns: None,
            keep_requested: false,
            ready_wait_started_ns: None,
            robot_samples: BTreeMap::new(),
            action_samples: BTreeMap::new(),
            video_paths: BTreeMap::new(),
        }
    }

    fn as_assembly_input(&self) -> EpisodeAssemblyInput {
        EpisodeAssemblyInput {
            episode_index: self.episode_index,
            start_time_ns: self.start_time_ns,
            stop_time_ns: self.stop_time_ns.unwrap_or(self.start_time_ns),
            robot_samples: self.robot_samples.clone(),
            action_samples: self.action_samples.clone(),
            video_paths: self.video_paths.clone(),
        }
    }
}

struct EpisodeManager {
    config: AssemblerRuntimeConfig,
    active_episode_index: Option<u32>,
    episodes: BTreeMap<u32, PendingEpisode>,
    camera_by_process_id: HashMap<String, String>,
}

impl EpisodeManager {
    fn new(config: AssemblerRuntimeConfig) -> Self {
        let camera_by_process_id = config
            .cameras
            .iter()
            .map(|camera| (camera.encoder_process_id.clone(), camera.camera_name.clone()))
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
            ControlEvent::RecordingStart { episode_index } => {
                self.episodes.insert(
                    episode_index,
                    PendingEpisode::new(episode_index, unix_timestamp_ns()),
                );
                self.active_episode_index = Some(episode_index);
                false
            }
            ControlEvent::RecordingStop { episode_index } => {
                if let Some(episode) = self.episodes.get_mut(&episode_index) {
                    episode.stop_time_ns = Some(unix_timestamp_ns());
                }
                if self.active_episode_index == Some(episode_index) {
                    self.active_episode_index = None;
                }
                false
            }
            ControlEvent::EpisodeKeep { episode_index } => {
                if let Some(episode) = self.episodes.get_mut(&episode_index) {
                    episode.keep_requested = true;
                    episode.ready_wait_started_ns = Some(unix_timestamp_ns());
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

    fn on_robot_state(&mut self, robot_name: &str, dof: u32, state: RobotState) {
        let Some(episode_index) = self.active_episode_index else {
            return;
        };
        let Some(episode) = self.episodes.get_mut(&episode_index) else {
            return;
        };
        let width = dof as usize;
        episode
            .robot_samples
            .entry(robot_name.to_owned())
            .or_default()
            .push(RobotObservationSample {
                timestamp_ns: state.timestamp_ns,
                positions: state.positions[..width].to_vec(),
                velocities: state.velocities[..width].to_vec(),
                efforts: state.efforts[..width].to_vec(),
            });
    }

    fn on_action_command(&mut self, source_name: &str, dof: u32, command: RobotCommand) {
        let Some(episode_index) = self.active_episode_index else {
            return;
        };
        let Some(episode) = self.episodes.get_mut(&episode_index) else {
            return;
        };
        let width = dof as usize;
        let mut values = vec![0.0; width];
        match command.mode {
            rollio_types::messages::CommandMode::Joint => {
                values.copy_from_slice(&command.joint_targets[..width]);
            }
            rollio_types::messages::CommandMode::Cartesian => {
                let active = width.min(command.cartesian_target.len());
                for (index, value) in command.cartesian_target.iter().take(active).enumerate() {
                    values[index] = *value;
                }
            }
        }
        episode
            .action_samples
            .entry(source_name.to_owned())
            .or_default()
            .push(ActionSample {
                timestamp_ns: command.timestamp_ns,
                values,
            });
    }

    fn on_video_ready(&mut self, ready: VideoReady) {
        let Some(camera_name) = self.camera_by_process_id.get(ready.process_id.as_str()) else {
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
            .insert(camera_name.clone(), PathBuf::from(ready.file_path.as_str()));
    }

    fn ready_and_timed_out_episode_indices(&self, now_ns: u64) -> (Vec<u32>, Vec<u32>) {
        let mut ready = Vec::new();
        let mut timed_out = Vec::new();
        let timeout_ns = self.config.missing_video_timeout_ms.saturating_mul(1_000_000);

        for (episode_index, episode) in &self.episodes {
            if !episode.keep_requested || episode.stop_time_ns.is_none() {
                continue;
            }
            if episode.video_paths.len() == self.config.cameras.len() {
                ready.push(*episode_index);
                continue;
            }
            if let Some(wait_started_ns) = episode.ready_wait_started_ns {
                if now_ns.saturating_sub(wait_started_ns) > timeout_ns {
                    timed_out.push(*episode_index);
                }
            }
        }

        (ready, timed_out)
    }

    fn publish_ready_episodes(
        &mut self,
        publisher: &iceoryx2::port::publisher::Publisher<ipc::Service, EpisodeReady, ()>,
    ) -> Result<bool, Box<dyn Error>> {
        let now_ns = unix_timestamp_ns();
        let (ready_episode_indices, timed_out_episode_indices) =
            self.ready_and_timed_out_episode_indices(now_ns);
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
            let staged = stage_episode(&self.config, &episode.as_assembly_input())?;
            eprintln!(
                "rollio-episode-assembler: staged episode {} ({} frames) at {}",
                staged.episode_index,
                staged.frame_count,
                staged.staging_dir.display()
            );
            publisher.send_copy(EpisodeReady {
                episode_index: staged.episode_index,
                staging_dir: FixedString256::new(&staged.staging_dir.to_string_lossy()),
            })?;
            changed = true;
        }

        Ok(changed)
    }
}

pub fn run(args: RunArgs) -> Result<(), Box<dyn Error>> {
    let config = load_runtime_config(&args)?;
    run_with_config(config)
}

fn load_runtime_config(args: &RunArgs) -> Result<AssemblerRuntimeConfig, Box<dyn Error>> {
    match (&args.config, &args.config_inline) {
        (Some(path), None) => Ok(AssemblerRuntimeConfig::from_file(path)?),
        (None, Some(inline)) => Ok(inline.parse::<AssemblerRuntimeConfig>()?),
        (None, None) => Err("episode assembler requires --config or --config-inline".into()),
        (Some(_), Some(_)) => Err("episode assembler config flags are mutually exclusive".into()),
    }
}

pub fn run_with_config(config: AssemblerRuntimeConfig) -> Result<(), Box<dyn Error>> {
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
    let robot_subscribers = create_robot_subscribers(&node, &config)?;
    let action_subscribers = create_action_subscribers(&node, &config)?;
    let mut manager = EpisodeManager::new(config);

    loop {
        let mut made_progress = false;
        if drain_control_events(&control_subscriber, &mut manager)? {
            break;
        }
        made_progress |= drain_video_ready(&video_ready_subscriber, &mut manager)?;
        made_progress |= drain_robot_states(&robot_subscribers, &mut manager)?;
        made_progress |= drain_action_commands(&action_subscribers, &mut manager)?;
        made_progress |= manager.publish_ready_episodes(&episode_ready_publisher)?;

        if made_progress {
            continue;
        }

        match node.wait(Duration::from_millis(10)) {
            Ok(()) => {}
            Err(NodeWaitFailure::Interrupt | NodeWaitFailure::TerminationRequest) => break,
        }
    }

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
) -> Result<iceoryx2::port::subscriber::Subscriber<ipc::Service, VideoReady, ()>, Box<dyn Error>>
{
    let service_name: ServiceName = VIDEO_READY_SERVICE.try_into()?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<VideoReady>()
        .open_or_create()?;
    Ok(service.subscriber_builder().create()?)
}

fn create_episode_ready_publisher(
    node: &Node<ipc::Service>,
) -> Result<iceoryx2::port::publisher::Publisher<ipc::Service, EpisodeReady, ()>, Box<dyn Error>>
{
    let service_name: ServiceName = EPISODE_READY_SERVICE.try_into()?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<EpisodeReady>()
        .open_or_create()?;
    Ok(service.publisher_builder().create()?)
}

fn create_robot_subscribers(
    node: &Node<ipc::Service>,
    config: &AssemblerRuntimeConfig,
) -> Result<Vec<RobotSubscriber>, Box<dyn Error>> {
    config
        .robots
        .iter()
        .map(|robot| {
            let service_name: ServiceName = robot.state_topic.as_str().try_into()?;
            let service = node
                .service_builder(&service_name)
                .publish_subscribe::<RobotState>()
                .open_or_create()?;
            Ok(RobotSubscriber {
                config: robot.clone(),
                subscriber: service.subscriber_builder().create()?,
            })
        })
        .collect()
}

fn create_action_subscribers(
    node: &Node<ipc::Service>,
    config: &AssemblerRuntimeConfig,
) -> Result<Vec<ActionSubscriber>, Box<dyn Error>> {
    config
        .actions
        .iter()
        .map(|action| {
            let service_name: ServiceName = action.command_topic.as_str().try_into()?;
            let service = node
                .service_builder(&service_name)
                .publish_subscribe::<RobotCommand>()
                .open_or_create()?;
            Ok(ActionSubscriber {
                config: action.clone(),
                subscriber: service.subscriber_builder().create()?,
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

fn drain_robot_states(
    subscribers: &[RobotSubscriber],
    manager: &mut EpisodeManager,
) -> Result<bool, Box<dyn Error>> {
    let mut changed = false;
    for robot in subscribers {
        loop {
            let Some(sample) = robot.subscriber.receive()? else {
                break;
            };
            manager.on_robot_state(&robot.config.robot_name, robot.config.dof, *sample.payload());
            changed = true;
        }
    }
    Ok(changed)
}

fn drain_action_commands(
    subscribers: &[ActionSubscriber],
    manager: &mut EpisodeManager,
) -> Result<bool, Box<dyn Error>> {
    let mut changed = false;
    for action in subscribers {
        loop {
            let Some(sample) = action.subscriber.receive()? else {
                break;
            };
            manager.on_action_command(&action.config.source_name, action.config.dof, *sample.payload());
            changed = true;
        }
    }
    Ok(changed)
}

fn unix_timestamp_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollio_types::config::{
        AssemblerActionRuntimeConfig, AssemblerCameraRuntimeConfig, AssemblerRobotRuntimeConfig,
        EncoderArtifactFormat, EncoderCodec,
    };
    use rollio_types::messages::{CommandMode, PixelFormat};
    use std::fs;

    #[test]
    fn manager_buffers_state_and_commands_for_active_episode() {
        let mut manager = EpisodeManager::new(test_config());
        assert!(!manager.on_control_event(ControlEvent::RecordingStart { episode_index: 4 }));

        manager.on_robot_state(
            "leader_arm",
            6,
            RobotState {
                timestamp_ns: 10,
                num_joints: 6,
                positions: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                velocities: [0.1; 16],
                efforts: [0.2; 16],
                ..RobotState::default()
            },
        );
        manager.on_action_command(
            "follower_arm",
            6,
            RobotCommand {
                timestamp_ns: 12,
                mode: CommandMode::Joint,
                num_joints: 6,
                joint_targets: [0.5, 0.4, 0.3, 0.2, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ..RobotCommand::default()
            },
        );
        assert!(!manager.on_control_event(ControlEvent::RecordingStop { episode_index: 4 }));

        let episode = manager.episodes.get(&4).expect("episode should exist");
        assert_eq!(episode.robot_samples["leader_arm"].len(), 1);
        assert_eq!(episode.action_samples["follower_arm"].len(), 1);
        assert!(episode.stop_time_ns.is_some());
    }

    #[test]
    fn manager_discards_episode_and_artifacts() {
        let mut manager = EpisodeManager::new(test_config());
        let temp_file = std::env::temp_dir().join(format!("rollio-assembler-runtime-{}.mp4", unix_timestamp_ns()));
        fs::write(&temp_file, b"video").expect("temp video should exist");

        manager.on_control_event(ControlEvent::RecordingStart { episode_index: 2 });
        manager.on_control_event(ControlEvent::RecordingStop { episode_index: 2 });
        manager.on_video_ready(VideoReady {
            process_id: rollio_types::messages::FixedString64::new("encoder.camera_top"),
            episode_index: 2,
            file_path: rollio_types::messages::FixedString256::new(&temp_file.to_string_lossy()),
        });
        manager.on_control_event(ControlEvent::EpisodeDiscard { episode_index: 2 });

        assert!(!manager.episodes.contains_key(&2));
        assert!(!temp_file.exists());
    }

    #[test]
    fn manager_marks_episode_timed_out_after_keep() {
        let mut manager = EpisodeManager::new(test_config());
        manager.on_control_event(ControlEvent::RecordingStart { episode_index: 1 });
        manager.on_control_event(ControlEvent::RecordingStop { episode_index: 1 });
        manager.on_control_event(ControlEvent::EpisodeKeep { episode_index: 1 });
        manager.episodes.get_mut(&1).unwrap().ready_wait_started_ns = Some(0);

        let (ready, timed_out) = manager.ready_and_timed_out_episode_indices(10_000_000_000);
        assert!(ready.is_empty());
        assert_eq!(timed_out, vec![1]);
    }

    fn test_config() -> AssemblerRuntimeConfig {
        AssemblerRuntimeConfig {
            process_id: "episode-assembler".into(),
            format: EpisodeFormat::LeRobotV2_1,
            fps: 30,
            chunk_size: 1000,
            missing_video_timeout_ms: 1,
            staging_dir: std::env::temp_dir()
                .join("rollio-assembler-runtime")
                .to_string_lossy()
                .into_owned(),
            encoded_handoff: EncodedHandoffMode::File,
            cameras: vec![AssemblerCameraRuntimeConfig {
                camera_name: "camera_top".into(),
                encoder_process_id: "encoder.camera_top".into(),
                width: 640,
                height: 480,
                fps: 30,
                pixel_format: PixelFormat::Rgb24,
                codec: EncoderCodec::H264,
                artifact_format: EncoderArtifactFormat::Mp4,
            }],
            robots: vec![AssemblerRobotRuntimeConfig {
                robot_name: "leader_arm".into(),
                state_topic: "robot/leader_arm/state".into(),
                dof: 6,
            }],
            actions: vec![AssemblerActionRuntimeConfig {
                source_name: "follower_arm".into(),
                command_topic: "robot/follower_arm/command".into(),
                dof: 6,
            }],
            embedded_config_toml: "fps = 30".into(),
        }
    }
}
