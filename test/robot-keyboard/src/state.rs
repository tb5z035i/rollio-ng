use crate::controls::{ControlAction, JogMagnitude};
use rollio_types::config::{Config, RobotMode};
use rollio_types::messages::{CommandMode, RobotCommand, RobotState, MAX_JOINTS};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RobotSpec {
    pub name: String,
    pub dof: usize,
    pub configured_mode: RobotMode,
}

#[derive(Debug, Clone)]
pub struct PendingCommand {
    pub robot_name: String,
    pub command: RobotCommand,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActionOutcome {
    pub quit_requested: bool,
    pub publish_mode_switch: Option<RobotMode>,
}

#[derive(Debug)]
pub struct RobotSession {
    spec: RobotSpec,
    mode_hint: RobotMode,
    latest_state: Option<RobotState>,
    latest_state_at: Option<Instant>,
    target_positions: [f64; MAX_JOINTS],
    target_seeded: bool,
    selected_joint: usize,
    state_updates: u64,
}

#[derive(Debug)]
pub struct ControllerState {
    robots: Vec<RobotSession>,
    active_robot_index: usize,
    small_step: f64,
    large_step: f64,
    status_message: String,
}

pub fn robot_specs_from_config(config: &Config) -> Result<Vec<RobotSpec>, String> {
    let specs: Vec<RobotSpec> = config
        .robot_devices()
        .map(|device| -> Result<RobotSpec, String> {
            Ok(RobotSpec {
                name: device.name.clone(),
                dof: device
                    .dof
                    .ok_or_else(|| format!("robot \"{}\" is missing dof", device.name))?
                    as usize,
                configured_mode: device
                    .mode
                    .ok_or_else(|| format!("robot \"{}\" is missing mode", device.name))?,
            })
        })
        .collect::<Result<_, _>>()?;

    if specs.is_empty() {
        return Err("config does not contain any robot devices".into());
    }

    Ok(specs)
}

pub fn mode_label(mode: RobotMode) -> &'static str {
    match mode {
        RobotMode::FreeDrive => "free-drive",
        RobotMode::CommandFollowing => "command-following",
    }
}

impl RobotSession {
    fn new(spec: RobotSpec) -> Self {
        Self {
            mode_hint: spec.configured_mode,
            spec,
            latest_state: None,
            latest_state_at: None,
            target_positions: [0.0; MAX_JOINTS],
            target_seeded: false,
            selected_joint: 0,
            state_updates: 0,
        }
    }

    pub fn spec(&self) -> &RobotSpec {
        &self.spec
    }

    pub fn mode_hint(&self) -> RobotMode {
        self.mode_hint
    }

    pub fn latest_state(&self) -> Option<&RobotState> {
        self.latest_state.as_ref()
    }

    pub fn latest_state_age(&self, now: Instant) -> Option<Duration> {
        self.latest_state_at
            .map(|received_at| now.saturating_duration_since(received_at))
    }

    pub fn target_positions(&self) -> &[f64] {
        &self.target_positions[..self.spec.dof]
    }

    pub fn selected_joint(&self) -> usize {
        self.selected_joint
    }

    pub fn state_updates(&self) -> u64 {
        self.state_updates
    }

    pub fn has_seeded_target(&self) -> bool {
        self.target_seeded
    }

    fn reseed_from_state(&mut self, state: &RobotState) {
        self.target_positions[..self.spec.dof].copy_from_slice(&state.positions[..self.spec.dof]);
        self.target_seeded = true;
    }
}

impl ControllerState {
    pub fn new(specs: Vec<RobotSpec>, small_step: f64, large_step: f64) -> Result<Self, String> {
        if specs.is_empty() {
            return Err("at least one robot is required".into());
        }

        Ok(Self {
            robots: specs.into_iter().map(RobotSession::new).collect(),
            active_robot_index: 0,
            small_step,
            large_step,
            status_message: "Waiting for robot state...".into(),
        })
    }

    pub fn robots(&self) -> &[RobotSession] {
        &self.robots
    }

    pub fn active_robot_index(&self) -> usize {
        self.active_robot_index
    }

    pub fn active_robot(&self) -> &RobotSession {
        &self.robots[self.active_robot_index]
    }

    pub fn status_message(&self) -> &str {
        &self.status_message
    }

    pub fn set_status_message(&mut self, status_message: impl Into<String>) {
        self.status_message = status_message.into();
    }

    pub fn update_state(&mut self, robot_name: &str, state: RobotState, received_at: Instant) {
        let mut seeded_name = None;
        let Some(session) = self
            .robots
            .iter_mut()
            .find(|session| session.spec.name == robot_name)
        else {
            return;
        };

        if !session.target_seeded {
            session.reseed_from_state(&state);
            seeded_name = Some(session.spec.name.clone());
        }

        session.latest_state = Some(state);
        session.latest_state_at = Some(received_at);
        session.state_updates += 1;

        if let Some(name) = seeded_name {
            self.status_message = format!("Seeded targets from first state for {name}");
        }
    }

    pub fn active_command(&self) -> Option<PendingCommand> {
        let session = self.active_robot();
        if !session.has_seeded_target() {
            return None;
        }

        let mut joint_targets = [0.0; MAX_JOINTS];
        joint_targets[..session.spec.dof]
            .copy_from_slice(&session.target_positions[..session.spec.dof]);

        Some(PendingCommand {
            robot_name: session.spec.name.clone(),
            command: RobotCommand {
                timestamp_ns: unix_timestamp_ns(),
                mode: CommandMode::Joint,
                num_joints: session.spec.dof as u32,
                joint_targets,
                ..RobotCommand::default()
            },
        })
    }

    pub fn apply_action(&mut self, action: ControlAction) -> ActionOutcome {
        match action {
            ControlAction::Quit => ActionOutcome {
                quit_requested: true,
                ..ActionOutcome::default()
            },
            ControlAction::NextRobot => {
                self.active_robot_index = (self.active_robot_index + 1) % self.robots.len();
                self.sync_joint_selection();
                self.status_message = format!("Active robot: {}", self.active_robot().spec.name);
                ActionOutcome::default()
            }
            ControlAction::SelectRobot(index) => {
                if index < self.robots.len() {
                    self.active_robot_index = index;
                    self.sync_joint_selection();
                    self.status_message =
                        format!("Active robot: {}", self.active_robot().spec.name);
                } else {
                    self.status_message = format!(
                        "Robot {} is unavailable; only {} configured",
                        index + 1,
                        self.robots.len()
                    );
                }
                ActionOutcome::default()
            }
            ControlAction::SelectPrevJoint => {
                let (selected_joint, name) = {
                    let session = self.active_robot_mut();
                    session.selected_joint = if session.selected_joint == 0 {
                        session.spec.dof.saturating_sub(1)
                    } else {
                        session.selected_joint - 1
                    };
                    (session.selected_joint, session.spec.name.clone())
                };
                self.status_message = format!("Selected joint {} on {}", selected_joint, name);
                ActionOutcome::default()
            }
            ControlAction::SelectNextJoint => {
                let (selected_joint, name) = {
                    let session = self.active_robot_mut();
                    session.selected_joint = (session.selected_joint + 1) % session.spec.dof.max(1);
                    (session.selected_joint, session.spec.name.clone())
                };
                self.status_message = format!("Selected joint {} on {}", selected_joint, name);
                ActionOutcome::default()
            }
            ControlAction::JogActiveJoint {
                direction,
                magnitude,
            } => {
                let step = match magnitude {
                    JogMagnitude::Small => self.small_step,
                    JogMagnitude::Large => self.large_step,
                };
                let (name, selected_joint, target) = {
                    let session = self.active_robot_mut();
                    session.target_seeded = true;
                    session.target_positions[session.selected_joint] += step * f64::from(direction);
                    (
                        session.spec.name.clone(),
                        session.selected_joint,
                        session.target_positions[session.selected_joint],
                    )
                };
                self.status_message =
                    format!("{} joint {} target -> {:+.3}", name, selected_joint, target);
                ActionOutcome::default()
            }
            ControlAction::ResetActiveTarget => {
                let status_message = {
                    let session = self.active_robot_mut();
                    if let Some(state) = session.latest_state {
                        session.reseed_from_state(&state);
                        format!("Reset targets from latest state for {}", session.spec.name)
                    } else {
                        format!(
                            "No state received for {}; target left unchanged",
                            session.spec.name
                        )
                    }
                };
                self.status_message = status_message;
                ActionOutcome::default()
            }
            ControlAction::ToggleMode => {
                let next_mode = match self.active_robot().mode_hint {
                    RobotMode::FreeDrive => RobotMode::CommandFollowing,
                    RobotMode::CommandFollowing => RobotMode::FreeDrive,
                };
                for session in &mut self.robots {
                    session.mode_hint = next_mode;
                }
                self.status_message =
                    format!("Requested global mode switch to {}", mode_label(next_mode));
                ActionOutcome {
                    publish_mode_switch: Some(next_mode),
                    ..ActionOutcome::default()
                }
            }
        }
    }

    fn active_robot_mut(&mut self) -> &mut RobotSession {
        &mut self.robots[self.active_robot_index]
    }

    fn sync_joint_selection(&mut self) {
        let session = self.active_robot_mut();
        if session.selected_joint >= session.spec.dof {
            session.selected_joint = session.spec.dof.saturating_sub(1);
        }
    }
}

fn unix_timestamp_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

#[cfg(test)]
mod tests {
    use super::{mode_label, robot_specs_from_config, ControllerState, RobotSpec};
    use crate::controls::{ControlAction, JogMagnitude};
    use rollio_types::config::{Config, RobotMode};
    use rollio_types::messages::RobotState;
    use std::time::Instant;

    #[test]
    fn extracts_robot_specs_from_config() {
        let config: Config = sample_config().parse().expect("config should parse");
        let specs = robot_specs_from_config(&config).expect("robot specs should load");
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].name, "leader_arm");
        assert_eq!(specs[0].dof, 6);
        assert_eq!(specs[1].configured_mode, RobotMode::CommandFollowing);
    }

    #[test]
    fn seeds_target_from_first_state() {
        let mut controller = ControllerState::new(sample_specs(), 0.05, 0.2).unwrap();
        let mut state = RobotState::default();
        state.num_joints = 3;
        state.positions[0] = 0.25;
        state.positions[1] = -0.5;
        state.positions[2] = 1.0;

        controller.update_state("leader_arm", state, Instant::now());

        assert_eq!(
            &controller.active_robot().target_positions()[..3],
            &[0.25, -0.5, 1.0]
        );
        assert_eq!(
            controller.status_message(),
            "Seeded targets from first state for leader_arm"
        );
    }

    #[test]
    fn wraps_robot_and_joint_selection() {
        let mut controller = ControllerState::new(sample_specs(), 0.05, 0.2).unwrap();
        controller.apply_action(ControlAction::SelectPrevJoint);
        assert_eq!(controller.active_robot().selected_joint(), 5);

        controller.apply_action(ControlAction::NextRobot);
        assert_eq!(controller.active_robot_index(), 1);

        controller.apply_action(ControlAction::SelectNextJoint);
        assert_eq!(controller.active_robot().selected_joint(), 1);
    }

    #[test]
    fn jogs_and_resets_active_target() {
        let mut controller = ControllerState::new(sample_specs(), 0.1, 0.5).unwrap();
        let outcome = controller.apply_action(ControlAction::JogActiveJoint {
            direction: 1,
            magnitude: JogMagnitude::Large,
        });
        assert_eq!(outcome.publish_mode_switch, None);
        assert_eq!(controller.active_robot().target_positions()[0], 0.5);

        let mut state = RobotState::default();
        state.num_joints = 6;
        state.positions[0] = -0.25;
        controller.update_state("leader_arm", state, Instant::now());
        controller.apply_action(ControlAction::ResetActiveTarget);
        assert_eq!(controller.active_robot().target_positions()[0], -0.25);
    }

    #[test]
    fn toggle_mode_updates_hint_for_all_robots() {
        let mut controller = ControllerState::new(sample_specs(), 0.05, 0.2).unwrap();
        let outcome = controller.apply_action(ControlAction::ToggleMode);

        assert_eq!(
            outcome.publish_mode_switch,
            Some(RobotMode::CommandFollowing)
        );
        assert_eq!(
            controller
                .robots()
                .iter()
                .map(|robot| robot.mode_hint())
                .collect::<Vec<_>>(),
            vec![RobotMode::CommandFollowing, RobotMode::CommandFollowing]
        );
        assert_eq!(mode_label(RobotMode::FreeDrive), "free-drive");
    }

    #[test]
    fn builds_active_command_from_targets() {
        let mut controller = ControllerState::new(sample_specs(), 0.1, 0.4).unwrap();
        controller.apply_action(ControlAction::JogActiveJoint {
            direction: 1,
            magnitude: JogMagnitude::Small,
        });
        controller.apply_action(ControlAction::SelectNextJoint);
        controller.apply_action(ControlAction::JogActiveJoint {
            direction: -1,
            magnitude: JogMagnitude::Large,
        });

        let pending = controller
            .active_command()
            .expect("command should be ready");
        assert_eq!(pending.robot_name, "leader_arm");
        assert_eq!(pending.command.num_joints, 6);
        assert_eq!(pending.command.joint_targets[0], 0.1);
        assert_eq!(pending.command.joint_targets[1], -0.4);
    }

    fn sample_specs() -> Vec<RobotSpec> {
        vec![
            RobotSpec {
                name: "leader_arm".into(),
                dof: 6,
                configured_mode: RobotMode::FreeDrive,
            },
            RobotSpec {
                name: "follower_arm".into(),
                dof: 6,
                configured_mode: RobotMode::CommandFollowing,
            },
        ]
    }

    fn sample_config() -> &'static str {
        r#"
[episode]
format = "lerobot-v2.1"
fps = 30
chunk_size = 1000

[controller]
shutdown_timeout_ms = 3000
child_poll_interval_ms = 100

[visualizer]
port = 9090
max_preview_width = 320
max_preview_height = 240
jpeg_quality = 30
preview_fps = 30

[[devices]]
name = "camera_top"
type = "camera"
driver = "pseudo"
id = "camera_0"
width = 640
height = 480
fps = 30
pixel_format = "rgb24"
stream = "color"
transport = "simulated"

[[devices]]
name = "leader_arm"
type = "robot"
driver = "pseudo"
id = "leader_0"
dof = 6
mode = "free-drive"
control_frequency_hz = 60.0
transport = "simulated"

[[devices]]
name = "follower_arm"
type = "robot"
driver = "pseudo"
id = "follower_0"
dof = 6
mode = "command-following"
control_frequency_hz = 60.0
transport = "simulated"

[encoder]
codec = "libx264"
queue_size = 32

[storage]
backend = "local"
output_path = "./output"

[monitor]
metrics_frequency_hz = 1.0
"#
    }
}
