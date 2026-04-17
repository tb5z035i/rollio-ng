use airbot_play_rust::arm::{ArmJointFeedback, ArmState, ARM_DOF};
use airbot_play_rust::can::worker::{CanTxPriority, CanWorkerBackend};
use airbot_play_rust::client::AirbotPlayClient;
use airbot_play_rust::eef::SingleEefCommand;
use airbot_play_rust::model::{ModelBackendKind, MountedEefType, Pose};
use airbot_play_rust::protocol::board::gpio::PlayLedProtocol;
use airbot_play_rust::probe::discover::probe_all;
use clap::{Args, Parser, Subcommand};
use iceoryx2::prelude::*;
use rollio_bus::{
    channel_command_service_name, channel_mode_control_service_name,
    channel_mode_info_service_name, channel_state_service_name, CONTROL_EVENTS_SERVICE,
};
use rollio_types::config::{
    BinaryDeviceConfig, ChannelCommandDefaults, DeviceQueryChannel, DeviceQueryDevice,
    DeviceQueryResponse, DeviceType, DirectJointCompatibility, DirectJointCompatibilityPeer,
    RobotCommandKind, RobotMode, RobotStateKind,
};
use rollio_types::messages::{
    ControlEvent, DeviceChannelMode, JointMitCommand15, JointVector15, ParallelMitCommand2,
    ParallelVector2, Pose7,
};
use serde_json::json;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DRIVER_NAME: &str = "airbot-play";
const DEFAULT_PROBE_TIMEOUT_MS: u64 = 1000;

#[derive(Debug, Parser)]
#[command(name = "rollio-device-airbot-play")]
#[command(about = "AIRBOT Play device driver on the Sprint Extra A contract")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Subcommand)]
enum Command {
    Probe {
        #[arg(long, default_value_t = DEFAULT_PROBE_TIMEOUT_MS)]
        timeout_ms: u64,
        #[arg(long)]
        json: bool,
    },
    Validate {
        id: String,
        #[arg(long = "channel-type")]
        channel_types: Vec<String>,
        #[arg(long, default_value_t = DEFAULT_PROBE_TIMEOUT_MS)]
        timeout_ms: u64,
        #[arg(long)]
        json: bool,
    },
    Query {
        id: String,
        #[arg(long, default_value_t = DEFAULT_PROBE_TIMEOUT_MS)]
        timeout_ms: u64,
        #[arg(long)]
        json: bool,
    },
    Run(RunArgs),
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

#[derive(Debug, Clone)]
struct RuntimeConfig {
    bus_root: String,
    id: String,
    interface: String,
    arm: Option<ArmChannelRuntime>,
    eef: Option<EefChannelRuntime>,
}

#[derive(Debug, Clone)]
struct ArmChannelRuntime {
    channel_type: String,
    mode: RobotMode,
    dof: usize,
    control_frequency_hz: f64,
    publish_states: Vec<RobotStateKind>,
}

#[derive(Debug, Clone)]
struct EefChannelRuntime {
    channel_type: String,
    mode: RobotMode,
    control_frequency_hz: f64,
    command_defaults: ChannelCommandDefaults,
    mounted: MountedEefType,
}

type ShutdownSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, ControlEvent, ()>;
type ChannelModeSubscriber =
    iceoryx2::port::subscriber::Subscriber<ipc::Service, DeviceChannelMode, ()>;
type ChannelModePublisher =
    iceoryx2::port::publisher::Publisher<ipc::Service, DeviceChannelMode, ()>;
type ParallelPublisher = iceoryx2::port::publisher::Publisher<ipc::Service, ParallelVector2, ()>;
type ParallelPositionSubscriber =
    iceoryx2::port::subscriber::Subscriber<ipc::Service, ParallelVector2, ()>;
type ParallelMitSubscriber =
    iceoryx2::port::subscriber::Subscriber<ipc::Service, ParallelMitCommand2, ()>;

/// Coordinates the physical AIRBOT Play base LED so it stays on while **either** the arm or EEF
/// channel is in identifying mode (each channel has its own mode IPC service).
struct IdentifyLedGate {
    arm_identifying: AtomicBool,
    eef_identifying: AtomicBool,
    led_on: AtomicBool,
}

impl IdentifyLedGate {
    fn new() -> Self {
        Self {
            arm_identifying: AtomicBool::new(false),
            eef_identifying: AtomicBool::new(false),
            led_on: AtomicBool::new(false),
        }
    }

    fn set_arm_identifying(
        &self,
        runtime: &tokio::runtime::Runtime,
        client: &AirbotPlayClient,
        identifying: bool,
    ) -> Result<(), Box<dyn Error>> {
        self.arm_identifying.store(identifying, Ordering::Relaxed);
        self.sync(runtime, client)
    }

    fn set_eef_identifying(
        &self,
        runtime: &tokio::runtime::Runtime,
        client: &AirbotPlayClient,
        identifying: bool,
    ) -> Result<(), Box<dyn Error>> {
        self.eef_identifying.store(identifying, Ordering::Relaxed);
        self.sync(runtime, client)
    }

    fn sync(
        &self,
        runtime: &tokio::runtime::Runtime,
        client: &AirbotPlayClient,
    ) -> Result<(), Box<dyn Error>> {
        let want = self.arm_identifying.load(Ordering::Relaxed)
            || self.eef_identifying.load(Ordering::Relaxed);
        let prev = self.led_on.load(Ordering::Relaxed);
        if want == prev {
            return Ok(());
        }
        set_identify_led(runtime, client, want)?;
        self.led_on.store(want, Ordering::Relaxed);
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    if let Err(error) = run_cli(Cli::parse()).await {
        eprintln!("rollio-device-airbot-play: {error}");
        std::process::exit(1);
    }
}

async fn run_cli(cli: Cli) -> Result<(), Box<dyn Error>> {
    match cli.command {
        Command::Probe { timeout_ms, json } => {
            let devices = probe_devices(Duration::from_millis(timeout_ms)).await?;
            if json {
                let ids = devices
                    .iter()
                    .map(|device| device.id.clone())
                    .collect::<Vec<_>>();
                println!("{}", serde_json::to_string_pretty(&ids)?);
            } else {
                for device in &devices {
                    println!("AIRBOT Play ({})", device.id);
                }
            }
        }
        Command::Validate {
            id,
            channel_types,
            timeout_ms,
            json,
        } => {
            let device = resolve_probe_device(&id, Duration::from_millis(timeout_ms)).await?;
            let valid = channel_types.is_empty() || channel_types.iter().all(|value| value == "arm");
            let report = json!({
                "valid": valid,
                "id": device.id,
                "channel_types": channel_types,
            });
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else if valid {
                println!("{} is valid", id);
            } else {
                println!("{} is invalid", id);
            }
            if !valid {
                return Err("airbot-play validate failed".into());
            }
        }
        Command::Query { id, timeout_ms, json } => {
            let response = query_device(&id, Duration::from_millis(timeout_ms)).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else {
                for device in &response.devices {
                    println!("{} ({})", device.device_label, device.id);
                    for channel in &device.channels {
                        println!("  - {} [robot]", channel.channel_type);
                    }
                }
            }
        }
        Command::Run(args) => {
            let config = load_runtime_config(&args)?;
            if !args.dry_run {
                run_device(config).await?;
            }
        }
    }
    Ok(())
}

async fn probe_devices(timeout: Duration) -> Result<Vec<DeviceQueryDevice>, Box<dyn Error>> {
    let instances = probe_all(timeout).await?.instances;
    let mut devices = Vec::new();
    for instance in instances {
        let Some(id) = instance
            .product_sn
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let mut channels = vec![DeviceQueryChannel {
            channel_type: "arm".into(),
            kind: DeviceType::Robot,
            available: true,
            channel_label: Some("AIRBOT Play".into()),
            default_name: Some("airbot_play_arm".into()),
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
                RobotCommandKind::EndPose,
            ],
            supports_fk: true,
            supports_ik: true,
            dof: Some(ARM_DOF as u32),
            default_control_frequency_hz: Some(250.0),
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
            defaults: ChannelCommandDefaults::default(),
            optional_info: Default::default(),
        }];
        let mounted = instance
            .mounted_eef
            .as_deref()
            .map(MountedEefType::from_label);
        if let Some((eef_channel_type, defaults)) = mounted_eef_channel(mounted.as_ref()) {
            let (channel_label, default_name) = match eef_channel_type.as_str() {
                "e2" => ("AIRBOT E2".to_string(), "airbot_e2".to_string()),
                "g2" => ("AIRBOT G2".to_string(), "airbot_g2".to_string()),
                other => (
                    format!("AIRBOT {}", other.to_uppercase()),
                    format!("airbot_{other}"),
                ),
            };
            channels.push(DeviceQueryChannel {
                channel_type: eef_channel_type,
                kind: DeviceType::Robot,
                available: true,
                channel_label: Some(channel_label),
                default_name: Some(default_name),
                modes: vec![
                    "free-drive".into(),
                    "command-following".into(),
                    "identifying".into(),
                    "disabled".into(),
                ],
                profiles: Vec::new(),
                supported_states: vec![
                    RobotStateKind::ParallelPosition,
                    RobotStateKind::ParallelVelocity,
                    RobotStateKind::ParallelEffort,
                ],
                supported_commands: vec![
                    RobotCommandKind::ParallelPosition,
                    RobotCommandKind::ParallelMit,
                ],
                supports_fk: false,
                supports_ik: false,
                dof: Some(1),
                default_control_frequency_hz: Some(250.0),
                direct_joint_compatibility: DirectJointCompatibility::default(),
                defaults,
                optional_info: Default::default(),
            });
        }
        let mut device = DeviceQueryDevice {
            id: id.to_owned(),
            device_class: DRIVER_NAME.into(),
            device_label: "AIRBOT Play".into(),
            optional_info: Default::default(),
            channels,
        };
        device
            .optional_info
            .insert("interface".into(), instance.interface.clone().into());
        device.optional_info.insert("transport".into(), "can".into());
        if let Some(mounted) = mounted.as_ref() {
            if !matches!(mounted, MountedEefType::None) {
                device
                    .optional_info
                    .insert("end_effector".into(), mounted.as_label().to_owned().into());
            }
        }
        devices.push(device);
    }
    Ok(devices)
}

async fn resolve_probe_device(id: &str, timeout: Duration) -> Result<DeviceQueryDevice, Box<dyn Error>> {
    let devices = probe_devices(timeout).await?;
    devices
        .into_iter()
        .find(|device| device.id == id)
        .ok_or_else(|| format!("unknown AIRBOT Play device id: {id}").into())
}

async fn query_device(id: &str, timeout: Duration) -> Result<DeviceQueryResponse, Box<dyn Error>> {
    let device = resolve_probe_device(id, timeout).await?;
    Ok(DeviceQueryResponse {
        driver: DRIVER_NAME.into(),
        devices: vec![device],
    })
}

fn load_runtime_config(args: &RunArgs) -> Result<RuntimeConfig, Box<dyn Error>> {
    let device = if let Some(config_path) = &args.config {
        BinaryDeviceConfig::from_file(config_path)?
    } else if let Some(config_inline) = &args.config_inline {
        config_inline.parse::<BinaryDeviceConfig>()?
    } else {
        return Err("run requires either --config or --config-inline".into());
    };
    if device.driver != DRIVER_NAME {
        return Err(format!("expected driver {DRIVER_NAME}, got {}", device.driver).into());
    }
    let mounted = MountedEefType::from_label(
        device
            .extra
            .get("end_effector")
            .and_then(|value| value.as_str())
            .unwrap_or("none"),
    );
    let interface = device
        .extra
        .get("interface")
        .and_then(|value| value.as_str())
        .ok_or("AIRBOT Play requires extra.interface")?
        .to_owned();
    Ok(RuntimeConfig {
        bus_root: device.bus_root,
        id: device.id,
        interface,
        arm: device
            .channels
            .iter()
            .find(|channel| channel.enabled && channel.kind == DeviceType::Robot && channel.channel_type == "arm")
            .map(|channel| ArmChannelRuntime {
                channel_type: channel.channel_type.clone(),
                mode: channel.mode.unwrap_or(RobotMode::FreeDrive),
                dof: channel.dof.unwrap_or(ARM_DOF as u32) as usize,
                control_frequency_hz: channel.control_frequency_hz.unwrap_or(250.0),
                publish_states: if channel.publish_states.is_empty() {
                    vec![
                        RobotStateKind::JointPosition,
                        RobotStateKind::JointVelocity,
                        RobotStateKind::JointEffort,
                        RobotStateKind::EndEffectorPose,
                    ]
                } else {
                    channel.publish_states.clone()
                },
            }),
        eef: device
            .channels
            .iter()
            .find(|channel| channel.enabled && channel.kind == DeviceType::Robot && channel.channel_type != "arm")
            .map(|channel| EefChannelRuntime {
                channel_type: channel.channel_type.clone(),
                mode: channel.mode.unwrap_or(RobotMode::FreeDrive),
                control_frequency_hz: channel.control_frequency_hz.unwrap_or(250.0),
                command_defaults: channel.command_defaults.clone(),
                mounted,
            }),
    })
}

async fn run_device(config: RuntimeConfig) -> Result<(), Box<dyn Error>> {
    let client = Arc::new(
        AirbotPlayClient::connect_control_with_backends(
            config.interface.clone(),
            CanWorkerBackend::AsyncFd,
            ModelBackendKind::PlayAnalytical,
        )
        .await?,
    );
    let identify_led_gate = Arc::new(IdentifyLedGate::new());
    let stop = Arc::new(AtomicBool::new(false));

    // Catch SIGINT / SIGTERM and flip the same `stop` flag the channel
    // loops already poll, so the cleanup at the end of `run_arm_channel` /
    // `run_eef_channel` can run `set_arm_state(ArmState::Disabled)` /
    // `set_eef_state(EefState::Disabled)` before the process exits. Without
    // this, Ctrl+C in the controller's terminal also reaches the device
    // process group and kills the device with the default SIGINT action,
    // bypassing the cleanup — so the arm motors stayed energized after a
    // collect session ended.
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop))?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop))?;

    let mut handles = Vec::new();

    if let Some(arm) = config.arm.clone() {
        let client = Arc::clone(&client);
        let bus_root = config.bus_root.clone();
        let stop_flag = Arc::clone(&stop);
        let identify_led_gate = Arc::clone(&identify_led_gate);
        let handle = std::thread::Builder::new()
            .name(format!("rollio-airbot-arm-{}", arm.channel_type))
            .spawn(move || {
                let result = run_arm_channel(
                    client,
                    bus_root,
                    arm,
                    Arc::clone(&stop_flag),
                    identify_led_gate,
                );
                if result.is_err() {
                    stop_flag.store(true, Ordering::Relaxed);
                }
                result.map_err(|error| error.to_string())
            })?;
        handles.push(handle);
    }

    if let Some(eef) = config.eef.clone() {
        let client = Arc::clone(&client);
        let bus_root = config.bus_root.clone();
        let stop_flag = Arc::clone(&stop);
        let identify_led_gate = Arc::clone(&identify_led_gate);
        let handle = std::thread::Builder::new()
            .name(format!("rollio-airbot-eef-{}", eef.channel_type))
            .spawn(move || {
                let result = run_eef_channel(
                    client,
                    bus_root,
                    eef,
                    Arc::clone(&stop_flag),
                    identify_led_gate,
                );
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
                    first_error = Some("AIRBOT channel thread panicked".to_owned());
                }
            }
        }
    }

    client.shutdown_gracefully().await?;

    if let Some(error) = first_error {
        Err(error.into())
    } else {
        Ok(())
    }
}

fn arm_state_for_mode(mode: RobotMode) -> ArmState {
    match mode {
        RobotMode::Disabled => ArmState::Disabled,
        RobotMode::FreeDrive | RobotMode::Identifying => ArmState::FreeDrive,
        RobotMode::CommandFollowing => ArmState::CommandFollowing,
    }
}

fn eef_state_for_mode(mode: RobotMode) -> airbot_play_rust::eef::EefState {
    match mode {
        RobotMode::Disabled => airbot_play_rust::eef::EefState::Disabled,
        RobotMode::FreeDrive | RobotMode::CommandFollowing | RobotMode::Identifying => {
            airbot_play_rust::eef::EefState::Enabled
        }
    }
}

fn run_arm_channel(
    client: Arc<AirbotPlayClient>,
    bus_root: String,
    config: ArmChannelRuntime,
    stop: Arc<AtomicBool>,
    identify_led_gate: Arc<IdentifyLedGate>,
) -> Result<(), Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;
    let joint_position_topic: ServiceName = channel_state_service_name(
        &bus_root,
        &config.channel_type,
        RobotStateKind::JointPosition.topic_suffix(),
    )
    .as_str()
    .try_into()?;
    let joint_velocity_topic: ServiceName = channel_state_service_name(
        &bus_root,
        &config.channel_type,
        RobotStateKind::JointVelocity.topic_suffix(),
    )
    .as_str()
    .try_into()?;
    let joint_effort_topic: ServiceName = channel_state_service_name(
        &bus_root,
        &config.channel_type,
        RobotStateKind::JointEffort.topic_suffix(),
    )
    .as_str()
    .try_into()?;
    let ee_pose_topic: ServiceName = channel_state_service_name(
        &bus_root,
        &config.channel_type,
        RobotStateKind::EndEffectorPose.topic_suffix(),
    )
    .as_str()
    .try_into()?;
    let joint_position_command_topic: ServiceName = channel_command_service_name(
        &bus_root,
        &config.channel_type,
        RobotCommandKind::JointPosition.topic_suffix(),
    )
    .as_str()
    .try_into()?;
    let joint_mit_command_topic: ServiceName = channel_command_service_name(
        &bus_root,
        &config.channel_type,
        RobotCommandKind::JointMit.topic_suffix(),
    )
    .as_str()
    .try_into()?;
    let end_pose_command_topic: ServiceName = channel_command_service_name(
        &bus_root,
        &config.channel_type,
        RobotCommandKind::EndPose.topic_suffix(),
    )
    .as_str()
    .try_into()?;
    let joint_position_service = node
        .service_builder(&joint_position_topic)
        .publish_subscribe::<JointVector15>()
        .open_or_create()?;
    let joint_velocity_service = node
        .service_builder(&joint_velocity_topic)
        .publish_subscribe::<JointVector15>()
        .open_or_create()?;
    let joint_effort_service = node
        .service_builder(&joint_effort_topic)
        .publish_subscribe::<JointVector15>()
        .open_or_create()?;
    let ee_pose_service = node
        .service_builder(&ee_pose_topic)
        .publish_subscribe::<Pose7>()
        .open_or_create()?;
    let joint_position_publisher = joint_position_service.publisher_builder().create()?;
    let joint_velocity_publisher = joint_velocity_service.publisher_builder().create()?;
    let joint_effort_publisher = joint_effort_service.publisher_builder().create()?;
    let ee_pose_publisher = ee_pose_service.publisher_builder().create()?;
    let joint_position_command_service = node
        .service_builder(&joint_position_command_topic)
        .publish_subscribe::<JointVector15>()
        .open_or_create()?;
    let joint_mit_command_service = node
        .service_builder(&joint_mit_command_topic)
        .publish_subscribe::<JointMitCommand15>()
        .open_or_create()?;
    let end_pose_command_service = node
        .service_builder(&end_pose_command_topic)
        .publish_subscribe::<Pose7>()
        .open_or_create()?;
    let joint_position_subscriber = joint_position_command_service.subscriber_builder().create()?;
    let joint_mit_subscriber = joint_mit_command_service.subscriber_builder().create()?;
    let end_pose_subscriber = end_pose_command_service.subscriber_builder().create()?;
    let shutdown_subscriber = open_shutdown_subscriber(&node)?;
    let mode_subscriber = open_channel_mode_subscriber(&node, &bus_root, &config.channel_type)?;
    let mode_info_publisher = open_channel_mode_publisher(&node, &bus_root, &config.channel_type)?;
    let mut current_mode = config.mode;
    let period = Duration::from_secs_f64(1.0 / config.control_frequency_hz.max(1.0));
    let mut next_tick = std::time::Instant::now();

    runtime.block_on(async { client.set_arm_state(arm_state_for_mode(current_mode)).await })?;
    if current_mode == RobotMode::Identifying {
        identify_led_gate.set_arm_identifying(&runtime, &client, true)?;
    }

    loop {
        if stop.load(Ordering::Relaxed) || drain_shutdown_events(&shutdown_subscriber)? {
            break;
        }
        if let Some(next_mode) = drain_channel_mode_events(&mode_subscriber)? {
            if next_mode != current_mode {
                if current_mode == RobotMode::Identifying {
                    identify_led_gate.set_arm_identifying(&runtime, &client, false)?;
                }
                runtime.block_on(async { client.set_arm_state(arm_state_for_mode(next_mode)).await })?;
                if next_mode == RobotMode::Identifying {
                    identify_led_gate.set_arm_identifying(&runtime, &client, true)?;
                }
                current_mode = next_mode;
            }
        }
        mode_info_publisher.send_copy(robot_mode_to_channel_mode(current_mode))?;

        if current_mode == RobotMode::CommandFollowing {
            if let Some(command) = drain_joint_position_command(&joint_position_subscriber)? {
                client.submit_joint_target(command).map(|_| ())?;
            }
            if let Some(command) = drain_joint_mit_command(&joint_mit_subscriber)? {
                client.submit_joint_target(command).map(|_| ())?;
            }
            if let Some(command) = drain_end_pose_command(&end_pose_subscriber)? {
                client.submit_task_target(&command)?;
            }
        } else {
            let _ = drain_joint_position_command(&joint_position_subscriber)?;
            let _ = drain_joint_mit_command(&joint_mit_subscriber)?;
            let _ = drain_end_pose_command(&end_pose_subscriber)?;
        }

        if let Some(feedback) = client.arm().latest_feedback() {
            publish_states(
                &config,
                &feedback,
                client.query_current_pose().ok(),
                &joint_position_publisher,
                &joint_velocity_publisher,
                &joint_effort_publisher,
                &ee_pose_publisher,
            )?;
        }

        let now = std::time::Instant::now();
        next_tick += period;
        if next_tick > now {
            std::thread::sleep(next_tick - now);
        } else {
            next_tick = now;
        }
    }

    identify_led_gate.set_arm_identifying(&runtime, &client, false)?;
    runtime.block_on(async { client.set_arm_state(ArmState::Disabled).await })?;
    Ok(())
}

struct EefStatePublishers {
    position: Option<ParallelPublisher>,
    velocity: Option<ParallelPublisher>,
    effort: Option<ParallelPublisher>,
}

fn run_eef_channel(
    client: Arc<AirbotPlayClient>,
    bus_root: String,
    mut config: EefChannelRuntime,
    stop: Arc<AtomicBool>,
    identify_led_gate: Arc<IdentifyLedGate>,
) -> Result<(), Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;
    let publishers = open_eef_state_publishers(&node, &bus_root, &config)?;
    let parallel_position_sub = open_parallel_position_subscriber(&node, &bus_root, &config)?;
    let parallel_mit_sub = open_parallel_mit_subscriber(&node, &bus_root, &config)?;
    let shutdown_subscriber = open_shutdown_subscriber(&node)?;
    let mode_subscriber = open_channel_mode_subscriber(&node, &bus_root, &config.channel_type)?;
    let mode_info_publisher = open_channel_mode_publisher(&node, &bus_root, &config.channel_type)?;
    let mut current_mode = config.mode;
    let period = Duration::from_secs_f64(1.0 / config.control_frequency_hz.max(1.0));
    let mut next_tick = std::time::Instant::now();
    let mut identify_started_at = None;

    runtime.block_on(async { client.set_eef_state(eef_state_for_mode(current_mode)).await })?;
    if current_mode == RobotMode::Identifying {
        identify_led_gate.set_eef_identifying(&runtime, &client, true)?;
        identify_started_at = Some(std::time::Instant::now());
    }

    loop {
        if stop.load(Ordering::Relaxed) || drain_shutdown_events(&shutdown_subscriber)? {
            break;
        }
        if let Some(next_mode) = drain_channel_mode_events(&mode_subscriber)? {
            if next_mode != current_mode {
                if current_mode == RobotMode::Identifying {
                    identify_led_gate.set_eef_identifying(&runtime, &client, false)?;
                    identify_started_at = None;
                }
                runtime.block_on(async { client.set_eef_state(eef_state_for_mode(next_mode)).await })?;
                if next_mode == RobotMode::Identifying {
                    identify_led_gate.set_eef_identifying(&runtime, &client, true)?;
                    identify_started_at = Some(std::time::Instant::now());
                }
                current_mode = next_mode;
                config.mode = current_mode;
            }
        }
        mode_info_publisher.send_copy(robot_mode_to_channel_mode(current_mode))?;

        match current_mode {
            RobotMode::CommandFollowing => {
                if let Some(command) =
                    drain_eef_command(&parallel_position_sub, &parallel_mit_sub, &config)?
                {
                    submit_eef_command(&runtime, &client, &config.mounted, &command)?;
                }
            }
            RobotMode::Identifying => {
                let _ = drain_eef_command(&parallel_position_sub, &parallel_mit_sub, &config)?;
                if config.mounted == MountedEefType::G2 {
                    let command =
                        identifying_g2_command(&config.command_defaults, identify_started_at.unwrap_or_else(std::time::Instant::now));
                    submit_eef_command(&runtime, &client, &config.mounted, &command)?;
                }
            }
            RobotMode::FreeDrive => {
                let _ = drain_eef_command(&parallel_position_sub, &parallel_mit_sub, &config)?;
            }
            RobotMode::Disabled => {
                let _ = drain_eef_command(&parallel_position_sub, &parallel_mit_sub, &config)?;
            }
        }

        if let Some(feedback) = client.eef().latest_feedback() {
            publish_eef_state(&publishers, &config.publish_states(), &feedback)?;
        }

        let now = std::time::Instant::now();
        next_tick += period;
        if next_tick > now {
            std::thread::sleep(next_tick - now);
        } else {
            next_tick = now;
        }
    }

    identify_led_gate.set_eef_identifying(&runtime, &client, false)?;
    runtime.block_on(async {
        client
            .set_eef_state(airbot_play_rust::eef::EefState::Disabled)
            .await
    })?;
    Ok(())
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

fn drain_joint_position_command(
    subscriber: &iceoryx2::port::subscriber::Subscriber<ipc::Service, JointVector15, ()>,
) -> Result<Option<[f64; ARM_DOF]>, Box<dyn Error>> {
    let mut latest = None;
    loop {
        match subscriber.receive()? {
            Some(sample) => {
                let payload = sample.payload();
                let mut targets = [0.0; ARM_DOF];
                let active = (payload.len as usize).min(ARM_DOF);
                targets[..active].copy_from_slice(&payload.values[..active]);
                latest = Some(targets);
            }
            None => return Ok(latest),
        }
    }
}

fn drain_joint_mit_command(
    subscriber: &iceoryx2::port::subscriber::Subscriber<ipc::Service, JointMitCommand15, ()>,
) -> Result<Option<[f64; ARM_DOF]>, Box<dyn Error>> {
    let mut latest = None;
    loop {
        match subscriber.receive()? {
            Some(sample) => {
                let payload = sample.payload();
                let mut targets = [0.0; ARM_DOF];
                let active = (payload.len as usize).min(ARM_DOF);
                targets[..active].copy_from_slice(&payload.position[..active]);
                latest = Some(targets);
            }
            None => return Ok(latest),
        }
    }
}

fn drain_end_pose_command(
    subscriber: &iceoryx2::port::subscriber::Subscriber<ipc::Service, Pose7, ()>,
) -> Result<Option<Pose>, Box<dyn Error>> {
    let mut latest = None;
    loop {
        match subscriber.receive()? {
            Some(sample) => {
                latest = Some(Pose::from_slice(&sample.payload().values)?);
            }
            None => return Ok(latest),
        }
    }
}

fn publish_states(
    config: &ArmChannelRuntime,
    feedback: &ArmJointFeedback,
    pose: Option<Pose>,
    joint_position_publisher: &iceoryx2::port::publisher::Publisher<ipc::Service, JointVector15, ()>,
    joint_velocity_publisher: &iceoryx2::port::publisher::Publisher<ipc::Service, JointVector15, ()>,
    joint_effort_publisher: &iceoryx2::port::publisher::Publisher<ipc::Service, JointVector15, ()>,
    ee_pose_publisher: &iceoryx2::port::publisher::Publisher<ipc::Service, Pose7, ()>,
) -> Result<(), Box<dyn Error>> {
    if !feedback.valid {
        return Ok(());
    }
    let timestamp_ms = unix_timestamp_ms();
    if config.publish_states.contains(&RobotStateKind::JointPosition) {
        joint_position_publisher.send_copy(JointVector15::from_slice(
            timestamp_ms,
            &feedback.positions[..config.dof.min(ARM_DOF)],
        ))?;
    }
    if config.publish_states.contains(&RobotStateKind::JointVelocity) {
        joint_velocity_publisher.send_copy(JointVector15::from_slice(
            timestamp_ms,
            &feedback.velocities[..config.dof.min(ARM_DOF)],
        ))?;
    }
    if config.publish_states.contains(&RobotStateKind::JointEffort) {
        joint_effort_publisher.send_copy(JointVector15::from_slice(
            timestamp_ms,
            &feedback.torques[..config.dof.min(ARM_DOF)],
        ))?;
    }
    if config.publish_states.contains(&RobotStateKind::EndEffectorPose) {
        if let Some(pose) = pose {
            let pose_values: [f64; 7] = pose
                .as_vec()
                .try_into()
                .map_err(|_| "AIRBOT pose did not contain 7 values")?;
            ee_pose_publisher.send_copy(Pose7 {
                timestamp_ms,
                values: pose_values,
            })?;
        }
    }
    Ok(())
}

fn drain_eef_command(
    position_subscriber: &ParallelPositionSubscriber,
    mit_subscriber: &ParallelMitSubscriber,
    config: &EefChannelRuntime,
) -> Result<Option<SingleEefCommand>, Box<dyn Error>> {
    let mut latest = None;
    while let Some(sample) = position_subscriber.receive()? {
        latest = Some(SingleEefCommand {
            position: sample.payload().values[0],
            velocity: 0.0,
            effort: 0.0,
            mit_kp: config
                .command_defaults
                .parallel_mit_kp
                .first()
                .copied()
                .unwrap_or(0.0),
            mit_kd: config
                .command_defaults
                .parallel_mit_kd
                .first()
                .copied()
                .unwrap_or(0.0),
            current_threshold: 0.0,
        });
    }
    while let Some(sample) = mit_subscriber.receive()? {
        latest = Some(SingleEefCommand {
            position: sample.payload().position[0],
            velocity: sample.payload().velocity[0],
            effort: sample.payload().effort[0],
            mit_kp: if sample.payload().kp[0] == 0.0 {
                config
                    .command_defaults
                    .parallel_mit_kp
                    .first()
                    .copied()
                    .unwrap_or(0.0)
            } else {
                sample.payload().kp[0]
            },
            mit_kd: if sample.payload().kd[0] == 0.0 {
                config
                    .command_defaults
                    .parallel_mit_kd
                    .first()
                    .copied()
                    .unwrap_or(0.0)
            } else {
                sample.payload().kd[0]
            },
            current_threshold: 0.0,
        });
    }
    Ok(latest)
}

fn publish_eef_state(
    publishers: &EefStatePublishers,
    publish_states: &[RobotStateKind],
    feedback: &airbot_play_rust::eef::SingleEefFeedback,
) -> Result<(), Box<dyn Error>> {
    let timestamp_ms = unix_timestamp_ms();
    if publish_states.contains(&RobotStateKind::ParallelPosition) {
        if let Some(publisher) = &publishers.position {
            publisher.send_copy(ParallelVector2::from_slice(timestamp_ms, &[feedback.position]))?;
        }
    }
    if publish_states.contains(&RobotStateKind::ParallelVelocity) {
        if let Some(publisher) = &publishers.velocity {
            publisher.send_copy(ParallelVector2::from_slice(timestamp_ms, &[feedback.velocity]))?;
        }
    }
    if publish_states.contains(&RobotStateKind::ParallelEffort) {
        if let Some(publisher) = &publishers.effort {
            publisher.send_copy(ParallelVector2::from_slice(timestamp_ms, &[feedback.effort]))?;
        }
    }
    Ok(())
}

fn open_shutdown_subscriber(node: &Node<ipc::Service>) -> Result<ShutdownSubscriber, Box<dyn Error>> {
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
    let mode_service_name: ServiceName =
        channel_mode_control_service_name(bus_root, channel_type)
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
    let mode_service_name: ServiceName =
        channel_mode_info_service_name(bus_root, channel_type)
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

fn drain_channel_mode_events(
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

fn open_eef_state_publishers(
    node: &Node<ipc::Service>,
    bus_root: &str,
    config: &EefChannelRuntime,
) -> Result<EefStatePublishers, Box<dyn Error>> {
    let mut position = None;
    let mut velocity = None;
    let mut effort = None;
    if config.publish_states().contains(&RobotStateKind::ParallelPosition) {
        position = Some(open_parallel_publisher(
            node,
            bus_root,
            &config.channel_type,
            RobotStateKind::ParallelPosition,
        )?);
    }
    if config.publish_states().contains(&RobotStateKind::ParallelVelocity) {
        velocity = Some(open_parallel_publisher(
            node,
            bus_root,
            &config.channel_type,
            RobotStateKind::ParallelVelocity,
        )?);
    }
    if config.publish_states().contains(&RobotStateKind::ParallelEffort) {
        effort = Some(open_parallel_publisher(
            node,
            bus_root,
            &config.channel_type,
            RobotStateKind::ParallelEffort,
        )?);
    }
    Ok(EefStatePublishers {
        position,
        velocity,
        effort,
    })
}

fn open_parallel_publisher(
    node: &Node<ipc::Service>,
    bus_root: &str,
    channel_type: &str,
    state_kind: RobotStateKind,
) -> Result<ParallelPublisher, Box<dyn Error>> {
    let topic: ServiceName =
        channel_state_service_name(bus_root, channel_type, state_kind.topic_suffix())
            .as_str()
            .try_into()?;
    let service = node
        .service_builder(&topic)
        .publish_subscribe::<ParallelVector2>()
        .open_or_create()?;
    Ok(service.publisher_builder().create()?)
}

fn open_parallel_position_subscriber(
    node: &Node<ipc::Service>,
    bus_root: &str,
    config: &EefChannelRuntime,
) -> Result<ParallelPositionSubscriber, Box<dyn Error>> {
    let topic: ServiceName = channel_command_service_name(
        bus_root,
        &config.channel_type,
        RobotCommandKind::ParallelPosition.topic_suffix(),
    )
    .as_str()
    .try_into()?;
    let service = node
        .service_builder(&topic)
        .publish_subscribe::<ParallelVector2>()
        .open_or_create()?;
    Ok(service.subscriber_builder().create()?)
}

fn open_parallel_mit_subscriber(
    node: &Node<ipc::Service>,
    bus_root: &str,
    config: &EefChannelRuntime,
) -> Result<ParallelMitSubscriber, Box<dyn Error>> {
    let topic: ServiceName = channel_command_service_name(
        bus_root,
        &config.channel_type,
        RobotCommandKind::ParallelMit.topic_suffix(),
    )
    .as_str()
    .try_into()?;
    let service = node
        .service_builder(&topic)
        .publish_subscribe::<ParallelMitCommand2>()
        .open_or_create()?;
    Ok(service.subscriber_builder().create()?)
}

fn submit_eef_command(
    runtime: &tokio::runtime::Runtime,
    client: &AirbotPlayClient,
    mounted: &MountedEefType,
    command: &SingleEefCommand,
) -> Result<(), Box<dyn Error>> {
    match mounted {
        MountedEefType::E2B => {
            runtime.block_on(async { client.submit_e2_command(command).await })?;
        }
        MountedEefType::G2 => {
            runtime.block_on(async { client.submit_g2_mit_command(command).await })?;
        }
        _ => {}
    }
    Ok(())
}

fn set_identify_led(
    runtime: &tokio::runtime::Runtime,
    client: &AirbotPlayClient,
    enabled: bool,
) -> Result<(), Box<dyn Error>> {
    let frames =
        PlayLedProtocol::new(0x00).generate_led_effect(if enabled { 0x22 } else { 0x1F })?;
    runtime.block_on(async {
        client
            .worker()
            .send_frames(CanTxPriority::Lifecycle, frames)
            .await
    })?;
    Ok(())
}

fn identifying_g2_command(
    defaults: &ChannelCommandDefaults,
    started_at: std::time::Instant,
) -> SingleEefCommand {
    let elapsed = started_at.elapsed().as_secs_f64();
    let phase = elapsed.rem_euclid(2.0);
    let position = if phase < 1.0 {
        0.07 * phase
    } else {
        0.07 * (2.0 - phase)
    };
    SingleEefCommand {
        position,
        velocity: 0.0,
        effort: 0.0,
        mit_kp: defaults.parallel_mit_kp.first().copied().unwrap_or(10.0),
        mit_kd: defaults.parallel_mit_kd.first().copied().unwrap_or(0.5),
        current_threshold: 0.0,
    }
}

impl EefChannelRuntime {
    fn publish_states(&self) -> Vec<RobotStateKind> {
        vec![
            RobotStateKind::ParallelPosition,
            RobotStateKind::ParallelVelocity,
            RobotStateKind::ParallelEffort,
        ]
    }
}

fn mounted_eef_channel(
    mounted: Option<&MountedEefType>,
) -> Option<(String, ChannelCommandDefaults)> {
    match mounted? {
        MountedEefType::E2B => Some((
            "e2".into(),
            ChannelCommandDefaults {
                joint_mit_kp: Vec::new(),
                joint_mit_kd: Vec::new(),
                parallel_mit_kp: vec![0.0],
                parallel_mit_kd: vec![0.0],
            },
        )),
        MountedEefType::G2 => Some((
            "g2".into(),
            ChannelCommandDefaults {
                joint_mit_kp: Vec::new(),
                joint_mit_kd: Vec::new(),
                parallel_mit_kp: vec![10.0],
                parallel_mit_kd: vec![0.5],
            },
        )),
        _ => None,
    }
}

fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
