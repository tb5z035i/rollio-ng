use crate::cli::CollectArgs;
use crate::process::{
    monitor_children, spawn_child, terminate_children, ChildSpec, ManagedChild, ResolvedCommand,
    ShutdownTrigger,
};
use iceoryx2::prelude::*;
use rollio_bus::CONTROL_EVENTS_SERVICE;
use rollio_types::config::{Config, DeviceConfig, DeviceType};
use rollio_types::messages::ControlEvent;
use signal_hook::consts::signal::{SIGINT, SIGTERM};
use std::error::Error;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

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

    let shutdown_publisher = ShutdownPublisher::new()?;
    let specs = build_collect_specs(&config, &workspace_root, &current_exe_dir)?;

    let mut children: Vec<ManagedChild> = Vec::new();
    for spec in &specs {
        match spawn_child(spec, &log_dir) {
            Ok(child) => children.push(child),
            Err(error) => {
                let _ = shutdown_publisher.send_shutdown();
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

    let trigger = monitor_children(&mut children, shutdown_requested.as_ref(), poll_interval)?;
    match &trigger {
        ShutdownTrigger::Signal => eprintln!("rollio: shutdown requested by signal"),
        ShutdownTrigger::ChildExited { id, status } => {
            eprintln!("rollio: child \"{id}\" exited with status {status}")
        }
    }

    shutdown_publisher.send_shutdown()?;
    terminate_children(&mut children, shutdown_timeout, poll_interval)?;

    for child in &children {
        if let Some(log_path) = &child.log_path {
            eprintln!("rollio: log captured in {}", log_path.display());
        }
    }

    result_for_shutdown_trigger(&trigger)
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
            let python_package_root = workspace_root.join("robots").join(&driver_dir);
            let python_package_src = python_package_root.join("src");

            if let Some(local_binary) = local_binary {
                (
                    workspace_root.to_path_buf(),
                    ResolvedCommand {
                        program: local_binary.into_os_string(),
                        args: common_args,
                    },
                )
            } else if python_package_root.join("pyproject.toml").exists()
                && python_package_src.exists()
            {
                (
                    python_package_src,
                    ResolvedCommand {
                        program: OsString::from("python3"),
                        args: vec![
                            OsString::from("-m"),
                            OsString::from(format!("rollio_{driver_dir}")),
                            OsString::from("run"),
                            OsString::from("--config-inline"),
                            OsString::from(toml::to_string(device)?),
                        ],
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

struct ShutdownPublisher {
    _node: Node<ipc::Service>,
    publisher: iceoryx2::port::publisher::Publisher<ipc::Service, ControlEvent, ()>,
}

impl ShutdownPublisher {
    fn new() -> Result<Self, Box<dyn Error>> {
        let node = NodeBuilder::new()
            .signal_handling_mode(SignalHandlingMode::Disabled)
            .create::<ipc::Service>()?;
        let service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
        let service = node
            .service_builder(&service_name)
            .publish_subscribe::<ControlEvent>()
            .open_or_create()?;
        let publisher = service.publisher_builder().create()?;
        Ok(Self {
            _node: node,
            publisher,
        })
    }

    fn send_shutdown(&self) -> Result<(), Box<dyn Error>> {
        self.publisher.send_copy(ControlEvent::Shutdown)?;
        Ok(())
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
}
