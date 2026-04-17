use crate::cli::SetupArgs;
use crate::discovery::{
    discover_probe_entries, run_driver_json, DiscoveryOptions, KnownDriver,
};
use crate::process::{
    poll_children_once, spawn_child, terminate_children, ChildSpec, ManagedChild,
};
use crate::runtime_plan::{build_control_server_spec, build_preview_specs};
use crate::runtime_paths::{
    current_executable_dir, default_device_executable_name, resolve_registered_program,
    workspace_root,
};
use iceoryx2::prelude::*;
use rollio_bus::{
    channel_mode_control_service_name, CONTROL_EVENTS_SERVICE, SETUP_COMMAND_SERVICE,
    SETUP_STATE_SERVICE,
};
use rollio_types::config::{
    BinaryDeviceConfig, CameraChannelProfile, ChannelCommandDefaults, ChannelPairingConfig,
    CollectionMode, DeviceChannelConfigV2, DeviceType, EncoderCodec, EpisodeFormat,
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
use crate::discovery::known_drivers;

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
    "rollio-camera-v4l2",
    "rollio-robot-airbot-play",
    "rollio-robot-pseudo",
];


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
    device_type: DeviceType,
    driver: String,
    id: String,
    /// Device-level display label provided by the executable (e.g.
    /// "AIRBOT Play", or the V4L2 capabilities name). Used as the per-row
    /// label fallback when a channel does not provide its own label.
    display_name: String,
    camera_profiles: Vec<CameraProfile>,
    supported_modes_by_channel: BTreeMap<String, Vec<RobotMode>>,
    /// Per-channel display label and default name as reported by the
    /// device executable's `query --json`. Indexed by `channel_type`.
    channel_meta_by_channel: BTreeMap<String, DiscoveredChannelMeta>,
    dof: Option<u32>,
    supported_modes: Vec<RobotMode>,
    default_frequency_hz: Option<f64>,
    transport: Option<String>,
    interface: Option<String>,
    product_variant: Option<String>,
    end_effector: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
struct DiscoveredChannelMeta {
    channel_label: Option<String>,
    default_name: Option<String>,
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
    Pairing,
    Storage,
    Preview,
}

impl SetupStep {
    fn label(self) -> &'static str {
        match self {
            Self::Devices => "Devices",
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
        if self.config.mode == CollectionMode::Teleop {
            &[
                SetupStep::Devices,
                SetupStep::Storage,
                SetupStep::Pairing,
                SetupStep::Preview,
            ]
        } else {
            &[
                SetupStep::Devices,
                SetupStep::Storage,
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

    fn refresh_pairings_for_devices(&mut self) {
        self.teleop_pairing_cache = build_default_channel_pairings(&self.config.devices);
        if self.config.mode == CollectionMode::Teleop && !self.teleop_pairing_cache.is_empty() {
            self.config.pairings = self.teleop_pairing_cache.clone();
        } else {
            self.config.mode = CollectionMode::Intervention;
            self.config.pairings.clear();
        }
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
        self.config.devices[selected_index].channels[channel_index].name =
            Some(trimmed.to_owned());
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
                    }) && camera_channel_type_for_profile(profile)
                        == ch.channel_type
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
            if available.supported_modes.is_empty() {
                return Ok(false);
            }
            let Some(ch) = available.current.channels.first_mut() else {
                return Ok(false);
            };
            if ch.kind != DeviceType::Robot {
                return Ok(false);
            }
            let current_mode = ch.mode.unwrap_or(available.supported_modes[0]);
            let current_index = available
                .supported_modes
                .iter()
                .position(|mode| *mode == current_mode)
                .unwrap_or(0);
            let next_index = rotate_index(current_index, available.supported_modes.len(), delta);
            ch.mode = Some(available.supported_modes[next_index]);
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
            self.refresh_pairings_for_devices();
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
        self.refresh_pairings_for_devices();
        self.config.validate()?;
        Ok(true)
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
        let (
            leader_device,
            leader_channel_type,
            follower_device,
            follower_channel_type,
            current_mapping,
        ) = self
            .config
            .pairings
            .get(index)
            .map(|pair| {
                (
                    pair.leader_device.clone(),
                    pair.leader_channel_type.clone(),
                    pair.follower_device.clone(),
                    pair.follower_channel_type.clone(),
                    pair.mapping,
                )
            })
            .ok_or_else(|| "missing pairing".to_string())?;
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
        let Some(pair) = self.config.pairings.get_mut(index) else {
            return Ok(false);
        };
        let options = [MappingStrategy::DirectJoint, MappingStrategy::Cartesian];
        let current_index = options
            .iter()
            .position(|mapping| *mapping == current_mapping)
            .unwrap_or(0);
        let next_index = rotate_index(current_index, options.len(), delta);
        pair.mapping = options[next_index];
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

    fn cycle_collection_mode(&mut self, delta: i32) -> Result<bool, Box<dyn Error>> {
        let options = [CollectionMode::Intervention, CollectionMode::Teleop];
        let current_index = options
            .iter()
            .position(|mode| *mode == self.config.mode)
            .unwrap_or(0);
        let next_mode = options[rotate_index(current_index, options.len(), delta)];
        if next_mode == self.config.mode {
            return Ok(false);
        }

        if next_mode == CollectionMode::Teleop {
            if self.teleop_pairing_cache.is_empty() {
                self.teleop_pairing_cache = build_default_channel_pairings(&self.config.devices);
            }
            if self.teleop_pairing_cache.is_empty() {
                self.message = Some(
                    "Teleop mode requires leader/follower robots with a valid pairing.".into(),
                );
                return Ok(false);
            }
            self.config.mode = CollectionMode::Teleop;
            self.config.pairings = self.teleop_pairing_cache.clone();
        } else {
            self.config.mode = CollectionMode::Intervention;
            self.config.pairings.clear();
        }
        self.ensure_visible_current_step();
        self.config.validate()?;
        Ok(true)
    }

    fn cycle_video_codec(&mut self, delta: i32) -> Result<bool, Box<dyn Error>> {
        self.config.encoder.video_codec =
            rotate_encoder_codec(self.config.encoder.video_codec, delta);
        self.config.validate()?;
        Ok(true)
    }

    fn cycle_depth_codec(&mut self, delta: i32) -> Result<bool, Box<dyn Error>> {
        self.config.encoder.depth_codec =
            rotate_encoder_codec(self.config.encoder.depth_codec, delta);
        self.config.validate()?;
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

    fn jump_to_step(&mut self, value: &str) -> bool {
        let target = match value {
            "devices" | "discovery" | "selection" | "parameters" => SetupStep::Devices,
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
                if self.identify_device_name.as_deref() != Some(name) && !self.is_device_selected(name)
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

fn normalized_delta(delta: Option<i32>) -> i32 {
    match delta.unwrap_or(1).cmp(&0) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 1,
        std::cmp::Ordering::Greater => 1,
    }
}

pub fn run(args: SetupArgs) -> Result<(), Box<dyn Error>> {
    let workspace_root = workspace_root()?;
    let current_exe_dir = current_executable_dir()?;
    ensure_setup_dev_runtime_binaries_built(&workspace_root, &current_exe_dir)?;
    let output_path = args.output_path();
    let discovery_options = DiscoveryOptions {
        simulated_cameras: args.sim_cameras,
        simulated_arms: args.sim_arms,
    };

    let (config, available_devices, warnings, resume_mode) =
        if let Some(existing_config) = args.load_project_config()? {
            existing_config.validate().map_err(|e| -> Box<dyn Error> { e.to_string().into() })?;
            validate_existing_project(&existing_config, &workspace_root, &current_exe_dir)?;
            let available_devices = available_devices_from_project(&existing_config);
            (existing_config, available_devices, Vec::new(), true)
        } else {
            eprintln!("rollio: discovering devices...");
            let (discoveries, warnings) =
                discover_devices(&workspace_root, &current_exe_dir, discovery_options)?;
            if discoveries.is_empty() {
                return Err("setup did not discover any devices".into());
            }
            let config = build_discovery_config(&discoveries)?;
            let available_devices = available_devices_from_discoveries(&discoveries, &config)?;
            (config, available_devices, warnings, false)
        };

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
        &current_exe_dir,
    )
}

fn run_interactive_setup(
    config: ProjectConfig,
    available_devices: Vec<AvailableDevice>,
    output_path: PathBuf,
    resume_mode: bool,
    warnings: Vec<String>,
    workspace_root: &Path,
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
    let log_dir = workspace_root.join("target/rollio-setup-logs");
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
            current_exe_dir,
        )?;
        control_children = spawn_setup_children(std::slice::from_ref(&control_spec), &log_dir)?;

        let ui_spec = build_setup_ui_spec(
            workspace_root,
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
            let desired_preview_target = if should_preview && session.current_step == SetupStep::Devices
            {
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
    let specs = build_setup_preview_specs(&preview_config, workspace_root, current_exe_dir)?;
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
    current_exe_dir: &Path,
) -> Result<Vec<ChildSpec>, Box<dyn Error>> {
    build_preview_specs(project, workspace_root, current_exe_dir)
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
/// `BinaryDeviceConfig` driven by one process.
#[derive(Debug, Clone)]
struct CameraDiscoveryChannel {
    channel_type: String,
    profile: CameraProfile,
    /// Final per-channel name after dedup. Always populated by
    /// `build_discovery_config` so the wizard never shows a blank `name=`
    /// column or two rows that both say `name=camera`.
    name: String,
}

fn binary_device_from_camera_discovery(
    discovery: &DiscoveredDevice,
    channels: &[CameraDiscoveryChannel],
    name: String,
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
    let device_channels = channels
        .iter()
        .map(|channel| {
            let channel_meta = discovery
                .channel_meta_by_channel
                .get(&channel.channel_type)
                .cloned()
                .unwrap_or_default();
            DeviceChannelConfigV2 {
                channel_type: channel.channel_type.clone(),
                kind: DeviceType::Camera,
                enabled: true,
                name: Some(channel.name.clone()),
                channel_label: channel_meta.channel_label,
                mode: None,
                dof: None,
                publish_states: Vec::new(),
                recorded_states: Vec::new(),
                control_frequency_hz: None,
                profile: Some(CameraChannelProfile {
                    width: channel.profile.width,
                    height: channel.profile.height,
                    fps: channel.profile.fps,
                    pixel_format: channel.profile.pixel_format,
                    native_pixel_format: channel.profile.native_pixel_format.clone(),
                }),
                command_defaults: ChannelCommandDefaults::default(),
                extra: toml::Table::new(),
            }
        })
        .collect();
    BinaryDeviceConfig {
        name: name.clone(),
        executable: Some(default_device_executable_name(&discovery.driver)),
        driver: discovery.driver.clone(),
        id: discovery.id.clone(),
        bus_root: name,
        channels: device_channels,
        extra,
    }
}

/// Group a camera discovery's profiles by `channel_type`, picking the first
/// profile encountered as the default for each group. Order is the order of
/// first appearance in `camera_profiles`, so the highest-resolution profile
/// per stream is preferred when the discovery sorts profiles.
fn group_camera_profiles_by_channel(
    profiles: &[CameraProfile],
) -> Vec<(String, CameraProfile)> {
    let mut groups: Vec<(String, CameraProfile)> = Vec::new();
    for profile in profiles {
        let channel_type = camera_channel_type_for_profile(profile);
        if !groups.iter().any(|(existing, _)| existing == &channel_type) {
            groups.push((channel_type, profile.clone()));
        }
    }
    groups
}

/// Per-device base name when a discovery exposes more than one channel
/// (e.g. RealSense reports color + depth + infrared from one physical unit).
/// The single-channel path keeps using `default_device_name_base` so legacy
/// configs and existing tests stay byte-identical.
fn multi_channel_camera_device_base(driver: &str) -> String {
    match driver {
        "realsense" => "realsense".into(),
        "v4l2" => "camera".into(),
        "pseudo" => "pseudo_camera".into(),
        _ => format!("{}_camera", driver.replace('-', "_")),
    }
}

fn binary_device_from_robot_discovery(
    discovery: &DiscoveredDevice,
    name: String,
    preferred_mode: RobotMode,
) -> BinaryDeviceConfig {
    let mode = Some(select_supported_mode(
        &discovery.supported_modes,
        preferred_mode,
    ));
    let publish_states = vec![
        RobotStateKind::JointPosition,
        RobotStateKind::JointVelocity,
        RobotStateKind::JointEffort,
    ];
    let recorded_states = vec![
        RobotStateKind::JointPosition,
        RobotStateKind::JointVelocity,
        RobotStateKind::JointEffort,
    ];
    let command_defaults = ChannelCommandDefaults::default();
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
    let arm_channel_type = robot_default_channel_type(&discovery.driver);
    let arm_meta = discovery
        .channel_meta_by_channel
        .get(&arm_channel_type)
        .cloned()
        .unwrap_or_default();
    let mut channels = vec![DeviceChannelConfigV2 {
        channel_type: arm_channel_type,
        kind: DeviceType::Robot,
        enabled: true,
        name: arm_meta.default_name,
        channel_label: arm_meta.channel_label,
        mode,
        dof: discovery.dof.or(Some(6)),
        publish_states,
        recorded_states,
        control_frequency_hz: discovery.default_frequency_hz,
        profile: None,
        command_defaults,
        extra: toml::Table::new(),
    }];
    if discovery.driver == "airbot-play" {
        if let Some((channel_type, eef_defaults)) =
            mounted_airbot_end_effector_channel(discovery.end_effector.as_deref())
        {
            let eef_meta = discovery
                .channel_meta_by_channel
                .get(&channel_type)
                .cloned()
                .unwrap_or_default();
            channels.push(DeviceChannelConfigV2 {
                channel_type,
                kind: DeviceType::Robot,
                enabled: true,
                name: eef_meta.default_name,
                channel_label: eef_meta.channel_label,
                mode: Some(preferred_mode),
                dof: Some(1),
                publish_states: vec![
                    RobotStateKind::ParallelPosition,
                    RobotStateKind::ParallelVelocity,
                    RobotStateKind::ParallelEffort,
                ],
                recorded_states: vec![RobotStateKind::ParallelPosition],
                control_frequency_hz: discovery.default_frequency_hz.or(Some(250.0)),
                profile: None,
                command_defaults: eef_defaults,
                extra: toml::Table::new(),
            });
        }
    }
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

fn mounted_airbot_end_effector_channel(
    end_effector: Option<&str>,
) -> Option<(String, ChannelCommandDefaults)> {
    match end_effector?.trim().to_ascii_lowercase().as_str() {
        "e2" | "e2b" => Some((
            "e2".into(),
            ChannelCommandDefaults {
                joint_mit_kp: Vec::new(),
                joint_mit_kd: Vec::new(),
                parallel_mit_kp: vec![0.0],
                parallel_mit_kd: vec![0.0],
            },
        )),
        "g2" => Some((
            "g2".into(),
            ChannelCommandDefaults {
                joint_mit_kp: Vec::new(),
                joint_mit_kd: Vec::new(),
                parallel_mit_kp: vec![10.0],
                parallel_mit_kd: vec![0.5],
            },
        )),
        _ => None,
    }
}

fn channel_uses_parallel_teleop(ch: &DeviceChannelConfigV2) -> bool {
    ch.publish_states
        .contains(&RobotStateKind::ParallelPosition)
}

fn build_default_channel_pairings(devices: &[BinaryDeviceConfig]) -> Vec<ChannelPairingConfig> {
    let mut pairings = Vec::new();
    let arms = primary_robot_channels(devices, false);
    let eefs = primary_robot_channels(devices, true);
    for pairs in [pair_robot_channels_by_order(&arms), pair_robot_channels_by_order(&eefs)] {
        if let Some((leader_dev, leader_ch, follower_dev, follower_ch)) = pairs {
            let dof = follower_ch.dof.unwrap_or(6);
            let parallel_pair =
                channel_uses_parallel_teleop(leader_ch) && channel_uses_parallel_teleop(follower_ch);
            let (leader_state, follower_command, map_len) = if parallel_pair {
                (
                    RobotStateKind::ParallelPosition,
                    RobotCommandKind::ParallelMit,
                    dof.min(MAX_PARALLEL as u32),
                )
            } else {
                (RobotStateKind::JointPosition, RobotCommandKind::JointPosition, dof)
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
        }
    }
    pairings
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
    workspace_root: &Path,
    control_websocket_url: &str,
    preview_websocket_url: &str,
) -> Result<ChildSpec, Box<dyn Error>> {
    let ui_entry = workspace_root.join("ui/terminal/dist/index.js");
    if !ui_entry.exists() {
        return Err(format!(
            "Terminal UI bundle not found at {}. Run `cd ui/terminal && npm run build` first.",
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
        working_directory: workspace_root.to_path_buf(),
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
        BTreeMap<
            String,
            iceoryx2::port::publisher::Publisher<ipc::Service, DeviceChannelMode, ()>,
        >,
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

fn available_devices_from_project(project: &ProjectConfig) -> Vec<AvailableDevice> {
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
                Some(AvailableDevice {
                    name: available_device_key_from_binary(&current),
                    display_name: display_name_for_binary_channel(device, channel),
                    device_type,
                    driver: device.driver.clone(),
                    id: device.id.clone(),
                    camera_profiles,
                    supported_modes,
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
        match discovery.device_type {
            DeviceType::Camera => {
                let profile = discovery.camera_profiles.first().cloned().ok_or_else(|| {
                    format!("camera \"{}\" exposed no supported profiles", discovery.id)
                })?;
                let mut current = project
                    .devices
                    .iter()
                    .find(|device| {
                        device_matches_discovery_binary(
                            device,
                            discovery,
                            profile.stream.as_deref(),
                            profile.channel,
                        )
                    })
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "missing setup device for discovered camera {} ({})",
                            discovery.display_name, discovery.id
                        )
                    })?;
                enrich_current_device_from_discovery(&mut current, discovery);
                let rows = available_rows_from_discovery(&current, discovery);
                for row in rows {
                    available.push(row);
                }
            }
            DeviceType::Robot => {
                let mut current = project
                    .devices
                    .iter()
                    .find(|device| device_matches_discovery_binary(device, discovery, None, None))
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "missing setup device for discovered robot {} ({})",
                            discovery.display_name, discovery.id
                        )
                    })?;
                enrich_current_device_from_discovery(&mut current, discovery);
                let rows = available_rows_from_discovery(&current, discovery);
                for row in rows {
                    available.push(row);
                }
            }
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
                    .camera_profiles
                    .iter()
                    .filter(|profile| {
                        camera_channel_type_for_profile(profile) == channel.channel_type
                    })
                    .cloned()
                    .collect()
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
    device: &BinaryDeviceConfig,
    channel: &DeviceChannelConfigV2,
) -> Vec<RobotMode> {
    if channel.kind != DeviceType::Robot {
        return Vec::new();
    }
    match device.driver.as_str() {
        "airbot-play" => default_supported_robot_modes(),
        _ => channel.mode.into_iter().collect(),
    }
}

fn supported_modes_from_discovery(
    discovery: &DiscoveredDevice,
    channel: &DeviceChannelConfigV2,
) -> Vec<RobotMode> {
    if channel.kind != DeviceType::Robot {
        return Vec::new();
    }
    discovery
        .supported_modes_by_channel
        .get(&channel.channel_type)
        .cloned()
        .unwrap_or_else(|| match discovery.driver.as_str() {
            "airbot-play" => default_supported_robot_modes(),
            _ => channel.mode.into_iter().collect(),
        })
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
    // driver-agnostic. The hardcoded `canonical_device_display_name` is only
    // a fallback for legacy configs / tests where the channel hasn't been
    // populated by a recent device executable.
    if let Some(label) = channel.channel_label.as_deref() {
        if !label.trim().is_empty() {
            return label.to_owned();
        }
    }
    match (channel.kind, device.driver.as_str(), channel.channel_type.as_str()) {
        (DeviceType::Camera, _, channel_type) => {
            let (stream, camera_channel) = split_camera_channel_type(channel_type);
            canonical_device_display_name(
                DeviceType::Camera,
                &device.driver,
                None,
                stream,
                camera_channel,
            )
        }
        _ => canonical_device_display_name(
            channel.kind,
            &device.driver,
            channel.dof,
            None,
            None,
        ),
    }
}

fn discover_devices(
    workspace_root: &Path,
    current_exe_dir: &Path,
    options: DiscoveryOptions,
) -> Result<(Vec<DiscoveredDevice>, Vec<String>), Box<dyn Error>> {
    let (probe_entries, mut probe_errors) =
        discover_probe_entries(workspace_root, current_exe_dir, options, DISCOVERY_TIMEOUT)?;
    let mut discoveries = Vec::new();

    for entry in probe_entries {
        match build_discovered_device(
            entry.driver,
            &entry.probe_entry,
            &entry.program,
            workspace_root,
            DISCOVERY_TIMEOUT,
        ) {
            Ok(device) => discoveries.push(device),
            Err(error) => probe_errors.push(format!("{}: {error}", entry.driver.driver)),
        }
    }

    Ok((discoveries, probe_errors))
}

fn build_discovered_device(
    driver: KnownDriver,
    probe_entry: &Value,
    program: &OsString,
    workspace_root: &Path,
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
        workspace_root,
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

    let channel_meta_by_channel = parse_query_channel_meta(query_device);
    // Prefer the device-supplied label; fall back to canonical display
    // mapping only when the executable doesn't expose one. New device
    // executables MUST set device_label or channel_label so the controller
    // stays driver-agnostic.
    let device_label = value_as_string(query_device.get("device_label"));

    match driver.device_type {
        DeviceType::Camera => {
            let camera_profiles = parse_query_camera_profiles(driver.driver, query_device);
            let display_name = device_label.clone().unwrap_or_else(|| {
                canonical_device_display_name(
                    DeviceType::Camera,
                    driver.driver,
                    None,
                    camera_profiles
                        .first()
                        .and_then(|profile| profile.stream.as_deref()),
                    camera_profiles.first().and_then(|profile| profile.channel),
                )
            });
            Ok(DiscoveredDevice {
                device_type: DeviceType::Camera,
                driver: driver.driver.to_owned(),
                id,
                display_name,
                camera_profiles,
                supported_modes_by_channel: BTreeMap::new(),
                channel_meta_by_channel,
                dof: None,
                supported_modes: Vec::new(),
                default_frequency_hz: None,
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
        DeviceType::Robot => {
            let primary_channel = query_primary_robot_channel(query_device)?;
            let supported_modes_by_channel = parse_query_robot_modes_by_channel(query_device);
            let dof = value_as_u32(primary_channel.get("dof"))
                .or_else(|| value_as_u32(probe_entry.get("dof")));
            let display_name = device_label.clone().unwrap_or_else(|| {
                canonical_device_display_name(DeviceType::Robot, driver.driver, dof, None, None)
            });
            Ok(DiscoveredDevice {
                device_type: DeviceType::Robot,
                driver: driver.driver.to_owned(),
                id,
                display_name,
                camera_profiles: Vec::new(),
                supported_modes_by_channel,
                channel_meta_by_channel,
                dof,
                supported_modes: parse_query_robot_modes(primary_channel),
                default_frequency_hz: value_as_f64(primary_channel.get("default_control_frequency_hz"))
                    .or_else(|| value_as_f64(primary_channel.get("control_frequency_hz"))),
                transport: query_metadata_string(query_device, "transport")
                    .or_else(|| value_as_string(probe_entry.get("transport"))),
                interface: query_metadata_string(query_device, "interface")
                    .or_else(|| value_as_string(probe_entry.get("interface"))),
                product_variant: query_metadata_string(query_device, "product_variant")
                    .or_else(|| value_as_string(probe_entry.get("product_variant"))),
                end_effector: query_robot_end_effector(query_device)
                    .or_else(|| value_as_string(probe_entry.get("end_effector"))),
            })
        }
    }
}

fn validate_existing_project(
    project: &ProjectConfig,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    for device in &project.devices {
        validate_binary_device_hardware(device, workspace_root, current_exe_dir)?;
    }
    Ok(())
}

fn validate_binary_device_hardware(
    device: &BinaryDeviceConfig,
    workspace_root: &Path,
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
    let report = run_driver_json(
        &program,
        &args,
        workspace_root,
        VALIDATION_TIMEOUT,
    )?;
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

fn build_discovery_config(discoveries: &[DiscoveredDevice]) -> Result<ProjectConfig, Box<dyn Error>> {
    let mut config = ProjectConfig::draft_setup_template();
    let mut default_name_counts = BTreeMap::new();
    let mut arm_index = 0usize;
    let mut eef_index = 0usize;

    for discovery in discoveries {
        match discovery.device_type {
            DeviceType::Camera => {
                let groups = group_camera_profiles_by_channel(&discovery.camera_profiles);
                if groups.is_empty() {
                    return Err(format!(
                        "camera \"{}\" exposed no supported profiles",
                        discovery.id
                    )
                    .into());
                }

                let multi_channel = groups.len() > 1;
                let device_base = if multi_channel {
                    multi_channel_camera_device_base(&discovery.driver)
                } else {
                    let (_, first_profile) = &groups[0];
                    default_device_name_base(
                        discovery.device_type,
                        &discovery.driver,
                        discovery.dof,
                        first_profile.stream.as_deref(),
                        first_profile.channel,
                    )
                };
                let device_name =
                    next_default_device_name(device_base, &mut default_name_counts);

                let channels = groups
                    .into_iter()
                    .map(|(channel_type, profile)| {
                        // Single-channel camera (e.g. V4L2 webcam): keep the
                        // channel name in lockstep with the deduped device
                        // name so the wizard never shows two rows that both
                        // say `name=camera`. Multi-channel cameras
                        // (RealSense) need per-channel names so users can
                        // tell color/depth/infrared apart at a glance.
                        let channel_name = if multi_channel {
                            let base = discovery
                                .channel_meta_by_channel
                                .get(&channel_type)
                                .and_then(|meta| meta.default_name.clone())
                                .unwrap_or_else(|| {
                                    default_device_name_base(
                                        DeviceType::Camera,
                                        &discovery.driver,
                                        None,
                                        profile.stream.as_deref(),
                                        profile.channel,
                                    )
                                });
                            next_default_device_name(base, &mut default_name_counts)
                        } else {
                            device_name.clone()
                        };
                        CameraDiscoveryChannel {
                            channel_type,
                            profile,
                            name: channel_name,
                        }
                    })
                    .collect::<Vec<_>>();

                config.devices.push(binary_device_from_camera_discovery(
                    discovery,
                    &channels,
                    device_name,
                ));
            }
            DeviceType::Robot => {
                let is_eef = discovery.dof == Some(1);
                let preferred_mode = if is_eef {
                    let preferred_mode = group_default_mode(eef_index);
                    eef_index += 1;
                    preferred_mode
                } else {
                    let preferred_mode = group_default_mode(arm_index);
                    arm_index += 1;
                    preferred_mode
                };
                let name = next_default_device_name(
                    default_device_name_base(
                        discovery.device_type,
                        &discovery.driver,
                        discovery.dof,
                        None,
                        None,
                    ),
                    &mut default_name_counts,
                );
                config.devices.push(binary_device_from_robot_discovery(
                    discovery,
                    name,
                    preferred_mode,
                ));
            }
        }
    }

    config.pairings = build_default_channel_pairings(&config.devices);
    config.mode = if config.pairings.is_empty() {
        CollectionMode::Intervention
    } else {
        CollectionMode::Teleop
    };
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

fn parse_query_camera_profiles(driver: &str, device: &Value) -> Vec<CameraProfile> {
    let profiles = device
        .get("channels")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|channel| {
            value_as_string(channel.get("kind")).as_deref() == Some("camera")
                && channel
                    .get("available")
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
        })
        .flat_map(|channel| {
            let channel_type = value_as_string(channel.get("channel_type"));
            channel
                .get("profiles")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(move |profile| {
                    let width = value_as_u32(profile.get("width"))?;
                    let height = value_as_u32(profile.get("height"))?;
                    let fps = value_as_u32(profile.get("fps")).or_else(|| {
                        value_as_f64(profile.get("fps")).map(|fps| fps.round() as u32)
                    })?;
                    let pixel_format = value_as_string(profile.get("pixel_format"))
                        .and_then(|value| parse_pixel_format_name(&value))
                        .unwrap_or(PixelFormat::Rgb24);
                    Some(CameraProfile {
                        width,
                        height,
                        fps,
                        pixel_format,
                        native_pixel_format: value_as_string(profile.get("native_pixel_format")),
                        stream: channel_type.clone(),
                        channel: None,
                    })
                })
        })
        .collect::<Vec<_>>();
    normalize_camera_profiles(driver, profiles)
}

fn query_primary_robot_channel<'a>(device: &'a Value) -> Result<&'a Value, Box<dyn Error>> {
    device
        .get("channels")
        .and_then(Value::as_array)
        .and_then(|channels| {
            channels
                .iter()
                .find(|channel| value_as_string(channel.get("channel_type")).as_deref() == Some("arm"))
                .or_else(|| {
                    channels.iter().find(|channel| {
                        value_as_string(channel.get("kind")).as_deref() == Some("robot")
                    })
                })
        })
        .ok_or_else(|| "query returned no robot channels".into())
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
            let channel_label = value_as_string(channel.get("channel_label"));
            let default_name = value_as_string(channel.get("default_name"));
            Some((
                channel_type,
                DiscoveredChannelMeta {
                    channel_label,
                    default_name,
                },
            ))
        })
        .collect()
}

fn parse_query_robot_modes_by_channel(device: &Value) -> BTreeMap<String, Vec<RobotMode>> {
    device
        .get("channels")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|channel| value_as_string(channel.get("kind")).as_deref() == Some("robot"))
        .filter_map(|channel| {
            let channel_type = value_as_string(channel.get("channel_type"))?;
            Some((channel_type, parse_query_robot_modes(channel)))
        })
        .collect()
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

fn query_robot_end_effector(device: &Value) -> Option<String> {
    device
        .get("channels")
        .and_then(Value::as_array)
        .and_then(|channels| {
            channels.iter().find_map(|channel| {
                let channel_type = value_as_string(channel.get("channel_type"))?;
                match channel_type.as_str() {
                    "e2" | "g2" => Some(channel_type),
                    _ => None,
                }
            })
        })
        .or_else(|| query_metadata_string(device, "end_effector"))
}

fn enrich_current_device_from_discovery(
    current: &mut BinaryDeviceConfig,
    discovery: &DiscoveredDevice,
) {
    merge_discovery_extra(&mut current.extra, discovery);
    if discovery.device_type != DeviceType::Camera {
        return;
    }
    let Some(channel) = current
        .channels
        .iter_mut()
        .find(|channel| channel.kind == DeviceType::Camera)
    else {
        return;
    };
    let Some(profile) = channel.profile.as_mut() else {
        return;
    };
    if profile.native_pixel_format.is_some() {
        return;
    }
    let matched = discovery.camera_profiles.iter().find(|candidate| {
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

fn normalize_camera_profiles(driver: &str, profiles: Vec<CameraProfile>) -> Vec<CameraProfile> {
    let mut normalized = Vec::with_capacity(profiles.len());

    for mut profile in profiles {
        if driver == "v4l2" {
            // The V4L2 driver advertises native capture formats (for example
            // MJPG/YUYV) in capabilities, but its runtime config must request a
            // converted RGB/BGR output format.
            profile.pixel_format = PixelFormat::Rgb24;
            if profile.stream.is_none() {
                profile.stream = Some("color".into());
            }
            profile.channel = None;
        }

        if !normalized.contains(&profile) {
            normalized.push(profile);
        }
    }

    normalized
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
    if supported_modes.contains(&preferred) {
        preferred
    } else {
        supported_modes
            .first()
            .copied()
            .unwrap_or(RobotMode::FreeDrive)
    }
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

fn device_matches_discovery_binary(
    device: &BinaryDeviceConfig,
    discovery: &DiscoveredDevice,
    stream: Option<&str>,
    channel: Option<u32>,
) -> bool {
    let channel_type = match discovery.device_type {
        DeviceType::Camera => discovery_camera_channel_key(stream, channel),
        DeviceType::Robot => robot_default_channel_type(&discovery.driver),
    };
    device_identity_from_binary(device)
        == DeviceIdentity {
            device_type: discovery.device_type,
            driver: discovery.driver.clone(),
            id: discovery.id.clone(),
            channel_type,
        }
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
    format!("{kind}|{}|{}|{}|-", device.driver, device.id, ch.channel_type)
}

fn canonical_device_display_name(
    device_type: DeviceType,
    driver: &str,
    dof: Option<u32>,
    stream: Option<&str>,
    channel: Option<u32>,
) -> String {
    match device_type {
        DeviceType::Camera => match (driver, stream) {
            ("realsense", Some("color")) => "Intel RealSense RGB".into(),
            ("realsense", Some("depth")) => "Intel RealSense Depth".into(),
            ("realsense", Some("infrared")) => channel
                .map(|value| format!("Intel RealSense Infrared {value}"))
                .unwrap_or_else(|| "Intel RealSense Infrared".into()),
            ("realsense", _) => "Intel RealSense Camera".into(),
            ("v4l2", _) => "V4L2 Camera".into(),
            ("pseudo", _) => "Pseudo Camera".into(),
            _ => format!("{driver} camera"),
        },
        DeviceType::Robot => match driver {
            "airbot-play" => "AIRBOT Play".into(),
            "airbot-e2" => "AIRBOT E2".into(),
            "airbot-g2" => "AIRBOT G2".into(),
            "pseudo" if dof == Some(1) => "Pseudo End Effector".into(),
            "pseudo" => "Pseudo Arm".into(),
            _ => format!("{driver} robot"),
        },
    }
}

fn default_device_name_base(
    device_type: DeviceType,
    driver: &str,
    dof: Option<u32>,
    stream: Option<&str>,
    channel: Option<u32>,
) -> String {
    match device_type {
        DeviceType::Camera => match (driver, stream) {
            ("realsense", Some("color")) => "realsense_rgb".into(),
            ("realsense", Some("depth")) => "realsense_depth".into(),
            ("realsense", Some("infrared")) => channel
                .map(|value| format!("realsense_ir{value}"))
                .unwrap_or_else(|| "realsense_ir".into()),
            ("realsense", _) => "realsense_camera".into(),
            ("v4l2", _) => "camera".into(),
            ("pseudo", _) => "pseudo_camera".into(),
            _ => format!("{}_camera", driver.replace('-', "_")),
        },
        DeviceType::Robot => match driver {
            "airbot-play" => "airbot_play".into(),
            "airbot-e2" => "airbot_e2".into(),
            "airbot-g2" => "airbot_g2".into(),
            "pseudo" if dof == Some(1) => "pseudo_eef".into(),
            "pseudo" => "pseudo_arm".into(),
            _ => format!("{}_robot", driver.replace('-', "_")),
        },
    }
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

    fn camera_discovery(id: &str) -> DiscoveredDevice {
        DiscoveredDevice {
            device_type: DeviceType::Camera,
            driver: "pseudo".into(),
            id: id.into(),
            display_name: id.into(),
            camera_profiles: vec![CameraProfile {
                width: 640,
                height: 480,
                fps: 30,
                pixel_format: PixelFormat::Rgb24,
                native_pixel_format: None,
                stream: Some("color".into()),
                channel: None,
            }],
            supported_modes_by_channel: BTreeMap::new(),
            channel_meta_by_channel: BTreeMap::new(),
            dof: None,
            supported_modes: Vec::new(),
            default_frequency_hz: None,
            transport: Some("simulated".into()),
            interface: None,
            product_variant: None,
            end_effector: None,
        }
    }

    fn robot_discovery(id: &str, dof: u32) -> DiscoveredDevice {
        DiscoveredDevice {
            device_type: DeviceType::Robot,
            driver: "pseudo".into(),
            id: id.into(),
            display_name: id.into(),
            camera_profiles: Vec::new(),
            supported_modes_by_channel: BTreeMap::from([(
                "arm".into(),
                default_supported_robot_modes(),
            )]),
            channel_meta_by_channel: BTreeMap::new(),
            dof: Some(dof),
            supported_modes: default_supported_robot_modes(),
            default_frequency_hz: Some(60.0),
            transport: Some("simulated".into()),
            interface: None,
            product_variant: None,
            end_effector: None,
        }
    }

    fn airbot_play_discovery(end_effector: Option<&str>) -> DiscoveredDevice {
        let mut supported_modes_by_channel = BTreeMap::from([(
            "arm".into(),
            default_supported_robot_modes(),
        )]);
        let mut channel_meta_by_channel = BTreeMap::from([(
            "arm".into(),
            DiscoveredChannelMeta {
                channel_label: Some("AIRBOT Play".into()),
                default_name: Some("airbot_play_arm".into()),
            },
        )]);
        if let Some(channel_type) = end_effector.map(|value| value.to_ascii_lowercase()) {
            supported_modes_by_channel.insert(
                channel_type.clone(),
                default_supported_robot_modes(),
            );
            let (label, name) = match channel_type.as_str() {
                "e2" => ("AIRBOT E2", "airbot_e2"),
                "g2" => ("AIRBOT G2", "airbot_g2"),
                _ => ("AIRBOT EEF", "airbot_eef"),
            };
            channel_meta_by_channel.insert(
                channel_type,
                DiscoveredChannelMeta {
                    channel_label: Some(label.into()),
                    default_name: Some(name.into()),
                },
            );
        }
        DiscoveredDevice {
            device_type: DeviceType::Robot,
            driver: "airbot-play".into(),
            id: "PZ123".into(),
            display_name: "AIRBOT Play".into(),
            camera_profiles: Vec::new(),
            supported_modes_by_channel,
            channel_meta_by_channel,
            dof: Some(6),
            supported_modes: default_supported_robot_modes(),
            default_frequency_hz: Some(250.0),
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

    #[test]
    fn parse_camera_capabilities_infers_stream_pixel_formats() {
        let profiles = parse_camera_capabilities(&json!({
            "profiles": [
                {"stream": "color", "width": 640, "height": 480, "fps": 30},
                {"stream": "depth", "width": 640, "height": 480, "fps": 30},
                {"stream": "infrared", "index": 1, "width": 640, "height": 480, "fps": 30}
            ]
        }));

        assert_eq!(profiles.len(), 3);
        assert_eq!(profiles[0].pixel_format, PixelFormat::Rgb24);
        assert_eq!(profiles[1].pixel_format, PixelFormat::Depth16);
        assert_eq!(profiles[2].pixel_format, PixelFormat::Gray8);
        assert_eq!(profiles[2].channel, Some(1));
    }

    #[test]
    fn parse_camera_capabilities_accepts_v4l2_native_format_shape() {
        let profiles = parse_camera_capabilities(&json!({
            "profiles": [
                {"native_pixel_format": "MJPG", "width": 640, "height": 480, "fps": 30.0},
                {"native_pixel_format": "YUYV", "width": 1280, "height": 720, "fps": 29.97}
            ]
        }));

        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].pixel_format, PixelFormat::Mjpeg);
        assert_eq!(profiles[0].fps, 30);
        assert_eq!(profiles[1].pixel_format, PixelFormat::Yuyv);
        assert_eq!(profiles[1].fps, 30);
    }

    #[test]
    fn normalize_camera_profiles_maps_v4l2_native_formats_to_rgb24() {
        let profiles = normalize_camera_profiles(
            "v4l2",
            vec![
                CameraProfile {
                    width: 640,
                    height: 480,
                    fps: 30,
                    pixel_format: PixelFormat::Mjpeg,
                    native_pixel_format: Some("MJPG".into()),
                    stream: None,
                    channel: None,
                },
                CameraProfile {
                    width: 640,
                    height: 480,
                    fps: 30,
                    pixel_format: PixelFormat::Yuyv,
                    native_pixel_format: Some("YUYV".into()),
                    stream: None,
                    channel: None,
                },
                CameraProfile {
                    width: 1280,
                    height: 720,
                    fps: 30,
                    pixel_format: PixelFormat::Yuyv,
                    native_pixel_format: Some("YUYV".into()),
                    stream: None,
                    channel: None,
                },
            ],
        );

        assert_eq!(
            profiles,
            vec![
                CameraProfile {
                    width: 640,
                    height: 480,
                    fps: 30,
                    pixel_format: PixelFormat::Rgb24,
                    native_pixel_format: Some("MJPG".into()),
                    stream: Some("color".into()),
                    channel: None,
                },
                CameraProfile {
                    width: 640,
                    height: 480,
                    fps: 30,
                    pixel_format: PixelFormat::Rgb24,
                    native_pixel_format: Some("YUYV".into()),
                    stream: Some("color".into()),
                    channel: None,
                },
                CameraProfile {
                    width: 1280,
                    height: 720,
                    fps: 30,
                    pixel_format: PixelFormat::Rgb24,
                    native_pixel_format: Some("YUYV".into()),
                    stream: Some("color".into()),
                    channel: None,
                },
            ]
        );
    }

    #[test]
    fn available_devices_from_discoveries_merges_airbot_interface_into_existing_config() {
        let discovery = airbot_play_discovery(Some("e2"));
        let mut config = build_discovery_config(std::slice::from_ref(&discovery))
            .expect("config should build");
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
    fn available_devices_from_discoveries_splits_airbot_channels_into_rows() {
        let discovery = airbot_play_discovery(Some("e2"));
        let config = build_discovery_config(std::slice::from_ref(&discovery))
            .expect("config should build");

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
        assert!(device.channel_named("arm").is_some_and(|channel| channel.enabled));
        assert!(device.channel_named("e2").is_some_and(|channel| !channel.enabled));
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
    fn visible_steps_merge_devices_into_one_stage() {
        let mut session = setup_session(&[camera_discovery("cam0"), robot_discovery("robot0", 6)]);

        assert_eq!(
            session.visible_steps(),
            &[
                SetupStep::Devices,
                SetupStep::Storage,
                SetupStep::Preview,
            ]
        );

        session.config.mode = CollectionMode::Teleop;
        assert_eq!(
            session.visible_steps(),
            &[
                SetupStep::Devices,
                SetupStep::Storage,
                SetupStep::Pairing,
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
        assert_eq!(session.identify_device_name.as_deref(), Some(device_name.as_str()));

        assert!(
            session
                .toggle_device_selection(&device_name)
                .expect("deselect should succeed")
        );
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
    fn known_drivers_skip_pseudo_and_standalone_eef_by_default() {
        let drivers = known_drivers()
            .iter()
            .map(|driver| driver.driver)
            .collect::<Vec<_>>();

        assert_eq!(drivers, vec!["realsense", "v4l2", "airbot-play"]);
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
        DiscoveredDevice {
            device_type: DeviceType::Camera,
            driver: "v4l2".into(),
            id: id.into(),
            display_name: "V4L2 Camera".into(),
            camera_profiles: vec![CameraProfile {
                width: 640,
                height: 480,
                fps: 30,
                pixel_format: PixelFormat::Rgb24,
                native_pixel_format: Some("MJPG".into()),
                stream: Some("color".into()),
                channel: None,
            }],
            supported_modes_by_channel: BTreeMap::new(),
            channel_meta_by_channel: BTreeMap::from([(
                "color".into(),
                DiscoveredChannelMeta {
                    channel_label: Some("V4L2 Camera".into()),
                    default_name: Some("camera".into()),
                },
            )]),
            dof: None,
            supported_modes: Vec::new(),
            default_frequency_hz: None,
            transport: None,
            interface: None,
            product_variant: None,
            end_effector: None,
        }
    }

    fn realsense_multi_stream_discovery(id: &str) -> DiscoveredDevice {
        DiscoveredDevice {
            device_type: DeviceType::Camera,
            driver: "realsense".into(),
            id: id.into(),
            display_name: "Intel RealSense".into(),
            camera_profiles: vec![
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
                    fps: 30,
                    pixel_format: PixelFormat::Gray8,
                    native_pixel_format: None,
                    stream: Some("infrared".into()),
                    channel: None,
                },
            ],
            supported_modes_by_channel: BTreeMap::new(),
            channel_meta_by_channel: BTreeMap::new(),
            dof: None,
            supported_modes: Vec::new(),
            default_frequency_hz: None,
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
        let config = build_discovery_config(&[
            v4l2_discovery("/dev/video0"),
            v4l2_discovery("/dev/video2"),
        ])
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
            vec!["color".to_string(), "depth".to_string(), "infrared".to_string()]
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
        // Each device still exposes all three streams.
        assert_eq!(camera_channel_types(&config.devices[0]).len(), 3);
        assert_eq!(camera_channel_types(&config.devices[1]).len(), 3);
    }
}
