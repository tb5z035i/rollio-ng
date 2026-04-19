use clap::{Args, Parser, Subcommand};
use iceoryx2::prelude::*;
use rollio_bus::{
    channel_command_service_name, channel_frames_service_name, channel_mode_control_service_name,
    channel_mode_info_service_name, channel_state_service_name, CONTROL_EVENTS_SERVICE,
};
use rollio_types::config::{
    BinaryDeviceConfig, CameraChannelProfile, ChannelCommandDefaults, DeviceQueryChannel,
    DeviceQueryDevice, DeviceQueryResponse, DeviceType, DirectJointCompatibility,
    DirectJointCompatibilityPeer, RobotCommandKind, RobotMode, RobotStateKind,
    StateValueLimitsEntry,
};
use rollio_types::messages::{
    CameraFrameHeader, ControlEvent, DeviceChannelMode, JointMitCommand15, JointVector15,
    PixelFormat, Pose7,
};
use serde_json::json;
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DRIVER_NAME: &str = "pseudo";

#[derive(Debug, Parser)]
#[command(name = "rollio-device-pseudo")]
#[command(about = "Synthetic hierarchical multi-channel device driver for Rollio")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Probe(ProbeArgs),
    Validate(ValidateArgs),
    Query(QueryArgs),
    Run(RunArgs),
}

#[derive(Debug, Clone, Args)]
struct ProbeArgs {
    #[arg(long, default_value_t = 0)]
    sim_cameras: u32,
    #[arg(long, default_value_t = 0)]
    sim_arms: u32,
    #[arg(long, default_value_t = 6)]
    dof: u32,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Args)]
struct ValidateArgs {
    id: String,
    #[arg(long = "channel-type")]
    channel_types: Vec<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Args)]
struct QueryArgs {
    id: String,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Args)]
struct RunArgs {
    #[arg(long, value_name = "PATH", conflicts_with = "config_inline")]
    config: Option<PathBuf>,
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    config_inline: Option<String>,
    #[arg(long)]
    dry_run: bool,
}

struct CameraRuntime {
    _channel_type: String,
    width: u32,
    height: u32,
    fps: u32,
    publisher: iceoryx2::port::publisher::Publisher<ipc::Service, [u8], CameraFrameHeader>,
    frame: Vec<u8>,
    frame_index: u64,
    next_tick: Instant,
}

struct RobotRuntime {
    _channel_type: String,
    dof: usize,
    mode: RobotMode,
    frequency_hz: f64,
    state_publishers: Vec<StatePublisher>,
    command_subscribers: Vec<CommandSubscriber>,
    command_defaults: ChannelCommandDefaults,
    current_positions: [f64; rollio_types::messages::MAX_DOF],
    target_positions: [f64; rollio_types::messages::MAX_DOF],
    previous_positions: [f64; rollio_types::messages::MAX_DOF],
    next_tick: Instant,
    started_at: Instant,
}

enum StatePublisher {
    JointPosition(iceoryx2::port::publisher::Publisher<ipc::Service, JointVector15, ()>),
    JointVelocity(iceoryx2::port::publisher::Publisher<ipc::Service, JointVector15, ()>),
    JointEffort(iceoryx2::port::publisher::Publisher<ipc::Service, JointVector15, ()>),
    EndEffectorPose(iceoryx2::port::publisher::Publisher<ipc::Service, Pose7, ()>),
}

enum CommandSubscriber {
    JointPosition(iceoryx2::port::subscriber::Subscriber<ipc::Service, JointVector15, ()>),
    JointMit(iceoryx2::port::subscriber::Subscriber<ipc::Service, JointMitCommand15, ()>),
}

type ShutdownSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, ControlEvent, ()>;
type ChannelModeSubscriber =
    iceoryx2::port::subscriber::Subscriber<ipc::Service, DeviceChannelMode, ()>;
type ChannelModePublisher =
    iceoryx2::port::publisher::Publisher<ipc::Service, DeviceChannelMode, ()>;

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Probe(args) => run_probe(args)?,
        Command::Validate(args) => run_validate(args)?,
        Command::Query(args) => run_query(args)?,
        Command::Run(args) => run_device_command(args)?,
    }
    Ok(())
}

fn run_probe(args: ProbeArgs) -> Result<(), Box<dyn Error>> {
    let ids = pseudo_probe_ids(args.sim_cameras, args.sim_arms, args.dof);
    if args.json {
        println!("{}", serde_json::to_string_pretty(&ids)?);
    } else {
        if ids.is_empty() {
            println!("no pseudo devices discovered");
        } else {
            for id in ids {
                println!("{id}");
            }
        }
    }
    Ok(())
}

fn run_validate(args: ValidateArgs) -> Result<(), Box<dyn Error>> {
    let valid = query_pseudo_device(&args.id).is_some_and(|device| {
        args.channel_types.is_empty()
            || args.channel_types.iter().all(|channel_type| {
                device
                    .channels
                    .iter()
                    .any(|channel| channel.channel_type == *channel_type)
            })
    });
    let report = json!({
        "valid": valid,
        "id": args.id,
        "channel_types": args.channel_types,
    });
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if valid {
        println!("{} is valid", args.id);
    } else {
        println!("{} is invalid", args.id);
    }
    if valid {
        Ok(())
    } else {
        Err("pseudo validate failed".into())
    }
}

fn run_query(args: QueryArgs) -> Result<(), Box<dyn Error>> {
    let Some(device) = query_pseudo_device(&args.id) else {
        return Err(format!("unknown pseudo device: {}", args.id).into());
    };
    let response = DeviceQueryResponse {
        driver: DRIVER_NAME.into(),
        devices: vec![device],
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        print_query_human(&response);
    }
    Ok(())
}

fn run_device_command(args: RunArgs) -> Result<(), Box<dyn Error>> {
    let device = load_device_config(&args)?;
    if args.dry_run {
        return Ok(());
    }
    run_device(device)
}

fn load_device_config(args: &RunArgs) -> Result<BinaryDeviceConfig, Box<dyn Error>> {
    let device = if let Some(config_path) = &args.config {
        BinaryDeviceConfig::from_file(config_path)?
    } else if let Some(config_inline) = &args.config_inline {
        config_inline.parse::<BinaryDeviceConfig>()?
    } else {
        return Err("run requires either --config or --config-inline".into());
    };
    if device.driver != DRIVER_NAME {
        return Err(format!(
            "device \"{}\" uses driver \"{}\", expected {DRIVER_NAME}",
            device.name, device.driver
        )
        .into());
    }
    Ok(device)
}

fn run_device(device: BinaryDeviceConfig) -> Result<(), Box<dyn Error>> {
    let stop = Arc::new(AtomicBool::new(false));

    // See `robots/airbot_play_rust/src/bin/device.rs::run_device`. The same
    // SIGINT/SIGTERM rationale applies — the per-channel loops poll `stop`
    // and want a chance to run their cleanup (publish a final state
    // snapshot, transition to Disabled) before the process exits.
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop))?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop))?;

    let mut handles = Vec::new();

    for channel in device
        .channels
        .iter()
        .filter(|channel| channel.enabled)
        .cloned()
    {
        let bus_root = device.bus_root.clone();
        let stop_flag = Arc::clone(&stop);
        let thread_name = format!("rollio-pseudo-{}", channel.channel_type);
        let handle = std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                let result = match channel.kind {
                    DeviceType::Camera => {
                        run_camera_channel(bus_root, channel, Arc::clone(&stop_flag))
                    }
                    DeviceType::Robot => {
                        run_robot_channel(bus_root, channel, Arc::clone(&stop_flag))
                    }
                };
                if result.is_err() {
                    stop_flag.store(true, Ordering::Relaxed);
                }
                result.map_err(|error| error.to_string())
            })?;
        handles.push(handle);
    }

    let mut first_error = None;
    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
            Err(_) => {
                if first_error.is_none() {
                    first_error = Some("pseudo channel thread panicked".to_owned());
                }
            }
        }
    }

    if let Some(error) = first_error {
        Err(error.into())
    } else {
        Ok(())
    }
}

fn camera_period(fps: u32) -> Duration {
    Duration::from_secs_f64(1.0 / fps.max(1) as f64)
}

fn publish_camera_frame(camera: &mut CameraRuntime) -> Result<(), Box<dyn Error>> {
    generate_color_bars(
        &mut camera.frame,
        camera.width,
        camera.height,
        camera.frame_index,
    );
    let timestamp_ms = unix_timestamp_ms();
    let mut sample = camera.publisher.loan_slice_uninit(camera.frame.len())?;
    *sample.user_header_mut() = CameraFrameHeader {
        timestamp_ms,
        width: camera.width,
        height: camera.height,
        pixel_format: PixelFormat::Rgb24,
        frame_index: camera.frame_index,
    };
    let sample = sample.write_from_slice(&camera.frame);
    sample.send()?;
    camera.frame_index += 1;
    Ok(())
}

fn run_camera_channel(
    bus_root: String,
    channel: rollio_types::config::DeviceChannelConfigV2,
    stop: Arc<AtomicBool>,
) -> Result<(), Box<dyn Error>> {
    let channel_type = channel.channel_type.clone();
    let profile = channel.profile.ok_or("pseudo camera requires profile")?;
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;
    let service_name: ServiceName = channel_frames_service_name(&bus_root, &channel.channel_type)
        .as_str()
        .try_into()?;
    let payload_len =
        profile.width as usize * profile.height as usize * profile.pixel_format.bytes_per_pixel();
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<[u8]>()
        .user_header::<CameraFrameHeader>()
        .open_or_create()?;
    let publisher = service
        .publisher_builder()
        .initial_max_slice_len(payload_len)
        .allocation_strategy(AllocationStrategy::PowerOfTwo)
        .create()?;
    let shutdown_subscriber = open_shutdown_subscriber(&node)?;
    let mode_info_publisher = open_channel_mode_publisher(&node, &bus_root, &channel_type)?;
    let mut camera = CameraRuntime {
        _channel_type: channel_type,
        width: profile.width,
        height: profile.height,
        fps: profile.fps,
        publisher,
        frame: vec![0; payload_len],
        frame_index: 0,
        next_tick: Instant::now(),
    };

    loop {
        if stop.load(Ordering::Relaxed) || drain_shutdown_events(&shutdown_subscriber)? {
            return Ok(());
        }

        let now = Instant::now();
        if now >= camera.next_tick {
            publish_camera_frame(&mut camera)?;
            camera.next_tick += camera_period(camera.fps);
            mode_info_publisher.send_copy(DeviceChannelMode::Enabled)?;
        } else {
            std::thread::sleep((camera.next_tick - now).min(Duration::from_millis(5)));
        }
    }
}

fn publish_robot_states(robot: &mut RobotRuntime) -> Result<(), Box<dyn Error>> {
    if robot.mode == RobotMode::CommandFollowing {
        drain_commands(robot)?;
        update_command_following_state(robot);
    } else if robot.mode != RobotMode::Disabled {
        update_free_drive_state(robot);
    }

    let timestamp_ms = unix_timestamp_ms();
    let positions = robot.current_positions;
    let mut velocities = [0.0f64; rollio_types::messages::MAX_DOF];
    let mut efforts = [0.0f64; rollio_types::messages::MAX_DOF];
    let elapsed_secs = robot.started_at.elapsed().as_secs_f64();
    for joint_idx in 0..robot.dof {
        velocities[joint_idx] = positions[joint_idx] - robot.previous_positions[joint_idx];
        efforts[joint_idx] = match robot.mode {
            RobotMode::Disabled => 0.0,
            RobotMode::FreeDrive | RobotMode::Identifying => {
                0.1 * (elapsed_secs * 0.5 + joint_idx as f64).sin()
            }
            RobotMode::CommandFollowing => {
                let kp = robot
                    .command_defaults
                    .joint_mit_kp
                    .get(joint_idx)
                    .copied()
                    .unwrap_or(1.0);
                (robot.target_positions[joint_idx] - positions[joint_idx]) * kp
            }
        };
    }
    robot.previous_positions = positions;

    let joint_positions = JointVector15::from_slice(timestamp_ms, &positions[..robot.dof]);
    let joint_velocities = JointVector15::from_slice(timestamp_ms, &velocities[..robot.dof]);
    let joint_efforts = JointVector15::from_slice(timestamp_ms, &efforts[..robot.dof]);
    let ee_pose = Pose7 {
        timestamp_ms,
        values: [
            positions[0],
            positions.get(1).copied().unwrap_or_default(),
            positions.get(2).copied().unwrap_or_default(),
            0.0,
            0.0,
            0.0,
            1.0,
        ],
    };

    for publisher in &robot.state_publishers {
        match publisher {
            StatePublisher::JointPosition(publisher) => {
                publisher.send_copy(joint_positions)?;
            }
            StatePublisher::JointVelocity(publisher) => {
                publisher.send_copy(joint_velocities)?;
            }
            StatePublisher::JointEffort(publisher) => {
                publisher.send_copy(joint_efforts)?;
            }
            StatePublisher::EndEffectorPose(publisher) => {
                publisher.send_copy(ee_pose)?;
            }
        }
    }

    Ok(())
}

fn run_robot_channel(
    bus_root: String,
    channel: rollio_types::config::DeviceChannelConfigV2,
    stop: Arc<AtomicBool>,
) -> Result<(), Box<dyn Error>> {
    let dof = channel.dof.ok_or("pseudo robot requires dof")? as usize;
    let mode = channel.mode.ok_or("pseudo robot requires mode")?;
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;
    let shutdown_subscriber = open_shutdown_subscriber(&node)?;
    let mode_subscriber = open_channel_mode_subscriber(&node, &bus_root, &channel.channel_type)?;
    let mode_info_publisher = open_channel_mode_publisher(&node, &bus_root, &channel.channel_type)?;
    let mut state_publishers = Vec::new();
    for state_kind in &channel.publish_states {
        let topic: ServiceName =
            channel_state_service_name(&bus_root, &channel.channel_type, state_kind.topic_suffix())
                .as_str()
                .try_into()?;
        match state_kind {
            RobotStateKind::JointPosition => {
                let service = node
                    .service_builder(&topic)
                    .publish_subscribe::<JointVector15>()
                    .open_or_create()?;
                state_publishers.push(StatePublisher::JointPosition(
                    service.publisher_builder().create()?,
                ));
            }
            RobotStateKind::JointVelocity => {
                let service = node
                    .service_builder(&topic)
                    .publish_subscribe::<JointVector15>()
                    .open_or_create()?;
                state_publishers.push(StatePublisher::JointVelocity(
                    service.publisher_builder().create()?,
                ));
            }
            RobotStateKind::JointEffort => {
                let service = node
                    .service_builder(&topic)
                    .publish_subscribe::<JointVector15>()
                    .open_or_create()?;
                state_publishers.push(StatePublisher::JointEffort(
                    service.publisher_builder().create()?,
                ));
            }
            RobotStateKind::EndEffectorPose => {
                let service = node
                    .service_builder(&topic)
                    .publish_subscribe::<Pose7>()
                    .open_or_create()?;
                state_publishers.push(StatePublisher::EndEffectorPose(
                    service.publisher_builder().create()?,
                ));
            }
            _ => {}
        }
    }

    let mut command_subscribers = Vec::new();
    for command_kind in [RobotCommandKind::JointPosition, RobotCommandKind::JointMit] {
        let topic: ServiceName = channel_command_service_name(
            &bus_root,
            &channel.channel_type,
            command_kind.topic_suffix(),
        )
        .as_str()
        .try_into()?;
        match command_kind {
            RobotCommandKind::JointPosition => {
                let service = node
                    .service_builder(&topic)
                    .publish_subscribe::<JointVector15>()
                    .open_or_create()?;
                command_subscribers.push(CommandSubscriber::JointPosition(
                    service.subscriber_builder().create()?,
                ));
            }
            RobotCommandKind::JointMit => {
                let service = node
                    .service_builder(&topic)
                    .publish_subscribe::<JointMitCommand15>()
                    .open_or_create()?;
                command_subscribers.push(CommandSubscriber::JointMit(
                    service.subscriber_builder().create()?,
                ));
            }
            _ => {}
        }
    }

    let mut robot = RobotRuntime {
        _channel_type: channel.channel_type,
        dof,
        mode,
        frequency_hz: channel.control_frequency_hz.unwrap_or(60.0),
        state_publishers,
        command_subscribers,
        command_defaults: channel.command_defaults,
        current_positions: [0.0; rollio_types::messages::MAX_DOF],
        target_positions: [0.0; rollio_types::messages::MAX_DOF],
        previous_positions: [0.0; rollio_types::messages::MAX_DOF],
        next_tick: Instant::now(),
        started_at: Instant::now(),
    };

    loop {
        if stop.load(Ordering::Relaxed) || drain_shutdown_events(&shutdown_subscriber)? {
            return Ok(());
        }

        if let Some(next_mode) = drain_robot_mode_events(&mode_subscriber)? {
            robot.mode = next_mode;
        }
        mode_info_publisher.send_copy(robot_mode_to_channel_mode(robot.mode))?;

        let now = Instant::now();
        if now >= robot.next_tick {
            publish_robot_states(&mut robot)?;
            robot.next_tick += Duration::from_secs_f64(1.0 / robot.frequency_hz.max(1.0));
        } else {
            std::thread::sleep((robot.next_tick - now).min(Duration::from_millis(5)));
        }
    }
}

fn drain_commands(robot: &mut RobotRuntime) -> Result<(), Box<dyn Error>> {
    for subscriber in &robot.command_subscribers {
        match subscriber {
            CommandSubscriber::JointPosition(subscriber) => loop {
                let Some(sample) = subscriber.receive()? else {
                    break;
                };
                let payload = sample.payload();
                let active = robot.dof.min(payload.len as usize);
                robot.target_positions[..active].copy_from_slice(&payload.values[..active]);
            },
            CommandSubscriber::JointMit(subscriber) => loop {
                let Some(sample) = subscriber.receive()? else {
                    break;
                };
                let payload = sample.payload();
                let active = robot.dof.min(payload.len as usize);
                robot.target_positions[..active].copy_from_slice(&payload.position[..active]);
            },
        }
    }
    Ok(())
}

fn update_free_drive_state(robot: &mut RobotRuntime) {
    let elapsed_secs = robot.started_at.elapsed().as_secs_f64();
    for (joint_idx, position) in robot
        .current_positions
        .iter_mut()
        .take(robot.dof)
        .enumerate()
    {
        let frequency = 0.5 + joint_idx as f64 * 0.075;
        *position =
            (elapsed_secs * frequency * std::f64::consts::TAU + joint_idx as f64 * 0.4).sin();
    }
}

fn update_command_following_state(robot: &mut RobotRuntime) {
    let period = 1.0 / robot.frequency_hz.max(1.0);
    let alpha = (period / 0.05f64.max(period)).clamp(0.0, 1.0);
    for joint_idx in 0..robot.dof {
        let error = robot.target_positions[joint_idx] - robot.current_positions[joint_idx];
        robot.current_positions[joint_idx] += error * alpha;
    }
}

fn drain_shutdown_events(subscriber: &ShutdownSubscriber) -> Result<bool, Box<dyn Error>> {
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

fn open_shutdown_subscriber(
    node: &Node<ipc::Service>,
) -> Result<ShutdownSubscriber, Box<dyn Error>> {
    let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
    let control_service = node
        .service_builder(&control_service_name)
        .publish_subscribe::<ControlEvent>()
        .open_or_create()?;
    Ok(control_service.subscriber_builder().create()?)
}

fn open_channel_mode_subscriber(
    node: &Node<ipc::Service>,
    bus_root: &str,
    channel_type: &str,
) -> Result<ChannelModeSubscriber, Box<dyn Error>> {
    let mode_service_name: ServiceName = channel_mode_control_service_name(bus_root, channel_type)
        .as_str()
        .try_into()?;
    let mode_service = node
        .service_builder(&mode_service_name)
        .publish_subscribe::<DeviceChannelMode>()
        .max_publishers(16)
        .max_subscribers(16)
        .max_nodes(16)
        .open_or_create()?;
    Ok(mode_service.subscriber_builder().create()?)
}

fn open_channel_mode_publisher(
    node: &Node<ipc::Service>,
    bus_root: &str,
    channel_type: &str,
) -> Result<ChannelModePublisher, Box<dyn Error>> {
    let mode_service_name: ServiceName = channel_mode_info_service_name(bus_root, channel_type)
        .as_str()
        .try_into()?;
    let mode_service = node
        .service_builder(&mode_service_name)
        .publish_subscribe::<DeviceChannelMode>()
        .max_publishers(16)
        .max_subscribers(16)
        .max_nodes(16)
        .open_or_create()?;
    Ok(mode_service.publisher_builder().create()?)
}

fn drain_robot_mode_events(
    subscriber: &ChannelModeSubscriber,
) -> Result<Option<RobotMode>, Box<dyn Error>> {
    let mut latest = None;
    loop {
        match subscriber.receive()? {
            Some(sample) => {
                latest = match *sample.payload() {
                    DeviceChannelMode::FreeDrive => Some(RobotMode::FreeDrive),
                    DeviceChannelMode::CommandFollowing => Some(RobotMode::CommandFollowing),
                    DeviceChannelMode::Identifying => Some(RobotMode::Identifying),
                    DeviceChannelMode::Disabled => Some(RobotMode::Disabled),
                    DeviceChannelMode::Enabled => Some(RobotMode::FreeDrive),
                };
            }
            None => return Ok(latest),
        }
    }
}

fn robot_mode_to_channel_mode(mode: RobotMode) -> DeviceChannelMode {
    match mode {
        RobotMode::FreeDrive => DeviceChannelMode::FreeDrive,
        RobotMode::CommandFollowing => DeviceChannelMode::CommandFollowing,
        RobotMode::Identifying => DeviceChannelMode::Identifying,
        RobotMode::Disabled => DeviceChannelMode::Disabled,
    }
}

fn pseudo_probe_ids(sim_cameras: u32, sim_arms: u32, dof: u32) -> Vec<String> {
    let mut ids = Vec::new();
    for index in 0..sim_cameras {
        ids.push(format!("pseudo_camera_{index}"));
    }
    for index in 0..sim_arms {
        ids.push(format!("pseudo_robot_{index}_dof_{dof}"));
    }
    ids
}

#[allow(clippy::manual_map)]
fn query_pseudo_device(id: &str) -> Option<DeviceQueryDevice> {
    if id.starts_with("pseudo_camera_") {
        Some(DeviceQueryDevice {
            id: id.into(),
            device_class: "pseudo-camera".into(),
            device_label: "Pseudo Camera".into(),
            default_device_name: Some("pseudo_camera".into()),
            optional_info: Default::default(),
            channels: vec![DeviceQueryChannel {
                channel_type: "color".into(),
                kind: DeviceType::Camera,
                available: true,
                channel_label: Some("Pseudo Camera".into()),
                default_name: Some("pseudo_camera".into()),
                modes: vec!["enabled".into(), "disabled".into()],
                profiles: vec![
                    CameraChannelProfile {
                        width: 640,
                        height: 480,
                        fps: 30,
                        pixel_format: PixelFormat::Rgb24,
                        native_pixel_format: None,
                    },
                    CameraChannelProfile {
                        width: 1280,
                        height: 720,
                        fps: 30,
                        pixel_format: PixelFormat::Rgb24,
                        native_pixel_format: None,
                    },
                ],
                supported_states: Vec::new(),
                supported_commands: Vec::new(),
                supports_fk: false,
                supports_ik: false,
                dof: None,
                default_control_frequency_hz: None,
                direct_joint_compatibility: DirectJointCompatibility::default(),
                defaults: ChannelCommandDefaults::default(),
                value_limits: Vec::new(),
                optional_info: Default::default(),
            }],
        })
    } else if let Some(dof) = parse_robot_dof(id) {
        Some(DeviceQueryDevice {
            id: id.into(),
            device_class: "pseudo-robot".into(),
            device_label: if dof == 1 {
                "Pseudo End Effector".into()
            } else {
                "Pseudo Arm".into()
            },
            default_device_name: Some(if dof == 1 {
                "pseudo_eef".into()
            } else {
                "pseudo_arm".into()
            }),
            optional_info: Default::default(),
            channels: vec![DeviceQueryChannel {
                channel_type: "arm".into(),
                kind: DeviceType::Robot,
                available: true,
                channel_label: Some(if dof == 1 {
                    "Pseudo End Effector".into()
                } else {
                    "Pseudo Arm".into()
                }),
                default_name: Some(if dof == 1 {
                    "pseudo_eef".into()
                } else {
                    "pseudo_arm".into()
                }),
                modes: vec![
                    "free-drive".into(),
                    "command-following".into(),
                    "identifying".into(),
                    "disabled".into(),
                ],
                profiles: Vec::new(),
                supported_states: vec![
                    RobotStateKind::JointPosition,
                    RobotStateKind::JointVelocity,
                    RobotStateKind::JointEffort,
                    RobotStateKind::EndEffectorPose,
                ],
                supported_commands: vec![
                    RobotCommandKind::JointPosition,
                    RobotCommandKind::JointMit,
                ],
                supports_fk: true,
                supports_ik: false,
                dof: Some(dof),
                default_control_frequency_hz: Some(60.0),
                direct_joint_compatibility: DirectJointCompatibility {
                    can_lead: vec![DirectJointCompatibilityPeer {
                        driver: DRIVER_NAME.into(),
                        channel_type: "arm".into(),
                    }],
                    can_follow: vec![DirectJointCompatibilityPeer {
                        driver: DRIVER_NAME.into(),
                        channel_type: "arm".into(),
                    }],
                },
                defaults: ChannelCommandDefaults {
                    joint_mit_kp: vec![1.0; dof as usize],
                    joint_mit_kd: vec![0.1; dof as usize],
                    parallel_mit_kp: Vec::new(),
                    parallel_mit_kd: Vec::new(),
                },
                value_limits: pseudo_robot_value_limits(dof),
                optional_info: Default::default(),
            }],
        })
    } else {
        None
    }
}

/// Pseudo robot sweeps joints between -1 and 1 rad in `update_free_drive_state`,
/// so a ±π envelope keeps a comfortable margin while making the bars
/// meaningful. Velocity / effort use bounds matching the synthetic feedback
/// behaviour (small noise + sinusoid).
fn pseudo_robot_value_limits(dof: u32) -> Vec<StateValueLimitsEntry> {
    let dof = dof as usize;
    vec![
        StateValueLimitsEntry::symmetric(RobotStateKind::JointPosition, std::f64::consts::PI, dof),
        StateValueLimitsEntry::symmetric(RobotStateKind::JointVelocity, 1.0, dof),
        StateValueLimitsEntry::symmetric(RobotStateKind::JointEffort, 1.0, dof),
        StateValueLimitsEntry::new(
            RobotStateKind::EndEffectorPose,
            vec![-1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0],
            vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        ),
    ]
}

fn parse_robot_dof(id: &str) -> Option<u32> {
    let marker = "_dof_";
    let (_, tail) = id.rsplit_once(marker)?;
    tail.parse().ok()
}

fn print_query_human(response: &DeviceQueryResponse) {
    for device in &response.devices {
        println!("{} ({})", device.device_label, device.id);
        for channel in &device.channels {
            println!("  - {} [{}]", channel.channel_type, kind_name(channel.kind));
        }
    }
}

fn kind_name(kind: DeviceType) -> &'static str {
    match kind {
        DeviceType::Camera => "camera",
        DeviceType::Robot => "robot",
    }
}

fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn generate_color_bars(buf: &mut [u8], width: u32, height: u32, frame_index: u64) {
    const COLORS: [(u8, u8, u8); 8] = [
        (255, 255, 255),
        (255, 255, 0),
        (0, 255, 255),
        (0, 255, 0),
        (255, 0, 255),
        (255, 0, 0),
        (0, 0, 255),
        (0, 0, 0),
    ];

    let w = width as usize;
    let h = height as usize;
    let bar_width = (w / COLORS.len()).max(1);
    let scroll = (frame_index as usize) % w.max(1);

    for y in 0..h {
        let row_offset = y * w * 3;
        for x in 0..w {
            let shifted = (x + scroll) % w;
            let bar_idx = (shifted / bar_width).min(COLORS.len() - 1);
            let (r, g, b) = COLORS[bar_idx];
            let px = row_offset + x * 3;
            buf[px] = r;
            buf[px + 1] = g;
            buf[px + 2] = b;
        }
    }
}
