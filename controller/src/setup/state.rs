use super::pairings::build_default_channel_pairings;
use super::runtime::SETUP_UI_SUCCESS_DELAY;
use rollio_types::config::{
    BinaryDeviceConfig, ChannelPairingConfig, CollectionMode, DeviceType, ProjectConfig,
    RobotCommandKind, RobotMode, RobotStateKind,
};
use rollio_types::messages::PixelFormat;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::path::PathBuf;
use std::time::Instant;

pub(super) const IDENTIFY_ACTIVE_MESSAGE_PREFIX: &str = "Identify active for ";

pub(super) type SetupDeviceChannel = (String, String);
pub(super) type TeleopPairEndpoints = (SetupDeviceChannel, SetupDeviceChannel);

/// All inputs the wizard's modal pairing picker collects before the
/// controller materializes a new pair: chosen policy, both endpoints,
/// and (for `Parallel`) the operator-supplied scaling ratio. `Parallel`
/// pairs default to `ratio = 1.0` when the operator skips the ratio
/// phase; non-Parallel policies ignore the field.
#[derive(Debug, Clone)]
pub(super) struct TeleopPairCreate {
    pub(super) policy: rollio_types::config::MappingStrategy,
    pub(super) leader: SetupDeviceChannel,
    pub(super) follower: SetupDeviceChannel,
    pub(super) ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct DiscoveredDevice {
    pub(super) driver: String,
    pub(super) id: String,
    /// Device-level display label provided by the executable (e.g.
    /// "AIRBOT Play", or the V4L2 capabilities name). Used as the per-row
    /// label fallback when a channel does not provide its own label.
    pub(super) display_name: String,
    /// Default user-facing name for the device row when the wizard collapses
    /// channels into one entry. Sourced from the driver's
    /// `DeviceQueryDevice.default_device_name`. Falls back to a snake-case
    /// driver name in the wizard.
    pub(super) default_device_name: Option<String>,
    /// Per-channel metadata keyed by `channel_type`. Holds everything the
    /// wizard / setup config needs (kind, modes, dof, profiles, defaults,
    /// value_limits, direct_joint_compatibility, ...).
    pub(super) channel_meta_by_channel: std::collections::BTreeMap<String, DiscoveredChannelMeta>,
    /// Generic device-level metadata mirroring the driver's
    /// `optional_info` (transport, interface, product_variant, end_effector).
    /// Persisted into `BinaryDeviceConfig.extra` so downstream consumers
    /// stay schema-driven; new keys flow through automatically without
    /// controller changes.
    pub(super) transport: Option<String>,
    pub(super) interface: Option<String>,
    pub(super) product_variant: Option<String>,
    pub(super) end_effector: Option<String>,
}

impl DiscoveredDevice {
    /// "Primary" kind for compatibility with the legacy single-row UI: a
    /// device counts as a robot if any of its channels is a robot kind,
    /// otherwise camera. Used only by the wizard's row-rendering paths;
    /// authoritative kind lives on each channel.
    pub(super) fn primary_device_type(&self) -> DeviceType {
        if self
            .channel_meta_by_channel
            .values()
            .any(|meta| meta.kind == DeviceType::Robot)
        {
            DeviceType::Robot
        } else {
            DeviceType::Camera
        }
    }

    /// All camera profiles across every camera channel, flattened for
    /// callers that still need a device-wide list (e.g. legacy wizard
    /// helpers that grouped profiles before knowing channels).
    pub(super) fn all_camera_profiles(&self) -> Vec<CameraProfile> {
        self.channel_meta_by_channel
            .values()
            .filter(|meta| meta.kind == DeviceType::Camera)
            .flat_map(|meta| meta.profiles.iter().cloned())
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub(super) struct DiscoveredChannelMeta {
    /// `kind` per channel from `query --json`: camera, robot, etc.
    /// Defaults to `Robot` to keep older fixtures working unchanged.
    #[serde(default = "default_channel_kind")]
    pub(super) kind: DeviceType,
    pub(super) channel_label: Option<String>,
    pub(super) default_name: Option<String>,
    /// Robot modes the driver accepts on this channel. For camera channels
    /// this is conventionally `["enabled", "disabled"]`; the controller no
    /// longer cares about exact strings here — it just maps known ones to
    /// `RobotMode` enum variants.
    #[serde(default)]
    pub(super) modes: Vec<RobotMode>,
    /// `dof` reported per channel; only meaningful for robot kinds.
    #[serde(default)]
    pub(super) dof: Option<u32>,
    /// Camera profiles reported per channel. Empty for non-camera kinds.
    #[serde(default)]
    pub(super) profiles: Vec<CameraProfile>,
    /// Driver-suggested default control frequency for this channel.
    #[serde(default)]
    pub(super) default_control_frequency_hz: Option<f64>,
    /// Default command parameters (`joint_mit_kp/kd`, `parallel_mit_kp/kd`).
    /// Used to seed `DeviceChannelConfigV2.command_defaults` without any
    /// vendor-specific lookup table.
    #[serde(default)]
    pub(super) defaults: rollio_types::config::ChannelCommandDefaults,
    /// Per-state value limits reported by the device driver (rad / rad·s⁻¹ /
    /// Nm / m for parallel kinds). Captured from the channel's `query --json`
    /// response so the visualizer can render limit-aware bars instead of
    /// guessing the value envelope.
    #[serde(default)]
    pub(super) value_limits: Vec<rollio_types::config::StateValueLimitsEntry>,
    /// All `RobotStateKind` values this driver reports it can publish on
    /// this channel. The setup wizard's "States" sub-step uses this list
    /// to render the toggleable publish/recorded options. Falls back to
    /// `value_limits` keys when the driver doesn't populate it explicitly.
    #[serde(default)]
    pub(super) supported_states: Vec<RobotStateKind>,
    /// Robot command kinds the driver advertises it accepts on this channel.
    /// Persisted on `DeviceChannelConfigV2.supported_commands` so downstream
    /// teleop / pairing logic stays driver-agnostic.
    #[serde(default)]
    pub(super) supported_commands: Vec<rollio_types::config::RobotCommandKind>,
    /// Direct-joint pairing peers as reported by the driver. Persisted on
    /// `DeviceChannelConfigV2.direct_joint_compatibility` so pairing
    /// validation can consult the schema instead of any vendor table.
    #[serde(default)]
    pub(super) direct_joint_compatibility: rollio_types::config::DirectJointCompatibility,
    /// Sensor sample kinds this channel publishes. Empty for camera/robot.
    #[serde(default)]
    pub(super) supported_sensor_kinds: Vec<rollio_types::config::SensorStateKind>,
    /// Driver-suggested sample period for sensor channels (`None` for
    /// camera/robot).
    #[serde(default)]
    pub(super) default_sample_rate_hz: Option<f64>,
    /// Per-kind shape hints reported by the driver for sensor channels.
    #[serde(default)]
    pub(super) sensor_shape_hints:
        std::collections::BTreeMap<rollio_types::config::SensorStateKind, Vec<u32>>,
}

pub(super) fn default_channel_kind() -> DeviceType {
    DeviceType::Robot
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct CameraProfile {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) fps: u32,
    pub(super) pixel_format: PixelFormat,
    pub(super) native_pixel_format: Option<String>,
    pub(super) stream: Option<String>,
    pub(super) channel: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct AvailableDevice {
    pub(super) name: String,
    pub(super) display_name: String,
    pub(super) device_type: DeviceType,
    pub(super) driver: String,
    pub(super) id: String,
    pub(super) camera_profiles: Vec<CameraProfile>,
    pub(super) supported_modes: Vec<RobotMode>,
    /// All `RobotStateKind` values the driver advertises it can publish on
    /// this channel. The setup wizard's "States" sub-step uses this list to
    /// render the toggleable publish/recorded options. Empty for camera
    /// channels.
    #[serde(default)]
    pub(super) supported_states: Vec<RobotStateKind>,
    /// All `RobotCommandKind` values the driver accepts on this channel.
    /// The setup wizard's "Pairing" picker uses this to filter follower
    /// candidates by policy (DirectJoint needs `JointPosition`, Cartesian
    /// needs `EndPose`, Parallel needs `ParallelPosition` / `ParallelMit`).
    /// Empty for camera channels.
    #[serde(default)]
    pub(super) supported_commands: Vec<RobotCommandKind>,
    /// Driver-advertised direct-joint compatibility whitelist. The
    /// pairing picker uses this to enforce the two-sided whitelist
    /// for DirectJoint pairs without round-tripping through the
    /// controller.
    #[serde(default)]
    pub(super) direct_joint_compatibility: rollio_types::config::DirectJointCompatibility,
    /// Single-binary snapshot for this discovery row (one channel).
    pub(super) current: BinaryDeviceConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DeviceIdentity {
    pub(super) device_type: DeviceType,
    pub(super) driver: String,
    pub(super) id: String,
    /// Logical camera/robot channel id (`color`, `arm`, `infrared_1`, …).
    pub(super) channel_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum SetupStep {
    Devices,
    States,
    Pairing,
    Storage,
    Preview,
}

impl SetupStep {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Devices => "Devices",
            Self::States => "States",
            Self::Pairing => "Pairing",
            Self::Storage => "Settings",
            Self::Preview => "Overview",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum SetupUiStatus {
    Editing,
    Saved,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SetupExitKind {
    Saved,
    Cancelled,
}

#[derive(Debug)]
pub(super) struct SetupSession {
    pub(super) config: ProjectConfig,
    pub(super) available_devices: Vec<AvailableDevice>,
    pub(super) teleop_pairing_cache: Vec<ChannelPairingConfig>,
    pub(super) identify_device_name: Option<String>,
    /// Name (= `AvailableDevice.name`) of the channel whose subpanel is
    /// currently open in Step 1. `None` when no subpanel is active.
    /// Mutated by `open_subpanel` / `close_subpanel` and consumed by
    /// the Ink UI to render the modal overlay.
    pub(super) subpanel_target_name: Option<String>,
    pub(super) current_step: SetupStep,
    pub(super) output_path: PathBuf,
    pub(super) resume_mode: bool,
    pub(super) warnings: Vec<String>,
    pub(super) message: Option<String>,
    pub(super) status: SetupUiStatus,
    pub(super) completed_at: Option<Instant>,
    pub(super) exit_kind: Option<SetupExitKind>,
}

#[derive(Debug, Deserialize)]
pub(super) struct SetupCommandEnvelope {
    #[serde(rename = "type")]
    pub(super) msg_type: String,
    pub(super) action: String,
    pub(super) name: Option<String>,
    pub(super) index: Option<usize>,
    pub(super) delta: Option<i32>,
    pub(super) value: Option<String>,
    /// Optional sub-field selector used by generic subpanel commands
    /// (`setup_subpanel_set_record_field` /
    /// `setup_subpanel_cycle_record_field` and their preview
    /// counterparts) — identifies WHICH record/preview encoder knob
    /// the operator is editing.
    pub(super) field: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct SetupStateEnvelope {
    #[serde(rename = "type")]
    pub(super) msg_type: &'static str,
    pub(super) step: SetupStep,
    pub(super) step_index: usize,
    pub(super) step_name: &'static str,
    pub(super) total_steps: usize,
    pub(super) output_path: String,
    pub(super) resume_mode: bool,
    pub(super) status: SetupUiStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) message: Option<String>,
    pub(super) identify_device: Option<String>,
    /// Mirrors `SetupSession.subpanel_target_name` so the Ink UI knows
    /// when to render the channel subpanel modal overlay.
    pub(super) subpanel_target: Option<String>,
    pub(super) warnings: Vec<String>,
    pub(super) config: ProjectConfig,
    pub(super) available_devices: Vec<AvailableDevice>,
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct SessionMutation {
    pub(super) state_changed: bool,
    pub(super) config_changed: bool,
    pub(super) step_changed: bool,
}

impl SessionMutation {
    pub(super) fn state_only(changed: bool) -> Self {
        Self {
            state_changed: changed,
            ..Self::default()
        }
    }

    pub(super) fn config_changed(changed: bool) -> Self {
        Self {
            state_changed: changed,
            config_changed: changed,
            ..Self::default()
        }
    }

    pub(super) fn step_changed(changed: bool) -> Self {
        Self {
            state_changed: changed,
            step_changed: changed,
            ..Self::default()
        }
    }

    pub(super) fn merge(&mut self, other: Self) {
        self.state_changed |= other.state_changed;
        self.config_changed |= other.config_changed;
        self.step_changed |= other.step_changed;
    }
}

impl SetupSession {
    pub(super) fn new(
        config: ProjectConfig,
        available_devices: Vec<AvailableDevice>,
        output_path: PathBuf,
        resume_mode: bool,
        warnings: Vec<String>,
    ) -> Self {
        let teleop_pairing_cache = if config.pairings.is_empty() {
            build_default_channel_pairings(&config.devices)
        } else {
            config.pairings.clone()
        };
        Self {
            current_step: if resume_mode {
                SetupStep::Preview
            } else {
                SetupStep::Devices
            },
            config,
            available_devices,
            teleop_pairing_cache,
            identify_device_name: None,
            subpanel_target_name: None,
            output_path,
            resume_mode,
            warnings,
            message: None,
            status: SetupUiStatus::Editing,
            completed_at: None,
            exit_kind: None,
        }
    }

    pub(super) fn build_state_json(&self) -> Result<String, Box<dyn Error>> {
        Ok(serde_json::to_string(&SetupStateEnvelope {
            msg_type: "setup_state",
            step: self.current_step,
            step_index: self.current_step_index(),
            step_name: self.current_step.label(),
            total_steps: self.total_steps(),
            output_path: self.output_path.display().to_string(),
            resume_mode: self.resume_mode,
            status: self.status,
            message: self.message.clone(),
            identify_device: self.identify_device_name.clone(),
            subpanel_target: self.subpanel_target_name.clone(),
            warnings: self.warnings.clone(),
            config: self.config.clone(),
            available_devices: self.available_devices.clone(),
        })?)
    }

    pub(super) fn should_exit(&self) -> bool {
        self.completed_at
            .is_some_and(|completed_at| completed_at.elapsed() >= SETUP_UI_SUCCESS_DELAY)
    }

    pub(super) fn mark_saved(&mut self) {
        self.status = SetupUiStatus::Saved;
        self.message = Some(format!("Saved {}", self.output_path.display()));
        self.completed_at = Some(Instant::now());
        self.exit_kind = Some(SetupExitKind::Saved);
    }

    pub(super) fn mark_cancelled(&mut self) {
        self.status = SetupUiStatus::Cancelled;
        self.message = Some("Setup cancelled".into());
        self.completed_at = Some(Instant::now());
        self.exit_kind = Some(SetupExitKind::Cancelled);
    }

    pub(super) fn clear_identify_message(&mut self) -> bool {
        if self
            .message
            .as_deref()
            .is_some_and(|message| message.starts_with(IDENTIFY_ACTIVE_MESSAGE_PREFIX))
        {
            self.message = None;
            return true;
        }
        false
    }

    pub(super) fn clear_identify_state(&mut self) -> bool {
        let had_identify_target = self.identify_device_name.take().is_some();
        self.clear_identify_message() || had_identify_target
    }

    pub(super) fn visible_steps(&self) -> &'static [SetupStep] {
        // Final 4-step flow is Devices → Pairings → Settings → Overview.
        // States is still a separate step today; it folds into the Step 1
        // channel subpanel in a follow-up. Pairing must precede States so
        // selecting a teleop mapping locks in `leader_state` before the
        // operator can toggle state kinds (and so the States step can
        // refuse to drop a kind a live pairing depends on).
        if self.config.mode == CollectionMode::Teleop {
            &[
                SetupStep::Devices,
                SetupStep::Pairing,
                SetupStep::States,
                SetupStep::Storage,
                SetupStep::Preview,
            ]
        } else {
            &[
                SetupStep::Devices,
                SetupStep::States,
                SetupStep::Storage,
                SetupStep::Preview,
            ]
        }
    }

    pub(super) fn current_step_index(&self) -> usize {
        self.visible_steps()
            .iter()
            .position(|step| *step == self.current_step)
            .map(|index| index + 1)
            .unwrap_or(1)
    }

    pub(super) fn total_steps(&self) -> usize {
        self.visible_steps().len()
    }

    pub(super) fn advance_step(&mut self) -> bool {
        let steps = self.visible_steps();
        let index = steps
            .iter()
            .position(|step| *step == self.current_step)
            .unwrap_or(0);
        let next = steps[(index + 1).min(steps.len() - 1)];
        let changed = self.current_step != next;
        self.current_step = next;
        if self.current_step != SetupStep::Devices {
            self.clear_identify_state();
            self.subpanel_target_name = None;
        }
        changed
    }

    pub(super) fn retreat_step(&mut self) -> bool {
        let steps = self.visible_steps();
        let index = steps
            .iter()
            .position(|step| *step == self.current_step)
            .unwrap_or(0);
        let previous = if index == 0 {
            steps[0]
        } else {
            steps[index - 1]
        };
        let changed = self.current_step != previous;
        self.current_step = previous;
        if self.current_step != SetupStep::Devices {
            self.clear_identify_state();
            self.subpanel_target_name = None;
        }
        changed
    }

    pub(super) fn ensure_visible_current_step(&mut self) {
        if self.current_step == SetupStep::Pairing && self.config.mode != CollectionMode::Teleop {
            self.current_step = SetupStep::Storage;
        }
        if self.current_step != SetupStep::Devices {
            self.clear_identify_state();
            self.subpanel_target_name = None;
        }
    }

    /// Prune any operator-created pair whose leader/follower channel was
    /// disabled or removed in another step. Pairs the operator did NOT
    /// create are no longer auto-rebuilt: the wizard's pairing step now
    /// requires manual `setup_create_pairing` commands. This call is
    /// invoked from `toggle_device_selection` so a disabled channel can't
    /// dangle in `config.pairings`.
    pub(super) fn prune_invalid_pairings(&mut self) {
        let device_has_enabled_channel = |device_name: &str, channel_type: &str| -> bool {
            self.config.devices.iter().any(|device| {
                device.name == device_name
                    && device
                        .channels
                        .iter()
                        .any(|channel| channel.channel_type == channel_type && channel.enabled)
            })
        };
        self.config.pairings.retain(|pair| {
            device_has_enabled_channel(&pair.leader_device, &pair.leader_channel_type)
                && device_has_enabled_channel(&pair.follower_device, &pair.follower_channel_type)
        });
        // Teleop is now the only collection mode the wizard exposes, and
        // teleop with zero pairings is a valid intermediate state — the
        // operator may have just deleted their last pair before they
        // create a new one. Keep `mode` as-is and let downstream consumers
        // (`teleop_runtime_configs_v2`) emit zero teleop runtimes when
        // pairings are empty.
        self.teleop_pairing_cache = self.config.pairings.clone();
    }

    pub(super) fn available_device_mut(&mut self, name: &str) -> Option<&mut AvailableDevice> {
        self.available_devices
            .iter_mut()
            .find(|device| device.name == name)
    }

    pub(super) fn available_device(&self, name: &str) -> Option<&AvailableDevice> {
        self.available_devices
            .iter()
            .find(|device| device.name == name)
    }

    pub(super) fn configured_device_channel_index(&self, name: &str) -> Option<(usize, usize)> {
        let identity = self
            .available_device(name)
            .map(|device| device_identity_from_binary(&device.current))?;
        self.config
            .devices
            .iter()
            .enumerate()
            .find_map(|(device_index, device)| {
                if device.driver != identity.driver || device.id != identity.id {
                    return None;
                }
                device
                    .channels
                    .iter()
                    .enumerate()
                    .find(|(_, channel)| {
                        channel.kind == identity.device_type
                            && channel.channel_type == identity.channel_type
                    })
                    .map(|(channel_index, _)| (device_index, channel_index))
            })
    }

    pub(super) fn selected_device_index(&self, name: &str) -> Option<(usize, usize)> {
        self.configured_device_channel_index(name)
            .filter(|(device_index, channel_index)| {
                self.config.devices[*device_index].channels[*channel_index].enabled
            })
    }

    pub(super) fn is_device_selected(&self, name: &str) -> bool {
        self.selected_device_index(name).is_some()
    }
}

pub(super) fn rotate_index(current_index: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    let len = len as i32;
    (((current_index as i32 + delta) % len) + len) as usize % len as usize
}

pub(super) fn device_identity_from_binary(device: &BinaryDeviceConfig) -> DeviceIdentity {
    let ch = device
        .channels
        .first()
        .expect("setup devices always include a primary channel");
    DeviceIdentity {
        device_type: ch.kind,
        driver: device.driver.clone(),
        id: device.id.clone(),
        channel_type: ch.channel_type.clone(),
    }
}
