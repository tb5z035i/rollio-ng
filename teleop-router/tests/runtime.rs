use iceoryx2::prelude::*;
use rollio_bus::CONTROL_EVENTS_SERVICE;
use rollio_types::config::{MappingStrategy, TeleopRuntimeConfig};
use rollio_types::messages::{CommandMode, ControlEvent, RobotCommand, RobotState};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type StatePublisher = iceoryx2::port::publisher::Publisher<ipc::Service, RobotState, ()>;
type CommandSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, RobotCommand, ()>;
type ControlPublisher = iceoryx2::port::publisher::Publisher<ipc::Service, ControlEvent, ()>;

struct TestPorts {
    _node: Node<ipc::Service>,
    leader_state_publisher: StatePublisher,
    follower_state_publisher: StatePublisher,
    command_subscriber: CommandSubscriber,
    control_publisher: ControlPublisher,
}

#[test]
fn router_forwards_identity_joint_commands() {
    let _guard = test_guard();
    let id = unique_id("identity");
    let config = teleop_config(&id, MappingStrategy::DirectJoint);
    let ports = create_test_ports(&config).expect("ports should be created");
    let mut child = spawn_router(&config);
    thread::sleep(Duration::from_millis(100));

    publish_state(&ports.follower_state_publisher, leader_state(now_ns()))
        .expect("follower publish should work");
    publish_state(&ports.leader_state_publisher, leader_state(now_ns()))
        .expect("leader publish should work");
    let command =
        wait_for_command(&ports.command_subscriber, Duration::from_secs(2)).expect("command");

    assert_eq!(command.mode, CommandMode::Joint);
    assert_eq!(command.num_joints, 6);
    assert_eq!(&command.joint_targets[..6], &[0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);

    send_shutdown(&ports.control_publisher);
    wait_for_exit(&mut child, Duration::from_secs(2));
}

#[test]
fn router_forwards_cartesian_commands() {
    let _guard = test_guard();
    let id = unique_id("cartesian");
    let mut config = teleop_config(&id, MappingStrategy::Cartesian);
    config.joint_index_map.clear();
    config.joint_scales.clear();
    let ports = create_test_ports(&config).expect("ports should be created");
    let mut child = spawn_router(&config);
    thread::sleep(Duration::from_millis(100));

    let mut state = leader_state(now_ns());
    state.has_ee_pose = true;
    state.ee_pose = [0.3, 0.0, 0.5, 0.0, 0.0, 0.0, 1.0];
    publish_state(&ports.follower_state_publisher, leader_state(now_ns()))
        .expect("follower publish should work");
    publish_state(&ports.leader_state_publisher, state).expect("leader publish should work");
    let command =
        wait_for_command(&ports.command_subscriber, Duration::from_secs(2)).expect("command");

    assert_eq!(command.mode, CommandMode::Cartesian);
    assert_eq!(
        command.cartesian_target,
        [0.3, 0.0, 0.5, 0.0, 0.0, 0.0, 1.0]
    );

    send_shutdown(&ports.control_publisher);
    wait_for_exit(&mut child, Duration::from_secs(2));
}

#[test]
fn router_shutdown_exits_within_500ms() {
    let _guard = test_guard();
    let id = unique_id("shutdown");
    let config = teleop_config(&id, MappingStrategy::DirectJoint);
    let ports = create_test_ports(&config).expect("ports should be created");
    let mut child = spawn_router(&config);
    thread::sleep(Duration::from_millis(100));

    let started = Instant::now();
    send_shutdown(&ports.control_publisher);
    wait_for_exit(&mut child, Duration::from_millis(500));
    assert!(
        started.elapsed() <= Duration::from_millis(500),
        "router did not exit within 500ms"
    );
}

#[test]
fn router_latency_stays_below_budget_at_200_hz() {
    let _guard = test_guard();
    let id = unique_id("latency");
    let config = teleop_config(&id, MappingStrategy::DirectJoint);
    let ports = create_test_ports(&config).expect("ports should be created");
    let mut child = spawn_router(&config);
    thread::sleep(Duration::from_millis(100));

    publish_state(&ports.follower_state_publisher, leader_state(now_ns()))
        .expect("follower publish should work");

    let mut latencies_us = Vec::new();
    for sample_index in 0..80 {
        let timestamp_ns = now_ns();
        let mut state = leader_state(timestamp_ns);
        state.positions[0] = sample_index as f64 / 10.0;
        publish_state(&ports.leader_state_publisher, state).expect("leader publish should work");
        let command = wait_for_command_timestamp(
            &ports.command_subscriber,
            timestamp_ns,
            Duration::from_secs(2),
        )
        .expect("timed command should arrive");
        assert_eq!(command.timestamp_ns, timestamp_ns);
        latencies_us.push((now_ns().saturating_sub(timestamp_ns)) / 1_000);
        thread::sleep(Duration::from_millis(5));
    }

    latencies_us.sort_unstable();
    let median_us = percentile(&latencies_us, 0.5);
    let p99_us = percentile(&latencies_us, 0.99);
    assert!(median_us < 1_000, "median latency too high: {median_us}us");
    assert!(p99_us < 5_000, "p99 latency too high: {p99_us}us");

    send_shutdown(&ports.control_publisher);
    wait_for_exit(&mut child, Duration::from_secs(2));
}

fn binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_rollio-teleop-router")
}

fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn unique_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{prefix}_{nanos}")
}

fn teleop_config(id: &str, mapping: MappingStrategy) -> TeleopRuntimeConfig {
    TeleopRuntimeConfig {
        process_id: format!("teleop.{id}"),
        leader_name: format!("leader_{id}"),
        follower_name: format!("follower_{id}"),
        leader_state_topic: format!("robot/{id}/leader-state"),
        follower_state_topic: format!("robot/{id}/follower-state"),
        follower_command_topic: format!("robot/{id}/follower-command"),
        mapping,
        joint_index_map: vec![0, 1, 2, 3, 4, 5],
        joint_scales: vec![1.0; 6],
    }
}

fn create_test_ports(
    config: &TeleopRuntimeConfig,
) -> Result<TestPorts, Box<dyn std::error::Error>> {
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;

    let state_service_name: ServiceName = config.leader_state_topic.as_str().try_into()?;
    let state_service = node
        .service_builder(&state_service_name)
        .publish_subscribe::<RobotState>()
        .open_or_create()?;
    let leader_state_publisher = state_service.publisher_builder().create()?;

    let follower_state_service_name: ServiceName =
        config.follower_state_topic.as_str().try_into()?;
    let follower_state_service = node
        .service_builder(&follower_state_service_name)
        .publish_subscribe::<RobotState>()
        .open_or_create()?;
    let follower_state_publisher = follower_state_service.publisher_builder().create()?;

    let command_service_name: ServiceName = config.follower_command_topic.as_str().try_into()?;
    let command_service = node
        .service_builder(&command_service_name)
        .publish_subscribe::<RobotCommand>()
        .open_or_create()?;
    let command_subscriber = command_service.subscriber_builder().create()?;

    let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
    let control_service = node
        .service_builder(&control_service_name)
        .publish_subscribe::<ControlEvent>()
        .open_or_create()?;
    let control_publisher = control_service.publisher_builder().create()?;

    Ok(TestPorts {
        _node: node,
        leader_state_publisher,
        follower_state_publisher,
        command_subscriber,
        control_publisher,
    })
}

fn spawn_router(config: &TeleopRuntimeConfig) -> Child {
    let config_toml = toml::to_string(config).expect("config should serialize");
    Command::new(binary_path())
        .arg("run")
        .arg("--config-inline")
        .arg(config_toml)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("teleop router should start")
}

fn publish_state(
    publisher: &StatePublisher,
    state: RobotState,
) -> Result<(), Box<dyn std::error::Error>> {
    publisher.send_copy(state)?;
    Ok(())
}

fn wait_for_command(
    subscriber: &CommandSubscriber,
    timeout: Duration,
) -> Result<RobotCommand, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(sample) = subscriber.receive()? {
            return Ok(*sample.payload());
        }
        thread::sleep(Duration::from_micros(50));
    }
    Err("timed out waiting for follower command".into())
}

fn wait_for_command_timestamp(
    subscriber: &CommandSubscriber,
    expected_timestamp_ns: u64,
    timeout: Duration,
) -> Result<RobotCommand, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(sample) = subscriber.receive()? {
            let command = *sample.payload();
            if command.timestamp_ns == expected_timestamp_ns {
                return Ok(command);
            }
        } else {
            thread::sleep(Duration::from_micros(50));
        }
    }
    Err("timed out waiting for matching follower command".into())
}

fn send_shutdown(publisher: &ControlPublisher) {
    publisher
        .send_copy(ControlEvent::Shutdown)
        .expect("shutdown event should publish");
}

fn wait_for_exit(child: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if child.try_wait().expect("wait should succeed").is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("child did not exit within {timeout:?}");
}

fn leader_state(timestamp_ns: u64) -> RobotState {
    let mut state = RobotState {
        timestamp_ns,
        num_joints: 6,
        ..RobotState::default()
    };
    state.positions[..6].copy_from_slice(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);
    state
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

fn percentile(values: &[u64], quantile: f64) -> u64 {
    let index = ((values.len().saturating_sub(1)) as f64 * quantile).round() as usize;
    values[index]
}
