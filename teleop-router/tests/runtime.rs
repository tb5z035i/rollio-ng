use iceoryx2::prelude::*;
use rollio_bus::CONTROL_EVENTS_SERVICE;
use rollio_types::config::{
    ChannelCommandDefaults, MappingStrategy, RobotCommandKind, RobotStateKind,
    TeleopRuntimeConfigV2,
};
use rollio_types::messages::{ControlEvent, Pose7};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Cartesian initial-syncing ramp end-to-end test.
//
// Drives the v2 router binary against `Pose7` topics: publishes a fixed
// follower EE pose far from the leader, asserts the first forwarded
// command lands within one cartesian sync step of the follower (proving
// the ramp engaged), then drives the follower toward the leader until
// pass-through engages and verifies the leader's pose is forwarded
// verbatim.
// ---------------------------------------------------------------------------

type PosePublisher = iceoryx2::port::publisher::Publisher<ipc::Service, Pose7, ()>;
type PoseSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, Pose7, ()>;
type ControlPublisher = iceoryx2::port::publisher::Publisher<ipc::Service, ControlEvent, ()>;

struct PoseTestPorts {
    _node: Node<ipc::Service>,
    leader_state_publisher: PosePublisher,
    follower_state_publisher: PosePublisher,
    command_subscriber: PoseSubscriber,
    control_publisher: ControlPublisher,
}

// Must stay in sync with `teleop_router::SYNC_MAX_STEP_M` (private).
const SYNC_MAX_STEP_M: f64 = 0.0025;
// Must stay in sync with `teleop_router::SYNC_HOLD_DURATION` (private):
// the cartesian ramp now requires this much sustained leader/follower
// closeness before declaring sync complete and engaging pass-through.
// Tests pad a little to absorb scheduling jitter.
const SYNC_HOLD_DURATION: Duration = Duration::from_secs(1);
const SYNC_HOLD_PADDING: Duration = Duration::from_millis(200);

#[test]
fn router_cartesian_initial_sync_ramps_pose() {
    let _guard = test_guard();
    let id = unique_id("cart_sync");
    let config = cartesian_sync_config(&id);
    let ports = create_pose_test_ports(&config).expect("ports should be created");
    let mut child = spawn_router_v2(&config);
    thread::sleep(Duration::from_millis(150));

    // The router skips leader samples whose `timestamp_us` matches the
    // last forwarded one, so we use a monotonic counter rather than
    // wallclock millis to ensure each leader publish is treated as a
    // distinct sample even when the test loop iterates inside a single
    // millisecond.
    let mut tick_ts = 1u64;
    let mut next_ts = || {
        let value = tick_ts;
        tick_ts += 1;
        value
    };

    // Follower starts at the origin with identity orientation.
    let follower_pose = Pose7 {
        timestamp_us: next_ts(),
        values: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
    };
    publish_pose(&ports.follower_state_publisher, follower_pose)
        .expect("follower state publish should work");

    // Leader is 1 m away along +X with identity orientation. This is
    // far above the SYNC_COMPLETE_THRESHOLD_M so the very first
    // forwarded command MUST be clamped.
    let leader_pose = Pose7 {
        timestamp_us: next_ts(),
        values: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
    };
    publish_pose(&ports.leader_state_publisher, leader_pose)
        .expect("leader state publish should work");

    let first_command = wait_for_pose_command(&ports.command_subscriber, Duration::from_secs(2))
        .expect("first cartesian command should arrive");
    let dx = first_command.values[0] - follower_pose.values[0];
    let dy = first_command.values[1] - follower_pose.values[1];
    let dz = first_command.values[2] - follower_pose.values[2];
    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
    assert!(
        dist <= SYNC_MAX_STEP_M + 1e-9,
        "first command translation step {dist} exceeded SYNC_MAX_STEP_M {SYNC_MAX_STEP_M}",
    );
    assert!(
        first_command.values[0] > 0.0,
        "ramp should still point toward the leader (+X)"
    );

    // Walk the follower toward the leader until the router exits the
    // ramp. We do this by repeatedly publishing follower poses that
    // approach the leader and a fresh leader pose that nudges forward
    // by one step (so the router always sees a *new* leader timestamp
    // and re-evaluates).
    let mut follower_x = 0.0_f64;
    // Phase 1: walk the follower toward the leader until the published
    // target equals the leader (proving the live (leader, follower) gap
    // has dropped inside the completion thresholds and the router is in
    // the "publish leader directly" branch). Sync is NOT yet complete
    // -- the new design only declares completion after SYNC_HOLD_DURATION
    // of *sustained* closeness, which we exercise in phase 2 below.
    let mut close_branch_engaged = false;
    for tick in 0..200 {
        follower_x = (follower_x + 0.05).min(1.0);
        let updated_follower = Pose7 {
            timestamp_us: next_ts(),
            values: [follower_x, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        };
        publish_pose(&ports.follower_state_publisher, updated_follower)
            .expect("follower state publish should work");
        let updated_leader = Pose7 {
            timestamp_us: next_ts(),
            values: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        };
        publish_pose(&ports.leader_state_publisher, updated_leader)
            .expect("leader state publish should work");
        let command = wait_for_pose_command(&ports.command_subscriber, Duration::from_millis(500))
            .expect("router should keep producing commands");
        if (command.values[0] - 1.0).abs() < 1e-9 && follower_x >= 1.0 {
            close_branch_engaged = true;
            break;
        }
        // Defensive sanity: never let the ramp publish a command past
        // the leader's actual pose.
        assert!(
            command.values[0] <= 1.0 + 1e-9,
            "ramp produced overshoot at tick {tick}: {}",
            command.values[0]
        );
    }
    assert!(
        close_branch_engaged,
        "router did not enter the close branch within 200 ticks"
    );

    // Phase 2: sustain the close pose for at least SYNC_HOLD_DURATION
    // (plus padding for scheduling jitter) so the sustained-closeness
    // counter trips and the router declares pass-through. We can tell
    // pass-through has engaged by sending a *big* leader jump and
    // observing it arrive verbatim; until pass-through engages the
    // router would clamp it to a small step.
    let hold_until = Instant::now() + SYNC_HOLD_DURATION + SYNC_HOLD_PADDING;
    while Instant::now() < hold_until {
        let updated_follower = Pose7 {
            timestamp_us: next_ts(),
            values: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        };
        publish_pose(&ports.follower_state_publisher, updated_follower)
            .expect("follower state publish should work");
        let updated_leader = Pose7 {
            timestamp_us: next_ts(),
            values: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        };
        publish_pose(&ports.leader_state_publisher, updated_leader)
            .expect("leader state publish should work");
        // Drain any commands the router emits during the hold so the
        // queue does not back up.
        let _ = wait_for_pose_command(&ports.command_subscriber, Duration::from_millis(50));
        thread::sleep(Duration::from_millis(20));
    }

    // After completion, an arbitrary big leader jump must be forwarded
    // verbatim (pass-through engaged).
    let jump_leader = Pose7 {
        timestamp_us: next_ts(),
        values: [-2.0, 1.0, 3.0, 0.0, 0.0, 0.0, 1.0],
    };
    publish_pose(&ports.leader_state_publisher, jump_leader)
        .expect("leader jump publish should work");
    let jump_command = wait_for_pose_command(&ports.command_subscriber, Duration::from_secs(2))
        .expect("jump command should arrive");
    assert_eq!(jump_command.values, jump_leader.values);

    send_shutdown(&ports.control_publisher);
    wait_for_exit(&mut child, Duration::from_secs(2));
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn cartesian_sync_config(id: &str) -> TeleopRuntimeConfigV2 {
    TeleopRuntimeConfigV2 {
        process_id: format!("teleop.{id}"),
        leader_channel_id: format!("leader_{id}"),
        follower_channel_id: format!("follower_{id}"),
        leader_state_kind: RobotStateKind::EndEffectorPose,
        leader_state_topic: format!("robot/{id}/leader/end_effector_pose"),
        follower_command_kind: RobotCommandKind::EndPose,
        follower_command_topic: format!("robot/{id}/follower/end_pose"),
        follower_state_kind: Some(RobotStateKind::EndEffectorPose),
        follower_state_topic: Some(format!("robot/{id}/follower/end_effector_pose")),
        sync_max_step_rad: None,
        sync_complete_threshold_rad: None,
        mapping: MappingStrategy::Cartesian,
        joint_index_map: Vec::new(),
        joint_scales: Vec::new(),
        command_defaults: ChannelCommandDefaults::default(),
    }
}

fn create_pose_test_ports(
    config: &TeleopRuntimeConfigV2,
) -> Result<PoseTestPorts, Box<dyn std::error::Error>> {
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;

    let leader_service_name: ServiceName = config.leader_state_topic.as_str().try_into()?;
    let leader_service = node
        .service_builder(&leader_service_name)
        .publish_subscribe::<Pose7>()
        .open_or_create()?;
    let leader_state_publisher = leader_service.publisher_builder().create()?;

    let follower_topic = config
        .follower_state_topic
        .as_deref()
        .expect("cartesian sync config must have a follower state topic");
    let follower_service_name: ServiceName = follower_topic.try_into()?;
    let follower_service = node
        .service_builder(&follower_service_name)
        .publish_subscribe::<Pose7>()
        .open_or_create()?;
    let follower_state_publisher = follower_service.publisher_builder().create()?;

    let command_service_name: ServiceName = config.follower_command_topic.as_str().try_into()?;
    let command_service = node
        .service_builder(&command_service_name)
        .publish_subscribe::<Pose7>()
        .open_or_create()?;
    let command_subscriber = command_service.subscriber_builder().create()?;

    let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
    let control_service = node
        .service_builder(&control_service_name)
        .publish_subscribe::<ControlEvent>()
        .open_or_create()?;
    let control_publisher = control_service.publisher_builder().create()?;

    Ok(PoseTestPorts {
        _node: node,
        leader_state_publisher,
        follower_state_publisher,
        command_subscriber,
        control_publisher,
    })
}

fn spawn_router_v2(config: &TeleopRuntimeConfigV2) -> Child {
    let config_toml = toml::to_string(config).expect("v2 config should serialize");
    Command::new(binary_path())
        .arg("run")
        .arg("--config-inline")
        .arg(config_toml)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("teleop router should start")
}

fn publish_pose(publisher: &PosePublisher, pose: Pose7) -> Result<(), Box<dyn std::error::Error>> {
    publisher.send_copy(pose)?;
    Ok(())
}

fn wait_for_pose_command(
    subscriber: &PoseSubscriber,
    timeout: Duration,
) -> Result<Pose7, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(sample) = subscriber.receive()? {
            return Ok(*sample.payload());
        }
        thread::sleep(Duration::from_micros(50));
    }
    Err("timed out waiting for follower pose command".into())
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
