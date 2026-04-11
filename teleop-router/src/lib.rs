use clap::{Args, Parser, Subcommand};
use iceoryx2::node::NodeWaitFailure;
use iceoryx2::prelude::*;
use rollio_bus::CONTROL_EVENTS_SERVICE;
use rollio_types::config::{MappingStrategy, TeleopRuntimeConfig};
use rollio_types::messages::{CommandMode, ControlEvent, RobotCommand, RobotState, MAX_JOINTS};
use std::error::Error;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

type StateSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, RobotState, ()>;
type ControlSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, ControlEvent, ()>;
type CommandPublisher = iceoryx2::port::publisher::Publisher<ipc::Service, RobotCommand, ()>;

const STARTUP_SYNC_DURATION: Duration = Duration::from_secs(2);
const STARTUP_SYNC_REQUIRED_DELTA: f64 = 0.25;
const STARTUP_SYNC_COMMAND_PERIOD: Duration = Duration::from_millis(20);
// Rollio joint positions are expressed in radians; convert the safety cap from degrees.
const STARTUP_SYNC_MAX_POSITION_ERROR_RAD: f64 = 0.5 * std::f64::consts::PI / 180.0;

#[derive(Debug, Error)]
pub enum TeleopRouterError {
    #[error("leader state only exposes {available} joints, required source joint {requested}")]
    LeaderJointOutOfRange { requested: usize, available: usize },
    #[error("follower state only exposes {available} joints, required follower joint {requested}")]
    FollowerJointOutOfRange { requested: usize, available: usize },
    #[error("cartesian forwarding requires leader end-effector pose")]
    MissingCartesianPose,
}

#[derive(Debug, Clone, Copy, Default)]
struct RouterState {
    latest_leader_state: Option<RobotState>,
    latest_follower_state: Option<RobotState>,
    startup_sync_state: StartupSyncState,
    last_forwarded_leader_timestamp_ns: Option<u64>,
    fresh_state_cutoff_ns: u64,
}

#[derive(Debug, Clone, Copy, Default)]
enum StartupSyncState {
    #[default]
    Pending,
    Syncing(StartupSync),
    Complete,
}

#[derive(Debug, Clone, Copy)]
struct StartupSync {
    started_at: Instant,
    initial_follower_positions: [f64; MAX_JOINTS],
    last_publish_at: Option<Instant>,
}

impl StartupSync {
    fn new(follower_state: &RobotState) -> Self {
        Self {
            started_at: Instant::now(),
            initial_follower_positions: follower_state.positions,
            last_publish_at: None,
        }
    }

    fn interpolated_command(&self, now: Instant, target_command: RobotCommand) -> RobotCommand {
        let progress = self.progress(now);
        let joint_count = target_command.num_joints as usize;
        let mut command = target_command;
        command.timestamp_ns = unix_timestamp_ns();

        for joint_index in 0..joint_count {
            let start = self.initial_follower_positions[joint_index];
            let target = target_command.joint_targets[joint_index];
            command.joint_targets[joint_index] = start + (target - start) * progress;
        }

        command
    }

    fn is_complete(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started_at) >= STARTUP_SYNC_DURATION
    }

    fn progress(&self, now: Instant) -> f64 {
        (now.saturating_duration_since(self.started_at).as_secs_f64()
            / STARTUP_SYNC_DURATION.as_secs_f64())
        .clamp(0.0, 1.0)
    }
}

#[derive(Parser, Debug)]
#[command(name = "rollio-teleop-router")]
#[command(about = "Leader-to-follower teleop command forwarding")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Run(RunArgs),
}

#[derive(Args, Debug)]
struct RunArgs {
    /// TOML file containing TeleopRuntimeConfig
    #[arg(long, value_name = "PATH", conflicts_with = "config_inline")]
    config: Option<PathBuf>,

    /// Inline TOML containing TeleopRuntimeConfig
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    config_inline: Option<String>,
}

pub fn run_cli() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => {
            let config = load_runtime_config(&args)?;
            run_router(config)
        }
    }
}

fn load_runtime_config(args: &RunArgs) -> Result<TeleopRuntimeConfig, Box<dyn Error>> {
    match (&args.config, &args.config_inline) {
        (Some(path), None) => Ok(TeleopRuntimeConfig::from_file(path)?),
        (None, Some(inline)) => Ok(inline.parse::<TeleopRuntimeConfig>()?),
        (None, None) => Err("teleop router requires --config or --config-inline".into()),
        (Some(_), Some(_)) => Err("teleop router config flags are mutually exclusive".into()),
    }
}

pub fn run_router(config: TeleopRuntimeConfig) -> Result<(), Box<dyn Error>> {
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;

    let leader_state_subscriber = create_state_subscriber(&node, &config.leader_state_topic)?;
    let follower_state_subscriber = create_state_subscriber(&node, &config.follower_state_topic)?;
    let follower_command_publisher =
        create_command_publisher(&node, &config.follower_command_topic)?;
    let control_subscriber = create_control_subscriber(&node)?;
    let mut router_state = RouterState {
        fresh_state_cutoff_ns: unix_timestamp_ns(),
        ..RouterState::default()
    };

    eprintln!(
        "rollio-teleop-router: {} forwarding {} -> {} with {:?}",
        config.process_id, config.leader_name, config.follower_name, config.mapping
    );

    loop {
        if drain_control_events(&control_subscriber)? {
            break;
        }
        drain_latest_state(
            &leader_state_subscriber,
            &mut router_state.latest_leader_state,
            router_state.fresh_state_cutoff_ns,
        )?;
        drain_latest_state(
            &follower_state_subscriber,
            &mut router_state.latest_follower_state,
            router_state.fresh_state_cutoff_ns,
        )?;
        let forwarded_any =
            route_latest_state(&follower_command_publisher, &config, &mut router_state)?;
        if forwarded_any {
            continue;
        }

        match node.wait(Duration::from_micros(100)) {
            Ok(()) => {}
            Err(NodeWaitFailure::Interrupt | NodeWaitFailure::TerminationRequest) => break,
        }
    }

    eprintln!(
        "rollio-teleop-router: {} shutdown complete",
        config.process_id
    );
    Ok(())
}

fn create_state_subscriber(
    node: &Node<ipc::Service>,
    topic: &str,
) -> Result<StateSubscriber, Box<dyn Error>> {
    let service_name: ServiceName = topic.try_into()?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<RobotState>()
        .open_or_create()?;
    Ok(service.subscriber_builder().create()?)
}

fn create_command_publisher(
    node: &Node<ipc::Service>,
    topic: &str,
) -> Result<CommandPublisher, Box<dyn Error>> {
    let service_name: ServiceName = topic.try_into()?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<RobotCommand>()
        .open_or_create()?;
    Ok(service.publisher_builder().create()?)
}

fn create_control_subscriber(
    node: &Node<ipc::Service>,
) -> Result<ControlSubscriber, Box<dyn Error>> {
    let service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<ControlEvent>()
        .open_or_create()?;
    Ok(service.subscriber_builder().create()?)
}

fn drain_control_events(subscriber: &ControlSubscriber) -> Result<bool, Box<dyn Error>> {
    loop {
        match subscriber.receive()? {
            Some(sample) => {
                if matches!(*sample.payload(), ControlEvent::Shutdown) {
                    return Ok(true);
                }
            }
            None => return Ok(false),
        }
    }
}

fn drain_latest_state(
    subscriber: &StateSubscriber,
    latest_state: &mut Option<RobotState>,
    fresh_state_cutoff_ns: u64,
) -> Result<bool, Box<dyn Error>> {
    let mut updated = false;
    loop {
        let Some(sample) = subscriber.receive()? else {
            return Ok(updated);
        };
        let state = *sample.payload();
        if state.timestamp_ns < fresh_state_cutoff_ns {
            continue;
        }
        *latest_state = Some(state);
        updated = true;
    }
}

fn route_latest_state(
    publisher: &CommandPublisher,
    config: &TeleopRuntimeConfig,
    router_state: &mut RouterState,
) -> Result<bool, Box<dyn Error>> {
    match config.mapping {
        MappingStrategy::Cartesian => match prepare_direct_command(config, router_state) {
            Ok(Some((leader_timestamp_ns, command))) => {
                publisher.send_copy(command)?;
                router_state.last_forwarded_leader_timestamp_ns = Some(leader_timestamp_ns);
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(TeleopRouterError::MissingCartesianPose) => Ok(false),
            Err(error) => {
                log_router_error(config, &error);
                Ok(false)
            }
        },
        MappingStrategy::DirectJoint => route_direct_joint_state(publisher, config, router_state),
    }
}

fn route_direct_joint_state(
    publisher: &CommandPublisher,
    config: &TeleopRuntimeConfig,
    router_state: &mut RouterState,
) -> Result<bool, Box<dyn Error>> {
    if let Err(error) = ensure_startup_sync_state(config, router_state) {
        log_router_error(config, &error);
        return Ok(false);
    }

    let mut forwarded_any = maybe_publish_startup_sync_command(publisher, config, router_state)?;
    if matches!(router_state.startup_sync_state, StartupSyncState::Complete) {
        match prepare_direct_command(config, router_state) {
            Ok(Some((leader_timestamp_ns, command))) => {
                publisher.send_copy(command)?;
                router_state.last_forwarded_leader_timestamp_ns = Some(leader_timestamp_ns);
                forwarded_any = true;
            }
            Ok(None) => {}
            Err(error) => log_router_error(config, &error),
        }
    }

    Ok(forwarded_any)
}

fn ensure_startup_sync_state(
    config: &TeleopRuntimeConfig,
    router_state: &mut RouterState,
) -> Result<(), TeleopRouterError> {
    if !matches!(router_state.startup_sync_state, StartupSyncState::Pending) {
        return Ok(());
    }

    let Some(leader_state) = router_state.latest_leader_state else {
        return Ok(());
    };
    let Some(follower_state) = router_state.latest_follower_state else {
        return Ok(());
    };

    let target_command = map_direct_joint_state(config, &leader_state)?;
    let max_delta = max_follower_delta(&target_command, &follower_state)?;
    if max_delta > STARTUP_SYNC_REQUIRED_DELTA {
        eprintln!(
            "rollio-teleop-router: {} startup sync over {:?} (max joint delta {:.3})",
            config.process_id, STARTUP_SYNC_DURATION, max_delta
        );
        router_state.startup_sync_state =
            StartupSyncState::Syncing(StartupSync::new(&follower_state));
    } else {
        router_state.startup_sync_state = StartupSyncState::Complete;
    }

    Ok(())
}

fn maybe_publish_startup_sync_command(
    publisher: &CommandPublisher,
    config: &TeleopRuntimeConfig,
    router_state: &mut RouterState,
) -> Result<bool, Box<dyn Error>> {
    let now = Instant::now();
    let Some(leader_state) = router_state.latest_leader_state else {
        return Ok(false);
    };
    let Some(follower_state) = router_state.latest_follower_state else {
        return Ok(false);
    };
    let live_target_command = match map_direct_joint_state(config, &leader_state) {
        Ok(command) => command,
        Err(error) => {
            log_router_error(config, &error);
            return Ok(false);
        }
    };
    let mut completed_leader_timestamp_ns = None;
    let command = {
        let StartupSyncState::Syncing(sync) = &mut router_state.startup_sync_state else {
            return Ok(false);
        };

        let should_publish = match sync.last_publish_at {
            Some(last_publish_at) => {
                sync.is_complete(now)
                    || now.saturating_duration_since(last_publish_at) >= STARTUP_SYNC_COMMAND_PERIOD
            }
            None => true,
        };

        if !should_publish {
            return Ok(false);
        }

        let desired_command = sync.interpolated_command(now, live_target_command);
        let command = clamp_startup_sync_command(&desired_command, &follower_state)?;
        sync.last_publish_at = Some(now);
        if sync.is_complete(now)
            && max_follower_delta(&live_target_command, &follower_state)?
                <= STARTUP_SYNC_MAX_POSITION_ERROR_RAD
        {
            completed_leader_timestamp_ns = Some(leader_state.timestamp_ns);
        }
        command
    };

    publisher.send_copy(command)?;
    if let Some(leader_timestamp_ns) = completed_leader_timestamp_ns {
        router_state.last_forwarded_leader_timestamp_ns = Some(leader_timestamp_ns);
        router_state.startup_sync_state = StartupSyncState::Complete;
        eprintln!(
            "rollio-teleop-router: {} startup sync complete",
            config.process_id
        );
    }

    Ok(true)
}

fn prepare_direct_command(
    config: &TeleopRuntimeConfig,
    router_state: &mut RouterState,
) -> Result<Option<(u64, RobotCommand)>, TeleopRouterError> {
    let Some(leader_state) = router_state.latest_leader_state else {
        return Ok(None);
    };

    if router_state.last_forwarded_leader_timestamp_ns == Some(leader_state.timestamp_ns) {
        return Ok(None);
    }

    let command = map_leader_state(config, &leader_state)?;
    Ok(Some((leader_state.timestamp_ns, command)))
}

fn max_follower_delta(
    command: &RobotCommand,
    follower_state: &RobotState,
) -> Result<f64, TeleopRouterError> {
    let joint_count = command.num_joints as usize;
    let follower_available = follower_state.num_joints as usize;
    if follower_available < joint_count {
        return Err(TeleopRouterError::FollowerJointOutOfRange {
            requested: joint_count.saturating_sub(1),
            available: follower_available,
        });
    }

    let mut max_delta = 0.0_f64;
    for joint_index in 0..joint_count {
        let delta =
            (command.joint_targets[joint_index] - follower_state.positions[joint_index]).abs();
        max_delta = max_delta.max(delta);
    }

    Ok(max_delta)
}

fn clamp_startup_sync_command(
    desired_command: &RobotCommand,
    follower_state: &RobotState,
) -> Result<RobotCommand, TeleopRouterError> {
    let joint_count = desired_command.num_joints as usize;
    let follower_available = follower_state.num_joints as usize;
    if follower_available < joint_count {
        return Err(TeleopRouterError::FollowerJointOutOfRange {
            requested: joint_count.saturating_sub(1),
            available: follower_available,
        });
    }

    let mut command = *desired_command;
    for joint_index in 0..joint_count {
        let current = follower_state.positions[joint_index];
        let min_target = current - STARTUP_SYNC_MAX_POSITION_ERROR_RAD;
        let max_target = current + STARTUP_SYNC_MAX_POSITION_ERROR_RAD;
        command.joint_targets[joint_index] =
            desired_command.joint_targets[joint_index].clamp(min_target, max_target);
    }

    Ok(command)
}

fn log_router_error(config: &TeleopRuntimeConfig, error: &TeleopRouterError) {
    eprintln!(
        "rollio-teleop-router: {} dropped invalid state: {error}",
        config.process_id
    );
}

fn unix_timestamp_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

pub fn map_leader_state(
    config: &TeleopRuntimeConfig,
    leader_state: &RobotState,
) -> Result<RobotCommand, TeleopRouterError> {
    match config.mapping {
        MappingStrategy::DirectJoint => map_direct_joint_state(config, leader_state),
        MappingStrategy::Cartesian => map_cartesian_state(leader_state),
    }
}

fn map_direct_joint_state(
    config: &TeleopRuntimeConfig,
    leader_state: &RobotState,
) -> Result<RobotCommand, TeleopRouterError> {
    let available = leader_state.num_joints as usize;
    let output_len = if !config.joint_index_map.is_empty() {
        config.joint_index_map.len()
    } else if !config.joint_scales.is_empty() {
        config.joint_scales.len()
    } else {
        available.min(MAX_JOINTS)
    };

    let mut command = RobotCommand {
        timestamp_ns: leader_state.timestamp_ns,
        mode: CommandMode::Joint,
        num_joints: output_len.min(MAX_JOINTS) as u32,
        ..RobotCommand::default()
    };

    for output_index in 0..output_len.min(MAX_JOINTS) {
        let source_index = config
            .joint_index_map
            .get(output_index)
            .copied()
            .unwrap_or(output_index as u32) as usize;
        if source_index >= available {
            return Err(TeleopRouterError::LeaderJointOutOfRange {
                requested: source_index,
                available,
            });
        }
        let scale = config
            .joint_scales
            .get(output_index)
            .copied()
            .unwrap_or(1.0);
        command.joint_targets[output_index] = leader_state.positions[source_index] * scale;
    }

    Ok(command)
}

fn map_cartesian_state(leader_state: &RobotState) -> Result<RobotCommand, TeleopRouterError> {
    if !leader_state.has_ee_pose {
        return Err(TeleopRouterError::MissingCartesianPose);
    }

    Ok(RobotCommand {
        timestamp_ns: leader_state.timestamp_ns,
        mode: CommandMode::Cartesian,
        num_joints: 0,
        cartesian_target: leader_state.ee_pose,
        ..RobotCommand::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct_config() -> TeleopRuntimeConfig {
        TeleopRuntimeConfig {
            process_id: "teleop.test".into(),
            leader_name: "leader".into(),
            follower_name: "follower".into(),
            leader_state_topic: "robot/leader/state".into(),
            follower_state_topic: "robot/follower/state".into(),
            follower_command_topic: "robot/follower/command".into(),
            mapping: MappingStrategy::DirectJoint,
            joint_index_map: Vec::new(),
            joint_scales: Vec::new(),
        }
    }

    fn leader_state() -> RobotState {
        let mut state = RobotState {
            timestamp_ns: 123,
            num_joints: 6,
            ..RobotState::default()
        };
        state.positions[..6].copy_from_slice(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);
        state.ee_pose = [0.3, 0.0, 0.5, 0.0, 0.0, 0.0, 1.0];
        state.has_ee_pose = true;
        state
    }

    #[test]
    fn direct_joint_identity_mapping_preserves_positions() {
        let config = direct_config();
        let command = map_leader_state(&config, &leader_state()).expect("mapping should work");
        assert_eq!(command.mode, CommandMode::Joint);
        assert_eq!(command.num_joints, 6);
        assert_eq!(&command.joint_targets[..6], &[0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);
    }

    #[test]
    fn direct_joint_remap_reorders_source_joints() {
        let mut config = direct_config();
        config.joint_index_map = vec![5, 4, 3, 2, 1, 0];
        let command = map_leader_state(&config, &leader_state()).expect("mapping should work");
        assert_eq!(&command.joint_targets[..6], &[0.6, 0.5, 0.4, 0.3, 0.2, 0.1]);
    }

    #[test]
    fn direct_joint_scaling_is_applied_per_output_joint() {
        let mut config = direct_config();
        config.joint_index_map = vec![0, 1, 2, 3, 4, 5];
        config.joint_scales = vec![2.0, 1.0, 1.0, 1.0, 1.0, 0.5];
        let command = map_leader_state(&config, &leader_state()).expect("mapping should work");
        assert_eq!(command.joint_targets[0], 0.2);
        assert_eq!(command.joint_targets[5], 0.3);
    }

    #[test]
    fn startup_sync_is_required_for_large_joint_gap() {
        let config = direct_config();
        let command = map_leader_state(&config, &leader_state()).expect("mapping should work");
        let follower = RobotState {
            num_joints: 6,
            ..RobotState::default()
        };

        let delta = max_follower_delta(&command, &follower).expect("delta should compute");
        assert!(delta > STARTUP_SYNC_REQUIRED_DELTA);
    }

    #[test]
    fn startup_sync_interpolates_from_follower_to_leader() {
        let follower = RobotState {
            num_joints: 6,
            ..RobotState::default()
        };
        let mut sync = StartupSync::new(&follower);
        sync.started_at = Instant::now() - Duration::from_secs(1);

        let command = sync.interpolated_command(
            Instant::now(),
            map_leader_state(&direct_config(), &leader_state()).expect("mapping should work"),
        );
        assert!((command.joint_targets[0] - 0.05).abs() < 0.02);
        assert!((command.joint_targets[5] - 0.30).abs() < 0.02);
    }

    #[test]
    fn startup_sync_blends_toward_latest_leader_target() {
        let follower = RobotState {
            num_joints: 6,
            ..RobotState::default()
        };
        let mut sync = StartupSync::new(&follower);
        sync.started_at = Instant::now() - Duration::from_secs(1);

        let mut live_leader = leader_state();
        live_leader.positions[..6].copy_from_slice(&[1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
        let command = sync.interpolated_command(
            Instant::now(),
            map_leader_state(&direct_config(), &live_leader).expect("mapping should work"),
        );
        assert!((command.joint_targets[0] - 0.5).abs() < 0.02);
        assert!((command.joint_targets[5] - 0.5).abs() < 0.02);
    }

    #[test]
    fn startup_sync_command_is_clamped_to_small_step_from_follower() {
        let desired_command = RobotCommand {
            num_joints: 6,
            joint_targets: [1.0; MAX_JOINTS],
            ..RobotCommand::default()
        };
        let follower = RobotState {
            num_joints: 6,
            ..RobotState::default()
        };

        let command =
            clamp_startup_sync_command(&desired_command, &follower).expect("clamp should work");
        assert!((command.joint_targets[0] - STARTUP_SYNC_MAX_POSITION_ERROR_RAD).abs() < 1e-12);
        assert!((command.joint_targets[5] - STARTUP_SYNC_MAX_POSITION_ERROR_RAD).abs() < 1e-12);
    }

    #[test]
    fn cartesian_mapping_forwards_end_effector_pose() {
        let mut config = direct_config();
        config.mapping = MappingStrategy::Cartesian;
        config.joint_index_map.clear();
        let command = map_leader_state(&config, &leader_state()).expect("mapping should work");
        assert_eq!(command.mode, CommandMode::Cartesian);
        assert_eq!(
            command.cartesian_target,
            [0.3, 0.0, 0.5, 0.0, 0.0, 0.0, 1.0]
        );
    }
}
