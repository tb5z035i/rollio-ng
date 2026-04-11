use crate::cli::CollectArgs;
use crate::episode::EpisodeLifecycle;
use crate::process::{
    poll_children_once, spawn_child, terminate_children, ChildSpec, ManagedChild, ResolvedCommand,
    ShutdownTrigger,
};
use iceoryx2::prelude::*;
use rollio_bus::{
    robot_command_service_name, robot_state_service_name, CONTROL_EVENTS_SERVICE,
    EPISODE_COMMAND_SERVICE, EPISODE_STATUS_SERVICE,
};
use rollio_types::config::{
    Config, DeviceConfig, DeviceType, MappingStrategy, TeleopRuntimeConfig,
};
use rollio_types::messages::{ControlEvent, EpisodeCommand, EpisodeState, EpisodeStatus};
use signal_hook::consts::signal::{SIGINT, SIGTERM};
use std::error::Error;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
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
    loop {
        if shutdown_requested.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(ShutdownTrigger::Signal);
        }

        if let Some(trigger) = poll_children_once(children)? {
            return Ok(trigger);
        }

        let commands = controller_ipc.drain_episode_commands()?;
        let now = Instant::now();
        let mut status_changed = false;
        for command in commands {
            match lifecycle.handle_command(command, now) {
                Ok(event) => {
                    controller_ipc.publish_control_event(event)?;
                    status_changed = true;
                }
                Err(error) => eprintln!("rollio: {error}"),
            }
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
    let mut specs = Vec::new();

    let visualizer_config = toml::to_string(&config.visualizer_runtime_config())?;
    specs.push(ChildSpec {
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
    });

    for device in &config.devices {
        specs.push(build_device_spec(device, workspace_root, current_exe_dir)?);
    }

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

    let ui_runtime_config = config.ui_runtime_config();
    let ui_entrypoint = workspace_root.join("ui/terminal/dist/index.js");
    if !ui_entrypoint.exists() {
        return Err(format!(
            "UI bundle not found at {}. Run `cd ui/terminal && npm run build` first.",
            ui_entrypoint.display()
        )
        .into());
    }

    let websocket_url = ui_runtime_config
        .websocket_url
        .ok_or("ui runtime config did not produce a websocket url")?;
    specs.push(ChildSpec {
        id: "ui".into(),
        command: ResolvedCommand {
            program: OsString::from("node"),
            args: vec![
                ui_entrypoint.into_os_string(),
                OsString::from("--ws"),
                OsString::from(websocket_url),
                OsString::from("--start-key"),
                OsString::from(ui_runtime_config.start_key),
                OsString::from("--stop-key"),
                OsString::from(ui_runtime_config.stop_key),
                OsString::from("--keep-key"),
                OsString::from(ui_runtime_config.keep_key),
                OsString::from("--discard-key"),
                OsString::from(ui_runtime_config.discard_key),
            ],
        },
        working_directory: workspace_root.to_path_buf(),
        inherit_stdio: true,
    });

    Ok(specs)
}

fn build_device_spec(
    device: &DeviceConfig,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Result<ChildSpec, Box<dyn Error>> {
    let inline_config = toml::to_string(device)?;
    let driver_dir = driver_dir_name(&device.driver);
    let executable_name = device.executable_name();
    let camera_local_binary = resolve_existing_path([
        workspace_root
            .join("cameras/build")
            .join(&driver_dir)
            .join(&executable_name),
        current_exe_dir.join(&executable_name),
        workspace_root.join("target/debug").join(&executable_name),
    ]);
    let common_args = vec![
        OsString::from("run"),
        OsString::from("--config-inline"),
        OsString::from(inline_config),
    ];

    let (working_directory, command) = match device.device_type {
        DeviceType::Camera => (
            workspace_root.to_path_buf(),
            ResolvedCommand {
                program: camera_local_binary
                    .map(PathBuf::into_os_string)
                    .unwrap_or_else(|| OsString::from(&executable_name)),
                args: common_args,
            },
        ),
        DeviceType::Robot => {
            let local_binary = resolve_existing_path([
                current_exe_dir.join(&executable_name),
                workspace_root.join("target/debug").join(&executable_name),
            ]);

            if let Some(local_binary) = local_binary {
                (
                    workspace_root.to_path_buf(),
                    ResolvedCommand {
                        program: local_binary.into_os_string(),
                        args: common_args,
                    },
                )
            } else {
                (
                    workspace_root.to_path_buf(),
                    ResolvedCommand {
                        program: OsString::from(&executable_name),
                        args: common_args,
                    },
                )
            }
        }
    };

    Ok(ChildSpec {
        id: format!("device-{}", device.name),
        command,
        working_directory,
        inherit_stdio: false,
    })
}

fn build_teleop_spec(
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

fn resolve_program(local_candidate: PathBuf, fallback_name: &str) -> OsString {
    if local_candidate.exists() {
        local_candidate.into_os_string()
    } else {
        OsString::from(fallback_name)
    }
}

fn driver_dir_name(driver: &str) -> String {
    driver.replace('-', "_")
}

fn resolve_existing_path(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|candidate| candidate.exists())
}

fn workspace_root() -> Result<PathBuf, Box<dyn Error>> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "failed to resolve workspace root".into())
}

fn current_executable_dir() -> Result<PathBuf, Box<dyn Error>> {
    let current_executable = std::env::current_exe()?;
    current_executable
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "failed to resolve controller executable directory".into())
}

struct ControllerIpc {
    _node: Node<ipc::Service>,
    control_publisher: iceoryx2::port::publisher::Publisher<ipc::Service, ControlEvent, ()>,
    episode_command_subscriber:
        iceoryx2::port::subscriber::Subscriber<ipc::Service, EpisodeCommand, ()>,
    episode_status_publisher: iceoryx2::port::publisher::Publisher<ipc::Service, EpisodeStatus, ()>,
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

        Ok(Self {
            _node: node,
            control_publisher: control_service.publisher_builder().create()?,
            episode_command_subscriber: command_service.subscriber_builder().create()?,
            episode_status_publisher: status_service.publisher_builder().create()?,
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

    fn send_shutdown(&self) -> Result<(), Box<dyn Error>> {
        self.publish_control_event(ControlEvent::Shutdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
