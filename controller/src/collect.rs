use crate::cli::CollectArgs;
use crate::episode::EpisodeLifecycle;
use crate::process::{
    poll_children_once, spawn_child, terminate_children, ChildSpec, ManagedChild, ResolvedCommand,
    ShutdownTrigger,
};
use crate::runtime_paths::{
    current_executable_dir, resolve_device_program, resolve_program, workspace_root,
};
use iceoryx2::prelude::*;
use rollio_bus::{
    robot_command_service_name, robot_state_service_name, BACKPRESSURE_SERVICE,
    CONTROL_EVENTS_SERVICE, EPISODE_COMMAND_SERVICE, EPISODE_STATUS_SERVICE,
    EPISODE_STORED_SERVICE,
};
use rollio_types::config::{
    AssemblerRuntimeConfig, CollectionMode, Config, DeviceConfig, EncoderRuntimeConfig,
    MappingStrategy, StorageRuntimeConfig, TeleopRuntimeConfig,
};
use rollio_types::messages::{
    BackpressureEvent, ControlEvent, EpisodeCommand, EpisodeState, EpisodeStatus, EpisodeStored,
};
use signal_hook::consts::signal::{SIGINT, SIGTERM};
use std::error::Error;
use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub fn run(args: CollectArgs) -> Result<(), Box<dyn Error>> {
    let config = args.load_config()?;
    run_with_config(config)
}

fn run_with_config(config: Config) -> Result<(), Box<dyn Error>> {
    let workspace_root = workspace_root()?;
    let current_exe_dir = current_executable_dir()?;
    let poll_interval = Duration::from_millis(config.controller.child_poll_interval_ms);
    let shutdown_timeout = Duration::from_millis(config.controller.shutdown_timeout_ms);
    let log_dir = workspace_root.join("target/rollio-logs");
    std::fs::create_dir_all(&log_dir)?;

    let shutdown_requested = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(SIGINT, Arc::clone(&shutdown_requested))?;
    signal_hook::flag::register(SIGTERM, Arc::clone(&shutdown_requested))?;

    let controller_ipc = ControllerIpc::new()?;
    let specs = build_collect_specs(&config, &workspace_root, &current_exe_dir)?;

    let mut children = spawn_collect_children(
        &specs,
        &log_dir,
        shutdown_timeout,
        poll_interval,
        &controller_ipc,
    )?;

    let mut lifecycle = EpisodeLifecycle::default();
    controller_ipc.publish_status(lifecycle.status(Instant::now()))?;

    let trigger = run_collect_loop(
        &mut children,
        shutdown_requested.as_ref(),
        poll_interval,
        &controller_ipc,
        &mut lifecycle,
    )?;

    match &trigger {
        ShutdownTrigger::Signal => eprintln!("rollio: shutdown requested by signal"),
        ShutdownTrigger::ChildExited { id, status } => {
            eprintln!("rollio: child \"{id}\" exited with status {status}")
        }
    }

    controller_ipc.send_shutdown()?;
    terminate_children(&mut children, shutdown_timeout, poll_interval)?;

    for child in &children {
        if let Some(log_path) = &child.log_path {
            eprintln!("rollio: log captured in {}", log_path.display());
        }
    }

    result_for_shutdown_trigger(&trigger)
}

fn spawn_collect_children(
    specs: &[ChildSpec],
    log_dir: &Path,
    shutdown_timeout: Duration,
    poll_interval: Duration,
    controller_ipc: &ControllerIpc,
) -> Result<Vec<ManagedChild>, Box<dyn Error>> {
    let mut children = Vec::new();
    for spec in specs {
        match spawn_child(spec, log_dir) {
            Ok(child) => children.push(child),
            Err(error) => {
                let _ = controller_ipc.send_shutdown();
                let _ = terminate_children(&mut children, shutdown_timeout, poll_interval);
                return Err(format!(
                    "failed to spawn {} (program={:?}, cwd={}): {error}",
                    spec.id,
                    spec.command.program,
                    spec.working_directory.display()
                )
                .into());
            }
        }
    }
    Ok(children)
}

fn run_collect_loop(
    children: &mut [ManagedChild],
    shutdown_requested: &AtomicBool,
    poll_interval: Duration,
    controller_ipc: &ControllerIpc,
    lifecycle: &mut EpisodeLifecycle,
) -> Result<ShutdownTrigger, Box<dyn Error>> {
    let mut start_blocked = false;
    loop {
        if shutdown_requested.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(ShutdownTrigger::Signal);
        }

        if let Some(trigger) = poll_children_once(children)? {
            return Ok(trigger);
        }

        let backpressure_events = controller_ipc.drain_backpressure_events()?;
        apply_backpressure_events(&mut start_blocked, &backpressure_events);

        let stored_events = controller_ipc.drain_episode_stored()?;
        let mut status_changed =
            apply_episode_stored_events(lifecycle, &mut start_blocked, &stored_events);

        let commands = controller_ipc.drain_episode_commands()?;
        let now = Instant::now();
        let (control_events, command_changed) =
            collect_control_events(lifecycle, commands, &mut start_blocked, now);
        status_changed |= command_changed;
        for event in control_events {
            controller_ipc.publish_control_event(event)?;
        }

        if status_changed || lifecycle.state() == EpisodeState::Recording {
            controller_ipc.publish_status(lifecycle.status(Instant::now()))?;
        }

        thread::sleep(poll_interval);
    }
}

fn result_for_shutdown_trigger(trigger: &ShutdownTrigger) -> Result<(), Box<dyn Error>> {
    match trigger {
        ShutdownTrigger::Signal => Ok(()),
        ShutdownTrigger::ChildExited { id, status } => {
            Err(format!("child \"{id}\" exited with status {status}").into())
        }
    }
}

fn build_collect_specs(
    config: &Config,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Result<Vec<ChildSpec>, Box<dyn Error>> {
    let mut specs = build_preview_specs(config, workspace_root, current_exe_dir)?;

    for encoder_config in config.encoder_runtime_configs() {
        specs.push(build_encoder_spec(
            &encoder_config,
            workspace_root,
            current_exe_dir,
        )?);
    }

    let embedded_config_toml = toml::to_string(config)?;
    let assembler_config = config.assembler_runtime_config(embedded_config_toml);
    specs.push(build_assembler_spec(
        &assembler_config,
        workspace_root,
        current_exe_dir,
    )?);

    let storage_config = config.storage_runtime_config();
    specs.push(build_storage_spec(
        &storage_config,
        workspace_root,
        current_exe_dir,
    )?);

    let ui_runtime_config = config.ui_runtime_config();
    let web_bundle_dir = workspace_root.join("ui/web/dist");
    let web_index = web_bundle_dir.join("index.html");
    if !web_index.exists() {
        return Err(format!(
            "Web UI bundle not found at {}. Run `cd ui/web && npm run build` first.",
            web_index.display()
        )
        .into());
    }

    ui_runtime_config
        .websocket_url
        .as_ref()
        .ok_or("ui runtime config did not produce an upstream websocket url")?;
    eprintln!(
        "rollio: web ui available at {}",
        ui_browser_url(&ui_runtime_config.http_host, ui_runtime_config.http_port)
    );
    specs.push(ChildSpec {
        id: "ui".into(),
        command: ResolvedCommand {
            program: resolve_program(current_exe_dir.join("rollio-ui-server"), "rollio-ui-server"),
            args: vec![
                OsString::from("--config-inline"),
                OsString::from(toml::to_string(&ui_runtime_config)?),
                OsString::from("--asset-dir"),
                web_bundle_dir.into_os_string(),
            ],
        },
        working_directory: workspace_root.to_path_buf(),
        inherit_stdio: false,
    });

    Ok(specs)
}

pub(crate) fn build_preview_specs(
    config: &Config,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Result<Vec<ChildSpec>, Box<dyn Error>> {
    let mut specs = Vec::new();

    specs.push(build_visualizer_spec(
        config,
        workspace_root,
        current_exe_dir,
    )?);

    for device in &config.devices {
        specs.push(build_device_spec(device, workspace_root, current_exe_dir)?);
    }

    if config.mode == CollectionMode::Teleop {
        for pair in &config.pairing {
            let leader = config
                .device_named(&pair.leader)
                .ok_or_else(|| format!("missing pairing leader {}", pair.leader))?;
            let follower = config
                .device_named(&pair.follower)
                .ok_or_else(|| format!("missing pairing follower {}", pair.follower))?;
            specs.push(build_teleop_spec(
                pair,
                leader,
                follower,
                workspace_root,
                current_exe_dir,
            )?);
        }
    }

    Ok(specs)
}

pub(crate) fn build_visualizer_spec(
    config: &Config,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Result<ChildSpec, Box<dyn Error>> {
    let visualizer_config = toml::to_string(&config.visualizer_runtime_config())?;
    Ok(ChildSpec {
        id: "visualizer".into(),
        command: ResolvedCommand {
            program: resolve_program(
                current_exe_dir.join("rollio-visualizer"),
                "rollio-visualizer",
            ),
            args: vec![
                OsString::from("--config-inline"),
                OsString::from(visualizer_config),
            ],
        },
        working_directory: workspace_root.to_path_buf(),
        inherit_stdio: false,
    })
}

pub(crate) fn build_device_spec(
    device: &DeviceConfig,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Result<ChildSpec, Box<dyn Error>> {
    let inline_config = toml::to_string(device)?;
    let program = resolve_device_program(
        device.device_type,
        &device.driver,
        workspace_root,
        current_exe_dir,
    );
    let common_args = vec![
        OsString::from("run"),
        OsString::from("--config-inline"),
        OsString::from(inline_config),
    ];

    Ok(ChildSpec {
        id: format!("device-{}", device.name),
        command: ResolvedCommand {
            program,
            args: common_args,
        },
        working_directory: workspace_root.to_path_buf(),
        inherit_stdio: false,
    })
}

pub(crate) fn build_teleop_spec(
    pair: &rollio_types::config::PairConfig,
    leader: &DeviceConfig,
    follower: &DeviceConfig,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Result<ChildSpec, Box<dyn Error>> {
    let follower_dof = follower.dof.unwrap_or(0);
    let joint_index_map = match pair.mapping {
        MappingStrategy::DirectJoint if !pair.joint_index_map.is_empty() => {
            pair.joint_index_map.clone()
        }
        MappingStrategy::DirectJoint => (0..follower_dof).collect(),
        MappingStrategy::Cartesian => Vec::new(),
    };
    let runtime_config = TeleopRuntimeConfig {
        process_id: format!("teleop.{}.to.{}", leader.name, follower.name),
        leader_name: leader.name.clone(),
        follower_name: follower.name.clone(),
        leader_state_topic: robot_state_service_name(&leader.name),
        follower_state_topic: robot_state_service_name(&follower.name),
        follower_command_topic: robot_command_service_name(&follower.name),
        mapping: pair.mapping,
        joint_index_map,
        joint_scales: pair.joint_scales.clone(),
    };
    let inline_config = toml::to_string(&runtime_config)?;

    Ok(ChildSpec {
        id: format!("teleop-{}-to-{}", leader.name, follower.name),
        command: ResolvedCommand {
            program: resolve_program(
                current_exe_dir.join("rollio-teleop-router"),
                "rollio-teleop-router",
            ),
            args: vec![
                OsString::from("run"),
                OsString::from("--config-inline"),
                OsString::from(inline_config),
            ],
        },
        working_directory: workspace_root.to_path_buf(),
        inherit_stdio: false,
    })
}

fn build_encoder_spec(
    config: &EncoderRuntimeConfig,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Result<ChildSpec, Box<dyn Error>> {
    let inline_config = toml::to_string(config)?;
    let camera_name = config
        .camera_name
        .as_deref()
        .unwrap_or(config.process_id.as_str());
    Ok(ChildSpec {
        id: format!("encoder-{camera_name}"),
        command: ResolvedCommand {
            program: resolve_program(current_exe_dir.join("rollio-encoder"), "rollio-encoder"),
            args: vec![
                OsString::from("run"),
                OsString::from("--config-inline"),
                OsString::from(inline_config),
            ],
        },
        working_directory: workspace_root.to_path_buf(),
        inherit_stdio: false,
    })
}

fn build_assembler_spec(
    config: &AssemblerRuntimeConfig,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Result<ChildSpec, Box<dyn Error>> {
    let inline_config = toml::to_string(config)?;
    Ok(ChildSpec {
        id: "assembler".into(),
        command: ResolvedCommand {
            program: resolve_program(
                current_exe_dir.join("rollio-episode-assembler"),
                "rollio-episode-assembler",
            ),
            args: vec![
                OsString::from("run"),
                OsString::from("--config-inline"),
                OsString::from(inline_config),
            ],
        },
        working_directory: workspace_root.to_path_buf(),
        inherit_stdio: false,
    })
}

fn build_storage_spec(
    config: &StorageRuntimeConfig,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Result<ChildSpec, Box<dyn Error>> {
    let inline_config = toml::to_string(config)?;
    Ok(ChildSpec {
        id: "storage".into(),
        command: ResolvedCommand {
            program: resolve_program(current_exe_dir.join("rollio-storage"), "rollio-storage"),
            args: vec![
                OsString::from("run"),
                OsString::from("--config-inline"),
                OsString::from(inline_config),
            ],
        },
        working_directory: workspace_root.to_path_buf(),
        inherit_stdio: false,
    })
}

fn apply_backpressure_events(start_blocked: &mut bool, events: &[BackpressureEvent]) {
    for event in events {
        eprintln!(
            "rollio: backpressure from {} queue={} - blocking new episode starts",
            event.process_id.as_str(),
            event.queue_name.as_str()
        );
        *start_blocked = true;
    }
}

fn apply_episode_stored_events(
    lifecycle: &mut EpisodeLifecycle,
    start_blocked: &mut bool,
    events: &[EpisodeStored],
) -> bool {
    let mut changed = false;
    for event in events {
        if lifecycle.record_episode_stored(event.episode_index) {
            *start_blocked = false;
            changed = true;
        }
    }
    changed
}

fn collect_control_events(
    lifecycle: &mut EpisodeLifecycle,
    commands: Vec<EpisodeCommand>,
    start_blocked: &mut bool,
    now: Instant,
) -> (Vec<ControlEvent>, bool) {
    let mut events = Vec::new();
    let mut status_changed = false;
    for command in commands {
        if matches!(command, EpisodeCommand::Start) && *start_blocked {
            eprintln!("rollio: episode start blocked by pipeline backpressure");
            continue;
        }
        match lifecycle.handle_command(command, now) {
            Ok(event) => {
                if matches!(event, ControlEvent::EpisodeDiscard { .. }) {
                    *start_blocked = false;
                }
                events.push(event);
                status_changed = true;
            }
            Err(error) => eprintln!("rollio: {error}"),
        }
    }
    (events, status_changed)
}

fn ui_browser_url(host: &str, port: u16) -> String {
    let display_host = match host {
        "0.0.0.0" | "::" => "127.0.0.1",
        _ => host,
    };
    format!("http://{display_host}:{port}")
}

struct ControllerIpc {
    _node: Node<ipc::Service>,
    control_publisher: iceoryx2::port::publisher::Publisher<ipc::Service, ControlEvent, ()>,
    episode_command_subscriber:
        iceoryx2::port::subscriber::Subscriber<ipc::Service, EpisodeCommand, ()>,
    episode_status_publisher: iceoryx2::port::publisher::Publisher<ipc::Service, EpisodeStatus, ()>,
    backpressure_subscriber:
        iceoryx2::port::subscriber::Subscriber<ipc::Service, BackpressureEvent, ()>,
    episode_stored_subscriber:
        iceoryx2::port::subscriber::Subscriber<ipc::Service, EpisodeStored, ()>,
}

impl ControllerIpc {
    fn new() -> Result<Self, Box<dyn Error>> {
        let node = NodeBuilder::new()
            .signal_handling_mode(SignalHandlingMode::Disabled)
            .create::<ipc::Service>()?;

        let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
        let control_service = node
            .service_builder(&control_service_name)
            .publish_subscribe::<ControlEvent>()
            .max_publishers(4)
            .max_subscribers(16)
            .max_nodes(16)
            .open_or_create()?;

        let command_service_name: ServiceName = EPISODE_COMMAND_SERVICE.try_into()?;
        let command_service = node
            .service_builder(&command_service_name)
            .publish_subscribe::<EpisodeCommand>()
            .max_publishers(4)
            .max_subscribers(8)
            .max_nodes(8)
            .open_or_create()?;

        let status_service_name: ServiceName = EPISODE_STATUS_SERVICE.try_into()?;
        let status_service = node
            .service_builder(&status_service_name)
            .publish_subscribe::<EpisodeStatus>()
            .max_publishers(4)
            .max_subscribers(8)
            .max_nodes(8)
            .open_or_create()?;

        let backpressure_service_name: ServiceName = BACKPRESSURE_SERVICE.try_into()?;
        let backpressure_service = node
            .service_builder(&backpressure_service_name)
            .publish_subscribe::<BackpressureEvent>()
            .max_publishers(16)
            .max_subscribers(8)
            .max_nodes(16)
            .open_or_create()?;

        let stored_service_name: ServiceName = EPISODE_STORED_SERVICE.try_into()?;
        let stored_service = node
            .service_builder(&stored_service_name)
            .publish_subscribe::<EpisodeStored>()
            .max_publishers(8)
            .max_subscribers(8)
            .max_nodes(16)
            .open_or_create()?;

        Ok(Self {
            _node: node,
            control_publisher: control_service.publisher_builder().create()?,
            episode_command_subscriber: command_service.subscriber_builder().create()?,
            episode_status_publisher: status_service.publisher_builder().create()?,
            backpressure_subscriber: backpressure_service.subscriber_builder().create()?,
            episode_stored_subscriber: stored_service.subscriber_builder().create()?,
        })
    }

    fn drain_episode_commands(&self) -> Result<Vec<EpisodeCommand>, Box<dyn Error>> {
        let mut commands = Vec::new();
        loop {
            let Some(sample) = self.episode_command_subscriber.receive()? else {
                return Ok(commands);
            };
            commands.push(*sample.payload());
        }
    }

    fn publish_control_event(&self, event: ControlEvent) -> Result<(), Box<dyn Error>> {
        self.control_publisher.send_copy(event)?;
        Ok(())
    }

    fn publish_status(&self, status: EpisodeStatus) -> Result<(), Box<dyn Error>> {
        self.episode_status_publisher.send_copy(status)?;
        Ok(())
    }

    fn drain_backpressure_events(&self) -> Result<Vec<BackpressureEvent>, Box<dyn Error>> {
        let mut events = Vec::new();
        loop {
            let Some(sample) = self.backpressure_subscriber.receive()? else {
                return Ok(events);
            };
            events.push(*sample.payload());
        }
    }

    fn drain_episode_stored(&self) -> Result<Vec<EpisodeStored>, Box<dyn Error>> {
        let mut events = Vec::new();
        loop {
            let Some(sample) = self.episode_stored_subscriber.receive()? else {
                return Ok(events);
            };
            events.push(*sample.payload());
        }
    }

    fn send_shutdown(&self) -> Result<(), Box<dyn Error>> {
        self.publish_control_event(ControlEvent::Shutdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollio_types::config::{Config, DeviceType};
    use rollio_types::messages::{FixedString256, FixedString64};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn signal_trigger_is_successful_shutdown() {
        result_for_shutdown_trigger(&ShutdownTrigger::Signal)
            .expect("signal-driven shutdown should succeed");
    }

    #[cfg(unix)]
    #[test]
    fn child_exit_trigger_returns_error() {
        use std::os::unix::process::ExitStatusExt;

        let trigger = ShutdownTrigger::ChildExited {
            id: "ui".into(),
            status: std::process::ExitStatus::from_raw(0),
        };

        let error = result_for_shutdown_trigger(&trigger)
            .expect_err("child exit should surface as an error");
        assert!(
            error
                .to_string()
                .contains("child \"ui\" exited with status"),
            "error should include child context"
        );
    }

    #[test]
    fn build_teleop_spec_expands_identity_mapping_and_names() {
        let pair = rollio_types::config::PairConfig {
            leader: "leader_arm".into(),
            follower: "follower_arm".into(),
            mapping: MappingStrategy::DirectJoint,
            joint_index_map: Vec::new(),
            joint_scales: Vec::new(),
        };
        let leader = DeviceConfig {
            name: "leader_arm".into(),
            device_type: DeviceType::Robot,
            driver: "pseudo".into(),
            id: "leader".into(),
            width: None,
            height: None,
            fps: None,
            pixel_format: None,
            stream: None,
            channel: None,
            dof: Some(6),
            mode: Some(rollio_types::config::RobotMode::FreeDrive),
            control_frequency_hz: Some(60.0),
            transport: Some("simulated".into()),
            interface: None,
            product_variant: None,
            end_effector: None,
            model_path: None,
            gravity_comp_torque_scales: None,
            mit_kp: None,
            mit_kd: None,
            command_latency_ms: Some(10),
            state_noise_stddev: Some(0.0),
            extra: toml::Table::new(),
        };
        let follower = DeviceConfig {
            name: "follower_arm".into(),
            device_type: DeviceType::Robot,
            driver: "pseudo".into(),
            id: "follower".into(),
            width: None,
            height: None,
            fps: None,
            pixel_format: None,
            stream: None,
            channel: None,
            dof: Some(6),
            mode: Some(rollio_types::config::RobotMode::CommandFollowing),
            control_frequency_hz: Some(60.0),
            transport: Some("simulated".into()),
            interface: None,
            product_variant: None,
            end_effector: None,
            model_path: None,
            gravity_comp_torque_scales: None,
            mit_kp: None,
            mit_kd: None,
            command_latency_ms: Some(10),
            state_noise_stddev: Some(0.0),
            extra: toml::Table::new(),
        };

        let spec = build_teleop_spec(&pair, &leader, &follower, Path::new("."), Path::new("."))
            .expect("teleop spec should build");

        assert_eq!(spec.id, "teleop-leader_arm-to-follower_arm");
        assert_eq!(spec.command.program, OsString::from("rollio-teleop-router"));
        assert_eq!(spec.command.args[0], OsString::from("run"));
        let inline = spec.command.args[2].to_string_lossy();
        assert!(inline.contains("joint_index_map = [0, 1, 2, 3, 4, 5]"));
        assert!(inline.contains("leader_state_topic = \"robot/leader_arm/state\""));
        assert!(inline.contains("follower_state_topic = \"robot/follower_arm/state\""));
        assert!(inline.contains("follower_command_topic = \"robot/follower_arm/command\""));
    }

    #[test]
    fn build_collect_specs_adds_encoder_assembler_and_storage_children() {
        let mut config = include_str!("../../config/config.example.toml")
            .parse::<Config>()
            .expect("example config should parse");
        let workspace_root = temp_workspace_root();
        let staging_root = workspace_root.join("staging");
        config.assembler.staging_dir = staging_root.to_string_lossy().into_owned();
        create_fake_web_bundle(&workspace_root);

        let specs = build_collect_specs(&config, &workspace_root, Path::new("."))
            .expect("specs should build");

        let ids = specs
            .iter()
            .map(|spec| spec.id.as_str())
            .collect::<Vec<_>>();
        assert!(ids.contains(&"encoder-camera_top"));
        assert!(ids.contains(&"encoder-camera_side"));
        assert!(ids.contains(&"assembler"));
        assert!(ids.contains(&"storage"));

        let encoder_spec = specs
            .iter()
            .find(|spec| spec.id == "encoder-camera_top")
            .expect("encoder spec should exist");
        let inline = encoder_spec.command.args[2].to_string_lossy();
        assert!(inline.contains("process_id = \"encoder.camera_top\""));
        assert!(inline.contains("frame_topic = \"camera/camera_top/frames\""));
        assert!(
            inline.contains(&format!(
                "output_dir = \"{}\"",
                workspace_root
                    .join("staging/encoders/camera_top")
                    .to_string_lossy()
            )),
            "unexpected encoder inline config: {inline}"
        );

        let assembler_spec = specs
            .iter()
            .find(|spec| spec.id == "assembler")
            .expect("assembler spec should exist");
        let assembler_inline = assembler_spec.command.args[2].to_string_lossy();
        assert!(assembler_inline.contains("encoded_handoff = \"file\""));
        assert!(assembler_inline.contains("process_id = \"episode-assembler\""));

        let storage_spec = specs
            .iter()
            .find(|spec| spec.id == "storage")
            .expect("storage spec should exist");
        let storage_inline = storage_spec.command.args[2].to_string_lossy();
        assert!(storage_inline.contains("process_id = \"storage\""));

        let ui_spec = specs
            .iter()
            .find(|spec| spec.id == "ui")
            .expect("ui spec should exist");
        assert_eq!(ui_spec.command.program, OsString::from("rollio-ui-server"));
        assert_eq!(ui_spec.command.args[0], OsString::from("--config-inline"));
        assert_eq!(ui_spec.command.args[2], OsString::from("--asset-dir"));
        assert_eq!(
            ui_spec.command.args[3],
            workspace_root.join("ui/web/dist").into_os_string()
        );

        let _ = fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn build_preview_specs_skips_teleop_router_for_intervention_mode() {
        let mut config = include_str!("../../config/config.example.toml")
            .parse::<Config>()
            .expect("example config should parse");
        config.mode = rollio_types::config::CollectionMode::Intervention;
        config.pairing.clear();

        let specs = build_preview_specs(&config, Path::new("."), Path::new("."))
            .expect("specs should build");

        assert!(
            specs.iter().all(|spec| !spec.id.starts_with("teleop-")),
            "intervention previews should not spawn teleop router children"
        );
    }

    #[test]
    fn collect_control_events_blocks_start_until_storage_completes() {
        let now = Instant::now();
        let mut lifecycle = EpisodeLifecycle::default();
        let mut start_blocked = false;
        let (start_events, changed) = collect_control_events(
            &mut lifecycle,
            vec![
                EpisodeCommand::Start,
                EpisodeCommand::Stop,
                EpisodeCommand::Keep,
            ],
            &mut start_blocked,
            now,
        );
        assert!(changed);
        assert_eq!(start_events.len(), 3);
        assert!(matches!(
            start_events[0],
            ControlEvent::RecordingStart { episode_index: 0 }
        ));
        assert!(matches!(
            start_events[2],
            ControlEvent::EpisodeKeep { episode_index: 0 }
        ));

        start_blocked = true;
        let (blocked_events, changed) = collect_control_events(
            &mut lifecycle,
            vec![EpisodeCommand::Start],
            &mut start_blocked,
            now,
        );
        assert!(blocked_events.is_empty());
        assert!(!changed);
        assert_eq!(lifecycle.state(), EpisodeState::Idle);

        start_blocked = true;
        let stored_changed = apply_episode_stored_events(
            &mut lifecycle,
            &mut start_blocked,
            &[EpisodeStored {
                episode_index: 0,
                output_path: FixedString256::new("./output"),
            }],
        );
        assert!(stored_changed);
        assert!(!start_blocked);
        assert_eq!(lifecycle.status(now).episode_count, 1);
    }

    #[test]
    fn apply_backpressure_events_blocks_future_starts() {
        let mut start_blocked = false;
        apply_backpressure_events(
            &mut start_blocked,
            &[BackpressureEvent {
                process_id: FixedString64::new("encoder.camera_top"),
                queue_name: FixedString64::new("frame_queue"),
            }],
        );
        assert!(start_blocked);
    }

    fn temp_workspace_root() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rollio-controller-tests-{suffix}"));
        fs::create_dir_all(&root).expect("temp workspace root should exist");
        root
    }

    fn create_fake_web_bundle(workspace_root: &Path) {
        let ui_dir = workspace_root.join("ui/web/dist");
        fs::create_dir_all(&ui_dir).expect("ui dist dir should exist");
        fs::write(
            ui_dir.join("index.html"),
            "<!doctype html>\n<title>Rollio UI</title>\n",
        )
        .expect("fake ui bundle should be written");
    }
}
