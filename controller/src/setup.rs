#![allow(dead_code)]

use crate::cli::SetupArgs;
use crate::discovery::{discover_probe_entries, run_driver_json, DiscoveryOptions};
use crate::process::{
    poll_children_once, spawn_child, terminate_children, ChildSpec, ManagedChild,
};
use crate::runtime_paths::{
    current_executable_dir, default_device_executable_name, resolve_registered_program,
    resolve_share_root, resolve_state_dir, workspace_root,
};
use crate::runtime_plan::{build_control_server_spec, build_preview_specs};
use iceoryx2::prelude::*;
use rollio_bus::{
    channel_mode_control_service_name, CONTROL_EVENTS_SERVICE, SETUP_COMMAND_SERVICE,
    SETUP_STATE_SERVICE,
};
use rollio_types::config::{
    BinaryDeviceConfig, CameraChannelProfile, ChannelPairingConfig, CollectionMode,
    DeviceChannelConfigV2, DeviceType, EncoderBackend, EncoderCodec, EpisodeFormat,
    MappingStrategy, ProjectConfig, RobotCommandKind, RobotMode, RobotStateKind, StorageBackend,
};
use rollio_types::messages::{
    ControlEvent, DeviceChannelMode, PixelFormat, SetupCommandMessage, SetupStateMessage,
    MAX_PARALLEL,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

#[cfg(test)]
use crate::discovery::known_device_executables;

const DISCOVERY_TIMEOUT: Duration = Duration::from_millis(2_000);
const VALIDATION_TIMEOUT: Duration = Duration::from_millis(1_000);
const SETUP_POLL_INTERVAL: Duration = Duration::from_millis(50);
const SETUP_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);
const SETUP_STATE_MAX_AGE: Duration = Duration::from_millis(500);
const SETUP_UI_SUCCESS_DELAY: Duration = Duration::from_millis(300);
const IDENTIFY_ACTIVE_MESSAGE_PREFIX: &str = "Identify active for ";
const SETUP_DEV_RUNTIME_PACKAGES: &[&str] = &[
    "rollio-ui-server",
    "rollio-visualizer",
    "rollio-control-server",
    "rollio-device-v4l2",
    "rollio-device-airbot-play",
    "rollio-device-pseudo",
];

type SetupDeviceChannel = (String, String);
type TeleopPairEndpoints = (SetupDeviceChannel, SetupDeviceChannel);

fn dev_build_profile(workspace_root: &Path, current_exe_dir: &Path) -> Option<&'static str> {
    let target_root = workspace_root.join("target");
    if current_exe_dir == target_root.join("release") {
        Some("release")
    } else if current_exe_dir == target_root.join("debug") {
        Some("debug")
    } else {
        None
    }
}

fn ensure_setup_dev_runtime_binaries_built(
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

#[derive(Debug, Clone, Serialize)]
struct DiscoveredDevice {
    driver: String,
    id: String,
    /// Device-level display label provided by the executable (e.g.
    /// "AIRBOT Play", or the V4L2 capabilities name). Used as the per-row
    /// label fallback when a channel does not provide its own label.
    display_name: String,
    /// Default user-facing name for the device row when the wizard collapses
    /// channels into one entry. Sourced from the driver's
    /// `DeviceQueryDevice.default_device_name`. Falls back to a snake-case
    /// driver name in the wizard.
    default_device_name: Option<String>,
    /// Per-channel metadata keyed by `channel_type`. Holds everything the
    /// wizard / setup config needs (kind, modes, dof, profiles, defaults,
    /// value_limits, direct_joint_compatibility, ...).
    channel_meta_by_channel: BTreeMap<String, DiscoveredChannelMeta>,
    /// Generic device-level metadata mirroring the driver's
    /// `optional_info` (transport, interface, product_variant, end_effector).
    /// Persisted into `BinaryDeviceConfig.extra` so downstream consumers
    /// stay schema-driven; new keys flow through automatically without
    /// controller changes.
    transport: Option<String>,
    interface: Option<String>,
    product_variant: Option<String>,
    end_effector: Option<String>,
}

impl DiscoveredDevice {
    /// "Primary" kind for compatibility with the legacy single-row UI: a
    /// device counts as a robot if any of its channels is a robot kind,
    /// otherwise camera. Used only by the wizard's row-rendering paths;
    /// authoritative kind lives on each channel.
    fn primary_device_type(&self) -> DeviceType {
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
    fn all_camera_profiles(&self) -> Vec<CameraProfile> {
        self.channel_meta_by_channel
            .values()
            .filter(|meta| meta.kind == DeviceType::Camera)
            .flat_map(|meta| meta.profiles.iter().cloned())
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Default)]
struct DiscoveredChannelMeta {
    /// `kind` per channel from `query --json`: camera, robot, etc.
    /// Defaults to `Robot` to keep older fixtures working unchanged.
    #[serde(default = "default_channel_kind")]
    kind: DeviceType,
    channel_label: Option<String>,
    default_name: Option<String>,
    /// Robot modes the driver accepts on this channel. For camera channels
    /// this is conventionally `["enabled", "disabled"]`; the controller no
    /// longer cares about exact strings here — it just maps known ones to
    /// `RobotMode` enum variants.
    #[serde(default)]
    modes: Vec<RobotMode>,
    /// `dof` reported per channel; only meaningful for robot kinds.
    #[serde(default)]
    dof: Option<u32>,
    /// Camera profiles reported per channel. Empty for non-camera kinds.
    #[serde(default)]
    profiles: Vec<CameraProfile>,
    /// Driver-suggested default control frequency for this channel.
    #[serde(default)]
    default_control_frequency_hz: Option<f64>,
    /// Default command parameters (`joint_mit_kp/kd`, `parallel_mit_kp/kd`).
    /// Used to seed `DeviceChannelConfigV2.command_defaults` without any
    /// vendor-specific lookup table.
    #[serde(default)]
    defaults: rollio_types::config::ChannelCommandDefaults,
    /// Per-state value limits reported by the device driver (rad / rad·s⁻¹ /
    /// Nm / m for parallel kinds). Captured from the channel's `query --json`
    /// response so the visualizer can render limit-aware bars instead of
    /// guessing the value envelope.
    #[serde(default)]
    value_limits: Vec<rollio_types::config::StateValueLimitsEntry>,
    /// All `RobotStateKind` values this driver reports it can publish on
    /// this channel. The setup wizard's "States" sub-step uses this list
    /// to render the toggleable publish/recorded options. Falls back to
    /// `value_limits` keys when the driver doesn't populate it explicitly.
    #[serde(default)]
    supported_states: Vec<RobotStateKind>,
    /// Robot command kinds the driver advertises it accepts on this channel.
    /// Persisted on `DeviceChannelConfigV2.supported_commands` so downstream
    /// teleop / pairing logic stays driver-agnostic.
    #[serde(default)]
    supported_commands: Vec<rollio_types::config::RobotCommandKind>,
    /// Direct-joint pairing peers as reported by the driver. Persisted on
    /// `DeviceChannelConfigV2.direct_joint_compatibility` so pairing
    /// validation can consult the schema instead of any vendor table.
    #[serde(default)]
    direct_joint_compatibility: rollio_types::config::DirectJointCompatibility,
}

fn default_channel_kind() -> DeviceType {
    DeviceType::Robot
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CameraProfile {
    width: u32,
    height: u32,
    fps: u32,
    pixel_format: PixelFormat,
    native_pixel_format: Option<String>,
    stream: Option<String>,
    channel: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
struct AvailableDevice {
    name: String,
    display_name: String,
    device_type: DeviceType,
    driver: String,
    id: String,
    camera_profiles: Vec<CameraProfile>,
    supported_modes: Vec<RobotMode>,
    /// All `RobotStateKind` values the driver advertises it can publish on
    /// this channel. The setup wizard's "States" sub-step uses this list to
    /// render the toggleable publish/recorded options. Empty for camera
    /// channels.
    #[serde(default)]
    supported_states: Vec<RobotStateKind>,
    /// Single-binary snapshot for this discovery row (one channel).
    current: BinaryDeviceConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeviceIdentity {
    device_type: DeviceType,
    driver: String,
    id: String,
    /// Logical camera/robot channel id (`color`, `arm`, `infrared_1`, …).
    channel_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SetupStep {
    Devices,
    States,
    Pairing,
    Storage,
    Preview,
}

impl SetupStep {
    fn label(self) -> &'static str {
        match self {
            Self::Devices => "Devices",
            Self::States => "States",
            Self::Pairing => "Pairing",
            Self::Storage => "Settings",
            Self::Preview => "Preview",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SetupUiStatus {
    Editing,
    Saved,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupExitKind {
    Saved,
    Cancelled,
}

#[derive(Debug)]
struct SetupSession {
    config: ProjectConfig,
    available_devices: Vec<AvailableDevice>,
    teleop_pairing_cache: Vec<ChannelPairingConfig>,
    identify_device_name: Option<String>,
    current_step: SetupStep,
    output_path: PathBuf,
    resume_mode: bool,
    warnings: Vec<String>,
    message: Option<String>,
    status: SetupUiStatus,
    completed_at: Option<Instant>,
    exit_kind: Option<SetupExitKind>,
}

#[derive(Debug, Deserialize)]
struct SetupCommandEnvelope {
    #[serde(rename = "type")]
    msg_type: String,
    action: String,
    name: Option<String>,
    index: Option<usize>,
    delta: Option<i32>,
    value: Option<String>,
}

#[derive(Debug, Serialize)]
struct SetupStateEnvelope {
    #[serde(rename = "type")]
    msg_type: &'static str,
    step: SetupStep,
    step_index: usize,
    step_name: &'static str,
    total_steps: usize,
    output_path: String,
    resume_mode: bool,
    status: SetupUiStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    identify_device: Option<String>,
    warnings: Vec<String>,
    config: ProjectConfig,
    available_devices: Vec<AvailableDevice>,
}

#[derive(Debug, Default, Clone, Copy)]
struct SessionMutation {
    state_changed: bool,
    config_changed: bool,
    step_changed: bool,
}

impl SessionMutation {
    fn state_only(changed: bool) -> Self {
        Self {
            state_changed: changed,
            ..Self::default()
        }
    }

    fn config_changed(changed: bool) -> Self {
        Self {
            state_changed: changed,
            config_changed: changed,
            ..Self::default()
        }
    }

    fn step_changed(changed: bool) -> Self {
        Self {
            state_changed: changed,
            step_changed: changed,
            ..Self::default()
        }
    }

    fn merge(&mut self, other: Self) {
        self.state_changed |= other.state_changed;
        self.config_changed |= other.config_changed;
        self.step_changed |= other.step_changed;
    }
}

#[derive(Debug)]
struct SetupRuntimeState {
    children: Vec<ManagedChild>,
    temp_config_path: Option<PathBuf>,
    preview_target_name: Option<String>,
}

impl SetupSession {
    fn new(
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
            output_path,
            resume_mode,
            warnings,
            message: None,
            status: SetupUiStatus::Editing,
            completed_at: None,
            exit_kind: None,
        }
    }

    fn build_state_json(&self) -> Result<String, Box<dyn Error>> {
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
            warnings: self.warnings.clone(),
            config: self.config.clone(),
            available_devices: self.available_devices.clone(),
        })?)
    }

    fn should_exit(&self) -> bool {
        self.completed_at
            .is_some_and(|completed_at| completed_at.elapsed() >= SETUP_UI_SUCCESS_DELAY)
    }

    fn mark_saved(&mut self) {
        self.status = SetupUiStatus::Saved;
        self.message = Some(format!("Saved {}", self.output_path.display()));
        self.completed_at = Some(Instant::now());
        self.exit_kind = Some(SetupExitKind::Saved);
    }

    fn mark_cancelled(&mut self) {
        self.status = SetupUiStatus::Cancelled;
        self.message = Some("Setup cancelled".into());
        self.completed_at = Some(Instant::now());
        self.exit_kind = Some(SetupExitKind::Cancelled);
    }

    fn clear_identify_message(&mut self) -> bool {
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

    fn clear_identify_state(&mut self) -> bool {
        let had_identify_target = self.identify_device_name.take().is_some();
        self.clear_identify_message() || had_identify_target
    }

    fn visible_steps(&self) -> &'static [SetupStep] {
        // Pairing must precede States: choosing a teleop mapping decides
        // which `leader_state` is required, and the States step refuses to
        // toggle off a kind that an active pairing depends on. Putting
        // States after Pairing surfaces those locked rows immediately
        // instead of forcing the operator to backtrack.
        if self.config.mode == CollectionMode::Teleop {
            &[
                SetupStep::Devices,
                SetupStep::Storage,
                SetupStep::Pairing,
                SetupStep::States,
                SetupStep::Preview,
            ]
        } else {
            &[
                SetupStep::Devices,
                SetupStep::Storage,
                SetupStep::States,
                SetupStep::Preview,
            ]
        }
    }

    fn current_step_index(&self) -> usize {
        self.visible_steps()
            .iter()
            .position(|step| *step == self.current_step)
            .map(|index| index + 1)
            .unwrap_or(1)
    }

    fn total_steps(&self) -> usize {
        self.visible_steps().len()
    }

    fn advance_step(&mut self) -> bool {
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
        }
        changed
    }

    fn retreat_step(&mut self) -> bool {
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
        }
        changed
    }

    fn ensure_visible_current_step(&mut self) {
        if self.current_step == SetupStep::Pairing && self.config.mode != CollectionMode::Teleop {
            self.current_step = SetupStep::Storage;
        }
        if self.current_step != SetupStep::Devices {
            self.clear_identify_state();
        }
    }

    /// Prune any operator-created pair whose leader/follower channel was
    /// disabled or removed in another step. Pairs the operator did NOT
    /// create are no longer auto-rebuilt: the wizard's pairing step now
    /// requires manual `setup_create_pairing` commands. This call is
    /// invoked from `toggle_device_selection` so a disabled channel can't
    /// dangle in `config.pairings`.
    fn prune_invalid_pairings(&mut self) {
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

    fn available_device_mut(&mut self, name: &str) -> Option<&mut AvailableDevice> {
        self.available_devices
            .iter_mut()
            .find(|device| device.name == name)
    }

    fn available_device(&self, name: &str) -> Option<&AvailableDevice> {
        self.available_devices
            .iter()
            .find(|device| device.name == name)
    }

    fn configured_device_channel_index(&self, name: &str) -> Option<(usize, usize)> {
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

    fn selected_device_index(&self, name: &str) -> Option<(usize, usize)> {
        self.configured_device_channel_index(name)
            .filter(|(device_index, channel_index)| {
                self.config.devices[*device_index].channels[*channel_index].enabled
            })
    }

    fn is_device_selected(&self, name: &str) -> bool {
        self.selected_device_index(name).is_some()
    }

    fn set_device_name(&mut self, name: &str, value: &str) -> Result<bool, Box<dyn Error>> {
        // The "rename" action now targets a single channel's user-facing
        // name; the BinaryDeviceConfig.name (= bus_root, iceoryx2 service
        // root, pairing key) is treated as an internal, immutable
        // identifier. This avoids the previous behavior where renaming the
        // arm row also renamed the e2 row because they shared the parent
        // BinaryDeviceConfig.name.
        let Some((selected_index, channel_index)) = self.selected_device_index(name) else {
            return Ok(false);
        };
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.message = Some("Channel name must not be empty.".into());
            return Ok(false);
        }

        // Uniqueness check: channel names must not collide across rows
        // (whether on the same device or another), excluding the row we are
        // editing.
        let duplicate_name = self.available_devices.iter().any(|device| {
            device.name != name
                && device
                    .current
                    .channels
                    .first()
                    .and_then(|channel| channel.name.as_deref())
                    .is_some_and(|existing| existing == trimmed)
        });
        if duplicate_name {
            self.message = Some(format!("Channel name \"{trimmed}\" is already in use."));
            return Ok(false);
        }

        let current_name = self.config.devices[selected_index].channels[channel_index]
            .name
            .clone();
        if current_name.as_deref() == Some(trimmed) {
            return Ok(false);
        }

        // Mirror the rename into both the persisted project config and the
        // matching available_device row's snapshot. We do NOT touch
        // BinaryDeviceConfig.name / bus_root or pairings.
        self.config.devices[selected_index].channels[channel_index].name = Some(trimmed.to_owned());
        if let Some(available) = self.available_device_mut(name) {
            if let Some(channel) = available.current.channels.first_mut() {
                channel.name = Some(trimmed.to_owned());
            }
        }
        self.config.validate()?;

        Ok(true)
    }

    fn set_identify_device(&mut self, name: Option<&str>) -> bool {
        if name.is_none() {
            return self.clear_identify_state();
        }
        if self.identify_device_name.as_deref() == name {
            return false;
        }
        self.identify_device_name = name.map(ToOwned::to_owned);
        if let Some(name) = name {
            if let Some(device) = self.available_device(name) {
                self.message = Some(format!(
                    "Identify active for {} ({})",
                    device.display_name, device.id
                ));
            }
        }
        true
    }

    fn cycle_device_profile(&mut self, name: &str, delta: i32) -> Result<bool, Box<dyn Error>> {
        let Some((device_index, channel_index)) = self.selected_device_index(name) else {
            return Ok(false);
        };
        let updated_current = {
            let Some(available) = self.available_device_mut(name) else {
                return Ok(false);
            };
            if available.camera_profiles.is_empty() {
                return Ok(false);
            }
            let Some(ch) = available.current.channels.first_mut() else {
                return Ok(false);
            };
            if ch.kind != DeviceType::Camera {
                return Ok(false);
            }
            let prof = ch.profile.as_ref();
            let current_profile = available
                .camera_profiles
                .iter()
                .position(|profile| {
                    prof.is_some_and(|p| {
                        p.width == profile.width
                            && p.height == profile.height
                            && p.fps == profile.fps
                            && p.pixel_format == profile.pixel_format
                            && p.native_pixel_format == profile.native_pixel_format
                    }) && camera_channel_type_for_profile(profile) == ch.channel_type
                })
                .unwrap_or(0);
            let next_index = rotate_index(current_profile, available.camera_profiles.len(), delta);
            let profile = available.camera_profiles[next_index].clone();
            ch.channel_type = camera_channel_type_for_profile(&profile);
            ch.profile = Some(CameraChannelProfile {
                width: profile.width,
                height: profile.height,
                fps: profile.fps,
                pixel_format: profile.pixel_format,
                native_pixel_format: profile.native_pixel_format.clone(),
            });
            available.current.clone()
        };
        self.config.devices[device_index].channels[channel_index] =
            updated_current.channels[0].clone();
        Ok(true)
    }

    fn cycle_robot_mode(&mut self, name: &str, delta: i32) -> Result<bool, Box<dyn Error>> {
        let Some((device_index, channel_index)) = self.selected_device_index(name) else {
            return Ok(false);
        };
        let updated_current = {
            let Some(available) = self.available_device_mut(name) else {
                return Ok(false);
            };
            let selectable = wizard_selectable_modes(&available.supported_modes);
            if selectable.is_empty() {
                return Ok(false);
            }
            let Some(ch) = available.current.channels.first_mut() else {
                return Ok(false);
            };
            if ch.kind != DeviceType::Robot {
                return Ok(false);
            }
            // Snap to the first selectable mode if the persisted mode (e.g.
            // a legacy `Disabled`/`Identifying`) isn't part of the cycle.
            let current_index = ch
                .mode
                .and_then(|mode| selectable.iter().position(|candidate| *candidate == mode))
                .unwrap_or(0);
            let next_index = rotate_index(current_index, selectable.len(), delta);
            ch.mode = Some(selectable[next_index]);
            available.current.clone()
        };
        self.config.devices[device_index].channels[channel_index] =
            updated_current.channels[0].clone();
        Ok(true)
    }

    fn toggle_device_selection(&mut self, name: &str) -> Result<bool, Box<dyn Error>> {
        if let Some((device_index, channel_index)) = self.selected_device_index(name) {
            let enabled_channels = self.config.devices[device_index]
                .channels
                .iter()
                .filter(|channel| channel.enabled)
                .count();
            if enabled_channels <= 1 {
                self.config.devices.remove(device_index);
            } else {
                self.config.devices[device_index].channels[channel_index].enabled = false;
            }
            if self.identify_device_name.as_deref() == Some(name) {
                self.clear_identify_state();
            }
            self.prune_invalid_pairings();
            self.config.validate()?;
            return Ok(true);
        }

        let Some(available) = self
            .available_devices
            .iter()
            .find(|device| device.name == name)
            .cloned()
        else {
            return Ok(false);
        };
        if let Some((device_index, channel_index)) = self.configured_device_channel_index(name) {
            self.config.devices[device_index].channels[channel_index] =
                available.current.channels[0].clone();
            self.config.devices[device_index].channels[channel_index].enabled = true;
        } else if let Some(device) = self.build_selected_device_from_available(name) {
            self.config.devices.push(device);
        } else {
            return Ok(false);
        }
        self.prune_invalid_pairings();
        self.config.validate()?;
        Ok(true)
    }

    /// Flip whether `state_kind` appears in the addressed channel's
    /// `publish_states`. When turning a kind off, also drop it from
    /// `recorded_states` to preserve the subset invariant. Reject the
    /// toggle if any active pairing currently relies on the kind as
    /// `leader_state` (we can't quietly break a configured teleop pair).
    fn toggle_publish_state(
        &mut self,
        name: &str,
        state_kind: RobotStateKind,
    ) -> Result<bool, Box<dyn Error>> {
        let Some((device_index, channel_index)) = self.selected_device_index(name) else {
            return Ok(false);
        };
        if self.config.devices[device_index].channels[channel_index].kind != DeviceType::Robot {
            return Ok(false);
        }
        let device_name = self.config.devices[device_index].name.clone();
        let channel_type = self.config.devices[device_index].channels[channel_index]
            .channel_type
            .clone();
        let supported_states: Vec<RobotStateKind> = self
            .available_devices
            .iter()
            .find(|available| available.name == name)
            .map(|available| available.supported_states.clone())
            .unwrap_or_default();

        let channel = &mut self.config.devices[device_index].channels[channel_index];
        let currently_enabled = channel.publish_states.contains(&state_kind);
        if currently_enabled {
            // Block removal if a pairing depends on this state as its
            // leader_state.
            if let Some(pairing) = self.config.pairings.iter().find(|pair| {
                pair.leader_device == device_name
                    && pair.leader_channel_type == channel_type
                    && pair.leader_state == state_kind
            }) {
                self.message = Some(format!(
                    "{:?} is required by pairing {}:{} (leader_state); change the pairing first.",
                    state_kind, pairing.leader_device, pairing.leader_channel_type
                ));
                return Ok(false);
            }
            channel.publish_states.retain(|kind| *kind != state_kind);
            channel.recorded_states.retain(|kind| *kind != state_kind);
        } else {
            // Refuse to toggle on a kind the driver doesn't advertise so the
            // wizard cannot publish unsupported topics.
            if !supported_states.is_empty() && !supported_states.contains(&state_kind) {
                self.message = Some(format!(
                    "{:?} is not advertised as supported by this device.",
                    state_kind
                ));
                return Ok(false);
            }
            channel.publish_states.push(state_kind);
        }
        self.config.validate()?;
        // Mirror the latest publish/recorded sets into the AvailableDevice
        // snapshot the wizard UI renders from. The wizard otherwise keeps
        // showing the stale glyphs because every other toggle in the
        // session writes through both `config.devices` and
        // `available_devices` (see `cycle_robot_mode`).
        self.sync_available_channel_state_lists(name, device_index, channel_index);
        // Pairing defaults can change once the publish set changes (e.g.
        // `parallel_position` becoming available enables parallel teleop).
        self.teleop_pairing_cache = self.config.pairings.clone();
        Ok(true)
    }

    /// Flip whether `state_kind` appears in the addressed channel's
    /// `recorded_states`. The validator already enforces
    /// `recorded_states ⊆ publish_states`; we surface a clearer message
    /// here when the operator tries to record a kind that isn't being
    /// published.
    fn toggle_recorded_state(
        &mut self,
        name: &str,
        state_kind: RobotStateKind,
    ) -> Result<bool, Box<dyn Error>> {
        let Some((device_index, channel_index)) = self.selected_device_index(name) else {
            return Ok(false);
        };
        if self.config.devices[device_index].channels[channel_index].kind != DeviceType::Robot {
            return Ok(false);
        }
        let channel = &mut self.config.devices[device_index].channels[channel_index];
        let currently_enabled = channel.recorded_states.contains(&state_kind);
        if currently_enabled {
            channel.recorded_states.retain(|kind| *kind != state_kind);
        } else {
            if !channel.publish_states.contains(&state_kind) {
                self.message = Some(format!(
                    "{:?} must be published before it can be recorded.",
                    state_kind
                ));
                return Ok(false);
            }
            channel.recorded_states.push(state_kind);
        }
        self.config.validate()?;
        self.sync_available_channel_state_lists(name, device_index, channel_index);
        Ok(true)
    }

    /// Copy `publish_states` / `recorded_states` from
    /// `self.config.devices[device_index].channels[channel_index]` onto the
    /// matching `AvailableDevice.current` snapshot so the wizard UI sees
    /// the freshest values on the next state publish.
    fn sync_available_channel_state_lists(
        &mut self,
        name: &str,
        device_index: usize,
        channel_index: usize,
    ) {
        let publish_states = self.config.devices[device_index].channels[channel_index]
            .publish_states
            .clone();
        let recorded_states = self.config.devices[device_index].channels[channel_index]
            .recorded_states
            .clone();
        let Some(available) = self.available_device_mut(name) else {
            return;
        };
        let Some(channel) = available.current.channels.first_mut() else {
            return;
        };
        channel.publish_states = publish_states;
        channel.recorded_states = recorded_states;
    }

    fn build_selected_device_from_available(&self, name: &str) -> Option<BinaryDeviceConfig> {
        let target = self.available_device(name)?;
        let mut device = target.current.clone();
        let enabled_channel = device.channels.first()?.channel_type.clone();
        let mut channels = self
            .available_devices
            .iter()
            .filter(|available| available.driver == target.driver && available.id == target.id)
            .filter_map(|available| available.current.channels.first().cloned())
            .collect::<Vec<_>>();
        channels.sort_by(|left, right| left.channel_type.cmp(&right.channel_type));
        for channel in &mut channels {
            channel.enabled = channel.channel_type == enabled_channel;
        }
        device.channels = channels;
        Some(device)
    }

    fn cycle_pair_mapping(&mut self, index: usize, delta: i32) -> Result<bool, Box<dyn Error>> {
        let Some(snapshot) = self.config.pairings.get(index).cloned() else {
            return Ok(false);
        };
        let (leader_device, leader_channel_type, follower_device, follower_channel_type) = (
            snapshot.leader_device.clone(),
            snapshot.leader_channel_type.clone(),
            snapshot.follower_device.clone(),
            snapshot.follower_channel_type.clone(),
        );
        let current_mapping = snapshot.mapping;
        let follower_dof_hint = self
            .config
            .device_named(&follower_device)
            .and_then(|device| device.channel_named(&follower_channel_type))
            .and_then(|channel| channel.dof)
            .unwrap_or(1);
        let leader_ch = self
            .config
            .device_named(&leader_device)
            .and_then(|d| d.channel_named(&leader_channel_type));
        let follower_ch = self
            .config
            .device_named(&follower_device)
            .and_then(|d| d.channel_named(&follower_channel_type));
        let parallel = leader_ch.is_some_and(channel_uses_parallel_teleop)
            && follower_ch.is_some_and(channel_uses_parallel_teleop);
        let options = [MappingStrategy::DirectJoint, MappingStrategy::Cartesian];
        let current_index = options
            .iter()
            .position(|mapping| *mapping == current_mapping)
            .unwrap_or(0);
        let next_index = rotate_index(current_index, options.len(), delta);
        let next_mapping = options[next_index];
        // Apply the cycle on a clone first so we can roll back without
        // touching `self.config.devices`/`pairings` if the new mapping
        // doesn't validate (e.g. follower DOF doesn't match the leader's
        // for direct-joint, or leader doesn't publish EndEffectorPose for
        // cartesian). Without this, validation errors from the cycle
        // would bubble out of `apply_raw_command` and abort the wizard.
        {
            let pair = &mut self.config.pairings[index];
            pair.mapping = next_mapping;
            match pair.mapping {
                MappingStrategy::DirectJoint => {
                    if parallel {
                        pair.leader_state = RobotStateKind::ParallelPosition;
                        pair.follower_command = RobotCommandKind::ParallelMit;
                        let map_len = follower_dof_hint.min(MAX_PARALLEL as u32);
                        pair.joint_index_map = (0..map_len).collect();
                        pair.joint_scales = vec![1.0; map_len as usize];
                    } else {
                        pair.leader_state = RobotStateKind::JointPosition;
                        pair.follower_command = RobotCommandKind::JointPosition;
                        pair.joint_index_map = (0..follower_dof_hint).collect();
                        pair.joint_scales = vec![1.0; follower_dof_hint as usize];
                    }
                }
                MappingStrategy::Cartesian => {
                    pair.leader_state = RobotStateKind::EndEffectorPose;
                    pair.follower_command = RobotCommandKind::EndPose;
                    pair.joint_index_map.clear();
                    pair.joint_scales.clear();
                }
            }
        }
        let leader_state = self.config.pairings[index].leader_state;
        // Cartesian (FK/IK) mapping requires the leader to publish
        // EndEffectorPose; the channel may have been discovered without it,
        // so opt the leader's publish_states in here. (DirectJoint maps to
        // JointPosition / ParallelPosition which the discovery defaults
        // already include.)
        let publish_states_snapshot = self
            .config
            .device_named(&leader_device)
            .and_then(|d| d.channel_named(&leader_channel_type))
            .map(|ch| ch.publish_states.clone());
        ensure_channel_publishes_state(
            &mut self.config.devices,
            &leader_device,
            &leader_channel_type,
            leader_state,
        );
        if let Err(error) = self.config.validate() {
            // Roll back BOTH the pair mutation and the publish_states
            // opt-in so the wizard stays in a self-consistent state and
            // the operator can simply pick a different mapping.
            self.config.pairings[index] = snapshot;
            if let Some(states) = publish_states_snapshot {
                if let Some(device) = self
                    .config
                    .devices
                    .iter_mut()
                    .find(|d| d.name == leader_device)
                {
                    if let Some(channel) = device
                        .channels
                        .iter_mut()
                        .find(|c| c.channel_type == leader_channel_type)
                    {
                        channel.publish_states = states;
                    }
                }
            }
            self.teleop_pairing_cache = self.config.pairings.clone();
            self.message = Some(format!(
                "Cannot switch to {} mapping: {error}",
                match next_mapping {
                    MappingStrategy::DirectJoint => "direct-joint",
                    MappingStrategy::Cartesian => "cartesian",
                }
            ));
            return Ok(false);
        }
        self.teleop_pairing_cache = self.config.pairings.clone();
        Ok(true)
    }

    /// Build a `(device_name, channel_type)` list of every enabled robot
    /// channel whose driver advertises **either** `FreeDrive` **or**
    /// `CommandFollowing`, with the pair at `except_pair_index` excluded
    /// from the no-self-loop / uniqueness checks (so editing a leader
    /// doesn't filter out the channel that pair already uses). This is
    /// the eligibility predicate for a teleop **leader**.
    ///
    /// Leaders are sources of motion the follower mirrors. The operator
    /// reads the leader's joint state via the device driver — that's the
    /// only requirement on the driver's side. A passive EEF (e.g. AIRBOT
    /// E2) with only `FreeDrive` qualifies as a leader because the
    /// operator manually moves it and the controller forwards the
    /// observed positions; an actuated arm (G2 / Play arm) qualifies
    /// because either mode lets the controller observe joint state.
    ///
    /// Leaders may be shared across pairs (one channel can demonstrate
    /// motion to multiple followers), so the only per-pair constraint
    /// applied here is the self-loop guard: the candidate leader must
    /// differ from the targeted pair's current follower.
    fn eligible_leader_channels(&self, except_pair_index: Option<usize>) -> Vec<(String, String)> {
        let blocked_self = except_pair_index
            .and_then(|idx| self.config.pairings.get(idx))
            .map(|pair| {
                (
                    pair.follower_device.clone(),
                    pair.follower_channel_type.clone(),
                )
            });
        self.eligible_channels(|modes| {
            modes.contains(&RobotMode::FreeDrive) || modes.contains(&RobotMode::CommandFollowing)
        })
        .into_iter()
        .filter(|candidate| Some(candidate) != blocked_self.as_ref())
        .collect()
    }

    /// `(device_name, channel_type)` list of every enabled robot channel
    /// whose driver advertises `CommandFollowing`, with the pair at
    /// `except_pair_index` excluded from the per-pair uniqueness checks.
    ///
    /// Followers must be unique across pairings — a single physical
    /// follower can't be driven by two different leaders simultaneously
    /// — and a follower cannot collapse onto its own pair's leader.
    fn eligible_follower_channels(
        &self,
        except_pair_index: Option<usize>,
    ) -> Vec<(String, String)> {
        let blocked_self = except_pair_index
            .and_then(|idx| self.config.pairings.get(idx))
            .map(|pair| (pair.leader_device.clone(), pair.leader_channel_type.clone()));
        let claimed_followers: BTreeMap<(String, String), ()> = self
            .config
            .pairings
            .iter()
            .enumerate()
            .filter_map(|(idx, pair)| {
                if Some(idx) == except_pair_index {
                    return None;
                }
                Some((
                    (
                        pair.follower_device.clone(),
                        pair.follower_channel_type.clone(),
                    ),
                    (),
                ))
            })
            .collect();
        self.eligible_channels(|modes| modes.contains(&RobotMode::CommandFollowing))
            .into_iter()
            .filter(|candidate| Some(candidate) != blocked_self.as_ref())
            .filter(|candidate| !claimed_followers.contains_key(candidate))
            .collect()
    }

    fn eligible_channels(&self, predicate: impl Fn(&[RobotMode]) -> bool) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for device in &self.config.devices {
            for channel in &device.channels {
                if !channel.enabled || channel.kind != DeviceType::Robot {
                    continue;
                }
                let supported = self
                    .available_devices
                    .iter()
                    .find(|available| {
                        available.driver == device.driver
                            && available.id == device.id
                            && available
                                .current
                                .channels
                                .first()
                                .is_some_and(|ch| ch.channel_type == channel.channel_type)
                    })
                    .map(|available| available.supported_modes.as_slice())
                    .unwrap_or(&[]);
                if predicate(supported) {
                    out.push((device.name.clone(), channel.channel_type.clone()));
                }
            }
        }
        out
    }

    /// Push a new pair into `config.pairings`. When `explicit` is `Some`,
    /// uses the operator-supplied `(leader, follower)`; otherwise falls
    /// back to the first eligible `(leader, follower)` combo that obeys
    /// the cross-pair uniqueness rules. Returns the new pair's index so
    /// the UI can immediately focus the row.
    ///
    /// The wizard's modal pairing picker invokes this with `explicit =
    /// Some(...)` after the operator has chosen both endpoints in the
    /// sub-step (deferred creation). The implicit form is kept for
    /// backwards compatibility and tests.
    fn create_pairing(
        &mut self,
        explicit: Option<TeleopPairEndpoints>,
    ) -> Result<Option<usize>, Box<dyn Error>> {
        let ((leader_device, leader_channel_type), (follower_device, follower_channel_type)) =
            match explicit {
                Some(pair) => pair,
                None => match self.pick_default_pair_endpoints()? {
                    Some(pair) => pair,
                    None => return Ok(None),
                },
            };

        // Validate eligibility against the live config so an explicit
        // request from the UI can't smuggle a stale or ineligible
        // channel past us (e.g. operator deselected a channel in step 1
        // while the picker was still open).
        let eligible_leaders = self.eligible_leader_channels(None);
        let eligible_followers = self.eligible_follower_channels(None);
        let leader_target = (leader_device.clone(), leader_channel_type.clone());
        let follower_target = (follower_device.clone(), follower_channel_type.clone());
        if !eligible_leaders.contains(&leader_target) {
            self.message = Some(format!(
                "Leader {}:{} is no longer eligible (channel may have been disabled in step 1 or already be a self-loop with the chosen follower).",
                leader_device, leader_channel_type,
            ));
            return Ok(None);
        }
        if !eligible_followers.contains(&follower_target) {
            self.message = Some(format!(
                "Follower {}:{} is no longer eligible (channel may have been disabled in step 1 or already follow another leader).",
                follower_device, follower_channel_type,
            ));
            return Ok(None);
        }
        if leader_target == follower_target {
            self.message = Some("Leader and follower channel must differ.".into());
            return Ok(None);
        }

        let pair = pairing_from_channels(
            &self.config.devices,
            &leader_device,
            &leader_channel_type,
            &follower_device,
            &follower_channel_type,
        );
        let leader_state = pair.leader_state;
        self.config.pairings.push(pair);
        ensure_channel_publishes_state(
            &mut self.config.devices,
            &leader_device,
            &leader_channel_type,
            leader_state,
        );
        // Teleop is already the only collection mode the wizard exposes;
        // creating a pair doesn't need to mutate `config.mode`.
        self.teleop_pairing_cache = self.config.pairings.clone();
        let new_index = self.config.pairings.len() - 1;
        if let Err(error) = self.config.validate() {
            // Roll back if validation fails (e.g. duplicate pair).
            self.config.pairings.pop();
            self.teleop_pairing_cache = self.config.pairings.clone();
            self.message = Some(format!("Could not create pairing: {error}"));
            return Ok(None);
        }
        Ok(Some(new_index))
    }

    /// Pick a default `(leader, follower)` for a brand-new pair using
    /// the same eligibility filters the picker uses. Used as the
    /// fallback when the UI doesn't supply explicit endpoints.
    fn pick_default_pair_endpoints(
        &mut self,
    ) -> Result<Option<TeleopPairEndpoints>, Box<dyn Error>> {
        let leaders = self.eligible_leader_channels(None);
        if leaders.is_empty() {
            self.message = Some(
                "No eligible leader channel: at least one selected robot channel must support free-drive or command-following.".into(),
            );
            return Ok(None);
        }
        let followers = self.eligible_follower_channels(None);
        if followers.is_empty() {
            self.message = Some(
                "No eligible follower channel: at least one selected robot channel must support command-following and not already be a follower in another pair.".into(),
            );
            return Ok(None);
        }
        for leader in &leaders {
            for follower in &followers {
                if leader != follower {
                    return Ok(Some((leader.clone(), follower.clone())));
                }
            }
        }
        self.message = Some(
            "Could not seed a new pair: every eligible leader collapses onto an eligible follower (need at least two distinct robot channels).".into(),
        );
        Ok(None)
    }

    fn remove_pairing(&mut self, index: usize) -> Result<bool, Box<dyn Error>> {
        if index >= self.config.pairings.len() {
            return Ok(false);
        }
        self.config.pairings.remove(index);
        // Stay in teleop even when pairings reach zero: teleop is the only
        // mode the wizard exposes now, and the operator is expected to
        // create a new pair via `m` before saving for runtime use.
        self.teleop_pairing_cache = self.config.pairings.clone();
        self.config.validate()?;
        Ok(true)
    }

    fn set_pairing_endpoint(
        &mut self,
        index: usize,
        endpoint: PairingEndpoint,
        device: &str,
        channel_type: &str,
    ) -> Result<bool, Box<dyn Error>> {
        if index >= self.config.pairings.len() {
            return Ok(false);
        }
        // Pass the targeted pair index so the eligibility check excludes
        // *this* pair's existing endpoint from the no-self-loop /
        // uniqueness filters (otherwise re-confirming the current
        // selection during an edit would falsely register as a duplicate).
        let eligible = match endpoint {
            PairingEndpoint::Leader => self.eligible_leader_channels(Some(index)),
            PairingEndpoint::Follower => self.eligible_follower_channels(Some(index)),
        };
        let target = (device.to_owned(), channel_type.to_owned());
        if !eligible.contains(&target) {
            // Distinguish the "supported_modes don't match" case from the
            // "channel is already in use" case so the operator gets an
            // actionable message in both situations.
            let raw_pool = match endpoint {
                PairingEndpoint::Leader => self.eligible_channels(|modes| {
                    modes.contains(&RobotMode::FreeDrive)
                        || modes.contains(&RobotMode::CommandFollowing)
                }),
                PairingEndpoint::Follower => {
                    self.eligible_channels(|modes| modes.contains(&RobotMode::CommandFollowing))
                }
            };
            let known_channel = raw_pool.contains(&target);
            self.message = Some(if known_channel {
                match endpoint {
                    PairingEndpoint::Leader => format!(
                        "Leader {}:{} would self-loop with this pair's follower; pick a different leader.",
                        device, channel_type,
                    ),
                    PairingEndpoint::Follower => format!(
                        "Follower {}:{} is already used by another pair (or matches this pair's leader); pick a different follower.",
                        device, channel_type,
                    ),
                }
            } else {
                format!(
                    "{} {}:{} is not eligible (channel must support {}).",
                    match endpoint {
                        PairingEndpoint::Leader => "Leader",
                        PairingEndpoint::Follower => "Follower",
                    },
                    device,
                    channel_type,
                    match endpoint {
                        PairingEndpoint::Leader => "free-drive or command-following",
                        PairingEndpoint::Follower => "command-following",
                    },
                )
            });
            return Ok(false);
        }
        let (leader_device, leader_channel_type, follower_device, follower_channel_type) = {
            let pair = &mut self.config.pairings[index];
            match endpoint {
                PairingEndpoint::Leader => {
                    pair.leader_device = device.to_owned();
                    pair.leader_channel_type = channel_type.to_owned();
                }
                PairingEndpoint::Follower => {
                    pair.follower_device = device.to_owned();
                    pair.follower_channel_type = channel_type.to_owned();
                }
            }
            (
                pair.leader_device.clone(),
                pair.leader_channel_type.clone(),
                pair.follower_device.clone(),
                pair.follower_channel_type.clone(),
            )
        };
        // Re-derive mapping/leader_state/follower_command from the new
        // leader/follower combo so the pair stays internally consistent.
        let rebuilt = pairing_from_channels(
            &self.config.devices,
            &leader_device,
            &leader_channel_type,
            &follower_device,
            &follower_channel_type,
        );
        self.config.pairings[index] = rebuilt;
        let leader_state = self.config.pairings[index].leader_state;
        ensure_channel_publishes_state(
            &mut self.config.devices,
            &leader_device,
            &leader_channel_type,
            leader_state,
        );
        self.teleop_pairing_cache = self.config.pairings.clone();
        self.config.validate()?;
        Ok(true)
    }

    fn cycle_episode_format(&mut self, delta: i32) -> Result<bool, Box<dyn Error>> {
        let options = [
            EpisodeFormat::LeRobotV2_1,
            EpisodeFormat::LeRobotV3_0,
            EpisodeFormat::Mcap,
        ];
        let current_index = options
            .iter()
            .position(|format| *format == self.config.episode.format)
            .unwrap_or(0);
        let next_index = rotate_index(current_index, options.len(), delta);
        self.config.episode.format = options[next_index];
        self.config.validate()?;
        Ok(true)
    }

    fn cycle_storage_backend(&mut self, delta: i32) -> Result<bool, Box<dyn Error>> {
        let options = [StorageBackend::Local, StorageBackend::Http];
        let current_index = options
            .iter()
            .position(|backend| *backend == self.config.storage.backend)
            .unwrap_or(0);
        let next_index = rotate_index(current_index, options.len(), delta);
        self.config.storage.backend = options[next_index];
        if matches!(self.config.storage.backend, StorageBackend::Local) {
            self.config.storage.endpoint = None;
            if self
                .config
                .storage
                .output_path
                .as_deref()
                .is_none_or(|path| path.trim().is_empty())
            {
                self.config.storage.output_path = Some("./output".into());
            }
        } else if self.config.storage.endpoint.is_none() {
            self.config.storage.endpoint = Some("http://127.0.0.1:8080/upload".into());
        }
        self.config.validate()?;
        Ok(true)
    }

    fn cycle_collection_mode(&mut self, _delta: i32) -> Result<bool, Box<dyn Error>> {
        // The wizard now exposes only `Teleop`. Pin the value here so any
        // legacy `Intervention` config that lands in the session (e.g.
        // resumed from an older save) is normalized on the first cycle
        // attempt. Otherwise this is a no-op cycle.
        if self.config.mode == CollectionMode::Teleop {
            return Ok(false);
        }
        self.config.mode = CollectionMode::Teleop;
        self.ensure_visible_current_step();
        self.config.validate()?;
        Ok(true)
    }

    fn cycle_video_codec(&mut self, delta: i32) -> Result<bool, Box<dyn Error>> {
        // RGB / IR streams should never be assigned the depth-only RVL codec
        // (the libav encoder physically rejects non-depth16 frames). The
        // cycle therefore iterates only the libav-backed codec/backend pairs
        // and skips RVL entirely.
        let (codec, backend) = rotate_encoder_codec_backend(
            self.config.encoder.video_codec,
            self.config.encoder.video_backend,
            VIDEO_CODEC_BACKEND_OPTIONS,
            delta,
        );
        let previous_codec = self.config.encoder.video_codec;
        let previous_backend = self.config.encoder.video_backend;
        self.config.encoder.video_codec = codec;
        self.config.encoder.video_backend = backend;
        if let Err(error) = self.config.validate() {
            self.config.encoder.video_codec = previous_codec;
            self.config.encoder.video_backend = previous_backend;
            return Err(error.into());
        }
        Ok(true)
    }

    fn cycle_depth_codec(&mut self, delta: i32) -> Result<bool, Box<dyn Error>> {
        // Depth pipeline supports the in-repo RVL encoder (CPU only) plus
        // every libav backend. The cycle therefore exposes RVL alongside the
        // libav (codec, backend) pairs.
        let (codec, backend) = rotate_encoder_codec_backend(
            self.config.encoder.depth_codec,
            self.config.encoder.depth_backend,
            DEPTH_CODEC_BACKEND_OPTIONS,
            delta,
        );
        let previous_codec = self.config.encoder.depth_codec;
        let previous_backend = self.config.encoder.depth_backend;
        self.config.encoder.depth_codec = codec;
        self.config.encoder.depth_backend = backend;
        if let Err(error) = self.config.validate() {
            self.config.encoder.depth_codec = previous_codec;
            self.config.encoder.depth_backend = previous_backend;
            return Err(error.into());
        }
        Ok(true)
    }

    fn set_project_name(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.message = Some("Project name must not be empty.".into());
            return Ok(false);
        }
        if self.config.project_name == trimmed {
            return Ok(false);
        }
        self.config.project_name = trimmed.into();
        self.config.validate()?;
        Ok(true)
    }

    fn set_storage_output_path(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.message = Some("Local storage output path must not be empty.".into());
            return Ok(false);
        }
        if self.config.storage.output_path.as_deref() == Some(trimmed) {
            return Ok(false);
        }
        self.config.storage.output_path = Some(trimmed.into());
        self.config.validate()?;
        Ok(true)
    }

    fn set_storage_endpoint(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.message = Some("HTTP storage endpoint must not be empty.".into());
            return Ok(false);
        }
        if self.config.storage.endpoint.as_deref() == Some(trimmed) {
            return Ok(false);
        }
        self.config.storage.endpoint = Some(trimmed.into());
        self.config.validate()?;
        Ok(true)
    }

    /// Update the host the browser UI server should bind to. Mutating the
    /// field through the wizard avoids forcing the operator to hand-edit
    /// the saved TOML when they need to expose the UI on a different
    /// interface (e.g. switching the default `0.0.0.0` to `127.0.0.1` for
    /// loopback-only access).
    fn set_ui_http_host(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.message = Some("UI host must not be empty.".into());
            return Ok(false);
        }
        if self.config.ui.http_host == trimmed {
            return Ok(false);
        }
        let previous = std::mem::replace(&mut self.config.ui.http_host, trimmed.into());
        if let Err(error) = self.config.validate() {
            self.config.ui.http_host = previous;
            self.message = Some(format!("UI host rejected: {error}"));
            return Ok(false);
        }
        Ok(true)
    }

    fn jump_to_step(&mut self, value: &str) -> bool {
        let target = match value {
            "devices" | "discovery" | "selection" | "parameters" => SetupStep::Devices,
            "states" => SetupStep::States,
            "storage" => SetupStep::Storage,
            "pairing" => SetupStep::Pairing,
            "preview" => SetupStep::Preview,
            _ => return false,
        };
        if !self.visible_steps().contains(&target) {
            return false;
        }
        let changed = self.current_step != target;
        self.current_step = target;
        if self.current_step != SetupStep::Devices {
            self.clear_identify_state();
        }
        changed
    }

    fn apply_raw_command(&mut self, raw_json: &str) -> Result<SessionMutation, Box<dyn Error>> {
        let command: SetupCommandEnvelope = serde_json::from_str(raw_json)?;
        if command.msg_type != "command" {
            return Ok(SessionMutation::default());
        }
        let delta = normalized_delta(command.delta);
        match command.action.as_str() {
            "setup_get_state" => Ok(SessionMutation::state_only(true)),
            "setup_prev_step" => Ok(SessionMutation::step_changed(self.retreat_step())),
            "setup_next_step" => Ok(SessionMutation::step_changed(self.advance_step())),
            "setup_jump_step" => {
                let Some(value) = command.value.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::step_changed(self.jump_to_step(value)))
            }
            "setup_toggle_device" => {
                let Some(name) = command.name.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.toggle_device_selection(name)?,
                ))
            }
            "setup_set_device_name" => {
                let (Some(name), Some(value)) = (command.name.as_deref(), command.value.as_deref())
                else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.set_device_name(name, value)?,
                ))
            }
            "setup_toggle_identify" => {
                let Some(name) = command.name.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                if self.identify_device_name.as_deref() != Some(name)
                    && !self.is_device_selected(name)
                {
                    return Ok(SessionMutation::default());
                }
                let target = if self.identify_device_name.as_deref() == Some(name) {
                    None
                } else {
                    Some(name)
                };
                Ok(SessionMutation::state_only(
                    self.set_identify_device(target),
                ))
            }
            "setup_cycle_camera_profile" => {
                let Some(name) = command.name.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.cycle_device_profile(name, delta)?,
                ))
            }
            "setup_cycle_robot_mode" => {
                let Some(name) = command.name.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.cycle_robot_mode(name, delta)?,
                ))
            }
            "setup_cycle_pair_mapping" => {
                let Some(index) = command.index else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.cycle_pair_mapping(index, delta)?,
                ))
            }
            "setup_create_pairing" => {
                // Optional `value` carries the operator's leader+follower
                // pick from the modal picker, encoded as
                // `"<leader_device>|<leader_channel_type>;<follower_device>|<follower_channel_type>"`.
                // When absent, the controller falls back to auto-seeding.
                let explicit = command
                    .value
                    .as_deref()
                    .and_then(parse_create_pairing_value);
                Ok(SessionMutation::config_changed(
                    self.create_pairing(explicit)?.is_some(),
                ))
            }
            "setup_remove_pairing" => {
                let Some(index) = command.index else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(self.remove_pairing(index)?))
            }
            "setup_set_pairing_leader" | "setup_set_pairing_follower" => {
                let (Some(index), Some(value)) = (command.index, command.value.as_deref()) else {
                    return Ok(SessionMutation::default());
                };
                let Some((device, channel_type)) = value.split_once('|') else {
                    return Ok(SessionMutation::default());
                };
                let endpoint = if command.action == "setup_set_pairing_leader" {
                    PairingEndpoint::Leader
                } else {
                    PairingEndpoint::Follower
                };
                Ok(SessionMutation::config_changed(self.set_pairing_endpoint(
                    index,
                    endpoint,
                    device,
                    channel_type,
                )?))
            }
            "setup_toggle_publish_state" => {
                let (Some(name), Some(value)) = (command.name.as_deref(), command.value.as_deref())
                else {
                    return Ok(SessionMutation::default());
                };
                let Some(state_kind) = parse_robot_state_kind(value) else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.toggle_publish_state(name, state_kind)?,
                ))
            }
            "setup_toggle_recorded_state" => {
                let (Some(name), Some(value)) = (command.name.as_deref(), command.value.as_deref())
                else {
                    return Ok(SessionMutation::default());
                };
                let Some(state_kind) = parse_robot_state_kind(value) else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.toggle_recorded_state(name, state_kind)?,
                ))
            }
            "setup_cycle_episode_format" => Ok(SessionMutation::config_changed(
                self.cycle_episode_format(delta)?,
            )),
            "setup_cycle_storage_backend" => Ok(SessionMutation::config_changed(
                self.cycle_storage_backend(delta)?,
            )),
            "setup_cycle_collection_mode" => Ok(SessionMutation::config_changed(
                self.cycle_collection_mode(delta)?,
            )),
            "setup_cycle_video_codec" => Ok(SessionMutation::config_changed(
                self.cycle_video_codec(delta)?,
            )),
            "setup_cycle_depth_codec" => Ok(SessionMutation::config_changed(
                self.cycle_depth_codec(delta)?,
            )),
            "setup_set_project_name" => {
                let Some(value) = command.value.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.set_project_name(value)?,
                ))
            }
            "setup_set_storage_output_path" => {
                let Some(value) = command.value.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.set_storage_output_path(value)?,
                ))
            }
            "setup_set_storage_endpoint" => {
                let Some(value) = command.value.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.set_storage_endpoint(value)?,
                ))
            }
            "setup_set_ui_http_host" => {
                let Some(value) = command.value.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.set_ui_http_host(value)?,
                ))
            }
            "setup_save" => {
                save_project_config(&self.config, &self.output_path)?;
                self.mark_saved();
                Ok(SessionMutation::state_only(true))
            }
            "setup_cancel" => {
                self.mark_cancelled();
                Ok(SessionMutation::state_only(true))
            }
            _ => Ok(SessionMutation::default()),
        }
    }
}

/// Parse the wire-format leader+follower payload sent by the wizard
/// when the operator confirms both endpoints in the pairing picker:
/// `"<leader_device>|<leader_channel_type>;<follower_device>|<follower_channel_type>"`.
/// Returns `None` if either half is missing or malformed; the caller
/// then falls back to auto-seeding.
fn parse_create_pairing_value(value: &str) -> Option<TeleopPairEndpoints> {
    let (leader_part, follower_part) = value.split_once(';')?;
    let (leader_device, leader_channel_type) = leader_part.split_once('|')?;
    let (follower_device, follower_channel_type) = follower_part.split_once('|')?;
    if leader_device.is_empty()
        || leader_channel_type.is_empty()
        || follower_device.is_empty()
        || follower_channel_type.is_empty()
    {
        return None;
    }
    Some((
        (leader_device.to_owned(), leader_channel_type.to_owned()),
        (follower_device.to_owned(), follower_channel_type.to_owned()),
    ))
}

fn parse_robot_state_kind(value: &str) -> Option<RobotStateKind> {
    serde_json::from_value(serde_json::Value::String(value.to_owned())).ok()
}

fn rotate_index(current_index: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    let len = len as i32;
    (((current_index as i32 + delta) % len) + len) as usize % len as usize
}

fn rotate_encoder_codec(current: EncoderCodec, delta: i32) -> EncoderCodec {
    let options = [
        EncoderCodec::H264,
        EncoderCodec::H265,
        EncoderCodec::Av1,
        EncoderCodec::Rvl,
    ];
    let current_index = options
        .iter()
        .position(|codec| *codec == current)
        .unwrap_or(0);
    options[rotate_index(current_index, options.len(), delta)]
}

/// Rotate through a flat `(codec, backend)` option table.
///
/// The wizard exposes the cross product as a single cycle so the operator can
/// choose a specific encoder implementation in one go (e.g. "av1 (nvidia)"
/// vs. "av1 (cpu)"). When the current value isn't in the table — for example
/// after loading an older config that paired a codec with `Auto` — we
/// snap to the first matching codec entry so the next cycle step still
/// lands on a sensible neighbour.
fn rotate_encoder_codec_backend(
    current_codec: EncoderCodec,
    current_backend: EncoderBackend,
    options: &[(EncoderCodec, EncoderBackend)],
    delta: i32,
) -> (EncoderCodec, EncoderBackend) {
    if options.is_empty() {
        return (current_codec, current_backend);
    }
    let exact = options
        .iter()
        .position(|entry| *entry == (current_codec, current_backend));
    let current_index = exact.unwrap_or_else(|| {
        options
            .iter()
            .position(|(codec, _)| *codec == current_codec)
            .unwrap_or(0)
    });
    options[rotate_index(current_index, options.len(), delta)]
}

/// Codec/backend cycle exposed for `video_codec` (color + IR fallback).
/// RVL is depth-only and intentionally excluded so RGB streams never end up
/// configured against an encoder that physically rejects their pixel format.
const VIDEO_CODEC_BACKEND_OPTIONS: &[(EncoderCodec, EncoderBackend)] = &[
    (EncoderCodec::H264, EncoderBackend::Cpu),
    (EncoderCodec::H264, EncoderBackend::Nvidia),
    (EncoderCodec::H264, EncoderBackend::Vaapi),
    (EncoderCodec::H265, EncoderBackend::Cpu),
    (EncoderCodec::H265, EncoderBackend::Nvidia),
    (EncoderCodec::H265, EncoderBackend::Vaapi),
    (EncoderCodec::Av1, EncoderBackend::Cpu),
    (EncoderCodec::Av1, EncoderBackend::Nvidia),
    (EncoderCodec::Av1, EncoderBackend::Vaapi),
];

/// Codec/backend cycle exposed for `depth_codec`. RVL leads the list because
/// it's the lossless in-repo default; the libav (codec, backend) pairs are
/// available for projects that want lossy depth compression.
const DEPTH_CODEC_BACKEND_OPTIONS: &[(EncoderCodec, EncoderBackend)] = &[
    (EncoderCodec::Rvl, EncoderBackend::Cpu),
    (EncoderCodec::H264, EncoderBackend::Cpu),
    (EncoderCodec::H264, EncoderBackend::Nvidia),
    (EncoderCodec::H264, EncoderBackend::Vaapi),
    (EncoderCodec::H265, EncoderBackend::Cpu),
    (EncoderCodec::H265, EncoderBackend::Nvidia),
    (EncoderCodec::H265, EncoderBackend::Vaapi),
    (EncoderCodec::Av1, EncoderBackend::Cpu),
    (EncoderCodec::Av1, EncoderBackend::Nvidia),
    (EncoderCodec::Av1, EncoderBackend::Vaapi),
];

fn normalized_delta(delta: Option<i32>) -> i32 {
    match delta.unwrap_or(1).cmp(&0) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 1,
        std::cmp::Ordering::Greater => 1,
    }
}

pub fn run(args: SetupArgs) -> Result<(), Box<dyn Error>> {
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
            existing_config
                .validate()
                .map_err(|e| -> Box<dyn Error> { e.to_string().into() })?;
            validate_existing_project(
                &existing_config,
                &workspace_root,
                state_dir.as_path(),
                &current_exe_dir,
            )?;
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
            (existing_config, available_devices, Vec::new(), true)
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
fn run_interactive_setup(
    config: ProjectConfig,
    available_devices: Vec<AvailableDevice>,
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
                    return Err(setup_trigger_error(trigger).into());
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

fn should_treat_trigger_as_shutdown(
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

fn setup_trigger_error(trigger: crate::ShutdownTrigger) -> String {
    match trigger {
        crate::ShutdownTrigger::Signal => "setup interrupted by signal".into(),
        crate::ShutdownTrigger::ChildExited { id, status } => {
            format!("child \"{id}\" exited with status {status}")
        }
    }
}

fn is_interrupt_exit_status(status: &ExitStatus) -> bool {
    if matches!(status.code(), Some(130 | 143)) {
        return true;
    }

    #[cfg(unix)]
    if matches!(status.signal(), Some(SIGINT | SIGTERM)) {
        return true;
    }

    false
}

fn spawn_setup_children(
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

fn should_run_preview_runtime(session: &SetupSession) -> bool {
    session.exit_kind.is_none()
        && (session.current_step == SetupStep::Preview
            || (session.current_step == SetupStep::Devices
                && session.identify_device_name.is_some()))
}

fn robot_mode_to_channel_mode(mode: RobotMode) -> DeviceChannelMode {
    match mode {
        RobotMode::FreeDrive => DeviceChannelMode::FreeDrive,
        RobotMode::CommandFollowing => DeviceChannelMode::CommandFollowing,
        RobotMode::Identifying => DeviceChannelMode::Identifying,
        RobotMode::Disabled => DeviceChannelMode::Disabled,
    }
}

fn available_primary_channel(available: &AvailableDevice) -> Option<&DeviceChannelConfigV2> {
    available.current.channels.first()
}

fn publish_available_device_mode(
    ipc: &SetupIpc,
    available: &AvailableDevice,
    mode: DeviceChannelMode,
) -> Result<(), Box<dyn Error>> {
    let Some(channel) = available_primary_channel(available) else {
        return Ok(());
    };
    ipc.publish_channel_mode(&available.current.bus_root, &channel.channel_type, mode)
}

fn configured_channel_mode_for_available(available: &AvailableDevice) -> Option<DeviceChannelMode> {
    let channel = available_primary_channel(available)?;
    if channel.kind != DeviceType::Robot || !channel.enabled {
        return None;
    }
    channel.mode.map(robot_mode_to_channel_mode)
}

fn sync_identify_mode(
    session: &SetupSession,
    ipc: &SetupIpc,
    active_identify_target: &mut Option<String>,
    preview_runtime_restarted: bool,
) -> Result<(), Box<dyn Error>> {
    let desired_target = if session.current_step == SetupStep::Devices {
        session.identify_device_name.clone()
    } else {
        None
    };

    if active_identify_target.as_ref() != desired_target.as_ref() {
        if let Some(previous_name) = active_identify_target.as_deref() {
            if let Some(previous_available) = session.available_device(previous_name) {
                if let Some(mode) = configured_channel_mode_for_available(previous_available) {
                    publish_available_device_mode(ipc, previous_available, mode)?;
                }
            }
        }
    }

    if preview_runtime_restarted || active_identify_target.as_ref() != desired_target.as_ref() {
        if let Some(target_name) = desired_target.as_deref() {
            if let Some(target_available) = session.available_device(target_name) {
                if available_primary_channel(target_available)
                    .is_some_and(|channel| channel.kind == DeviceType::Robot)
                {
                    publish_available_device_mode(
                        ipc,
                        target_available,
                        DeviceChannelMode::Identifying,
                    )?;
                }
            }
        }
    }

    *active_identify_target = desired_target;
    Ok(())
}

fn start_preview_runtime(
    session: &mut SetupSession,
    preview_port: u16,
    preview_websocket_url: &str,
    workspace_root: &Path,
    child_working_dir: &Path,
    current_exe_dir: &Path,
    log_dir: &Path,
) -> Result<SetupRuntimeState, Box<dyn Error>> {
    let preview_config =
        build_preview_project_config(session, preview_port, preview_websocket_url)?;
    let temp_config_path = write_setup_temp_config(
        &preview_config,
        log_dir,
        &format!("setup-preview-{preview_port}.toml"),
    )?;
    let specs = build_setup_preview_specs(
        &preview_config,
        workspace_root,
        child_working_dir,
        current_exe_dir,
    )?;
    let children = spawn_setup_children(&specs, log_dir)?;
    if session.current_step == SetupStep::Preview {
        session.message = Some(format!(
            "Preview running from {}",
            temp_config_path.display()
        ));
    }
    Ok(SetupRuntimeState {
        children,
        temp_config_path: Some(temp_config_path),
        preview_target_name: session.identify_device_name.clone(),
    })
}

fn stop_setup_runtime(
    runtime_state: &mut Option<SetupRuntimeState>,
    ipc: &SetupIpc,
) -> Result<(), Box<dyn Error>> {
    let Some(mut runtime) = runtime_state.take() else {
        return Ok(());
    };

    ipc.send_shutdown()?;

    let deadline = Instant::now() + SETUP_SHUTDOWN_TIMEOUT;
    loop {
        let mut remaining_children = 0usize;
        for child in runtime.children.iter_mut() {
            if child.child.try_wait()?.is_none() {
                remaining_children += 1;
            }
        }
        if remaining_children == 0 {
            break;
        }
        if Instant::now() >= deadline {
            terminate_children(
                &mut runtime.children,
                SETUP_SHUTDOWN_TIMEOUT,
                SETUP_POLL_INTERVAL,
            )?;
            break;
        }
        thread::sleep(SETUP_POLL_INTERVAL);
    }
    if let Some(temp_config_path) = runtime.temp_config_path.as_deref() {
        cleanup_preview_temp_config(temp_config_path);
    }
    Ok(())
}

fn build_preview_project_config(
    session: &SetupSession,
    websocket_port: u16,
    websocket_url: &str,
) -> Result<ProjectConfig, Box<dyn Error>> {
    let mut preview = if session.current_step == SetupStep::Devices {
        if let Some(target_name) = session.identify_device_name.as_deref() {
            let target = session
                .available_device(target_name)
                .ok_or_else(|| format!("missing identify target {target_name}"))?;
            let mut preview = session.config.clone();
            preview.mode = CollectionMode::Intervention;
            preview.pairings.clear();
            // Boot the identify target's robot channels directly into
            // RobotMode::Identifying so the device process never has the
            // chance to start in FreeDrive and miss a late-arriving mode
            // event from `sync_identify_mode` (race confirmed in debug
            // session 8d351b: device booted ~21 ms AFTER controller
            // published Identifying, so the publish landed before the
            // subscriber existed).
            let mut device = target.current.clone();
            for channel in device.channels.iter_mut() {
                if channel.kind == DeviceType::Robot && channel.enabled {
                    channel.mode = Some(RobotMode::Identifying);
                }
            }
            preview.devices = vec![device];
            preview
        } else {
            session.config.clone()
        }
    } else {
        session.config.clone()
    };
    preview.visualizer.port = websocket_port;
    preview.ui.preview_websocket_url = Some(websocket_url.into());
    Ok(preview)
}

fn write_setup_temp_config(
    project: &ProjectConfig,
    log_dir: &Path,
    filename: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let path = log_dir.join(filename);
    fs::write(&path, toml::to_string_pretty(project)?)?;
    Ok(path)
}

fn cleanup_preview_temp_config(path: &Path) {
    let _ = fs::remove_file(path);
}

fn build_setup_preview_specs(
    project: &ProjectConfig,
    workspace_root: &Path,
    child_working_dir: &Path,
    current_exe_dir: &Path,
) -> Result<Vec<ChildSpec>, Box<dyn Error>> {
    build_preview_specs(project, workspace_root, child_working_dir, current_exe_dir)
}

fn camera_channel_type_for_profile(profile: &CameraProfile) -> String {
    let base = profile
        .stream
        .clone()
        .unwrap_or_else(|| "color".to_string());
    match profile.channel {
        Some(ch) if ch > 0 => format!("{base}_{ch}"),
        _ => base,
    }
}

fn robot_default_channel_type(_driver: &str) -> String {
    "arm".into()
}

/// One channel + chosen profile + final user-visible name for a camera
/// discovery row. Built by `build_discovery_config` so multi-stream cameras
/// (e.g. RealSense color + depth + infrared) collapse into a single
/// Build a `BinaryDeviceConfig` from one discovered device by iterating
/// every channel returned in the driver's `query --json`. There is no
/// per-driver branching: cameras, robots, and mixed devices all flow
/// through the same loop. Each channel's `kind`, `dof`, `modes`,
/// `profiles`, `defaults`, `value_limits`, `direct_joint_compatibility`,
/// and `supported_commands` are taken straight from the channel meta.
fn binary_device_from_discovery(
    discovery: &DiscoveredDevice,
    name: String,
    preferred_mode: RobotMode,
    name_counts: &mut BTreeMap<String, usize>,
) -> BinaryDeviceConfig {
    let mut extra = toml::Table::new();
    if let Some(transport) = &discovery.transport {
        extra.insert("transport".into(), toml::Value::String(transport.clone()));
    }
    if let Some(interface) = &discovery.interface {
        extra.insert("interface".into(), toml::Value::String(interface.clone()));
    }
    if let Some(product_variant) = &discovery.product_variant {
        extra.insert(
            "product_variant".into(),
            toml::Value::String(product_variant.clone()),
        );
    }
    if let Some(end_effector) = &discovery.end_effector {
        extra.insert(
            "end_effector".into(),
            toml::Value::String(end_effector.clone()),
        );
    }
    let single_channel = discovery.channel_meta_by_channel.len() == 1;
    let channels = discovery
        .channel_meta_by_channel
        .iter()
        .map(|(channel_type, meta)| {
            let channel_name = if single_channel {
                // Avoid double-deduping: a single-channel device uses the
                // device-level name (already deduped in `build_discovery_config`)
                // as the channel name. Multi-channel devices need per-channel
                // names so the wizard can tell rows apart.
                Some(name.clone())
            } else {
                dedup_channel_default_name(meta.default_name.as_deref(), name_counts)
            };
            build_channel_config_from_meta(channel_type, meta, preferred_mode, channel_name)
        })
        .collect();
    BinaryDeviceConfig {
        name: name.clone(),
        executable: Some(default_device_executable_name(&discovery.driver)),
        driver: discovery.driver.clone(),
        id: discovery.id.clone(),
        bus_root: name,
        channels,
        extra,
    }
}

fn build_channel_config_from_meta(
    channel_type: &str,
    meta: &DiscoveredChannelMeta,
    preferred_mode: RobotMode,
    channel_name: Option<String>,
) -> DeviceChannelConfigV2 {
    match meta.kind {
        DeviceType::Camera => {
            let profile = pick_default_camera_profile(&meta.profiles);
            DeviceChannelConfigV2 {
                channel_type: channel_type.to_owned(),
                kind: DeviceType::Camera,
                enabled: true,
                name: channel_name,
                channel_label: meta.channel_label.clone(),
                mode: None,
                dof: None,
                publish_states: Vec::new(),
                recorded_states: Vec::new(),
                control_frequency_hz: None,
                profile,
                command_defaults: meta.defaults.clone(),
                value_limits: meta.value_limits.clone(),
                direct_joint_compatibility: meta.direct_joint_compatibility.clone(),
                supported_commands: meta.supported_commands.clone(),
                extra: toml::Table::new(),
            }
        }
        DeviceType::Robot => {
            let mode = Some(select_supported_mode(&meta.modes, preferred_mode));
            let publish_states =
                default_publish_states_for_meta(meta, &robot_publish_states_fallback(channel_type));
            let recorded_states = publish_states.clone();
            DeviceChannelConfigV2 {
                channel_type: channel_type.to_owned(),
                kind: DeviceType::Robot,
                enabled: true,
                name: channel_name,
                channel_label: meta.channel_label.clone(),
                mode,
                dof: meta.dof,
                publish_states,
                recorded_states,
                control_frequency_hz: meta.default_control_frequency_hz,
                profile: None,
                command_defaults: meta.defaults.clone(),
                value_limits: meta.value_limits.clone(),
                direct_joint_compatibility: meta.direct_joint_compatibility.clone(),
                supported_commands: meta.supported_commands.clone(),
                extra: toml::Table::new(),
            }
        }
    }
}

/// Channel-shape-generic fallback for `publish_states` when the driver
/// neither populates `supported_states` nor `value_limits`. Picks
/// joint-shaped defaults for typical arm channel names; parallel-shaped
/// defaults for grippers / end-effectors. Driver-specific tables are NOT
/// consulted — the driver should populate `supported_states` properly.
fn robot_publish_states_fallback(channel_type: &str) -> Vec<RobotStateKind> {
    let lower = channel_type.to_ascii_lowercase();
    if lower.contains("gripper")
        || lower.contains("eef")
        || lower == "e2"
        || lower == "g2"
        || lower == "e2b"
    {
        vec![
            RobotStateKind::ParallelPosition,
            RobotStateKind::ParallelVelocity,
            RobotStateKind::ParallelEffort,
        ]
    } else {
        vec![
            RobotStateKind::JointPosition,
            RobotStateKind::JointVelocity,
            RobotStateKind::JointEffort,
        ]
    }
}

fn pick_default_camera_profile(profiles: &[CameraProfile]) -> Option<CameraChannelProfile> {
    profiles
        .iter()
        .max_by_key(|profile| camera_profile_quality_key(profile))
        .map(|profile| CameraChannelProfile {
            width: profile.width,
            height: profile.height,
            fps: profile.fps,
            pixel_format: profile.pixel_format,
            native_pixel_format: profile.native_pixel_format.clone(),
        })
}

/// Quality ordering key for `CameraProfile` defaults: higher pixel count
/// first, ties broken by higher fps. Returned as a tuple so the caller
/// can compare with `>` directly.
fn camera_profile_quality_key(profile: &CameraProfile) -> (u64, u32) {
    let pixels = (profile.width as u64) * (profile.height as u64);
    (pixels, profile.fps)
}

fn channel_uses_parallel_teleop(ch: &DeviceChannelConfigV2) -> bool {
    ch.publish_states
        .contains(&RobotStateKind::ParallelPosition)
}

/// Pick the default `publish_states` for a freshly discovered robot
/// channel. Prefer whatever the driver advertises (`supported_states`,
/// falling back to the kinds enumerated by `value_limits`) so newly added
/// state kinds (e.g. `EndEffectorPose` on the airbot arm) are turned on
/// without requiring a config edit. Falls back to a static template when
/// the driver query returned nothing usable, so older drivers and tests
/// keep working unchanged.
fn default_publish_states_for_meta(
    meta: &DiscoveredChannelMeta,
    fallback: &[RobotStateKind],
) -> Vec<RobotStateKind> {
    if !meta.supported_states.is_empty() {
        return dedup_in_order(&meta.supported_states);
    }
    let from_limits: Vec<RobotStateKind> = meta
        .value_limits
        .iter()
        .map(|entry| entry.state_kind)
        .collect();
    if !from_limits.is_empty() {
        return dedup_in_order(&from_limits);
    }
    fallback.to_vec()
}

fn dedup_in_order(values: &[RobotStateKind]) -> Vec<RobotStateKind> {
    let mut out: Vec<RobotStateKind> = Vec::with_capacity(values.len());
    for value in values {
        if !out.contains(value) {
            out.push(*value);
        }
    }
    out
}

/// Ensure a robot channel publishes the given state kind. Used as a
/// safety net when switching pairings to FK/IK so we don't blow up on
/// validation if a legacy config (or an operator who toggled the kind off
/// in the new "States" sub-step) doesn't have it. Newly discovered
/// channels already opt every supported kind into `publish_states` via
/// `default_publish_states_for_meta`, so this rarely runs in fresh
/// projects.
fn ensure_channel_publishes_state(
    devices: &mut [BinaryDeviceConfig],
    device_name: &str,
    channel_type: &str,
    state: RobotStateKind,
) {
    let Some(device) = devices.iter_mut().find(|d| d.name == device_name) else {
        return;
    };
    let Some(channel) = device
        .channels
        .iter_mut()
        .find(|c| c.channel_type == channel_type)
    else {
        return;
    };
    if !channel.publish_states.contains(&state) {
        channel.publish_states.push(state);
    }
}

fn build_default_channel_pairings(devices: &[BinaryDeviceConfig]) -> Vec<ChannelPairingConfig> {
    let mut pairings = Vec::new();
    let arms = primary_robot_channels(devices, false);
    let eefs = primary_robot_channels(devices, true);
    for pairs in [
        pair_robot_channels_by_order(&arms),
        pair_robot_channels_by_order(&eefs),
    ] {
        let Some((leader_dev, leader_ch, follower_dev, follower_ch)) = pairs else {
            continue;
        };
        let leader_dof = leader_ch.dof.unwrap_or(0);
        let follower_dof = follower_ch.dof.unwrap_or(0);
        if leader_dof == 0 || follower_dof == 0 {
            continue;
        }
        if leader_dof == follower_dof {
            let dof = follower_dof;
            let parallel_pair = channel_uses_parallel_teleop(leader_ch)
                && channel_uses_parallel_teleop(follower_ch);
            let (leader_state, follower_command, map_len) = if parallel_pair {
                (
                    RobotStateKind::ParallelPosition,
                    RobotCommandKind::ParallelMit,
                    dof.min(MAX_PARALLEL as u32),
                )
            } else {
                (
                    RobotStateKind::JointPosition,
                    RobotCommandKind::JointPosition,
                    dof,
                )
            };
            pairings.push(ChannelPairingConfig {
                leader_device: leader_dev.name.clone(),
                leader_channel_type: leader_ch.channel_type.clone(),
                follower_device: follower_dev.name.clone(),
                follower_channel_type: follower_ch.channel_type.clone(),
                mapping: MappingStrategy::DirectJoint,
                leader_state,
                follower_command,
                joint_index_map: (0..map_len).collect(),
                joint_scales: vec![1.0; map_len as usize],
            });
        } else if channel_supports_cartesian_leader(leader_ch)
            && channel_supports_cartesian_follower(follower_ch)
        {
            // Cross-DOF arms (e.g. 6-DOF leader -> 7-DOF follower) cannot
            // safely use direct-joint identity mapping, but Cartesian
            // (end-effector pose) tracking is DOF-agnostic. Default to it
            // when both sides advertise the required state/command.
            pairings.push(ChannelPairingConfig {
                leader_device: leader_dev.name.clone(),
                leader_channel_type: leader_ch.channel_type.clone(),
                follower_device: follower_dev.name.clone(),
                follower_channel_type: follower_ch.channel_type.clone(),
                mapping: MappingStrategy::Cartesian,
                leader_state: RobotStateKind::EndEffectorPose,
                follower_command: RobotCommandKind::EndPose,
                joint_index_map: Vec::new(),
                joint_scales: Vec::new(),
            });
        }
    }
    pairings
}

/// Endpoint of a `ChannelPairingConfig` selected by the wizard's manual
/// pairing flow. Used by `set_pairing_endpoint` to know which side of an
/// existing pair to mutate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PairingEndpoint {
    Leader,
    Follower,
}

/// Build a single `ChannelPairingConfig` from a `(leader, follower)`
/// channel pair, deriving the mapping strategy + state / command kinds
/// from the two channels' shapes the same way `build_default_channel_pairings`
/// does. Falls back to a `DirectJoint` mapping with a placeholder
/// `joint_index_map` when DOFs are missing or mismatched without a
/// Cartesian-capable peer; downstream `validate()` will surface the
/// problem to the operator if the rebuilt pair isn't viable.
fn pairing_from_channels(
    devices: &[BinaryDeviceConfig],
    leader_device: &str,
    leader_channel_type: &str,
    follower_device: &str,
    follower_channel_type: &str,
) -> ChannelPairingConfig {
    let leader_ch = devices
        .iter()
        .find(|d| d.name == leader_device)
        .and_then(|d| {
            d.channels
                .iter()
                .find(|c| c.channel_type == leader_channel_type)
        });
    let follower_ch = devices
        .iter()
        .find(|d| d.name == follower_device)
        .and_then(|d| {
            d.channels
                .iter()
                .find(|c| c.channel_type == follower_channel_type)
        });
    let leader_dof = leader_ch.and_then(|ch| ch.dof).unwrap_or(0);
    let follower_dof = follower_ch.and_then(|ch| ch.dof).unwrap_or(0);
    let parallel_pair = leader_ch.is_some_and(channel_uses_parallel_teleop)
        && follower_ch.is_some_and(channel_uses_parallel_teleop);

    if leader_dof != 0 && leader_dof == follower_dof {
        let dof = follower_dof;
        let (leader_state, follower_command, map_len) = if parallel_pair {
            (
                RobotStateKind::ParallelPosition,
                RobotCommandKind::ParallelMit,
                dof.min(MAX_PARALLEL as u32),
            )
        } else {
            (
                RobotStateKind::JointPosition,
                RobotCommandKind::JointPosition,
                dof,
            )
        };
        return ChannelPairingConfig {
            leader_device: leader_device.to_owned(),
            leader_channel_type: leader_channel_type.to_owned(),
            follower_device: follower_device.to_owned(),
            follower_channel_type: follower_channel_type.to_owned(),
            mapping: MappingStrategy::DirectJoint,
            leader_state,
            follower_command,
            joint_index_map: (0..map_len).collect(),
            joint_scales: vec![1.0; map_len as usize],
        };
    }

    if leader_ch.is_some_and(channel_supports_cartesian_leader)
        && follower_ch.is_some_and(channel_supports_cartesian_follower)
    {
        return ChannelPairingConfig {
            leader_device: leader_device.to_owned(),
            leader_channel_type: leader_channel_type.to_owned(),
            follower_device: follower_device.to_owned(),
            follower_channel_type: follower_channel_type.to_owned(),
            mapping: MappingStrategy::Cartesian,
            leader_state: RobotStateKind::EndEffectorPose,
            follower_command: RobotCommandKind::EndPose,
            joint_index_map: Vec::new(),
            joint_scales: Vec::new(),
        };
    }

    // Worst-case fallback: a DirectJoint pair seeded for the smaller DOF.
    // `validate()` on the parent `apply_raw_command` call will surface a
    // descriptive error so the operator can pick a different combo.
    let dof = leader_dof.min(follower_dof);
    ChannelPairingConfig {
        leader_device: leader_device.to_owned(),
        leader_channel_type: leader_channel_type.to_owned(),
        follower_device: follower_device.to_owned(),
        follower_channel_type: follower_channel_type.to_owned(),
        mapping: MappingStrategy::DirectJoint,
        leader_state: if parallel_pair {
            RobotStateKind::ParallelPosition
        } else {
            RobotStateKind::JointPosition
        },
        follower_command: if parallel_pair {
            RobotCommandKind::ParallelMit
        } else {
            RobotCommandKind::JointPosition
        },
        joint_index_map: (0..dof).collect(),
        joint_scales: vec![1.0; dof as usize],
    }
}

fn channel_supports_cartesian_leader(ch: &DeviceChannelConfigV2) -> bool {
    ch.publish_states.contains(&RobotStateKind::EndEffectorPose)
}

fn channel_supports_cartesian_follower(ch: &DeviceChannelConfigV2) -> bool {
    ch.supported_commands.contains(&RobotCommandKind::EndPose)
}

fn primary_robot_channels(
    devices: &[BinaryDeviceConfig],
    end_effector_only: bool,
) -> Vec<(&BinaryDeviceConfig, &DeviceChannelConfigV2)> {
    devices
        .iter()
        .filter_map(|device| {
            let ch = device.channels.iter().find(|c| {
                c.kind == DeviceType::Robot
                    && c.enabled
                    && ((c.dof == Some(1)) == end_effector_only)
            })?;
            Some((device, ch))
        })
        .collect()
}

fn pair_robot_channels_by_order<'a>(
    channels: &[(&'a BinaryDeviceConfig, &'a DeviceChannelConfigV2)],
) -> Option<(
    &'a BinaryDeviceConfig,
    &'a DeviceChannelConfigV2,
    &'a BinaryDeviceConfig,
    &'a DeviceChannelConfigV2,
)> {
    match channels {
        [a, b, ..] => Some((a.0, a.1, b.0, b.1)),
        _ => None,
    }
}

fn build_setup_ui_spec(
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
        inherit_stdio: true,
    })
}

fn reserve_loopback_port() -> Result<u16, Box<dyn Error>> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

struct SetupIpc {
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
    fn new() -> Result<Self, Box<dyn Error>> {
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

    fn drain_setup_commands(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let mut commands = Vec::new();
        loop {
            let Some(sample) = self.setup_command_subscriber.receive()? else {
                return Ok(commands);
            };
            commands.push(sample.payload().as_str().to_owned());
        }
    }

    fn publish_state_json(&self, json: &str) -> Result<(), Box<dyn Error>> {
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

    fn send_shutdown(&self) -> Result<(), Box<dyn Error>> {
        self.control_publisher.send_copy(ControlEvent::Shutdown)?;
        Ok(())
    }

    fn publish_channel_mode(
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

fn available_devices_from_project(
    project: &ProjectConfig,
    runtime_meta: &crate::device_query::DeviceRuntimeMetaMap,
) -> Vec<AvailableDevice> {
    project
        .devices
        .iter()
        .flat_map(|device| {
            device.channels.iter().filter_map(|channel| {
                let current = row_current_from_binary_channel(device, channel)?;
                let device_type = channel.kind;
                let camera_profiles = if device_type == DeviceType::Camera {
                    channel
                        .profile
                        .as_ref()
                        .map(|profile| CameraProfile {
                            width: profile.width,
                            height: profile.height,
                            fps: profile.fps,
                            pixel_format: profile.pixel_format,
                            native_pixel_format: profile.native_pixel_format.clone(),
                            stream: split_camera_channel_type(&channel.channel_type)
                                .0
                                .map(ToOwned::to_owned),
                            channel: split_camera_channel_type(&channel.channel_type).1,
                        })
                        .into_iter()
                        .collect()
                } else {
                    Vec::new()
                };
                let supported_modes = supported_modes_from_project_channel(device, channel);
                let supported_states = if device_type == DeviceType::Robot {
                    runtime_meta
                        .get(&(device.name.clone(), channel.channel_type.clone()))
                        .map(|meta| meta.supported_states.clone())
                        .unwrap_or_else(|| {
                            // Older drivers may not advertise supported_states;
                            // fall back to whatever value_limits the latest
                            // refresh populated.
                            channel
                                .value_limits
                                .iter()
                                .map(|entry| entry.state_kind)
                                .collect()
                        })
                } else {
                    Vec::new()
                };
                Some(AvailableDevice {
                    name: available_device_key_from_binary(&current),
                    display_name: display_name_for_binary_channel(device, channel),
                    device_type,
                    driver: device.driver.clone(),
                    id: device.id.clone(),
                    camera_profiles,
                    supported_modes,
                    supported_states,
                    current,
                })
            })
        })
        .collect()
}

fn available_devices_from_discoveries(
    discoveries: &[DiscoveredDevice],
    project: &ProjectConfig,
) -> Result<Vec<AvailableDevice>, Box<dyn Error>> {
    let mut available = Vec::new();

    for discovery in discoveries {
        let mut current = project
            .devices
            .iter()
            .find(|device| device_matches_discovery_binary(device, discovery, None, None))
            .cloned()
            .ok_or_else(|| {
                format!(
                    "missing setup device for discovered device {} ({})",
                    discovery.display_name, discovery.id
                )
            })?;
        enrich_current_device_from_discovery(&mut current, discovery);
        for row in available_rows_from_discovery(&current, discovery) {
            available.push(row);
        }
    }

    Ok(available)
}

fn available_rows_from_discovery(
    current: &BinaryDeviceConfig,
    discovery: &DiscoveredDevice,
) -> Vec<AvailableDevice> {
    current
        .channels
        .iter()
        .filter_map(|channel| {
            let row_current = row_current_from_binary_channel(current, channel)?;
            let device_type = channel.kind;
            let camera_profiles = if device_type == DeviceType::Camera {
                discovery
                    .channel_meta_by_channel
                    .get(&channel.channel_type)
                    .map(|meta| meta.profiles.clone())
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let supported_states = if device_type == DeviceType::Robot {
                discovery
                    .channel_meta_by_channel
                    .get(&channel.channel_type)
                    .map(|meta| meta.supported_states.clone())
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            Some(AvailableDevice {
                name: available_device_key_from_binary(&row_current),
                display_name: display_name_for_binary_channel(current, channel),
                device_type,
                driver: current.driver.clone(),
                id: current.id.clone(),
                camera_profiles,
                supported_modes: supported_modes_from_discovery(discovery, channel),
                supported_states,
                current: row_current,
            })
        })
        .collect()
}

fn row_current_from_binary_channel(
    device: &BinaryDeviceConfig,
    channel: &DeviceChannelConfigV2,
) -> Option<BinaryDeviceConfig> {
    let mut current = device.clone();
    current.channels = vec![channel.clone()];
    Some(current)
}

fn supported_modes_from_project_channel(
    _device: &BinaryDeviceConfig,
    channel: &DeviceChannelConfigV2,
) -> Vec<RobotMode> {
    if channel.kind != DeviceType::Robot {
        return Vec::new();
    }
    // Without a live driver session we can only echo the persisted mode.
    // The discovery path uses `supported_modes_from_discovery` which does
    // consult the driver's `query --json` `modes` array directly.
    channel.mode.into_iter().collect()
}

fn supported_modes_from_discovery(
    discovery: &DiscoveredDevice,
    channel: &DeviceChannelConfigV2,
) -> Vec<RobotMode> {
    if channel.kind != DeviceType::Robot {
        return Vec::new();
    }
    discovery
        .channel_meta_by_channel
        .get(&channel.channel_type)
        .map(|meta| meta.modes.clone())
        .unwrap_or_else(|| channel.mode.into_iter().collect())
}

fn split_camera_channel_type(channel_type: &str) -> (Option<&str>, Option<u32>) {
    channel_type
        .rsplit_once('_')
        .and_then(|(stream, suffix)| suffix.parse::<u32>().ok().map(|channel| (stream, channel)))
        .map(|(stream, channel)| (Some(stream), Some(channel)))
        .unwrap_or((Some(channel_type), None))
}

fn display_name_for_binary_channel(
    device: &BinaryDeviceConfig,
    channel: &DeviceChannelConfigV2,
) -> String {
    // Prefer the device-provided per-channel label so the controller stays
    // driver-agnostic. Falls back to a generic `{driver_label} ({channel_type})`
    // format only when no driver-supplied label exists; new device
    // executables MUST set `channel_label` (or `device_label`) in their
    // `query --json` to control display strings.
    if let Some(label) = channel.channel_label.as_deref() {
        if !label.trim().is_empty() {
            return label.to_owned();
        }
    }
    let driver_label = driver_to_label_fallback(&device.driver);
    if channel.channel_type.is_empty() {
        driver_label
    } else {
        format!("{driver_label} ({})", channel.channel_type)
    }
}

/// Build a list of human-readable warnings for every robot channel whose
/// `publish_states` includes a kind that the driver did not report
/// `value_limits` for. The renderer paints these cells with `?` placeholder
/// bars (no fallback envelope), so flagging the misconfiguration during
/// setup tells the operator they need to update their device executable
/// rather than diagnose missing bars at run time.
fn missing_value_limit_warnings(config: &ProjectConfig) -> Vec<String> {
    let mut warnings = Vec::new();
    for device in &config.devices {
        for channel in &device.channels {
            if channel.kind != DeviceType::Robot || !channel.enabled {
                continue;
            }
            for state_kind in &channel.publish_states {
                let entry = channel
                    .value_limits
                    .iter()
                    .find(|entry| entry.state_kind == *state_kind);
                let needs_warning = match entry {
                    None => true,
                    Some(entry) => entry.min.is_empty() || entry.max.is_empty(),
                };
                if needs_warning {
                    warnings.push(format!(
                        "device \"{}\" channel \"{}\": driver did not report value_limits for {}; bars will render as ??? until the device executable provides them",
                        device.name,
                        channel.channel_type,
                        state_kind.topic_suffix()
                    ));
                }
            }
        }
    }
    warnings
}

fn discover_devices(
    workspace_root: &Path,
    process_working_dir: &Path,
    current_exe_dir: &Path,
    options: DiscoveryOptions,
) -> Result<(Vec<DiscoveredDevice>, Vec<String>), Box<dyn Error>> {
    let (probe_entries, mut probe_errors) = discover_probe_entries(
        workspace_root,
        process_working_dir,
        current_exe_dir,
        options,
        DISCOVERY_TIMEOUT,
    )?;
    let mut discoveries = Vec::new();

    for entry in probe_entries {
        match build_discovered_device(
            &entry.executable,
            &entry.probe_entry,
            &entry.program,
            process_working_dir,
            DISCOVERY_TIMEOUT,
        ) {
            Ok(device) => discoveries.push(device),
            Err(error) => probe_errors.push(format!("{}: {error}", entry.executable)),
        }
    }

    Ok((discoveries, probe_errors))
}

/// Single, device-type-free discovery entry. Every channel reported by the
/// driver's `query --json` is parsed into a `DiscoveredChannelMeta`,
/// preserving its `kind` (camera or robot). The wizard later builds one
/// `DeviceChannelConfigV2` per channel without any per-driver branching.
fn build_discovered_device(
    executable: &str,
    probe_entry: &Value,
    program: &OsString,
    process_working_dir: &Path,
    timeout: Duration,
) -> Result<DiscoveredDevice, Box<dyn Error>> {
    let id = probe_entry
        .as_str()
        .map(ToOwned::to_owned)
        .or_else(|| value_as_string(probe_entry.get("id")))
        .ok_or_else(|| format!("probe entry missing id: {probe_entry}"))?;
    let query = run_driver_json(
        program,
        &[
            OsString::from("query"),
            OsString::from("--json"),
            OsString::from(&id),
        ],
        process_working_dir,
        timeout,
    )?;
    let query_device = query
        .get("devices")
        .and_then(Value::as_array)
        .and_then(|devices| {
            devices
                .iter()
                .find(|device| value_as_string(device.get("id")).as_deref() == Some(id.as_str()))
                .or_else(|| devices.first())
        })
        .ok_or_else(|| format!("query returned no devices for id {id}: {query}"))?;

    // Driver name is authoritative from the query response. Fall back to
    // stripping the well-known `rollio-device-` prefix off the executable
    // name only if the response didn't include one (e.g. older drivers).
    let driver = value_as_string(query.get("driver"))
        .or_else(|| {
            executable
                .strip_prefix("rollio-device-")
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| executable.to_owned());

    let channel_meta_by_channel = parse_query_channel_meta(query_device);
    let device_label = value_as_string(query_device.get("device_label"));
    let default_device_name = value_as_string(query_device.get("default_device_name"));
    let display_name = device_label
        .clone()
        .unwrap_or_else(|| driver_to_label_fallback(&driver));

    Ok(DiscoveredDevice {
        driver,
        id,
        display_name,
        default_device_name,
        channel_meta_by_channel,
        transport: query_metadata_string(query_device, "transport")
            .or_else(|| value_as_string(probe_entry.get("transport"))),
        interface: query_metadata_string(query_device, "interface")
            .or_else(|| value_as_string(probe_entry.get("interface"))),
        product_variant: query_metadata_string(query_device, "product_variant")
            .or_else(|| value_as_string(probe_entry.get("product_variant"))),
        end_effector: query_metadata_string(query_device, "end_effector")
            .or_else(|| value_as_string(probe_entry.get("end_effector"))),
    })
}

/// "airbot-play" -> "Airbot Play" fallback when a driver doesn't supply its
/// own `device_label` in `query --json`. Generic, no per-driver lookup.
fn driver_to_label_fallback(driver: &str) -> String {
    driver
        .split('-')
        .filter(|s| !s.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn validate_existing_project(
    project: &ProjectConfig,
    workspace_root: &Path,
    process_working_dir: &Path,
    current_exe_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    for device in &project.devices {
        validate_binary_device_hardware(
            device,
            workspace_root,
            process_working_dir,
            current_exe_dir,
        )?;
    }
    Ok(())
}

fn validate_binary_device_hardware(
    device: &BinaryDeviceConfig,
    workspace_root: &Path,
    process_working_dir: &Path,
    current_exe_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    let executable_name = device
        .executable
        .clone()
        .unwrap_or_else(|| default_device_executable_name(&device.driver));
    let program = resolve_registered_program(&executable_name, workspace_root, current_exe_dir);
    let mut args = vec![OsString::from("validate"), OsString::from(&device.id)];
    for channel in device.channels.iter().filter(|channel| channel.enabled) {
        args.push(OsString::from("--channel-type"));
        args.push(OsString::from(&channel.channel_type));
    }
    args.push(OsString::from("--json"));
    let report = run_driver_json(&program, &args, process_working_dir, VALIDATION_TIMEOUT)?;
    if report
        .get("valid")
        .and_then(Value::as_bool)
        .is_some_and(|valid| !valid)
    {
        return Err(format!(
            "device \"{}\" ({}) is no longer valid",
            device.name, device.id
        )
        .into());
    }
    Ok(())
}

fn build_discovery_config(
    discoveries: &[DiscoveredDevice],
) -> Result<ProjectConfig, Box<dyn Error>> {
    let mut config = ProjectConfig::draft_setup_template();
    let mut default_name_counts = BTreeMap::new();
    let mut arm_index = 0usize;
    let mut eef_index = 0usize;

    for discovery in discoveries {
        if discovery.channel_meta_by_channel.is_empty() {
            return Err(format!(
                "device \"{}\" ({}) exposed no channels in its query response",
                discovery.display_name, discovery.id
            )
            .into());
        }
        // Pick a "preferred mode" per device. The legacy wizard alternated
        // `FreeDrive` / `CommandFollowing` between leader/follower groups
        // based on whether a device was detected as an EEF (dof == 1) or
        // arm. We approximate that here by checking the first robot
        // channel's dof; cameras don't care about mode and take None.
        let preferred_mode = if discovery
            .channel_meta_by_channel
            .values()
            .any(|meta| meta.kind == DeviceType::Robot && meta.dof != Some(1))
        {
            let mode = group_default_mode(arm_index);
            arm_index += 1;
            mode
        } else if discovery
            .channel_meta_by_channel
            .values()
            .any(|meta| meta.kind == DeviceType::Robot)
        {
            let mode = group_default_mode(eef_index);
            eef_index += 1;
            mode
        } else {
            // Pure camera devices don't use the mode field but the unified
            // builder still needs a placeholder value.
            RobotMode::FreeDrive
        };
        let name_base = discovery
            .default_device_name
            .clone()
            .unwrap_or_else(|| discovery.driver.replace('-', "_"));
        let device_name = next_default_device_name(name_base, &mut default_name_counts);
        config.devices.push(binary_device_from_discovery(
            discovery,
            device_name,
            preferred_mode,
            &mut default_name_counts,
        ));
    }

    // Auto-build the default pairings once on discovery to seed the
    // pairing step, but leave the operator free to delete them via `d`
    // and add their own with `m`. Teleop is now the only collection mode
    // the wizard exposes (intervention is removed from the cycle), and
    // teleop with zero pairings is a valid intermediate state — the
    // operator may save the config before they assemble pairings.
    config.pairings = build_default_channel_pairings(&config.devices);
    config.mode = CollectionMode::Teleop;
    config
        .validate()
        .map_err(|e| -> Box<dyn Error> { e.to_string().into() })?;
    Ok(config)
}

fn save_project_config(project: &ProjectConfig, output_path: &Path) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(output_path, toml::to_string_pretty(project)?)?;
    Ok(())
}

fn parse_camera_capabilities(capabilities: &Value) -> Vec<CameraProfile> {
    let pixel_formats = capabilities
        .get("pixel_formats")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value_as_string(Some(value)))
        .filter_map(|value| parse_pixel_format(&value))
        .collect::<Vec<_>>();
    let streams = capabilities
        .get("streams")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value_as_string(Some(value)))
        .collect::<Vec<_>>();

    capabilities
        .get("profiles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|profile| {
            let width = value_as_u32(profile.get("width"))?;
            let height = value_as_u32(profile.get("height"))?;
            let fps = value_as_fps_u32(profile.get("fps"))?;
            let stream =
                value_as_string(profile.get("stream")).or_else(|| streams.first().cloned());
            let channel =
                value_as_u32(profile.get("channel")).or_else(|| value_as_u32(profile.get("index")));
            let pixel_format = value_as_string(profile.get("pixel_format"))
                .or_else(|| value_as_string(profile.get("native_pixel_format")))
                .and_then(|value| parse_pixel_format(&value))
                .or_else(|| stream.as_deref().and_then(infer_stream_pixel_format))
                .or_else(|| pixel_formats.first().copied())
                .unwrap_or(PixelFormat::Rgb24);
            Some(CameraProfile {
                width,
                height,
                fps,
                pixel_format,
                native_pixel_format: value_as_string(profile.get("native_pixel_format")),
                stream,
                channel: channel.filter(|channel| *channel > 0),
            })
        })
        .collect()
}

fn parse_query_robot_modes(channel: &Value) -> Vec<RobotMode> {
    channel
        .get("modes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .filter_map(|mode| match mode {
            "free-drive" => Some(RobotMode::FreeDrive),
            "command-following" => Some(RobotMode::CommandFollowing),
            "identifying" => Some(RobotMode::Identifying),
            "disabled" => Some(RobotMode::Disabled),
            _ => None,
        })
        .collect()
}

fn parse_query_channel_meta(device: &Value) -> BTreeMap<String, DiscoveredChannelMeta> {
    device
        .get("channels")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|channel| {
            let channel_type = value_as_string(channel.get("channel_type"))?;
            let kind = value_as_string(channel.get("kind"))
                .and_then(|s| match s.as_str() {
                    "camera" => Some(DeviceType::Camera),
                    "robot" => Some(DeviceType::Robot),
                    _ => None,
                })
                .unwrap_or(DeviceType::Robot);
            let channel_label = value_as_string(channel.get("channel_label"));
            let default_name = value_as_string(channel.get("default_name"));
            let modes = parse_query_robot_modes(channel);
            let dof = value_as_u32(channel.get("dof"));
            let default_control_frequency_hz =
                value_as_f64(channel.get("default_control_frequency_hz"))
                    .or_else(|| value_as_f64(channel.get("control_frequency_hz")));
            let defaults = parse_query_command_defaults(channel.get("defaults"));
            let profiles = parse_channel_camera_profiles(channel);
            let value_limits =
                crate::device_query::parse_query_value_limits(channel.get("value_limits"));
            let mut supported_states =
                crate::device_query::parse_query_supported_states(channel.get("supported_states"));
            // Fall back to the kinds enumerated by value_limits so older
            // drivers that only populate value_limits still expose a
            // supported-state list to the wizard.
            if supported_states.is_empty() {
                supported_states = value_limits.iter().map(|entry| entry.state_kind).collect();
            }
            let supported_commands = crate::device_query::parse_query_supported_commands(
                channel.get("supported_commands"),
            );
            let direct_joint_compatibility =
                crate::device_query::parse_query_direct_joint_compatibility(
                    channel.get("direct_joint_compatibility"),
                );
            Some((
                channel_type,
                DiscoveredChannelMeta {
                    kind,
                    channel_label,
                    default_name,
                    modes,
                    dof,
                    profiles,
                    default_control_frequency_hz,
                    defaults,
                    value_limits,
                    supported_states,
                    supported_commands,
                    direct_joint_compatibility,
                },
            ))
        })
        .collect()
}

fn parse_channel_camera_profiles(channel: &Value) -> Vec<CameraProfile> {
    let stream = value_as_string(channel.get("channel_type"));
    channel
        .get("profiles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|profile| {
            let width = value_as_u32(profile.get("width"))?;
            let height = value_as_u32(profile.get("height"))?;
            let fps = value_as_u32(profile.get("fps"))
                .or_else(|| value_as_f64(profile.get("fps")).map(|fps| fps.round() as u32))?;
            let pixel_format = value_as_string(profile.get("pixel_format"))
                .and_then(|value| parse_pixel_format_name(&value))
                .unwrap_or(PixelFormat::Rgb24);
            Some(CameraProfile {
                width,
                height,
                fps,
                pixel_format,
                native_pixel_format: value_as_string(profile.get("native_pixel_format")),
                stream: stream.clone(),
                channel: None,
            })
        })
        .collect()
}

fn parse_query_command_defaults(
    value: Option<&Value>,
) -> rollio_types::config::ChannelCommandDefaults {
    let parse_array = |key: &str| -> Vec<f64> {
        value
            .and_then(|v| v.get(key))
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default()
    };
    rollio_types::config::ChannelCommandDefaults {
        joint_mit_kp: parse_array("joint_mit_kp"),
        joint_mit_kd: parse_array("joint_mit_kd"),
        parallel_mit_kp: parse_array("parallel_mit_kp"),
        parallel_mit_kd: parse_array("parallel_mit_kd"),
    }
}

fn query_metadata_string(device: &Value, key: &str) -> Option<String> {
    value_as_string(device.get(key)).or_else(|| {
        device
            .get("optional_info")
            .and_then(|optional| optional.get(key))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    })
}

fn enrich_current_device_from_discovery(
    current: &mut BinaryDeviceConfig,
    discovery: &DiscoveredDevice,
) {
    merge_discovery_extra(&mut current.extra, discovery);
    // Re-merge per-state value_limits from the latest query so a project
    // saved before the driver started reporting limits picks them up on the
    // next setup pass without manual editing.
    for channel in current.channels.iter_mut() {
        if let Some(meta) = discovery.channel_meta_by_channel.get(&channel.channel_type) {
            if !meta.value_limits.is_empty() {
                channel.value_limits = meta.value_limits.clone();
            }
        }
    }
    if !discovery
        .channel_meta_by_channel
        .values()
        .any(|meta| meta.kind == DeviceType::Camera)
    {
        return;
    }
    let camera_profiles = discovery.all_camera_profiles();
    for channel in current.channels.iter_mut() {
        if channel.kind != DeviceType::Camera {
            continue;
        }
        let Some(profile) = channel.profile.as_mut() else {
            continue;
        };
        if profile.native_pixel_format.is_some() {
            continue;
        }
        let matched = camera_profiles.iter().find(|candidate| {
            candidate.width == profile.width
                && candidate.height == profile.height
                && candidate.fps == profile.fps
                && candidate.pixel_format == profile.pixel_format
                && camera_channel_type_for_profile(candidate) == channel.channel_type
        });
        if let Some(matched) = matched {
            profile.native_pixel_format = matched.native_pixel_format.clone();
        }
    }
}

fn merge_discovery_extra(extra: &mut toml::Table, discovery: &DiscoveredDevice) {
    if let Some(transport) = &discovery.transport {
        extra.insert("transport".into(), toml::Value::String(transport.clone()));
    }
    if let Some(interface) = &discovery.interface {
        extra.insert("interface".into(), toml::Value::String(interface.clone()));
    }
    if let Some(product_variant) = &discovery.product_variant {
        extra.insert(
            "product_variant".into(),
            toml::Value::String(product_variant.clone()),
        );
    }
    if let Some(end_effector) = &discovery.end_effector {
        extra.insert(
            "end_effector".into(),
            toml::Value::String(end_effector.clone()),
        );
    }
}

fn parse_pixel_format_name(value: &str) -> Option<PixelFormat> {
    match value {
        "rgb24" => Some(PixelFormat::Rgb24),
        "bgr24" => Some(PixelFormat::Bgr24),
        "yuyv" => Some(PixelFormat::Yuyv),
        "mjpeg" => Some(PixelFormat::Mjpeg),
        "depth16" => Some(PixelFormat::Depth16),
        "gray8" => Some(PixelFormat::Gray8),
        _ => None,
    }
}

fn parse_robot_modes(value: Option<&Value>) -> Vec<RobotMode> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| value_as_string(Some(entry)))
        .filter_map(|mode| match mode.as_str() {
            "free-drive" => Some(RobotMode::FreeDrive),
            "command-following" => Some(RobotMode::CommandFollowing),
            "identifying" => Some(RobotMode::Identifying),
            "disabled" => Some(RobotMode::Disabled),
            _ => None,
        })
        .collect()
}

fn default_supported_robot_modes() -> Vec<RobotMode> {
    vec![
        RobotMode::FreeDrive,
        RobotMode::CommandFollowing,
        RobotMode::Identifying,
        RobotMode::Disabled,
    ]
}

fn parse_pixel_format(value: &str) -> Option<PixelFormat> {
    match value.trim().to_ascii_lowercase().as_str() {
        "rgb24" | "rgb3" => Some(PixelFormat::Rgb24),
        "bgr24" | "bgr3" => Some(PixelFormat::Bgr24),
        "yuyv" | "yuy2" => Some(PixelFormat::Yuyv),
        "mjpeg" | "mjpg" => Some(PixelFormat::Mjpeg),
        "depth16" | "z16" => Some(PixelFormat::Depth16),
        "gray8" | "grey" | "gray" | "y8" => Some(PixelFormat::Gray8),
        _ => None,
    }
}

fn infer_stream_pixel_format(stream: &str) -> Option<PixelFormat> {
    match stream {
        "color" => Some(PixelFormat::Rgb24),
        "depth" => Some(PixelFormat::Depth16),
        "infrared" => Some(PixelFormat::Gray8),
        _ => None,
    }
}

fn value_as_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(ToOwned::to_owned)
}

fn value_as_u32(value: Option<&Value>) -> Option<u32> {
    value
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn value_as_f64(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64)
}

fn value_as_fps_u32(value: Option<&Value>) -> Option<u32> {
    value_as_u32(value).or_else(|| {
        let fps = value_as_f64(value)?;
        if !fps.is_finite() || fps <= 0.0 || fps > u32::MAX as f64 {
            return None;
        }
        Some(fps.round() as u32)
    })
}

fn select_supported_mode(supported_modes: &[RobotMode], preferred: RobotMode) -> RobotMode {
    // Prefer a wizard-selectable mode when available so freshly discovered
    // channels never default to `Identifying`/`Disabled` (those modes
    // exist for the identify flow / channel disable, not for steady-state
    // operation). Falls back to the first advertised mode, then
    // `FreeDrive`, when the driver doesn't list any selectable mode.
    let selectable = wizard_selectable_modes(supported_modes);
    if selectable.contains(&preferred) {
        preferred
    } else if let Some(first) = selectable.first().copied() {
        first
    } else if supported_modes.contains(&preferred) {
        preferred
    } else {
        supported_modes
            .first()
            .copied()
            .unwrap_or(RobotMode::FreeDrive)
    }
}

/// The subset of `RobotMode` values the setup wizard offers via the cycle
/// keys. `Identifying` is set transiently by the identify flow and
/// `Disabled` is set via channel disable / removal — neither is a steady
/// runtime mode the operator should pick from the cycle. Returned in a
/// fixed order (`FreeDrive` first, then `CommandFollowing`) so cycling is
/// predictable across drivers regardless of the order they list modes in.
fn wizard_selectable_modes(supported_modes: &[RobotMode]) -> Vec<RobotMode> {
    [RobotMode::FreeDrive, RobotMode::CommandFollowing]
        .into_iter()
        .filter(|mode| supported_modes.contains(mode))
        .collect()
}

fn device_identity_from_binary(device: &BinaryDeviceConfig) -> DeviceIdentity {
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

fn discovery_camera_channel_key(stream: Option<&str>, channel: Option<u32>) -> String {
    let base = stream.unwrap_or("color").to_string();
    match channel {
        Some(ch) if ch > 0 => format!("{base}_{ch}"),
        _ => base,
    }
}

/// Match the project-side `BinaryDeviceConfig` to a fresh discovery by
/// `(driver, id)` only. Devices may now carry mixed-kind channels under a
/// single config, so matching on a "primary channel type" no longer makes
/// sense; the unified discovery loop emits one row per discovered device
/// regardless of which channels it exposes.
fn device_matches_discovery_binary(
    device: &BinaryDeviceConfig,
    discovery: &DiscoveredDevice,
    _stream: Option<&str>,
    _channel: Option<u32>,
) -> bool {
    device.driver == discovery.driver && device.id == discovery.id
}

fn available_device_key_from_binary(device: &BinaryDeviceConfig) -> String {
    let ch = device
        .channels
        .first()
        .expect("setup devices always include a primary channel");
    let kind = match ch.kind {
        DeviceType::Camera => "camera",
        DeviceType::Robot => "robot",
    };
    format!(
        "{kind}|{}|{}|{}|-",
        device.driver, device.id, ch.channel_type
    )
}

fn next_default_device_name(base: String, counts: &mut BTreeMap<String, usize>) -> String {
    let next_index = counts.entry(base.clone()).or_insert(0);
    let resolved = if *next_index == 0 {
        base.clone()
    } else {
        format!("{base}_{}", *next_index + 1)
    };
    *next_index += 1;
    resolved
}

/// Dedupe the per-channel `default_name` reported by a driver query (e.g.
/// "airbot_play_arm", "airbot_e2") against the same `counts` map the
/// device-name path uses, so two physical AIRBOT Play arms become
/// "airbot_play_arm" and "airbot_play_arm_2" instead of two rows with the
/// same name. Returns `None` when the driver did not advertise a default
/// channel name (callers fall back to the channel_type).
fn dedup_channel_default_name(
    default_name: Option<&str>,
    counts: &mut BTreeMap<String, usize>,
) -> Option<String> {
    let base = default_name?.trim();
    if base.is_empty() {
        return None;
    }
    Some(next_default_device_name(base.to_owned(), counts))
}

fn group_default_mode(index: usize) -> RobotMode {
    match index {
        0 => RobotMode::FreeDrive,
        1 => RobotMode::CommandFollowing,
        _ => RobotMode::FreeDrive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;

    fn project_camera_device_names(p: &ProjectConfig) -> Vec<String> {
        p.devices
            .iter()
            .filter(|d| {
                d.channels
                    .iter()
                    .any(|c| c.kind == DeviceType::Camera && c.enabled)
            })
            .map(|d| d.name.clone())
            .collect()
    }

    fn project_robot_device_names(p: &ProjectConfig) -> Vec<String> {
        p.devices
            .iter()
            .filter(|d| {
                d.channels
                    .iter()
                    .any(|c| c.kind == DeviceType::Robot && c.enabled)
            })
            .map(|d| d.name.clone())
            .collect()
    }

    fn make_robot_modes() -> Vec<RobotMode> {
        vec![
            RobotMode::FreeDrive,
            RobotMode::CommandFollowing,
            RobotMode::Identifying,
            RobotMode::Disabled,
        ]
    }

    fn camera_discovery(id: &str) -> DiscoveredDevice {
        let mut channel_meta_by_channel = BTreeMap::new();
        channel_meta_by_channel.insert(
            "color".to_owned(),
            DiscoveredChannelMeta {
                kind: DeviceType::Camera,
                channel_label: Some("Pseudo Camera".into()),
                default_name: None,
                modes: Vec::new(),
                profiles: vec![CameraProfile {
                    width: 640,
                    height: 480,
                    fps: 30,
                    pixel_format: PixelFormat::Rgb24,
                    native_pixel_format: None,
                    stream: Some("color".into()),
                    channel: None,
                }],
                ..DiscoveredChannelMeta::default()
            },
        );
        DiscoveredDevice {
            driver: "pseudo".into(),
            id: id.into(),
            display_name: id.into(),
            default_device_name: Some("pseudo_camera".into()),
            channel_meta_by_channel,
            transport: Some("simulated".into()),
            interface: None,
            product_variant: None,
            end_effector: None,
        }
    }

    fn robot_discovery(id: &str, dof: u32) -> DiscoveredDevice {
        let default_name = if dof == 1 { "pseudo_eef" } else { "pseudo_arm" };
        let mut channel_meta_by_channel = BTreeMap::new();
        channel_meta_by_channel.insert(
            "arm".to_owned(),
            DiscoveredChannelMeta {
                kind: DeviceType::Robot,
                channel_label: None,
                default_name: Some(default_name.to_owned()),
                modes: make_robot_modes(),
                dof: Some(dof),
                default_control_frequency_hz: Some(60.0),
                ..DiscoveredChannelMeta::default()
            },
        );
        DiscoveredDevice {
            driver: "pseudo".into(),
            id: id.into(),
            display_name: id.into(),
            default_device_name: Some(default_name.to_owned()),
            channel_meta_by_channel,
            transport: Some("simulated".into()),
            interface: None,
            product_variant: None,
            end_effector: None,
        }
    }

    fn airbot_play_discovery(end_effector: Option<&str>) -> DiscoveredDevice {
        let mut channel_meta_by_channel = BTreeMap::new();
        channel_meta_by_channel.insert(
            "arm".to_owned(),
            DiscoveredChannelMeta {
                kind: DeviceType::Robot,
                channel_label: Some("AIRBOT Play".into()),
                default_name: Some("airbot_play_arm".into()),
                modes: make_robot_modes(),
                dof: Some(6),
                default_control_frequency_hz: Some(250.0),
                direct_joint_compatibility: rollio_types::config::DirectJointCompatibility {
                    can_lead: vec![rollio_types::config::DirectJointCompatibilityPeer {
                        driver: "airbot-play".into(),
                        channel_type: "arm".into(),
                    }],
                    can_follow: vec![rollio_types::config::DirectJointCompatibilityPeer {
                        driver: "airbot-play".into(),
                        channel_type: "arm".into(),
                    }],
                },
                ..DiscoveredChannelMeta::default()
            },
        );
        if let Some(channel_type) = end_effector.map(|value| value.to_ascii_lowercase()) {
            let (label, name, defaults) = match channel_type.as_str() {
                "e2" => (
                    "AIRBOT E2",
                    "airbot_e2",
                    rollio_types::config::ChannelCommandDefaults {
                        joint_mit_kp: Vec::new(),
                        joint_mit_kd: Vec::new(),
                        parallel_mit_kp: vec![0.0],
                        parallel_mit_kd: vec![0.0],
                    },
                ),
                "g2" => (
                    "AIRBOT G2",
                    "airbot_g2",
                    rollio_types::config::ChannelCommandDefaults {
                        joint_mit_kp: Vec::new(),
                        joint_mit_kd: Vec::new(),
                        parallel_mit_kp: vec![10.0],
                        parallel_mit_kd: vec![0.5],
                    },
                ),
                _ => (
                    "AIRBOT EEF",
                    "airbot_eef",
                    rollio_types::config::ChannelCommandDefaults::default(),
                ),
            };
            channel_meta_by_channel.insert(
                channel_type,
                DiscoveredChannelMeta {
                    kind: DeviceType::Robot,
                    channel_label: Some(label.into()),
                    default_name: Some(name.into()),
                    modes: make_robot_modes(),
                    dof: Some(1),
                    default_control_frequency_hz: Some(250.0),
                    defaults,
                    ..DiscoveredChannelMeta::default()
                },
            );
        }
        DiscoveredDevice {
            driver: "airbot-play".into(),
            id: "PZ123".into(),
            display_name: "AIRBOT Play".into(),
            default_device_name: Some("airbot_play".into()),
            channel_meta_by_channel,
            transport: Some("can".into()),
            interface: Some("can0".into()),
            product_variant: Some("play-e2".into()),
            end_effector: end_effector.map(str::to_owned),
        }
    }

    fn setup_session(discoveries: &[DiscoveredDevice]) -> SetupSession {
        let config = build_discovery_config(discoveries).expect("config should build");
        let available_devices = available_devices_from_discoveries(discoveries, &config)
            .expect("available devices should build");
        SetupSession::new(
            config,
            available_devices,
            std::path::PathBuf::from("config.toml"),
            false,
            Vec::new(),
        )
    }

    #[test]
    fn build_discovery_config_assigns_default_roles_and_pairing() {
        let config = build_discovery_config(&[
            camera_discovery("cam0"),
            camera_discovery("cam1"),
            robot_discovery("robot0", 6),
            robot_discovery("robot1", 6),
            robot_discovery("eef0", 1),
            robot_discovery("eef1", 1),
        ])
        .expect("config should build");

        assert_eq!(
            project_camera_device_names(&config),
            vec!["pseudo_camera", "pseudo_camera_2"]
        );
        assert_eq!(
            project_robot_device_names(&config),
            vec!["pseudo_arm", "pseudo_arm_2", "pseudo_eef", "pseudo_eef_2"]
        );
        assert_eq!(config.pairings.len(), 2);
        assert_eq!(config.pairings[0].leader_device, "pseudo_arm");
        assert_eq!(config.pairings[0].follower_device, "pseudo_arm_2");
        assert_eq!(config.pairings[1].leader_device, "pseudo_eef");
        assert_eq!(config.pairings[1].follower_device, "pseudo_eef_2");
    }

    // (Legacy `parse_camera_capabilities_*` and `normalize_camera_profiles_*`
    // tests removed alongside their helper functions: profiles now flow
    // through `parse_query_channel_meta` directly from the driver's
    // `query --json`, and the v4l2 special-case is dead code because the
    // driver itself reports normalized RGB24/BGR24.)

    #[test]
    fn available_devices_from_discoveries_merges_airbot_interface_into_existing_config() {
        let discovery = airbot_play_discovery(Some("e2"));
        let mut config =
            build_discovery_config(std::slice::from_ref(&discovery)).expect("config should build");
        config.devices[0].extra.clear();

        let available = available_devices_from_discoveries(&[discovery], &config)
            .expect("available devices should build");

        assert_eq!(
            available[0]
                .current
                .extra
                .get("interface")
                .and_then(|value| value.as_str()),
            Some("can0")
        );
        assert_eq!(
            available[0]
                .current
                .extra
                .get("end_effector")
                .and_then(|value| value.as_str()),
            Some("e2")
        );
    }

    #[test]
    fn build_discovery_config_dedupes_robot_channel_names_across_two_airbot_devices() {
        // Two physical AIRBOT Play arms each report
        // `default_name = "airbot_play_arm"` from the driver query. The
        // discovery path must dedupe the channel-level name the same way it
        // dedupes the device-level name, otherwise the wizard's devices step
        // shows two rows with identical names and the operator can't tell
        // them apart.
        let mut leader = airbot_play_discovery(Some("e2"));
        leader.id = "PZ_LEADER".into();
        let mut follower = airbot_play_discovery(Some("e2"));
        follower.id = "PZ_FOLLOWER".into();

        let config = build_discovery_config(&[leader, follower]).expect("config should build");

        let arm_names: Vec<&str> = config
            .devices
            .iter()
            .filter(|device| device.driver == "airbot-play")
            .filter_map(|device| {
                device
                    .channel_named("arm")
                    .and_then(|channel| channel.name.as_deref())
            })
            .collect();
        assert_eq!(
            arm_names,
            vec!["airbot_play_arm", "airbot_play_arm_2"],
            "two airbot arms must get distinct channel names",
        );

        let eef_names: Vec<&str> = config
            .devices
            .iter()
            .filter(|device| device.driver == "airbot-play")
            .filter_map(|device| {
                device
                    .channel_named("e2")
                    .and_then(|channel| channel.name.as_deref())
            })
            .collect();
        assert_eq!(
            eef_names,
            vec!["airbot_e2", "airbot_e2_2"],
            "two airbot end-effector channels must get distinct names",
        );
    }

    #[test]
    fn available_devices_from_discoveries_splits_airbot_channels_into_rows() {
        let discovery = airbot_play_discovery(Some("e2"));
        let config =
            build_discovery_config(std::slice::from_ref(&discovery)).expect("config should build");

        let available = available_devices_from_discoveries(&[discovery], &config)
            .expect("available devices should build");

        assert_eq!(available.len(), 2);
        assert_eq!(available[0].current.channels.len(), 1);
        let arm_channel = &available[0].current.channels[0];
        assert_eq!(arm_channel.channel_type, "arm");
        assert_eq!(available[0].display_name, "AIRBOT Play");
        assert_eq!(arm_channel.channel_label.as_deref(), Some("AIRBOT Play"));
        assert_eq!(arm_channel.name.as_deref(), Some("airbot_play_arm"));

        assert_eq!(available[1].current.channels.len(), 1);
        let eef_channel = &available[1].current.channels[0];
        assert_eq!(eef_channel.channel_type, "e2");
        assert_eq!(available[1].display_name, "AIRBOT E2");
        assert_eq!(eef_channel.channel_label.as_deref(), Some("AIRBOT E2"));
        assert_eq!(eef_channel.name.as_deref(), Some("airbot_e2"));
        // The two rows share the same parent BinaryDeviceConfig.name (= bus
        // root / iceoryx2 service root), but their per-channel `name` fields
        // are independent so renaming one row no longer affects the other.
        assert_eq!(available[0].current.name, available[1].current.name);
        assert_ne!(arm_channel.name, eef_channel.name);
    }

    #[test]
    fn toggle_device_selection_disables_only_selected_airbot_channel() {
        let mut session = setup_session(&[airbot_play_discovery(Some("e2"))]);
        let e2_name = session
            .available_devices
            .iter()
            .find(|device| device.current.channels[0].channel_type == "e2")
            .expect("e2 row should exist")
            .name
            .clone();

        assert!(session
            .toggle_device_selection(&e2_name)
            .expect("toggle should succeed"));

        let device = session
            .config
            .devices
            .iter()
            .find(|device| device.driver == "airbot-play")
            .expect("physical airbot device should remain configured");
        assert!(device
            .channel_named("arm")
            .is_some_and(|channel| channel.enabled));
        assert!(device
            .channel_named("e2")
            .is_some_and(|channel| !channel.enabled));
    }

    /// `airbot_play_discovery` plus an explicit `EndEffectorPose` in
    /// supported_states. Used to exercise the new toggle commands so the
    /// fixture has enough surface area to flip kinds on and off.
    fn airbot_arm_discovery_with_supported_states(
        supported: Vec<RobotStateKind>,
    ) -> DiscoveredDevice {
        let mut discovery = airbot_play_discovery(None);
        discovery.channel_meta_by_channel.insert(
            "arm".into(),
            DiscoveredChannelMeta {
                kind: DeviceType::Robot,
                channel_label: Some("AIRBOT Play".into()),
                default_name: Some("airbot_play_arm".into()),
                modes: make_robot_modes(),
                dof: Some(6),
                default_control_frequency_hz: Some(250.0),
                supported_states: supported,
                ..DiscoveredChannelMeta::default()
            },
        );
        discovery
    }

    #[test]
    fn binary_device_from_discovery_defaults_publish_states_to_all_supported() {
        // The driver advertises EndEffectorPose alongside the standard
        // joint kinds; the wizard should opt all of them into both
        // publish_states and recorded_states by default so operators don't
        // hit the FK/IK pairing failure when switching mappings.
        let discovery = airbot_arm_discovery_with_supported_states(vec![
            RobotStateKind::JointPosition,
            RobotStateKind::JointVelocity,
            RobotStateKind::JointEffort,
            RobotStateKind::EndEffectorPose,
        ]);
        let device = binary_device_from_discovery(
            &discovery,
            "airbot_play".into(),
            RobotMode::FreeDrive,
            &mut BTreeMap::new(),
        );
        let arm = device
            .channel_named("arm")
            .expect("arm channel should exist");
        assert!(arm
            .publish_states
            .contains(&RobotStateKind::EndEffectorPose));
        assert_eq!(arm.publish_states, arm.recorded_states);
    }

    #[test]
    fn toggle_publish_state_drops_recorded_kind_with_it() {
        let discovery = airbot_arm_discovery_with_supported_states(vec![
            RobotStateKind::JointPosition,
            RobotStateKind::JointVelocity,
            RobotStateKind::JointEffort,
        ]);
        let mut session = setup_session(&[discovery]);
        let arm_name = session
            .available_devices
            .iter()
            .find(|device| device.current.channels[0].channel_type == "arm")
            .expect("arm row should exist")
            .name
            .clone();

        assert!(session
            .toggle_publish_state(&arm_name, RobotStateKind::JointEffort)
            .expect("toggle should succeed"));

        let arm = session
            .config
            .device_named("airbot_play")
            .and_then(|device| device.channel_named("arm"))
            .expect("arm channel still configured");
        assert!(!arm.publish_states.contains(&RobotStateKind::JointEffort));
        assert!(
            !arm.recorded_states.contains(&RobotStateKind::JointEffort),
            "recorded_states must stay a subset of publish_states",
        );
    }

    #[test]
    fn toggle_publish_state_blocks_removal_when_pairing_uses_kind() {
        let discovery = airbot_arm_discovery_with_supported_states(vec![
            RobotStateKind::JointPosition,
            RobotStateKind::JointVelocity,
            RobotStateKind::JointEffort,
        ]);
        // Two arms so a default pairing exists with leader_state = JointPosition.
        let mut session = setup_session(&[discovery.clone(), discovery]);
        let leader_name = session
            .config
            .pairings
            .first()
            .expect("default pairing should exist")
            .leader_device
            .clone();
        let leader_row = session
            .available_devices
            .iter()
            .find(|device| {
                device.current.name == leader_name
                    && device.current.channels[0].channel_type == "arm"
            })
            .expect("leader arm row should exist")
            .name
            .clone();

        let outcome = session
            .toggle_publish_state(&leader_row, RobotStateKind::JointPosition)
            .expect("call should not error");
        assert!(
            !outcome,
            "toggling off a leader_state must be rejected without applying the change",
        );
        let arm = session
            .config
            .device_named(&leader_name)
            .and_then(|device| device.channel_named("arm"))
            .expect("leader arm still configured");
        assert!(arm.publish_states.contains(&RobotStateKind::JointPosition));
        assert!(session.message.is_some(), "user should see a clear refusal");
    }

    #[test]
    fn toggle_publish_state_mirrors_into_available_devices_snapshot() {
        // Regression for a UI bug: the wizard reads publish_states /
        // recorded_states from `AvailableDevice.current.channels[0]`, so a
        // toggle that only updates `config.devices` leaves the rendered
        // [P R] glyph stale. Toggle methods must mirror the freshly
        // mutated kind into the AvailableDevice snapshot, the way
        // `cycle_robot_mode` does for the channel mode.
        let discovery = airbot_arm_discovery_with_supported_states(vec![
            RobotStateKind::JointPosition,
            RobotStateKind::JointVelocity,
            RobotStateKind::JointEffort,
        ]);
        let mut session = setup_session(&[discovery]);
        let arm_name = session
            .available_devices
            .iter()
            .find(|device| device.current.channels[0].channel_type == "arm")
            .expect("arm row should exist")
            .name
            .clone();

        assert!(session
            .toggle_publish_state(&arm_name, RobotStateKind::JointEffort)
            .expect("publish toggle should succeed"));

        let available_channel = session
            .available_devices
            .iter()
            .find(|device| device.name == arm_name)
            .and_then(|device| device.current.channels.first())
            .expect("available arm row should still exist");
        assert!(
            !available_channel
                .publish_states
                .contains(&RobotStateKind::JointEffort),
            "AvailableDevice snapshot must mirror the updated publish_states; \
             got {:?}",
            available_channel.publish_states,
        );
        assert!(
            !available_channel
                .recorded_states
                .contains(&RobotStateKind::JointEffort),
            "AvailableDevice snapshot must mirror the updated recorded_states",
        );

        assert!(session
            .toggle_recorded_state(&arm_name, RobotStateKind::JointVelocity)
            .expect("recorded toggle should succeed"));
        let available_channel = session
            .available_devices
            .iter()
            .find(|device| device.name == arm_name)
            .and_then(|device| device.current.channels.first())
            .expect("available arm row should still exist");
        assert!(
            !available_channel
                .recorded_states
                .contains(&RobotStateKind::JointVelocity),
            "recorded_state toggle should also propagate; got {:?}",
            available_channel.recorded_states,
        );
    }

    #[test]
    fn toggle_recorded_state_requires_published_kind() {
        let discovery = airbot_arm_discovery_with_supported_states(vec![
            RobotStateKind::JointPosition,
            RobotStateKind::JointVelocity,
            RobotStateKind::JointEffort,
        ]);
        let mut session = setup_session(&[discovery]);
        let arm_name = session
            .available_devices
            .iter()
            .find(|device| device.current.channels[0].channel_type == "arm")
            .expect("arm row should exist")
            .name
            .clone();

        // Drop joint_effort from publish_states (and implicitly from recorded_states)
        // and then attempt to record it: the wizard should reject the toggle.
        assert!(session
            .toggle_publish_state(&arm_name, RobotStateKind::JointEffort)
            .expect("publish toggle should succeed"));
        let outcome = session
            .toggle_recorded_state(&arm_name, RobotStateKind::JointEffort)
            .expect("call should not error");
        assert!(!outcome, "recording a non-published kind must be rejected",);
        let arm = session
            .config
            .device_named("airbot_play")
            .and_then(|device| device.channel_named("arm"))
            .expect("arm channel still configured");
        assert!(!arm.recorded_states.contains(&RobotStateKind::JointEffort));
    }

    #[test]
    fn preview_runtime_project_overrides_visualizer_port() {
        let config = build_discovery_config(&[
            camera_discovery("cam0"),
            camera_discovery("cam1"),
            robot_discovery("robot0", 6),
            robot_discovery("robot1", 6),
        ])
        .expect("config should build");

        let mut preview = config.clone();
        preview.visualizer.port = 42424;

        assert_eq!(preview.visualizer.port, 42424);
        assert_eq!(
            project_camera_device_names(&preview),
            project_camera_device_names(&config)
        );
        assert_eq!(
            project_robot_device_names(&preview),
            project_robot_device_names(&config)
        );
    }

    #[test]
    fn visible_steps_include_pairing_for_teleop_default() {
        // Teleop is now the only collection mode the wizard exposes — the
        // default discovery config lands on it directly so the Pairing
        // step is always visible.
        let session = setup_session(&[camera_discovery("cam0"), robot_discovery("robot0", 6)]);

        assert_eq!(session.config.mode, CollectionMode::Teleop);
        assert_eq!(
            session.visible_steps(),
            &[
                SetupStep::Devices,
                SetupStep::Storage,
                SetupStep::Pairing,
                SetupStep::States,
                SetupStep::Preview,
            ]
        );
    }

    #[test]
    fn visible_steps_drop_pairing_for_legacy_intervention_configs() {
        // Older saved configs may explicitly set `mode = "intervention"`;
        // round-trip through the wizard still hides the Pairing step in
        // that case so the operator can review/save without seeing a
        // step they no longer use.
        let mut session = setup_session(&[camera_discovery("cam0"), robot_discovery("robot0", 6)]);
        session.config.mode = CollectionMode::Intervention;
        assert_eq!(
            session.visible_steps(),
            &[
                SetupStep::Devices,
                SetupStep::Storage,
                SetupStep::States,
                SetupStep::Preview,
            ]
        );
    }

    #[test]
    fn jump_to_step_maps_legacy_device_stage_names_to_devices() {
        let mut session = setup_session(&[camera_discovery("cam0")]);

        session.current_step = SetupStep::Preview;
        assert!(session.jump_to_step("discovery"));
        assert_eq!(session.current_step, SetupStep::Devices);

        session.current_step = SetupStep::Preview;
        assert!(session.jump_to_step("selection"));
        assert_eq!(session.current_step, SetupStep::Devices);

        session.current_step = SetupStep::Preview;
        assert!(session.jump_to_step("parameters"));
        assert_eq!(session.current_step, SetupStep::Devices);

        session.current_step = SetupStep::Preview;
        assert!(session.jump_to_step("devices"));
        assert_eq!(session.current_step, SetupStep::Devices);
    }

    #[test]
    fn deselecting_identified_device_clears_identify_target() {
        let mut session = setup_session(&[camera_discovery("cam0"), robot_discovery("robot0", 6)]);
        let device_name = session.available_devices[0].name.clone();

        assert!(session.is_device_selected(&device_name));
        session.message = Some(format!("{IDENTIFY_ACTIVE_MESSAGE_PREFIX}{device_name}"));
        assert!(session.set_identify_device(Some(&device_name)));
        assert_eq!(
            session.identify_device_name.as_deref(),
            Some(device_name.as_str())
        );

        assert!(session
            .toggle_device_selection(&device_name)
            .expect("deselect should succeed"));
        assert!(!session.is_device_selected(&device_name));
        assert!(session.identify_device_name.is_none());
        assert!(session.message.is_none());
    }

    #[test]
    fn clearing_identify_target_removes_identify_message() {
        let mut session = setup_session(&[camera_discovery("cam0")]);
        let device_name = session.available_devices[0].name.clone();

        session.message = Some(format!("{IDENTIFY_ACTIVE_MESSAGE_PREFIX}{device_name}"));
        assert!(session.set_identify_device(Some(&device_name)));
        assert!(session.set_identify_device(None));
        assert!(session.identify_device_name.is_none());
        assert!(session.message.is_none());
    }

    #[test]
    fn setup_toggle_identify_ignores_unselected_devices() {
        let mut session = setup_session(&[camera_discovery("cam0"), robot_discovery("robot0", 6)]);
        let device_name = session.available_devices[0].name.clone();

        session
            .toggle_device_selection(&device_name)
            .expect("deselect should succeed");

        let mutation = session
            .apply_raw_command(
                &json!({
                    "type": "command",
                    "action": "setup_toggle_identify",
                    "name": device_name,
                })
                .to_string(),
            )
            .expect("identify command should parse");

        assert!(!mutation.state_changed);
        assert!(session.identify_device_name.is_none());
    }

    #[test]
    fn known_device_executables_skip_pseudo_and_standalone_eef_by_default() {
        let executables = known_device_executables()
            .iter()
            .copied()
            .collect::<Vec<_>>();

        assert_eq!(
            executables,
            vec![
                "rollio-device-airbot-play",
                "rollio-device-realsense",
                "rollio-device-v4l2",
                "rollio-device-agx-nero",
            ]
        );
        assert!(
            !executables.contains(&"rollio-device-pseudo"),
            "pseudo must stay opt-in via --sim-pseudo"
        );
    }

    #[cfg(unix)]
    #[test]
    fn interrupt_exit_statuses_are_treated_as_shutdown() {
        let sigint_status = std::process::ExitStatus::from_raw(SIGINT);
        let code_130_status = std::process::ExitStatus::from_raw(130 << 8);
        let code_143_status = std::process::ExitStatus::from_raw(143 << 8);
        let normal_error_status = std::process::ExitStatus::from_raw(1 << 8);

        assert!(is_interrupt_exit_status(&sigint_status));
        assert!(is_interrupt_exit_status(&code_130_status));
        assert!(is_interrupt_exit_status(&code_143_status));
        assert!(!is_interrupt_exit_status(&normal_error_status));
    }

    #[cfg(unix)]
    #[test]
    fn child_interrupt_trigger_is_not_treated_as_crash() {
        let trigger = crate::ShutdownTrigger::ChildExited {
            id: "setup-ui".into(),
            status: std::process::ExitStatus::from_raw(130 << 8),
        };

        assert!(should_treat_trigger_as_shutdown(&trigger, false, false));
    }

    fn v4l2_discovery(id: &str) -> DiscoveredDevice {
        let mut channel_meta_by_channel = BTreeMap::new();
        channel_meta_by_channel.insert(
            "color".to_owned(),
            DiscoveredChannelMeta {
                kind: DeviceType::Camera,
                channel_label: Some("V4L2 Camera".into()),
                default_name: Some("camera".into()),
                profiles: vec![CameraProfile {
                    width: 640,
                    height: 480,
                    fps: 30,
                    pixel_format: PixelFormat::Rgb24,
                    native_pixel_format: Some("MJPG".into()),
                    stream: Some("color".into()),
                    channel: None,
                }],
                ..DiscoveredChannelMeta::default()
            },
        );
        DiscoveredDevice {
            driver: "v4l2".into(),
            id: id.into(),
            display_name: "V4L2 Camera".into(),
            default_device_name: Some("camera".into()),
            channel_meta_by_channel,
            transport: None,
            interface: None,
            product_variant: None,
            end_effector: None,
        }
    }

    fn realsense_multi_stream_discovery(id: &str) -> DiscoveredDevice {
        let mut channel_meta_by_channel = BTreeMap::new();
        channel_meta_by_channel.insert(
            "color".to_owned(),
            DiscoveredChannelMeta {
                kind: DeviceType::Camera,
                channel_label: Some("Intel RealSense RGB".into()),
                default_name: Some("realsense_rgb".into()),
                profiles: vec![CameraProfile {
                    width: 1920,
                    height: 1080,
                    fps: 30,
                    pixel_format: PixelFormat::Rgb24,
                    native_pixel_format: None,
                    stream: Some("color".into()),
                    channel: None,
                }],
                ..DiscoveredChannelMeta::default()
            },
        );
        channel_meta_by_channel.insert(
            "depth".to_owned(),
            DiscoveredChannelMeta {
                kind: DeviceType::Camera,
                channel_label: Some("Intel RealSense Depth".into()),
                default_name: Some("realsense_depth".into()),
                profiles: vec![CameraProfile {
                    width: 640,
                    height: 480,
                    fps: 30,
                    pixel_format: PixelFormat::Depth16,
                    native_pixel_format: None,
                    stream: Some("depth".into()),
                    channel: None,
                }],
                ..DiscoveredChannelMeta::default()
            },
        );
        channel_meta_by_channel.insert(
            "infrared".to_owned(),
            DiscoveredChannelMeta {
                kind: DeviceType::Camera,
                channel_label: Some("Intel RealSense Infrared".into()),
                default_name: Some("realsense_ir".into()),
                profiles: vec![CameraProfile {
                    width: 640,
                    height: 480,
                    fps: 30,
                    pixel_format: PixelFormat::Gray8,
                    native_pixel_format: None,
                    stream: Some("infrared".into()),
                    channel: None,
                }],
                ..DiscoveredChannelMeta::default()
            },
        );
        DiscoveredDevice {
            driver: "realsense".into(),
            id: id.into(),
            display_name: "Intel RealSense".into(),
            default_device_name: Some("realsense".into()),
            channel_meta_by_channel,
            transport: None,
            interface: None,
            product_variant: None,
            end_effector: None,
        }
    }

    fn camera_channel_names(device: &BinaryDeviceConfig) -> Vec<Option<String>> {
        device.channels.iter().map(|c| c.name.clone()).collect()
    }

    fn camera_channel_types(device: &BinaryDeviceConfig) -> Vec<String> {
        device
            .channels
            .iter()
            .map(|c| c.channel_type.clone())
            .collect()
    }

    /// Regression for issue #1: when two V4L2 cameras are discovered, the
    /// channel name for the second one used to be set from the V4L2 driver's
    /// `default_name = "camera"` and was *not* deduplicated, so both setup
    /// rows showed `name=camera` and the user couldn't tell them apart in
    /// the wizard.
    #[test]
    fn build_discovery_config_dedupes_channel_name_for_two_v4l2_cameras() {
        let config =
            build_discovery_config(&[v4l2_discovery("/dev/video0"), v4l2_discovery("/dev/video2")])
                .expect("config should build");

        assert_eq!(
            project_camera_device_names(&config),
            vec!["camera", "camera_2"]
        );
        assert_eq!(
            camera_channel_names(&config.devices[0]),
            vec![Some("camera".to_string())]
        );
        assert_eq!(
            camera_channel_names(&config.devices[1]),
            vec![Some("camera_2".to_string())]
        );
    }

    /// Regression for issue #3: a RealSense unit reports color + depth +
    /// infrared in its `query --json` output, but `build_discovery_config`
    /// used to keep only the first camera profile, so the wizard showed
    /// just one `color` channel and depth / infrared were silently dropped.
    #[test]
    fn build_discovery_config_keeps_all_realsense_streams() {
        let config = build_discovery_config(&[realsense_multi_stream_discovery("332322071743")])
            .expect("config should build");

        assert_eq!(config.devices.len(), 1);
        let device = &config.devices[0];
        assert_eq!(device.driver, "realsense");
        assert_eq!(device.name, "realsense");
        assert_eq!(device.bus_root, "realsense");
        assert_eq!(
            camera_channel_types(device),
            vec![
                "color".to_string(),
                "depth".to_string(),
                "infrared".to_string()
            ]
        );
        assert_eq!(
            camera_channel_names(device),
            vec![
                Some("realsense_rgb".to_string()),
                Some("realsense_depth".to_string()),
                Some("realsense_ir".to_string()),
            ]
        );
    }

    /// Multi-channel + multi-device: two RealSense units both produce 3
    /// channels and the device-level dedup counter must produce
    /// `realsense` / `realsense_2`, not `realsense` / `realsense`.
    #[test]
    fn build_discovery_config_dedupes_multi_channel_devices() {
        let config = build_discovery_config(&[
            realsense_multi_stream_discovery("332322071743"),
            realsense_multi_stream_discovery("332322071744"),
        ])
        .expect("config should build");

        assert_eq!(config.devices.len(), 2);
        assert_eq!(config.devices[0].name, "realsense");
        assert_eq!(config.devices[1].name, "realsense_2");
        assert_eq!(camera_channel_types(&config.devices[0]).len(), 3);
        assert_eq!(camera_channel_types(&config.devices[1]).len(), 3);
    }

    /// `group_camera_profiles_by_channel` must pick the highest-resolution
    /// + highest-fps profile per channel as the wizard's default, even when
    /// the discovery happens to list lower-quality profiles first. We
    /// construct a discovery where the first listed profile per channel is
    /// the worst one and assert that each channel ends up defaulting to
    /// the best one (largest pixel count, then highest fps).
    #[test]
    fn build_discovery_config_picks_highest_resolution_and_fps_default_profile() {
        let mut channel_meta_by_channel = BTreeMap::new();
        channel_meta_by_channel.insert(
            "color".to_owned(),
            DiscoveredChannelMeta {
                kind: DeviceType::Camera,
                channel_label: Some("Intel RealSense RGB".into()),
                default_name: Some("realsense_rgb".into()),
                profiles: vec![
                    CameraProfile {
                        width: 640,
                        height: 480,
                        fps: 30,
                        pixel_format: PixelFormat::Rgb24,
                        native_pixel_format: None,
                        stream: Some("color".into()),
                        channel: None,
                    },
                    CameraProfile {
                        width: 1280,
                        height: 720,
                        fps: 60,
                        pixel_format: PixelFormat::Rgb24,
                        native_pixel_format: None,
                        stream: Some("color".into()),
                        channel: None,
                    },
                    CameraProfile {
                        width: 1920,
                        height: 1080,
                        fps: 30,
                        pixel_format: PixelFormat::Rgb24,
                        native_pixel_format: None,
                        stream: Some("color".into()),
                        channel: None,
                    },
                    CameraProfile {
                        width: 1920,
                        height: 1080,
                        fps: 60,
                        pixel_format: PixelFormat::Rgb24,
                        native_pixel_format: None,
                        stream: Some("color".into()),
                        channel: None,
                    },
                ],
                ..DiscoveredChannelMeta::default()
            },
        );
        channel_meta_by_channel.insert(
            "depth".to_owned(),
            DiscoveredChannelMeta {
                kind: DeviceType::Camera,
                channel_label: Some("Intel RealSense Depth".into()),
                default_name: Some("realsense_depth".into()),
                profiles: vec![
                    CameraProfile {
                        width: 640,
                        height: 480,
                        fps: 30,
                        pixel_format: PixelFormat::Depth16,
                        native_pixel_format: None,
                        stream: Some("depth".into()),
                        channel: None,
                    },
                    CameraProfile {
                        width: 640,
                        height: 480,
                        fps: 90,
                        pixel_format: PixelFormat::Depth16,
                        native_pixel_format: None,
                        stream: Some("depth".into()),
                        channel: None,
                    },
                ],
                ..DiscoveredChannelMeta::default()
            },
        );
        let discovery = DiscoveredDevice {
            driver: "realsense".into(),
            id: "best-default".into(),
            display_name: "Intel RealSense".into(),
            default_device_name: Some("realsense".into()),
            channel_meta_by_channel,
            transport: None,
            interface: None,
            product_variant: None,
            end_effector: None,
        };
        let config =
            build_discovery_config(std::slice::from_ref(&discovery)).expect("config should build");

        assert_eq!(config.devices.len(), 1);
        let device = &config.devices[0];
        assert_eq!(
            camera_channel_types(device),
            vec!["color".to_string(), "depth".to_string()]
        );
        let color = device.channels[0]
            .profile
            .as_ref()
            .expect("color channel must have a default profile");
        assert_eq!(
            (color.width, color.height, color.fps),
            (1920, 1080, 60),
            "color default must be highest-resolution + highest-fps"
        );
        let depth = device.channels[1]
            .profile
            .as_ref()
            .expect("depth channel must have a default profile");
        assert_eq!(
            (depth.width, depth.height, depth.fps),
            (640, 480, 90),
            "depth default must keep its highest-fps profile"
        );
    }

    #[test]
    fn missing_value_limit_warnings_flags_robot_channels_with_no_driver_limits() {
        // The pseudo-style discovery in tests carries no value_limits because
        // its `DiscoveredChannelMeta` is empty by default. This mirrors the
        // case where a real driver has not been updated to report limits yet.
        let config =
            build_discovery_config(&[robot_discovery("robot0", 6)]).expect("config should build");

        let warnings = missing_value_limit_warnings(&config);
        // Three publish_states (joint position/velocity/effort) → three
        // distinct warnings, one per kind.
        assert_eq!(
            warnings.len(),
            3,
            "expected one warning per missing kind, got: {:?}",
            warnings
        );
        for kind in ["joint_position", "joint_velocity", "joint_effort"] {
            assert!(
                warnings.iter().any(|w| w.contains(kind)),
                "missing warning for {kind}: {warnings:?}"
            );
        }
    }

    #[test]
    fn missing_value_limit_warnings_silent_when_driver_supplied_limits() {
        let mut config =
            build_discovery_config(&[robot_discovery("robot0", 6)]).expect("config should build");
        // Simulate the post-`enrich_current_device_from_discovery` state by
        // populating value_limits on every published kind.
        for device in &mut config.devices {
            for channel in &mut device.channels {
                channel.value_limits = channel
                    .publish_states
                    .iter()
                    .map(|kind| {
                        rollio_types::config::StateValueLimitsEntry::symmetric(
                            *kind,
                            std::f64::consts::PI,
                            channel.dof.unwrap_or(1) as usize,
                        )
                    })
                    .collect();
            }
        }
        assert!(
            missing_value_limit_warnings(&config).is_empty(),
            "no warnings expected when limits are present"
        );
    }

    /// `setup_set_ui_http_host` should accept a new host string and persist
    /// it to `config.ui.http_host`. An empty value is rejected with a
    /// descriptive message instead of silently committing a useless config.
    #[test]
    fn set_ui_http_host_rejects_empty_and_persists_valid_values() {
        let mut session = setup_session(&[camera_discovery("cam0")]);
        // Default value comes from rollio_types::default_ui_http_host
        // which now binds to all interfaces by default.
        assert_eq!(session.config.ui.http_host, "0.0.0.0");

        // A trimmed-empty value must not mutate the field and should set a
        // visible error message for the wizard footer.
        let changed = session
            .set_ui_http_host("   ")
            .expect("empty input should be reported via message, not error");
        assert!(!changed, "blank UI host must not be persisted");
        assert_eq!(session.config.ui.http_host, "0.0.0.0");
        assert_eq!(
            session.message.as_deref(),
            Some("UI host must not be empty.")
        );

        // A valid value updates the field; identical re-submissions are
        // a no-op so the wizard doesn't re-broadcast unchanged state.
        session.message = None;
        let changed = session
            .set_ui_http_host("127.0.0.1")
            .expect("valid host should be accepted");
        assert!(changed);
        assert_eq!(session.config.ui.http_host, "127.0.0.1");

        let changed = session
            .set_ui_http_host("127.0.0.1")
            .expect("repeated host should be a no-op");
        assert!(!changed);
    }

    /// The combined codec-backend cycle exposes `(codec, backend)` pairs for
    /// every libav-backed RGB option so the operator can pick a specific
    /// encoder implementation directly. RVL is intentionally excluded — it
    /// would silently fall back to the libav codec for non-depth frames.
    #[test]
    fn cycle_video_codec_walks_codec_backend_pairs_in_order() {
        let mut session = setup_session(&[camera_discovery("cam0")]);
        // Snap to the first option in the table so we can predict the cycle
        // order regardless of the saved default.
        session.config.encoder.video_codec = EncoderCodec::H264;
        session.config.encoder.video_backend = EncoderBackend::Cpu;

        let expected = [
            (EncoderCodec::H264, EncoderBackend::Nvidia),
            (EncoderCodec::H264, EncoderBackend::Vaapi),
            (EncoderCodec::H265, EncoderBackend::Cpu),
            (EncoderCodec::H265, EncoderBackend::Nvidia),
            (EncoderCodec::H265, EncoderBackend::Vaapi),
            (EncoderCodec::Av1, EncoderBackend::Cpu),
            (EncoderCodec::Av1, EncoderBackend::Nvidia),
            (EncoderCodec::Av1, EncoderBackend::Vaapi),
            (EncoderCodec::H264, EncoderBackend::Cpu),
        ];
        for (i, (codec, backend)) in expected.iter().enumerate() {
            session
                .cycle_video_codec(1)
                .unwrap_or_else(|err| panic!("cycle step {i} failed: {err}"));
            assert_eq!(session.config.encoder.video_codec, *codec, "step {i}");
            assert_eq!(session.config.encoder.video_backend, *backend, "step {i}");
        }
    }

    /// Depth cycle leads with RVL (the lossless in-repo default) and then
    /// walks the libav (codec, backend) pairs. The wizard relies on this
    /// ordering so the first forward cycle from the default puts the
    /// operator on a familiar libav option.
    #[test]
    fn cycle_depth_codec_includes_rvl_and_libav_backends() {
        let mut session = setup_session(&[camera_discovery("cam0")]);
        session.config.encoder.depth_codec = EncoderCodec::Rvl;
        session.config.encoder.depth_backend = EncoderBackend::Cpu;

        session
            .cycle_depth_codec(1)
            .expect("forward cycle should succeed");
        assert_eq!(session.config.encoder.depth_codec, EncoderCodec::H264);
        assert_eq!(session.config.encoder.depth_backend, EncoderBackend::Cpu);

        // Walk back to land on RVL again, proving it's reachable from both
        // directions and the wrap-around is correct.
        session
            .cycle_depth_codec(-1)
            .expect("reverse cycle should succeed");
        assert_eq!(session.config.encoder.depth_codec, EncoderCodec::Rvl);
        assert_eq!(session.config.encoder.depth_backend, EncoderBackend::Cpu);
    }

    #[test]
    fn wizard_selectable_modes_keeps_only_steady_state_modes_in_canonical_order() {
        // Drivers list modes in arbitrary order; the wizard cycle pins the
        // order to FreeDrive -> CommandFollowing so cycling is predictable
        // across every device.
        let driver_advertised = vec![
            RobotMode::Disabled,
            RobotMode::CommandFollowing,
            RobotMode::Identifying,
            RobotMode::FreeDrive,
        ];
        assert_eq!(
            wizard_selectable_modes(&driver_advertised),
            vec![RobotMode::FreeDrive, RobotMode::CommandFollowing],
        );
    }

    #[test]
    fn wizard_selectable_modes_reflects_capability_only_drivers() {
        // E2-style passive grippers only advertise free-drive: the wizard
        // surfaces a one-option cycle without the controller hardcoding
        // any per-driver knowledge.
        assert_eq!(
            wizard_selectable_modes(&[RobotMode::FreeDrive]),
            vec![RobotMode::FreeDrive],
        );
        assert_eq!(
            wizard_selectable_modes(&[RobotMode::Identifying, RobotMode::Disabled]),
            Vec::<RobotMode>::new(),
        );
    }

    #[test]
    fn cycle_robot_mode_only_alternates_between_free_drive_and_command_following() {
        // Two arms -> one auto-paired teleop pair, both with the full mode
        // list. The wizard cycle should hop between FreeDrive and
        // CommandFollowing only, ignoring Identifying / Disabled.
        let mut session = setup_session(&[
            robot_discovery("arm_lead", 6),
            robot_discovery("arm_follow", 6),
        ]);
        let first_robot = session
            .available_devices
            .iter()
            .find(|device| device.device_type == DeviceType::Robot)
            .expect("setup discovers two robots")
            .name
            .clone();
        // Walk the cycle a few times in each direction; only steady-state
        // modes should ever be observed.
        let mut observed = Vec::new();
        for _ in 0..6 {
            session
                .cycle_robot_mode(&first_robot, 1)
                .expect("forward cycle should succeed");
            let mode = session
                .available_device(&first_robot)
                .and_then(|available| available.current.channels.first().and_then(|ch| ch.mode))
                .expect("robot channel always has a mode");
            observed.push(mode);
        }
        assert!(
            observed
                .iter()
                .all(|mode| matches!(mode, RobotMode::FreeDrive | RobotMode::CommandFollowing)),
            "cycle landed on a non-steady-state mode: {observed:?}",
        );
        assert!(observed.contains(&RobotMode::FreeDrive));
        assert!(observed.contains(&RobotMode::CommandFollowing));
    }

    #[test]
    fn create_pairing_rejects_when_no_eligible_leader_exists() {
        // A single arm gives us a candidate follower but no leader (we'd
        // need at least two enabled command-following channels and the
        // pair must not collapse to a self-loop). The wizard should keep
        // pairings empty and surface a descriptive message instead of
        // silently producing a degenerate pair.
        let mut session = setup_session(&[robot_discovery("arm_only", 6)]);
        // Drop the auto-built default pairing so we test create_pairing
        // from a clean slate. Mode stays Teleop (the only mode now).
        session.config.pairings.clear();
        // With a single arm, the only `eligible_follower_channels` entry
        // collapses onto the same channel as the eligible leader, so
        // `create_pairing` falls back through follower picking and emits
        // no pair (the would-be self-loop is detected and rejected).
        let new_index = session
            .create_pairing(None)
            .expect("create_pairing should not bubble validation errors");
        // With only one channel, no eligible follower is selectable.
        assert!(new_index.is_none());
        assert!(
            session.message.is_some(),
            "rejection should leave a message for the operator",
        );
        assert!(session.config.pairings.is_empty());
    }

    #[test]
    fn create_then_remove_pairing_round_trip_reaches_empty_state() {
        let mut session = setup_session(&[
            robot_discovery("arm_lead", 6),
            robot_discovery("arm_follow", 6),
        ]);
        // The discovery path auto-builds one pair already; clear so
        // create_pairing's bookkeeping is exercised from a known-empty
        // baseline. Mode stays at Teleop (the only mode the wizard now
        // exposes) since pairings can be empty in teleop.
        session.config.pairings.clear();
        let new_index = session
            .create_pairing(None)
            .expect("create_pairing should succeed with two arms")
            .expect("a pair should land at index 0");
        assert_eq!(new_index, 0);
        assert_eq!(session.config.pairings.len(), 1);
        assert_eq!(session.config.mode, CollectionMode::Teleop);
        // Removing the pair leaves teleop in place with zero pairings —
        // the wizard treats this as a valid intermediate state.
        let removed = session
            .remove_pairing(0)
            .expect("remove_pairing should succeed");
        assert!(removed);
        assert!(session.config.pairings.is_empty());
        assert_eq!(session.config.mode, CollectionMode::Teleop);
    }

    #[test]
    fn set_pairing_endpoint_rejects_ineligible_channel() {
        // Build a pair, then try to set a follower that doesn't support
        // command-following (the camera channel is the easiest such
        // ineligible target). The set should fail and leave the pair
        // unchanged.
        let mut session = setup_session(&[
            robot_discovery("arm_lead", 6),
            robot_discovery("arm_follow", 6),
            camera_discovery("cam0"),
        ]);
        // Ensure we have a pair to operate on.
        if session.config.pairings.is_empty() {
            session
                .create_pairing(None)
                .expect("create_pairing should succeed")
                .expect("a pair should land at index 0");
        }
        let cam_device = session
            .config
            .devices
            .iter()
            .find(|device| {
                device
                    .channels
                    .iter()
                    .any(|channel| channel.kind == DeviceType::Camera)
            })
            .expect("camera discovery always lands in config")
            .clone();
        let cam_channel_type = cam_device
            .channels
            .iter()
            .find(|channel| channel.kind == DeviceType::Camera)
            .expect("camera device has at least one camera channel")
            .channel_type
            .clone();
        let follower_device_before = session.config.pairings[0].follower_device.clone();
        let follower_channel_before = session.config.pairings[0].follower_channel_type.clone();
        let mutated = session
            .set_pairing_endpoint(
                0,
                PairingEndpoint::Follower,
                &cam_device.name,
                &cam_channel_type,
            )
            .expect("set_pairing_endpoint should not bubble validation errors");
        assert!(!mutated, "ineligible channels should not mutate the pair");
        assert_eq!(
            session.config.pairings[0].follower_device,
            follower_device_before,
        );
        assert_eq!(
            session.config.pairings[0].follower_channel_type,
            follower_channel_before,
        );
        assert!(
            session.message.is_some(),
            "rejection should leave a message for the operator",
        );
    }

    #[test]
    fn set_pairing_endpoint_rejects_self_loop_leader() {
        // The picker should never let the operator pick a leader that
        // equals the pair's existing follower; the controller backstops
        // that with the same constraint.
        let mut session =
            setup_session(&[robot_discovery("arm_a", 6), robot_discovery("arm_b", 6)]);
        if session.config.pairings.is_empty() {
            session
                .create_pairing(None)
                .expect("create_pairing should succeed")
                .expect("a pair should land at index 0");
        }
        let follower_device = session.config.pairings[0].follower_device.clone();
        let follower_channel = session.config.pairings[0].follower_channel_type.clone();
        let mutated = session
            .set_pairing_endpoint(
                0,
                PairingEndpoint::Leader,
                &follower_device,
                &follower_channel,
            )
            .expect("set_pairing_endpoint should not bubble validation errors");
        assert!(!mutated, "self-loop leader should be rejected");
        assert!(session.message.is_some());
    }

    #[test]
    fn cycle_pair_mapping_rolls_back_validation_failures_into_a_warning() {
        // Mirrors the operator-reported bug: a 6-DOF leader paired with
        // a 7-DOF follower cannot use direct-joint identity mapping (the
        // joint_index_map would reach into a leader joint that doesn't
        // exist), but the wizard previously bubbled the validation error
        // out of `apply_raw_command` and aborted. After the fix we
        // expect a soft warning and a rollback to the pre-cycle pair.
        let mut session = setup_session(&[
            robot_discovery("arm_lead", 6),
            robot_discovery("arm_follow", 7),
        ]);
        // Wipe any auto-built pair and bake one with an end-effector
        // mapping so cycling forward lands on direct-joint (where the
        // 6→7 DOF mismatch trips validation). This avoids depending on
        // discovery's choice of starting mapping.
        session.config.pairings.clear();
        // `build_discovery_config` rewrites raw discovery ids
        // ("arm_lead", "arm_follow") into the per-driver default name
        // ("pseudo_arm", "pseudo_arm_2"), so reach into the config to
        // recover whichever names the discovery loop actually picked.
        let leader_name = session.config.devices[0].name.clone();
        let follower_name = session.config.devices[1].name.clone();
        // Bake a known-good cartesian baseline by hand: opt the leader
        // into publishing EndEffectorPose, opt the follower into
        // accepting EndPose commands, and push a hand-crafted pair so
        // we don't depend on `pairing_from_channels` finding the
        // cartesian branch.
        ensure_channel_publishes_state(
            &mut session.config.devices,
            &leader_name,
            "arm",
            RobotStateKind::EndEffectorPose,
        );
        if let Some(follower) = session
            .config
            .devices
            .iter_mut()
            .find(|d| d.name == follower_name)
        {
            if let Some(channel) = follower
                .channels
                .iter_mut()
                .find(|c| c.channel_type == "arm")
            {
                if !channel
                    .supported_commands
                    .contains(&RobotCommandKind::EndPose)
                {
                    channel.supported_commands.push(RobotCommandKind::EndPose);
                }
            }
        }
        session.config.pairings.push(ChannelPairingConfig {
            leader_device: leader_name.clone(),
            leader_channel_type: "arm".into(),
            follower_device: follower_name.clone(),
            follower_channel_type: "arm".into(),
            mapping: MappingStrategy::Cartesian,
            leader_state: RobotStateKind::EndEffectorPose,
            follower_command: RobotCommandKind::EndPose,
            joint_index_map: Vec::new(),
            joint_scales: Vec::new(),
        });
        session
            .config
            .validate()
            .expect("baseline cartesian pair should validate");
        let mapping_before = session.config.pairings[0].mapping;
        // Cycle forward to direct-joint, which must fail validation
        // because joint_index_map = [0..7] would index leader joint 6
        // (out of range for a 6-DOF arm).
        let mutated = session
            .cycle_pair_mapping(0, 1)
            .expect("cycle_pair_mapping should not bubble validation errors");
        assert!(!mutated, "incompatible mapping cycle should be a no-op");
        assert_eq!(session.config.pairings[0].mapping, mapping_before);
        assert!(
            session
                .message
                .as_ref()
                .is_some_and(|msg| msg.contains("Cannot switch")),
            "validation rejection should leave a descriptive warning",
        );
    }

    #[test]
    fn eligibility_lists_drop_channels_disabled_in_step_one() {
        // Three robots discovered → all auto-selected → all eligible
        // initially. Disabling one in step 1 must remove it from both
        // the leader and follower picker pools immediately, so the
        // operator can't accidentally pair a channel they've turned off.
        let mut session = setup_session(&[
            robot_discovery("arm_a", 6),
            robot_discovery("arm_b", 6),
            robot_discovery("arm_c", 6),
        ]);
        let target_name = session
            .available_devices
            .iter()
            .find(|device| device.device_type == DeviceType::Robot)
            .expect("setup discovers robot rows")
            .name
            .clone();
        // Find the (device, channel_type) the disabled row maps to so
        // we can assert it disappears from both eligibility lists.
        let (target_device, target_channel) = session
            .config
            .devices
            .iter()
            .find_map(|d| {
                d.channels.iter().find_map(|c| {
                    if format!("{}|{}|{}|{}|-", "robot", d.driver, d.id, c.channel_type)
                        == target_name
                    {
                        Some((d.name.clone(), c.channel_type.clone()))
                    } else {
                        None
                    }
                })
            })
            .expect("target row exists in config");

        // Sanity: it shows up before being disabled.
        assert!(session
            .eligible_leader_channels(None)
            .contains(&(target_device.clone(), target_channel.clone())));
        assert!(session
            .eligible_follower_channels(None)
            .contains(&(target_device.clone(), target_channel.clone())));

        // Toggle off via the same path the wizard uses (space in step 1).
        session
            .toggle_device_selection(&target_name)
            .expect("toggle_device_selection should succeed");

        // After disabling, neither pool should include the channel —
        // even if other pairs still reference it (the picker now shows
        // only eligible candidates, mirroring the controller's view).
        assert!(!session
            .eligible_leader_channels(None)
            .contains(&(target_device.clone(), target_channel.clone())));
        assert!(!session
            .eligible_follower_channels(None)
            .contains(&(target_device, target_channel)));
    }

    #[test]
    fn eligible_leader_channels_accept_free_drive_only_devices() {
        // E2-style channels advertise only `FreeDrive`. Per the
        // capability-driven leader predicate (free-drive OR
        // command-following), they should still be eligible leaders.
        let mut session = setup_session(&[robot_discovery("arm_lead", 6)]);
        // Synthesize an "e2-like" channel: drop CommandFollowing so the
        // available_device only advertises FreeDrive.
        if let Some(available) = session.available_devices.first_mut() {
            available
                .supported_modes
                .retain(|mode| *mode == RobotMode::FreeDrive);
        }
        let leaders = session.eligible_leader_channels(None);
        assert!(
            !leaders.is_empty(),
            "free-drive-only channels must qualify as leaders",
        );
    }

    #[test]
    fn set_pairing_endpoint_rejects_follower_already_used_in_another_pair() {
        // Two pairs share a single eligible follower pool (three arms,
        // three commands); after the first pair claims arm_b as
        // follower, the second pair must not be allowed to point its
        // follower at arm_b too — each follower can only follow one
        // leader at a time.
        let mut session = setup_session(&[
            robot_discovery("arm_a", 6),
            robot_discovery("arm_b", 6),
            robot_discovery("arm_c", 6),
        ]);
        // Discovery seeds one pair; create a second pair to test
        // cross-pair follower uniqueness.
        if session.config.pairings.is_empty() {
            session
                .create_pairing(None)
                .expect("create_pairing should succeed");
        }
        // Snapshot the first pair's follower so we can try to re-use it.
        let claimed_follower_device = session.config.pairings[0].follower_device.clone();
        let claimed_follower_channel = session.config.pairings[0].follower_channel_type.clone();
        // Need the second pair to exist to call set_pairing_endpoint on it.
        let second_index = session
            .create_pairing(None)
            .expect("second create_pairing should succeed")
            .expect("a second pair should land at the next index");
        let mutated = session
            .set_pairing_endpoint(
                second_index,
                PairingEndpoint::Follower,
                &claimed_follower_device,
                &claimed_follower_channel,
            )
            .expect("set_pairing_endpoint should not bubble validation errors");
        assert!(
            !mutated,
            "follower claimed by another pair should be rejected"
        );
        assert!(session.message.is_some());
        // The second pair must keep its previously-seeded follower (which
        // create_pairing chose to be distinct from any already-claimed
        // follower).
        assert_ne!(
            (
                &session.config.pairings[second_index].follower_device,
                &session.config.pairings[second_index].follower_channel_type,
            ),
            (&claimed_follower_device, &claimed_follower_channel),
        );
    }
}
