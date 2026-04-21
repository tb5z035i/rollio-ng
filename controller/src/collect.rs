use crate::cli::CollectArgs;
use crate::episode::EpisodeLifecycle;
use crate::process::{
    poll_children_once, spawn_child, terminate_children, ChildSpec, ManagedChild, ShutdownTrigger,
};
use crate::runtime_paths::{
    current_executable_dir, resolve_log_dir, resolve_share_root, resolve_state_dir, workspace_root,
};
#[cfg(test)]
pub(crate) use crate::runtime_plan::{build_collect_specs, build_preview_specs, build_teleop_spec};
use iceoryx2::prelude::*;
use rollio_bus::{
    BACKPRESSURE_SERVICE, CONTROL_EVENTS_SERVICE, EPISODE_COMMAND_SERVICE, EPISODE_STATUS_SERVICE,
    EPISODE_STORED_SERVICE,
};
use rollio_types::config::ProjectConfig;
use rollio_types::messages::{
    BackpressureEvent, ControlEvent, EpisodeCommand, EpisodeState, EpisodeStatus, EpisodeStored,
};
use signal_hook::consts::signal::{SIGINT, SIGTERM};
use std::error::Error;
#[cfg(test)]
use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub fn run(args: CollectArgs) -> Result<(), Box<dyn Error>> {
    let mut config = args.load_project_config()?;
    let workspace_root = workspace_root()?;
    let share_root = resolve_share_root()?;
    let state_dir = resolve_state_dir()?;
    let current_exe_dir = current_executable_dir()?;
    // Capture the cwd of the user's `rollio` invocation up front so that
    // relative paths in the config (e.g. storage `output_path = "./output"`)
    // resolve to ${PWD}/output instead of being silently re-rooted under the
    // children's working directory (`state_dir`, typically
    // `~/.local/state/rollio`). We must read this before any chdir-like
    // operation runs.
    let invocation_cwd = std::env::current_dir()?;
    // Persisted configs no longer carry value_limits; refresh them from a
    // fresh `query --json` per device before runtime children are spawned.
    // The visualizer treats absent limits as a hard error, so any failure
    // here aborts the run with a clear driver/path message.
    crate::device_query::refresh_value_limits_from_devices(
        &mut config,
        &workspace_root,
        state_dir.as_path(),
        &current_exe_dir,
    )?;
    run_with_config(
        config,
        workspace_root,
        share_root,
        state_dir,
        current_exe_dir,
        invocation_cwd,
    )
}

fn run_with_config(
    config: ProjectConfig,
    workspace_root: std::path::PathBuf,
    share_root: std::path::PathBuf,
    state_dir: std::path::PathBuf,
    current_exe_dir: std::path::PathBuf,
    invocation_cwd: std::path::PathBuf,
) -> Result<(), Box<dyn Error>> {
    let workspace_root = workspace_root.as_path();
    let share_root = share_root.as_path();
    let child_working_dir = state_dir.as_path();
    let current_exe_dir = current_exe_dir.as_path();
    let invocation_cwd = invocation_cwd.as_path();
    let poll_interval = Duration::from_millis(config.controller.child_poll_interval_ms);
    let shutdown_timeout =
        Duration::from_millis(config.controller.shutdown_timeout_ms).max(Duration::from_secs(30));
    // Logs land alongside the user's invocation cwd by default (overridable
    // via `ROLLIO_LOG_DIR`) so they are easy to find next to the config /
    // recorded dataset, instead of being hidden under `~/.local/state/rollio`.
    let log_dir = resolve_log_dir(invocation_cwd)?;

    let shutdown_requested = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(SIGINT, Arc::clone(&shutdown_requested))?;
    signal_hook::flag::register(SIGTERM, Arc::clone(&shutdown_requested))?;

    let controller_ipc = ControllerIpc::new()?;
    let specs = crate::runtime_plan::build_collect_specs(
        &config,
        workspace_root,
        share_root,
        child_working_dir,
        current_exe_dir,
        invocation_cwd,
    )?;

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

        // CONTROL_EVENTS_SERVICE is the fan-out bus every device, encoder,
        // teleop router, visualizer and assembler subscribes to for the
        // session-end Shutdown event. With multi-channel cameras (e.g. a
        // RealSense exposing color + depth + infrared = 3 encoders by
        // itself, plus 2 V4L2 webcams + 2 robots + ...) the previous cap of
        // 16 nodes is reached on a moderately-sized rig and the next
        // device fails with `ExceedsMaxNumberOfNodes`. The controller
        // always wins the race to create this service (`run_with_config`
        // builds `ControllerIpc` before spawning any child), so bumping the
        // caps here is enough — every subsequent `open_or_create` inherits
        // them.
        let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
        let control_service = node
            .service_builder(&control_service_name)
            .publish_subscribe::<ControlEvent>()
            .max_publishers(4)
            .max_subscribers(32)
            .max_nodes(32)
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
    use rollio_types::config::{
        ChannelCommandDefaults, MappingStrategy, ProjectConfig, RobotCommandKind, RobotStateKind,
        TeleopRuntimeConfigV2,
    };
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
        let teleop = TeleopRuntimeConfigV2 {
            process_id: "leader_arm-to-follower_arm".into(),
            leader_channel_id: "robot/leader_arm/arm".into(),
            follower_channel_id: "robot/follower_arm/arm".into(),
            leader_state_kind: RobotStateKind::JointPosition,
            leader_state_topic: "robot/leader_arm/state".into(),
            follower_command_kind: RobotCommandKind::JointPosition,
            follower_command_topic: "robot/follower_arm/command".into(),
            follower_state_kind: None,
            follower_state_topic: None,
            sync_max_step_rad: None,
            sync_complete_threshold_rad: None,
            mapping: MappingStrategy::DirectJoint,
            joint_index_map: (0..6).collect(),
            joint_scales: vec![1.0; 6],
            command_defaults: ChannelCommandDefaults::default(),
        };

        let spec = build_teleop_spec(&teleop, Path::new("."), Path::new("."), Path::new("."))
            .expect("teleop spec should build");

        assert_eq!(spec.id, "teleop-leader_arm-to-follower_arm");
        assert_eq!(spec.command.program, OsString::from("rollio-teleop-router"));
        assert_eq!(spec.command.args[0], OsString::from("run"));
        let inline = spec.command.args[2].to_string_lossy();
        assert!(inline.contains("joint_index_map = [0, 1, 2, 3, 4, 5]"));
        assert!(inline.contains("leader_state_topic = \"robot/leader_arm/state\""));
        assert!(inline.contains("follower_command_topic = \"robot/follower_arm/command\""));
    }

    #[test]
    fn build_collect_specs_adds_encoder_assembler_and_storage_children() {
        let mut config = include_str!("../../config/config.example.toml")
            .parse::<ProjectConfig>()
            .expect("example config should parse");
        let workspace_root = temp_workspace_root();
        let staging_root = workspace_root.join("staging");
        config.assembler.staging_dir = staging_root.to_string_lossy().into_owned();
        create_fake_web_bundle(&workspace_root);

        let specs = build_collect_specs(
            &config,
            &workspace_root,
            &workspace_root,
            Path::new("."),
            Path::new("."),
            Path::new("."),
        )
        .expect("specs should build");

        let ids = specs
            .iter()
            .map(|spec| spec.id.as_str())
            .collect::<Vec<_>>();
        assert!(ids.contains(&"encoder-camera_top-color"));
        assert!(ids.contains(&"encoder-camera_side-color"));
        assert!(ids.contains(&"assembler"));
        assert!(ids.contains(&"storage"));
        assert!(
            ids.contains(&"control-server"),
            "control-server child should be present: {ids:?}"
        );

        let control_spec = specs
            .iter()
            .find(|spec| spec.id == "control-server")
            .expect("control-server spec should exist");
        let control_inline = control_spec.command.args[1].to_string_lossy();
        assert!(
            control_inline.contains("role = \"collect\""),
            "expected collect role: {control_inline}"
        );
        assert!(
            control_inline.contains("port = "),
            "expected control port: {control_inline}"
        );

        let encoder_spec = specs
            .iter()
            .find(|spec| spec.id == "encoder-camera_top-color")
            .expect("encoder spec should exist");
        let inline = encoder_spec.command.args[2].to_string_lossy();
        assert!(inline.contains("process_id = \"encoder.camera_top.color\""));
        assert!(inline.contains("frame_topic = \"camera_top/color/frames\""));
        assert!(
            inline.contains(&format!(
                "output_dir = \"{}\"",
                workspace_root
                    .join("staging/encoders/camera_top__color")
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
        let ui_inline = ui_spec.command.args[1].to_string_lossy();
        assert!(
            ui_inline.contains("control_websocket_url = "),
            "ui spec should include control upstream URL: {ui_inline}"
        );
        assert!(
            ui_inline.contains("preview_websocket_url = "),
            "ui spec should include preview upstream URL: {ui_inline}"
        );

        let _ = fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn build_preview_specs_skips_teleop_router_for_intervention_mode() {
        let mut config = include_str!("../../config/config.example.toml")
            .parse::<ProjectConfig>()
            .expect("example config should parse");
        config.mode = rollio_types::config::CollectionMode::Intervention;
        config.pairings.clear();

        let specs = build_preview_specs(&config, Path::new("."), Path::new("."), Path::new("."))
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
            ControlEvent::RecordingStart {
                episode_index: 0,
                ..
            }
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
