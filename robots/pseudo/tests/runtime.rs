use iceoryx2::prelude::*;
use rollio_bus::{robot_command_service_name, robot_state_service_name, CONTROL_EVENTS_SERVICE};
use rollio_types::config::{DeviceConfig, DeviceType, RobotMode};
use rollio_types::messages::{CommandMode, ControlEvent, RobotCommand, RobotState, MAX_JOINTS};
use serde_json::Value;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type StateSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, RobotState, ()>;
type CommandPublisher = iceoryx2::port::publisher::Publisher<ipc::Service, RobotCommand, ()>;
type ControlPublisher = iceoryx2::port::publisher::Publisher<ipc::Service, ControlEvent, ()>;

struct TestPorts {
    _node: Node<ipc::Service>,
    state_subscriber: StateSubscriber,
    command_publisher: CommandPublisher,
    control_publisher: ControlPublisher,
}

#[test]
fn probe_outputs_requested_devices() {
    let _guard = test_guard();
    let output = Command::new(binary_path())
        .args(["probe", "--count", "2", "--dof", "6"])
        .output()
        .expect("probe command should run");
    assert!(output.status.success());

    let parsed: Value =
        serde_json::from_slice(&output.stdout).expect("probe output should be JSON");
    let devices = parsed
        .as_array()
        .expect("probe output should be a JSON array");
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0]["dof"], 6);
}

#[test]
fn capabilities_report_supported_modes() {
    let _guard = test_guard();
    let output = Command::new(binary_path())
        .args(["capabilities", "pseudo_robot_0", "--dof", "6"])
        .output()
        .expect("capabilities command should run");
    assert!(output.status.success());

    let parsed: Value =
        serde_json::from_slice(&output.stdout).expect("capabilities output should be JSON");
    assert_eq!(parsed["dof"], 6);
    assert_eq!(parsed["supported_modes"][0], "free-drive");
    assert_eq!(parsed["supported_modes"][1], "command-following");
}

#[test]
fn free_drive_run_publishes_monotonic_states() {
    let _guard = test_guard();
    let device = pseudo_robot_device(unique_name("free"), RobotMode::FreeDrive);
    let ports = create_test_ports(&device.name).expect("ports should be created");
    let mut child = spawn_driver(&device);

    let states = collect_states(&ports.state_subscriber, 24, Duration::from_secs(3))
        .expect("free-drive should publish robot states");
    assert_eq!(states[0].num_joints, 6);
    assert!(states
        .windows(2)
        .all(|window| window[0].timestamp_ns < window[1].timestamp_ns));

    let joint0_values: Vec<f64> = states.iter().map(|state| state.positions[0]).collect();
    let min = joint0_values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = joint0_values
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        max - min > 0.4,
        "joint 0 should move in free-drive mode: min={min}, max={max}"
    );

    send_control_event(&ports.control_publisher, ControlEvent::Shutdown);
    wait_for_exit(&mut child, Duration::from_secs(2));
}

#[test]
fn command_following_tracks_step_commands() {
    let _guard = test_guard();
    let device = pseudo_robot_device(unique_name("follow"), RobotMode::CommandFollowing);
    let ports = create_test_ports(&device.name).expect("ports should be created");
    let mut child = spawn_driver(&device);

    let warmup = collect_states(&ports.state_subscriber, 8, Duration::from_secs(2))
        .expect("command-following should publish warmup states");
    assert!(!warmup.is_empty());

    send_joint_command(&ports.command_publisher, 6, 1.0);
    let reached_one = wait_until_joint_close(
        &ports.state_subscriber,
        0,
        1.0,
        0.15,
        Duration::from_secs(2),
    )
    .expect("robot should move towards the first command");
    assert!(reached_one.positions[0] > 0.8);

    send_joint_command(&ports.command_publisher, 6, 0.0);
    let reached_zero = wait_until_joint_close(
        &ports.state_subscriber,
        0,
        0.0,
        0.15,
        Duration::from_secs(2),
    )
    .expect("robot should move towards the second command");
    assert!(reached_zero.positions[0].abs() < 0.15);

    send_control_event(&ports.control_publisher, ControlEvent::Shutdown);
    wait_for_exit(&mut child, Duration::from_secs(2));
}

#[test]
fn mode_switch_enables_command_following() {
    let _guard = test_guard();
    let device = pseudo_robot_device(unique_name("switch"), RobotMode::FreeDrive);
    let ports = create_test_ports(&device.name).expect("ports should be created");
    let mut child = spawn_driver(&device);

    let warmup = collect_states(&ports.state_subscriber, 8, Duration::from_secs(2))
        .expect("free-drive should publish warmup states");
    assert!(!warmup.is_empty());

    send_control_event(
        &ports.control_publisher,
        ControlEvent::ModeSwitch {
            target_mode: RobotMode::CommandFollowing.control_mode_value(),
        },
    );
    send_joint_command(&ports.command_publisher, 6, 1.0);

    let tracked =
        wait_until_joint_close(&ports.state_subscriber, 0, 1.0, 0.2, Duration::from_secs(2))
            .expect("robot should follow commands after mode switch");
    assert!(tracked.positions[0] > 0.75);

    send_control_event(&ports.control_publisher, ControlEvent::Shutdown);
    wait_for_exit(&mut child, Duration::from_secs(2));
}

fn binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_rollio-robot-pseudo")
}

fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

fn unique_name(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{prefix}_{}", nanos)
}

fn pseudo_robot_device(name: String, mode: RobotMode) -> DeviceConfig {
    DeviceConfig {
        name: name.clone(),
        device_type: DeviceType::Robot,
        driver: "pseudo".into(),
        id: format!("{name}_id"),
        width: None,
        height: None,
        fps: None,
        pixel_format: None,
        stream: None,
        channel: None,
        dof: Some(6),
        mode: Some(mode),
        control_frequency_hz: Some(80.0),
        transport: Some("simulated".into()),
        interface: None,
        product_variant: None,
        end_effector: None,
        model_path: None,
        gravity_comp_torque_scales: None,
        mit_kp: None,
        mit_kd: None,
        command_latency_ms: Some(60),
        state_noise_stddev: Some(0.0),
        extra: toml::Table::new(),
    }
}

fn create_test_ports(device_name: &str) -> Result<TestPorts, Box<dyn std::error::Error>> {
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;

    let state_service_name: ServiceName =
        robot_state_service_name(device_name).as_str().try_into()?;
    let state_service = node
        .service_builder(&state_service_name)
        .publish_subscribe::<RobotState>()
        .open_or_create()?;
    let state_subscriber = state_service.subscriber_builder().create()?;

    let command_service_name: ServiceName = robot_command_service_name(device_name)
        .as_str()
        .try_into()?;
    let command_service = node
        .service_builder(&command_service_name)
        .publish_subscribe::<RobotCommand>()
        .open_or_create()?;
    let command_publisher = command_service.publisher_builder().create()?;

    let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
    let control_service = node
        .service_builder(&control_service_name)
        .publish_subscribe::<ControlEvent>()
        .open_or_create()?;
    let control_publisher = control_service.publisher_builder().create()?;

    Ok(TestPorts {
        _node: node,
        state_subscriber,
        command_publisher,
        control_publisher,
    })
}

fn spawn_driver(device: &DeviceConfig) -> Child {
    let config_toml = toml::to_string(device).expect("device config should serialize");
    Command::new(binary_path())
        .arg("run")
        .arg("--config-inline")
        .arg(config_toml)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("pseudo robot should start")
}

fn collect_states(
    subscriber: &StateSubscriber,
    count: usize,
    timeout: Duration,
) -> Result<Vec<RobotState>, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + timeout;
    let mut states = Vec::with_capacity(count);

    while Instant::now() < deadline && states.len() < count {
        if let Some(sample) = subscriber.receive()? {
            states.push(*sample.payload());
        } else {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    if states.len() < count {
        return Err(format!("expected {count} states, got {}", states.len()).into());
    }

    Ok(states)
}

fn wait_until_joint_close(
    subscriber: &StateSubscriber,
    joint_idx: usize,
    target: f64,
    tolerance: f64,
    timeout: Duration,
) -> Result<RobotState, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + timeout;
    let mut last_state = None;

    while Instant::now() < deadline {
        if let Some(sample) = subscriber.receive()? {
            let state = *sample.payload();
            if (state.positions[joint_idx] - target).abs() <= tolerance {
                return Ok(state);
            }
            last_state = Some(state);
        } else {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    Err(format!(
        "joint {joint_idx} did not reach {target} within {timeout:?}, last={:?}",
        last_state.map(|state| state.positions[joint_idx])
    )
    .into())
}

fn send_joint_command(publisher: &CommandPublisher, dof: usize, target: f64) {
    let mut joint_targets = [0.0f64; MAX_JOINTS];
    for joint in joint_targets.iter_mut().take(dof) {
        *joint = target;
    }

    publisher
        .send_copy(RobotCommand {
            timestamp_ns: unix_timestamp_ns(),
            mode: CommandMode::Joint,
            num_joints: dof as u32,
            joint_targets,
            ..RobotCommand::default()
        })
        .expect("command should publish");
}

fn send_control_event(publisher: &ControlPublisher, event: ControlEvent) {
    publisher
        .send_copy(event)
        .expect("control event should publish");
}

fn wait_for_exit(child: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().expect("child wait should succeed") {
            Some(status) => {
                assert!(status.success(), "child exited unsuccessfully: {status}");
                return;
            }
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            None => {
                let _ = child.kill();
                panic!("child did not exit within {timeout:?}");
            }
        }
    }
}

fn unix_timestamp_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}
