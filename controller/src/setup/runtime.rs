use super::discovery::{
    available_devices_from_discoveries, available_devices_from_project, build_discovery_config,
    discover_devices, missing_value_limit_warnings, validate_existing_project,
};
use super::overview::{
    should_run_preview_runtime, start_preview_runtime, stop_setup_runtime, sync_identify_mode,
};
use super::save::save_project_config;
use super::state::{SessionMutation, SetupExitKind, SetupSession, SetupStep};
use crate::cli::SetupArgs;
use crate::discovery::DiscoveryOptions;
use crate::process::{
    poll_children_once, spawn_child, terminate_children, ChildSpec, ManagedChild,
};
use crate::runtime_paths::{
    current_executable_dir, resolve_share_root, resolve_state_dir, workspace_root,
};
use crate::runtime_plan::build_control_server_spec;
use iceoryx2::prelude::*;
use rollio_bus::{
    channel_mode_control_service_name, CONTROL_EVENTS_SERVICE, SETUP_COMMAND_SERVICE,
    SETUP_STATE_SERVICE,
};
use rollio_types::config::ProjectConfig;
use rollio_types::messages::{
    ControlEvent, DeviceChannelMode, SetupCommandMessage, SetupStateMessage,
};
use signal_hook::consts::signal::{SIGINT, SIGTERM};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub(super) const SETUP_POLL_INTERVAL: Duration = Duration::from_millis(50);
pub(super) const SETUP_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const SETUP_STATE_MAX_AGE: Duration = Duration::from_millis(500);
pub(super) const SETUP_UI_SUCCESS_DELAY: Duration = Duration::from_millis(300);
pub(super) const SETUP_DEV_RUNTIME_PACKAGES: &[&str] = &[
    "rollio-web-gateway",
    "rollio-visualizer",
    "rollio-control-server",
    "rollio-device-v4l2",
    "rollio-device-airbot-play",
    "rollio-device-pseudo",
];

#[derive(Debug)]
pub(super) struct SetupRuntimeState {
    pub(super) children: Vec<ManagedChild>,
    pub(super) temp_config_path: Option<PathBuf>,
    pub(super) preview_target_name: Option<String>,
}

/// Verifies that a usable `node` is on `PATH`. The setup wizard's terminal UI
/// is an Ink/React app spawned as `node /usr/share/rollio/ui/terminal/dist/index.js`,
/// and Ink requires a recent Node.js. Ubuntu's apt `nodejs` package lags
/// what Ink supports, so the rollio .deb intentionally does not pin
/// `nodejs` as a Depends. Instead we detect Node here at runtime and
/// point the operator at the upstream installer if it is missing or
/// broken. Returns `Ok(())` if `node --version` ran cleanly; otherwise
/// returns a single error message that mentions
/// <https://nodejs.org/en/download>.
pub(super) fn ensure_node_available() -> Result<(), Box<dyn Error>> {
    const NODE_INSTALL_HINT: &str =
        "Install a current Node.js build from https://nodejs.org/en/download \
         (the Ubuntu apt `nodejs` package is typically too old for the \
         rollio terminal UI).";

    match Command::new("node")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(format!(
            "rollio setup needs Node.js to run the terminal UI, but `node --version` \
             exited with {} (stderr: {}). {NODE_INSTALL_HINT}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim(),
        )
        .into()),
        Err(err) => Err(format!(
            "rollio setup needs Node.js to run the terminal UI, but `node` \
             could not be launched from PATH ({err}). {NODE_INSTALL_HINT}"
        )
        .into()),
    }
}

pub(super) fn dev_build_profile(
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Option<&'static str> {
    let target_root = workspace_root.join("target");
    if current_exe_dir == target_root.join("release") {
        Some("release")
    } else if current_exe_dir == target_root.join("debug") {
        Some("debug")
    } else {
        None
    }
}

pub(super) fn ensure_setup_dev_runtime_binaries_built(
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    let Some(profile) = dev_build_profile(workspace_root, current_exe_dir) else {
        return Ok(());
    };
    eprintln!(
        "rollio: ensuring setup UI/device binaries are built ({profile} profile; first run may take a while)..."
    );
    let mut command = Command::new("cargo");
    command.arg("build");
    if profile == "release" {
        command.arg("--release");
    }
    for package in SETUP_DEV_RUNTIME_PACKAGES {
        command.arg("-p").arg(package);
    }
    let status = command
        .current_dir(workspace_root)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    if !status.success() {
        return Err(format!(
            "failed to build setup runtime binaries for {profile} (cargo build exited with {status})"
        )
        .into());
    }
    Ok(())
}

pub fn run(args: SetupArgs) -> Result<(), Box<dyn Error>> {
    // The interactive wizard spawns the Ink terminal UI via `node`. Fail
    // fast with a pointer to https://nodejs.org/en/download so the operator
    // doesn't sit through device discovery + IPC bring-up only to die at
    // child-spawn time. The rollio .deb deliberately does not pin `nodejs`
    // as a Debian Depends because Ubuntu's package is too old for Ink, so
    // this is the first place we surface a missing/broken Node install.
    // `--accept-defaults` skips the UI entirely, so the check is only
    // mandatory in the interactive path.
    if !args.accept_defaults {
        ensure_node_available()?;
    }

    let workspace_root = workspace_root()?;
    let share_root = resolve_share_root()?;
    let state_dir = resolve_state_dir()?;
    let current_exe_dir = current_executable_dir()?;
    ensure_setup_dev_runtime_binaries_built(&workspace_root, &current_exe_dir)?;
    let output_path = args.output_path();
    let discovery_options = DiscoveryOptions {
        simulated_pseudo: args.sim_pseudo,
    };

    let (config, available_devices, mut warnings, resume_mode) =
        if let Some(mut existing_config) = args.load_project_config()? {
            // Demote validation errors on the loaded config to warnings so
            // the operator can open the wizard precisely to fix them. The
            // wizard's `setup_save` path still validates, so an invalid
            // state can't be silently persisted -- but we no longer abort
            // the launch, which previously left operators stuck at the
            // CLI with no way to repair the config short of editing TOML
            // by hand.
            let mut warnings = Vec::new();
            if let Err(error) = existing_config.validate() {
                warnings.push(format!("loaded config has validation issues: {error}"));
            }
            if let Err(error) = validate_existing_project(
                &existing_config,
                &workspace_root,
                state_dir.as_path(),
                &current_exe_dir,
            ) {
                warnings.push(format!("loaded project has runtime issues: {error}"));
            }
            // Persisted configs no longer carry value_limits: re-query each
            // device executable to refresh them in-memory before the wizard
            // (or the visualizer, on accept-defaults) consumes the config.
            // The returned meta map also carries `supported_states`, which
            // the wizard's "States" sub-step needs to render toggle lists.
            let runtime_meta = crate::device_query::refresh_value_limits_from_devices(
                &mut existing_config,
                &workspace_root,
                state_dir.as_path(),
                &current_exe_dir,
            )?;
            let available_devices = available_devices_from_project(&existing_config, &runtime_meta);
            (existing_config, available_devices, warnings, true)
        } else {
            eprintln!("rollio: discovering devices...");
            let (discoveries, warnings) = discover_devices(
                &workspace_root,
                state_dir.as_path(),
                &current_exe_dir,
                discovery_options,
            )?;
            if discoveries.is_empty() {
                return Err("setup did not discover any devices".into());
            }
            let config = build_discovery_config(&discoveries)?;
            let available_devices = available_devices_from_discoveries(&discoveries, &config)?;
            (config, available_devices, warnings, false)
        };

    // Surface any robot channel that publishes a state-kind without
    // driver-supplied value_limits. The visualization layer treats limits as
    // a hard requirement (no UI fallback); the warning prompts the operator
    // to upgrade the device executable instead of silently rendering empty
    // bars.
    warnings.extend(missing_value_limit_warnings(&config));

    if args.accept_defaults {
        eprintln!("rollio: setup accepted defaults without launching the interactive wizard");
        save_project_config(&config, &output_path)?;
        println!("wrote setup config to {}", output_path.display());
        return Ok(());
    }

    run_interactive_setup(
        config,
        available_devices,
        output_path,
        resume_mode,
        warnings,
        &workspace_root,
        share_root.as_path(),
        state_dir.as_path(),
        &current_exe_dir,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_interactive_setup(
    config: ProjectConfig,
    available_devices: Vec<super::state::AvailableDevice>,
    output_path: PathBuf,
    resume_mode: bool,
    warnings: Vec<String>,
    workspace_root: &Path,
    share_root: &Path,
    child_working_dir: &Path,
    current_exe_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    // Reserve two distinct loopback ports up front:
    // - control_port: long-lived `rollio-control-server` for setup_command/setup_state
    // - preview_port: visualizer that comes and goes with `should_run_preview_runtime`
    // The UI talks to both directly. Killing the visualizer no longer kills the
    // control plane, so identify swaps don't freeze the wizard.
    let control_port = reserve_loopback_port()?;
    let preview_port = reserve_loopback_port()?;
    let control_websocket_url = format!("ws://127.0.0.1:{control_port}");
    let preview_websocket_url = format!("ws://127.0.0.1:{preview_port}");

    let ipc = SetupIpc::new()?;
    let mut session = SetupSession::new(
        config,
        available_devices,
        output_path,
        resume_mode,
        warnings,
    );
    let log_dir = child_working_dir.join("rollio-setup-logs");
    fs::create_dir_all(&log_dir)?;

    let shutdown_requested = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(SIGINT, Arc::clone(&shutdown_requested))?;
    signal_hook::flag::register(SIGTERM, Arc::clone(&shutdown_requested))?;

    let mut control_children = Vec::new();
    let mut ui_children = Vec::new();
    let mut preview_runtime: Option<SetupRuntimeState> = None;
    let mut active_identify_target: Option<String> = None;
    let run_result = (|| -> Result<(), Box<dyn Error>> {
        let control_spec = build_control_server_spec(
            crate::runtime_plan::ControlServerRole::Setup,
            control_port,
            workspace_root,
            child_working_dir,
            current_exe_dir,
        )?;
        control_children = spawn_setup_children(std::slice::from_ref(&control_spec), &log_dir)?;

        let ui_spec = build_setup_ui_spec(
            share_root,
            child_working_dir,
            &control_websocket_url,
            &preview_websocket_url,
        )?;
        ui_children = spawn_setup_children(std::slice::from_ref(&ui_spec), &log_dir)?;

        let mut last_state_publish: Option<Instant> = None;
        let mut state_dirty = true;

        loop {
            let shutdown_active = shutdown_requested.load(std::sync::atomic::Ordering::Relaxed);
            if shutdown_active {
                break;
            }

            if let Some(trigger) = poll_children_once(&mut ui_children)? {
                if should_treat_trigger_as_shutdown(
                    &trigger,
                    shutdown_active,
                    session.exit_kind.is_some(),
                ) {
                    break;
                }
                return Err(setup_trigger_error(trigger).into());
            }

            if let Some(trigger) = poll_children_once(&mut control_children)? {
                if should_treat_trigger_as_shutdown(
                    &trigger,
                    shutdown_requested.load(std::sync::atomic::Ordering::Relaxed),
                    session.exit_kind.is_some(),
                ) {
                    break;
                }
                return Err(setup_trigger_error(trigger).into());
            }

            if let Some(runtime) = preview_runtime.as_mut() {
                if let Some(trigger) = poll_children_once(&mut runtime.children)? {
                    if should_treat_trigger_as_shutdown(
                        &trigger,
                        shutdown_requested.load(std::sync::atomic::Ordering::Relaxed),
                        session.exit_kind.is_some(),
                    ) {
                        break;
                    }
                    // A preview-runtime child died non-zero. The most
                    // common cause is the v4l2 camera being held by
                    // another process (e.g., a `rollio collect`
                    // session in another terminal → EBUSY when the
                    // setup-time device tries to open the same
                    // device). Tearing down the entire wizard for a
                    // recoverable error is hostile UX. Stop the
                    // preview runtime, clear identify so the operator
                    // can fix the conflict and try again, and surface
                    // a clear message in the wizard footer.
                    let summary = setup_trigger_error(trigger);
                    let _ = stop_setup_runtime(&mut preview_runtime, &ipc);
                    session.clear_identify_state();
                    session.message = Some(format!(
                        "Identify failed: {summary}. Another process may be holding the device \
                         (try `lsof /dev/video0` or stop any running `rollio collect`)."
                    ));
                    state_dirty = true;
                    continue;
                }
            }

            let mut mutations = SessionMutation::default();
            for raw_json in ipc.drain_setup_commands()? {
                mutations.merge(session.apply_raw_command(&raw_json)?);
            }

            if session
                .identify_device_name
                .as_deref()
                .is_some_and(|name| !session.is_device_selected(name))
            {
                mutations.state_changed |= session.clear_identify_state();
            }

            let should_preview = should_run_preview_runtime(&session);
            let desired_preview_target =
                if should_preview && session.current_step == SetupStep::Devices {
                    session.identify_device_name.clone()
                } else {
                    None
                };
            let mut preview_runtime_restarted = false;

            if preview_runtime.as_ref().is_some_and(|runtime| {
                !should_preview
                    || mutations.config_changed
                    || runtime.preview_target_name != desired_preview_target
            }) {
                stop_setup_runtime(&mut preview_runtime, &ipc)?;
                mutations.state_changed = true;
            }

            if should_preview && preview_runtime.is_none() {
                preview_runtime = Some(start_preview_runtime(
                    &mut session,
                    preview_port,
                    &preview_websocket_url,
                    workspace_root,
                    child_working_dir,
                    current_exe_dir,
                    &log_dir,
                )?);
                preview_runtime_restarted = true;
                mutations.state_changed = true;
            }

            sync_identify_mode(
                &session,
                &ipc,
                &mut active_identify_target,
                preview_runtime_restarted,
            )?;

            state_dirty |= mutations.state_changed;

            let should_publish = state_dirty
                || match last_state_publish {
                    Some(instant) => instant.elapsed() >= SETUP_STATE_MAX_AGE,
                    None => true,
                };
            if should_publish {
                ipc.publish_state_json(&session.build_state_json()?)?;
                last_state_publish = Some(Instant::now());
                state_dirty = false;
            }

            if session.should_exit() {
                break;
            }

            thread::sleep(SETUP_POLL_INTERVAL);
        }

        Ok(())
    })();

    let cleanup_result = stop_setup_runtime(&mut preview_runtime, &ipc)
        .and_then(|_| {
            // Neither the control-server nor the UI subscribes to
            // ControlEvent::Shutdown (the bus signal is a per-swap signal for
            // preview-runtime children). Use a tiny grace window so SIGTERM
            // fires almost immediately at session end. Without this the
            // wizard appeared to hang for ~30 s after pressing `q` (debug
            // session 8d351b confirmed the gap).
            let quick_grace = Duration::from_millis(200);
            terminate_children(&mut control_children, quick_grace, SETUP_POLL_INTERVAL)
                .map_err(|error| -> Box<dyn Error> { Box::new(error) })
        })
        .and_then(|_| {
            let quick_grace = Duration::from_millis(200);
            terminate_children(&mut ui_children, quick_grace, SETUP_POLL_INTERVAL)
                .map_err(|error| -> Box<dyn Error> { Box::new(error) })
        });

    if let Err(error) = run_result {
        if let Err(cleanup_error) = cleanup_result {
            eprintln!("rollio: cleanup after setup error failed: {cleanup_error}");
        }
        return Err(error);
    }

    cleanup_result?;

    match session.exit_kind {
        Some(SetupExitKind::Saved) => {
            println!("wrote setup config to {}", session.output_path.display());
            Ok(())
        }
        Some(SetupExitKind::Cancelled) => Ok(()),
        None => Ok(()),
    }
}

pub(super) fn should_treat_trigger_as_shutdown(
    trigger: &crate::ShutdownTrigger,
    shutdown_requested: bool,
    session_exiting: bool,
) -> bool {
    shutdown_requested
        || session_exiting
        || matches!(trigger, crate::ShutdownTrigger::Signal)
        || matches!(
            trigger,
            crate::ShutdownTrigger::ChildExited { status, .. } if is_interrupt_exit_status(status)
        )
}

pub(super) fn setup_trigger_error(trigger: crate::ShutdownTrigger) -> String {
    match trigger {
        crate::ShutdownTrigger::Signal => "setup interrupted by signal".into(),
        crate::ShutdownTrigger::ChildExited { id, status } => {
            format!("child \"{id}\" exited with status {status}")
        }
    }
}

pub(super) fn is_interrupt_exit_status(status: &ExitStatus) -> bool {
    if matches!(status.code(), Some(130 | 143)) {
        return true;
    }

    #[cfg(unix)]
    if matches!(status.signal(), Some(SIGINT | SIGTERM)) {
        return true;
    }

    false
}

pub(super) fn spawn_setup_children(
    specs: &[ChildSpec],
    log_dir: &Path,
) -> Result<Vec<ManagedChild>, Box<dyn Error>> {
    let mut children = Vec::new();
    for spec in specs {
        match spawn_child(spec, log_dir) {
            Ok(child) => children.push(child),
            Err(error) => {
                let _ =
                    terminate_children(&mut children, SETUP_SHUTDOWN_TIMEOUT, SETUP_POLL_INTERVAL);
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

pub(super) fn build_setup_ui_spec(
    share_root: &Path,
    child_working_dir: &Path,
    control_websocket_url: &str,
    preview_websocket_url: &str,
) -> Result<ChildSpec, Box<dyn Error>> {
    let ui_entry = share_root.join("ui/terminal/dist/index.js");
    if !ui_entry.exists() {
        return Err(format!(
            "Terminal UI bundle not found at {}. Run `cd ui/terminal && npm run build` first, \
             or set ROLLIO_SHARE_DIR.",
            ui_entry.display()
        )
        .into());
    }

    Ok(ChildSpec {
        id: "setup-ui".into(),
        command: crate::ResolvedCommand {
            program: OsString::from("node"),
            args: vec![
                ui_entry.into_os_string(),
                OsString::from("--mode"),
                OsString::from("setup"),
                OsString::from("--control-ws"),
                OsString::from(control_websocket_url),
                OsString::from("--preview-ws"),
                OsString::from(preview_websocket_url),
            ],
        },
        working_directory: child_working_dir.to_path_buf(),
        env: Vec::new(),
        inherit_stdio: true,
    })
}

pub(super) fn reserve_loopback_port() -> Result<u16, Box<dyn Error>> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

pub(super) struct SetupIpc {
    _node: Node<ipc::Service>,
    setup_command_subscriber:
        iceoryx2::port::subscriber::Subscriber<ipc::Service, SetupCommandMessage, ()>,
    setup_state_publisher:
        iceoryx2::port::publisher::Publisher<ipc::Service, SetupStateMessage, ()>,
    control_publisher: iceoryx2::port::publisher::Publisher<ipc::Service, ControlEvent, ()>,
    channel_mode_publishers: RefCell<
        BTreeMap<String, iceoryx2::port::publisher::Publisher<ipc::Service, DeviceChannelMode, ()>>,
    >,
}

impl SetupIpc {
    pub(super) fn new() -> Result<Self, Box<dyn Error>> {
        let node = NodeBuilder::new()
            .signal_handling_mode(SignalHandlingMode::Disabled)
            .create::<ipc::Service>()?;

        let command_service_name: ServiceName = SETUP_COMMAND_SERVICE.try_into()?;
        let command_service = node
            .service_builder(&command_service_name)
            .publish_subscribe::<SetupCommandMessage>()
            .max_publishers(8)
            .max_subscribers(8)
            .max_nodes(16)
            .open_or_create()?;

        let state_service_name: ServiceName = SETUP_STATE_SERVICE.try_into()?;
        let state_service = node
            .service_builder(&state_service_name)
            .publish_subscribe::<SetupStateMessage>()
            .max_publishers(8)
            .max_subscribers(8)
            .max_nodes(16)
            .open_or_create()?;

        // Match `controller::collect::ControllerIpc::new` — see the long
        // comment there. The setup preview runtime spawns the same set of
        // device + encoder + teleop processes as collect, so the same node
        // budget applies. Keeping the two call sites in sync also avoids a
        // mismatch where collect would create the service with quota 32
        // and a later setup re-run would try to open with 16, failing
        // `verify_max_nodes`.
        let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
        let control_service = node
            .service_builder(&control_service_name)
            .publish_subscribe::<ControlEvent>()
            .max_publishers(4)
            .max_subscribers(32)
            .max_nodes(32)
            .open_or_create()?;

        Ok(Self {
            _node: node,
            setup_command_subscriber: command_service.subscriber_builder().create()?,
            setup_state_publisher: state_service.publisher_builder().create()?,
            control_publisher: control_service.publisher_builder().create()?,
            channel_mode_publishers: RefCell::new(BTreeMap::new()),
        })
    }

    pub(super) fn drain_setup_commands(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let mut commands = Vec::new();
        loop {
            let Some(sample) = self.setup_command_subscriber.receive()? else {
                return Ok(commands);
            };
            commands.push(sample.payload().as_str().to_owned());
        }
    }

    pub(super) fn publish_state_json(&self, json: &str) -> Result<(), Box<dyn Error>> {
        if json.len() > SetupStateMessage::MAX_LEN {
            return Err(format!(
                "setup state payload too large: {} bytes exceeds {}",
                json.len(),
                SetupStateMessage::MAX_LEN
            )
            .into());
        }
        self.setup_state_publisher
            .send_copy(SetupStateMessage::new(json))?;
        Ok(())
    }

    pub(super) fn send_shutdown(&self) -> Result<(), Box<dyn Error>> {
        self.control_publisher.send_copy(ControlEvent::Shutdown)?;
        Ok(())
    }

    pub(super) fn publish_channel_mode(
        &self,
        bus_root: &str,
        channel_type: &str,
        mode: DeviceChannelMode,
    ) -> Result<(), Box<dyn Error>> {
        let key = channel_mode_control_service_name(bus_root, channel_type);
        if !self.channel_mode_publishers.borrow().contains_key(&key) {
            let service_name: ServiceName = key.as_str().try_into()?;
            let service = self
                ._node
                .service_builder(&service_name)
                .publish_subscribe::<DeviceChannelMode>()
                .max_publishers(16)
                .max_subscribers(16)
                .max_nodes(16)
                .open_or_create()?;
            let publisher = service.publisher_builder().create()?;
            self.channel_mode_publishers
                .borrow_mut()
                .insert(key.clone(), publisher);
        }
        if let Some(publisher) = self.channel_mode_publishers.borrow().get(&key) {
            publisher.send_copy(mode)?;
        }
        Ok(())
    }
}
