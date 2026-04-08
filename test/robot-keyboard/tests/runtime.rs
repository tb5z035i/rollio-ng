use rollio_test_robot_keyboard::{
    bus::RobotBus,
    controls::{ControlAction, JogMagnitude},
    state::{ControllerState, RobotSpec},
};
use rollio_types::config::{DeviceConfig, DeviceType, RobotMode};
use rollio_types::messages::RobotState;
use std::sync::{Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Harness {
    bus: RobotBus,
    controller: ControllerState,
    driver: Option<JoinHandle<()>>,
    shutdown: bool,
}

impl Harness {
    fn start(device: DeviceConfig) -> Self {
        let spec = RobotSpec {
            name: device.name.clone(),
            dof: device.dof.unwrap_or(6) as usize,
            configured_mode: device.mode.unwrap_or(RobotMode::FreeDrive),
        };
        let bus = RobotBus::connect(std::slice::from_ref(&spec)).expect("bus should connect");
        let controller =
            ControllerState::new(vec![spec], 0.1, 0.5).expect("controller should build");
        let driver = thread::spawn(move || {
            rollio_robot_pseudo::run_device(device).expect("pseudo robot should run");
        });

        Self {
            bus,
            controller,
            driver: Some(driver),
            shutdown: false,
        }
    }

    fn poll_states(&mut self) {
        let received_at = Instant::now();
        self.bus
            .drain_states(|robot_name, state| {
                self.controller.update_state(robot_name, state, received_at);
            })
            .expect("state drain should succeed");
    }

    fn publish_active_command(&self) {
        if let Some(pending) = self.controller.active_command() {
            self.bus
                .publish_command(&pending)
                .expect("command publish should succeed");
        }
    }

    fn shutdown(mut self) {
        self.shutdown_inner();
    }

    fn shutdown_inner(&mut self) {
        if self.shutdown {
            return;
        }
        self.shutdown = true;
        let _ = self.bus.publish_shutdown();
        if let Some(handle) = self.driver.take() {
            handle
                .join()
                .expect("pseudo robot thread should exit cleanly");
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.shutdown_inner();
    }
}

#[test]
fn mode_switch_and_joint_command_drive_the_pseudo_robot() {
    let _guard = test_guard();
    let device = pseudo_robot_device(unique_name("keyboard"), RobotMode::FreeDrive);
    let mut harness = Harness::start(device);

    wait_until(Duration::from_secs(2), Duration::from_millis(20), || {
        harness.poll_states();
        harness.controller.active_robot().latest_state().copied()
    })
    .expect("pseudo robot should publish initial state");

    let outcome = harness.controller.apply_action(ControlAction::ToggleMode);
    let target_mode = outcome
        .publish_mode_switch
        .expect("toggle mode should request a bus event");
    assert_eq!(target_mode, RobotMode::CommandFollowing);
    harness
        .bus
        .publish_mode_switch(target_mode)
        .expect("mode switch should publish");

    harness
        .controller
        .apply_action(ControlAction::JogActiveJoint {
            direction: 1,
            magnitude: JogMagnitude::Large,
        });
    let target = harness.controller.active_robot().target_positions()[0];

    let reached = wait_until(Duration::from_secs(3), Duration::from_millis(20), || {
        harness.poll_states();
        harness.publish_active_command();
        harness
            .controller
            .active_robot()
            .latest_state()
            .copied()
            .filter(|state| (state.positions[0] - target).abs() <= 0.15)
    })
    .expect("pseudo robot should track the target after mode switch");

    assert!((reached.positions[0] - target).abs() <= 0.15);
    assert_eq!(
        harness.controller.active_robot().mode_hint(),
        RobotMode::CommandFollowing
    );
    harness.shutdown();
}

fn wait_until<F>(timeout: Duration, poll_interval: Duration, mut step: F) -> Option<RobotState>
where
    F: FnMut() -> Option<RobotState>,
{
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(state) = step() {
            return Some(state);
        }
        thread::sleep(poll_interval);
    }

    None
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
        command_latency_ms: Some(50),
        state_noise_stddev: Some(0.0),
        extra: toml::Table::new(),
    }
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
    format!("{prefix}_{nanos}")
}
