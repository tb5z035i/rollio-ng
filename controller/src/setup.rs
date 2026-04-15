use crate::cli::SetupArgs;
use crate::collect::{build_preview_specs, build_visualizer_spec};
use crate::process::{
    poll_children_once, spawn_child, terminate_children, ChildSpec, ManagedChild,
};
use crate::runtime_paths::{current_executable_dir, resolve_device_program, workspace_root};
use airbot_play_rust::can::socketcan_io::SocketCanIo;
use airbot_play_rust::protocol::board::gpio::PlayLedProtocol;
use iceoryx2::prelude::*;
use rollio_bus::{SETUP_COMMAND_SERVICE, SETUP_STATE_SERVICE};
use rollio_types::config::{
    CollectionMode, Config, DeviceConfig, DeviceType, EncoderCodec, EpisodeFormat, MappingStrategy,
    PairConfig, RobotMode, StorageBackend,
};
use rollio_types::messages::{PixelFormat, SetupCommandMessage, SetupStateMessage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use signal_hook::consts::signal::{SIGINT, SIGTERM};
use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Read;
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const DISCOVERY_TIMEOUT: Duration = Duration::from_millis(2_000);
const VALIDATION_TIMEOUT: Duration = Duration::from_millis(1_000);
const SETUP_POLL_INTERVAL: Duration = Duration::from_millis(50);
const SETUP_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(2_000);
const SETUP_STATE_MAX_AGE: Duration = Duration::from_millis(500);
const SETUP_UI_SUCCESS_DELAY: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy)]
struct KnownDriver {
    device_type: DeviceType,
    driver: &'static str,
    probe_args: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, Default)]
struct DiscoveryOptions {
    simulated_cameras: usize,
    simulated_arms: usize,
}

#[derive(Debug, Clone, Serialize)]
struct DiscoveredDevice {
    device_type: DeviceType,
    driver: String,
    id: String,
    display_name: String,
    camera_profiles: Vec<CameraProfile>,
    dof: Option<u32>,
    supported_modes: Vec<RobotMode>,
    default_frequency_hz: Option<f64>,
    transport: Option<String>,
    interface: Option<String>,
    product_variant: Option<String>,
    end_effector: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CameraProfile {
    width: u32,
    height: u32,
    fps: u32,
    pixel_format: PixelFormat,
    stream: Option<String>,
    channel: Option<u32>,
}

#[derive(Debug)]
enum DriverCommandError {
    NotFound {
        program: String,
    },
    Io {
        program: String,
        source: std::io::Error,
    },
    Timeout {
        program: String,
        args: String,
    },
    Failed {
        program: String,
        args: String,
        details: String,
    },
    InvalidJson {
        program: String,
        source: serde_json::Error,
        stdout: String,
    },
}

impl std::fmt::Display for DriverCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { program } => write!(f, "driver executable not found: {program}"),
            Self::Io { program, source } => write!(f, "failed to run {program}: {source}"),
            Self::Timeout { program, args } => {
                write!(f, "driver command timed out: {program} {args}")
            }
            Self::Failed {
                program,
                args,
                details,
            } => write!(f, "driver command failed: {program} {args}: {details}"),
            Self::InvalidJson {
                program,
                source,
                stdout,
            } => write!(
                f,
                "driver command returned invalid JSON: {program}: {source}; stdout={stdout}"
            ),
        }
    }
}

impl Error for DriverCommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::InvalidJson { source, .. } => Some(source),
            _ => None,
        }
    }
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
    current: DeviceConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeviceIdentity {
    device_type: DeviceType,
    driver: String,
    id: String,
    stream: Option<String>,
    channel: Option<u32>,
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
    config: Config,
    available_devices: Vec<AvailableDevice>,
    teleop_pairing_cache: Vec<PairConfig>,
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
    config: Config,
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
    identify_device_name: Option<String>,
    airbot_identify_interface: Option<String>,
}

impl SetupSession {
    fn new(
        config: Config,
        available_devices: Vec<AvailableDevice>,
        output_path: PathBuf,
        resume_mode: bool,
        warnings: Vec<String>,
    ) -> Self {
        let teleop_pairing_cache = if config.pairing.is_empty() {
            build_default_pairings(&config.devices)
        } else {
            config.pairing.clone()
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
            self.identify_device_name = None;
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
            self.identify_device_name = None;
        }
        changed
    }

    fn ensure_visible_current_step(&mut self) {
        if self.current_step == SetupStep::Pairing && self.config.mode != CollectionMode::Teleop {
            self.current_step = SetupStep::Storage;
        }
        if self.current_step != SetupStep::Devices {
            self.identify_device_name = None;
        }
    }

    fn refresh_pairings_for_devices(&mut self) {
        self.teleop_pairing_cache = build_default_pairings(&self.config.devices);
        if self.config.mode == CollectionMode::Teleop && !self.teleop_pairing_cache.is_empty() {
            self.config.pairing = self.teleop_pairing_cache.clone();
        } else {
            self.config.mode = CollectionMode::Intervention;
            self.config.pairing.clear();
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

    fn selected_device_index(&self, name: &str) -> Option<usize> {
        let identity = self
            .available_device(name)
            .map(device_identity_from_available)?;
        self.config
            .devices
            .iter()
            .position(|device| device_identity_from_config(device) == identity)
    }

    fn is_device_selected(&self, name: &str) -> bool {
        self.selected_device_index(name).is_some()
    }

    fn set_device_name(&mut self, name: &str, value: &str) -> Result<bool, Box<dyn Error>> {
        let Some(selected_index) = self.selected_device_index(name) else {
            return Ok(false);
        };
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.message = Some("Device name must not be empty.".into());
            return Ok(false);
        }

        let Some(current_identity) = self
            .available_device(name)
            .map(device_identity_from_available)
        else {
            return Ok(false);
        };

        let duplicate_name = self.available_devices.iter().any(|device| {
            device.current.name == trimmed
                && device_identity_from_available(device) != current_identity
        });
        if duplicate_name {
            self.message = Some(format!("Device name \"{trimmed}\" is already in use."));
            return Ok(false);
        }

        let Some(available) = self.available_device_mut(name) else {
            return Ok(false);
        };
        if available.current.name == trimmed {
            return Ok(false);
        }

        let previous_name = available.current.name.clone();
        available.current.name = trimmed.to_owned();

        self.config.devices[selected_index].name = trimmed.to_owned();
        for pair in &mut self.config.pairing {
            if pair.leader == previous_name {
                pair.leader = trimmed.to_owned();
            }
            if pair.follower == previous_name {
                pair.follower = trimmed.to_owned();
            }
        }
        self.teleop_pairing_cache = self.config.pairing.clone();
        self.config.validate()?;

        Ok(true)
    }

    fn set_identify_device(&mut self, name: Option<&str>) -> bool {
        if self.identify_device_name.as_deref() == name {
            return false;
        }
        self.identify_device_name = name.map(ToOwned::to_owned);
        true
    }

    fn cycle_device_profile(&mut self, name: &str, delta: i32) -> Result<bool, Box<dyn Error>> {
        let Some(device_index) = self.selected_device_index(name) else {
            return Ok(false);
        };
        let updated_current = {
            let Some(available) = self.available_device_mut(name) else {
                return Ok(false);
            };
            if available.camera_profiles.is_empty() {
                return Ok(false);
            }
            let current_profile = available
                .camera_profiles
                .iter()
                .position(|profile| {
                    available.current.width == Some(profile.width)
                        && available.current.height == Some(profile.height)
                        && available.current.fps == Some(profile.fps)
                        && available.current.pixel_format == Some(profile.pixel_format)
                        && available.current.stream == profile.stream
                        && available.current.channel == profile.channel
                })
                .unwrap_or(0);
            let next_index = rotate_index(current_profile, available.camera_profiles.len(), delta);
            let profile = available.camera_profiles[next_index].clone();
            available.current.width = Some(profile.width);
            available.current.height = Some(profile.height);
            available.current.fps = Some(profile.fps);
            available.current.pixel_format = Some(profile.pixel_format);
            available.current.stream = profile.stream;
            available.current.channel = profile.channel;
            available.current.clone()
        };
        self.config.devices[device_index] = updated_current;
        Ok(true)
    }

    fn cycle_robot_mode(&mut self, name: &str, delta: i32) -> Result<bool, Box<dyn Error>> {
        let Some(device_index) = self.selected_device_index(name) else {
            return Ok(false);
        };
        let updated_current = {
            let Some(available) = self.available_device_mut(name) else {
                return Ok(false);
            };
            if available.supported_modes.is_empty() {
                return Ok(false);
            }
            let current_mode = available
                .current
                .mode
                .unwrap_or(available.supported_modes[0]);
            let current_index = available
                .supported_modes
                .iter()
                .position(|mode| *mode == current_mode)
                .unwrap_or(0);
            let next_index = rotate_index(current_index, available.supported_modes.len(), delta);
            available.current.mode = Some(available.supported_modes[next_index]);
            available.current.clone()
        };
        self.config.devices[device_index] = updated_current;
        Ok(true)
    }

    fn toggle_device_selection(&mut self, name: &str) -> Result<bool, Box<dyn Error>> {
        if let Some(index) = self.selected_device_index(name) {
            self.config.devices.remove(index);
            if self.identify_device_name.as_deref() == Some(name) {
                self.identify_device_name = None;
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
        self.config.devices.push(available.current);
        self.refresh_pairings_for_devices();
        self.config.validate()?;
        Ok(true)
    }

    fn cycle_pair_mapping(&mut self, index: usize, delta: i32) -> Result<bool, Box<dyn Error>> {
        let Some(pair) = self.config.pairing.get_mut(index) else {
            return Ok(false);
        };
        let options = [MappingStrategy::DirectJoint, MappingStrategy::Cartesian];
        let current_index = options
            .iter()
            .position(|mapping| *mapping == pair.mapping)
            .unwrap_or(0);
        let next_index = rotate_index(current_index, options.len(), delta);
        pair.mapping = options[next_index];
        self.teleop_pairing_cache = self.config.pairing.clone();
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
                self.teleop_pairing_cache = build_default_pairings(&self.config.devices);
            }
            if self.teleop_pairing_cache.is_empty() {
                self.message = Some(
                    "Teleop mode requires leader/follower robots with a valid pairing.".into(),
                );
                return Ok(false);
            }
            self.config.mode = CollectionMode::Teleop;
            self.config.pairing = self.teleop_pairing_cache.clone();
        } else {
            self.config.mode = CollectionMode::Intervention;
            self.config.pairing.clear();
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
            self.identify_device_name = None;
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
                save_config(&self.config, &self.output_path)?;
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
    let output_path = args.output_path();
    let discovery_options = DiscoveryOptions {
        simulated_cameras: args.sim_cameras,
        simulated_arms: args.sim_arms,
    };

    let (config, available_devices, warnings, resume_mode) =
        if let Some(existing_config) = args.load_config()? {
            validate_existing_config(&existing_config, &workspace_root, &current_exe_dir)?;
            let available_devices = available_devices_from_config(&existing_config);
            (existing_config, available_devices, Vec::new(), true)
        } else {
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
        save_config(&config, &output_path)?;
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
    config: Config,
    available_devices: Vec<AvailableDevice>,
    output_path: PathBuf,
    resume_mode: bool,
    warnings: Vec<String>,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    let websocket_port = reserve_loopback_port()?;
    let websocket_url = format!("ws://127.0.0.1:{websocket_port}");

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

    let mut bridge_runtime: Option<SetupRuntimeState> = None;
    let mut identify_runtime: Option<SetupRuntimeState> = None;
    let mut ui_children = Vec::new();
    let mut preview_runtime: Option<SetupRuntimeState> = None;
    let run_result = (|| -> Result<(), Box<dyn Error>> {
        bridge_runtime = Some(start_setup_bridge_runtime(
            &session,
            websocket_port,
            workspace_root,
            current_exe_dir,
            &log_dir,
        )?);
        let ui_spec = build_setup_ui_spec(workspace_root, &websocket_url)?;
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

            if let Some(runtime) = identify_runtime.as_mut() {
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

            if let Some(runtime) = bridge_runtime.as_mut() {
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
                session.identify_device_name = None;
                mutations.state_changed = true;
            }

            if mutations.config_changed
                || (mutations.step_changed && session.current_step != SetupStep::Preview)
            {
                stop_setup_runtime(&mut preview_runtime)?;
            }

            if mutations.config_changed && session.current_step == SetupStep::Devices {
                stop_setup_runtime(&mut identify_runtime)?;
            }

            if session.current_step != SetupStep::Devices
                || session.identify_device_name.is_none()
                || session
                    .identify_device_name
                    .as_deref()
                    .is_some_and(|name| !session.is_device_selected(name))
            {
                stop_setup_runtime(&mut identify_runtime)?;
            }

            if session.current_step == SetupStep::Preview
                && session.exit_kind.is_none()
                && preview_runtime.is_none()
            {
                stop_setup_runtime(&mut identify_runtime)?;
                stop_setup_runtime(&mut bridge_runtime)?;
                preview_runtime = Some(start_preview_runtime(
                    &mut session,
                    websocket_port,
                    &websocket_url,
                    workspace_root,
                    current_exe_dir,
                    &log_dir,
                )?);
                mutations.state_changed = true;
            }

            if session.current_step == SetupStep::Devices && session.exit_kind.is_none() {
                if let Some(target_name) = session.identify_device_name.clone() {
                    let should_restart_identify = identify_runtime
                        .as_ref()
                        .and_then(|runtime| runtime.identify_device_name.as_deref())
                        != Some(target_name.as_str());
                    if should_restart_identify {
                        stop_setup_runtime(&mut identify_runtime)?;
                    }
                    if identify_runtime.is_none() {
                        stop_setup_runtime(&mut bridge_runtime)?;
                        identify_runtime = Some(start_identify_runtime(
                            &mut session,
                            &target_name,
                            websocket_port,
                            &websocket_url,
                            workspace_root,
                            current_exe_dir,
                            &log_dir,
                        )?);
                        mutations.state_changed = true;
                    }
                }
            }

            if session.current_step != SetupStep::Preview
                && (session.current_step != SetupStep::Devices
                    || session.identify_device_name.is_none())
                && session.exit_kind.is_none()
                && bridge_runtime.is_none()
            {
                bridge_runtime = Some(start_setup_bridge_runtime(
                    &session,
                    websocket_port,
                    workspace_root,
                    current_exe_dir,
                    &log_dir,
                )?);
                mutations.state_changed = true;
            }

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

    let cleanup_result = stop_setup_runtime(&mut preview_runtime)
        .and_then(|_| stop_setup_runtime(&mut identify_runtime))
        .and_then(|_| stop_setup_runtime(&mut bridge_runtime))
        .and_then(|_| {
            terminate_children(
                &mut ui_children,
                SETUP_SHUTDOWN_TIMEOUT,
                SETUP_POLL_INTERVAL,
            )
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

fn start_preview_runtime(
    session: &mut SetupSession,
    websocket_port: u16,
    websocket_url: &str,
    workspace_root: &Path,
    current_exe_dir: &Path,
    log_dir: &Path,
) -> Result<SetupRuntimeState, Box<dyn Error>> {
    let preview_config = build_preview_config(&session.config, websocket_port, websocket_url);
    let temp_config_path = write_setup_temp_config(
        &preview_config,
        log_dir,
        &format!("setup-preview-{websocket_port}.toml"),
    )?;
    let specs = build_setup_preview_specs(&preview_config, workspace_root, current_exe_dir)?;
    let children = spawn_setup_children(&specs, log_dir)?;
    session.message = Some(format!(
        "Preview running from {}",
        temp_config_path.display()
    ));
    Ok(SetupRuntimeState {
        children,
        temp_config_path: Some(temp_config_path),
        identify_device_name: None,
        airbot_identify_interface: None,
    })
}

fn start_identify_runtime(
    session: &mut SetupSession,
    target_name: &str,
    websocket_port: u16,
    websocket_url: &str,
    workspace_root: &Path,
    current_exe_dir: &Path,
    log_dir: &Path,
) -> Result<SetupRuntimeState, Box<dyn Error>> {
    let available = session
        .available_device(target_name)
        .cloned()
        .ok_or_else(|| format!("missing identify target {target_name}"))?;
    let identify_config =
        build_identify_config(&available, &session.config, websocket_port, websocket_url)?;
    let temp_config_path = write_setup_temp_config(
        &identify_config,
        log_dir,
        &format!("setup-identify-{}.toml", sanitize_temp_name(target_name)),
    )?;
    let specs = build_setup_preview_specs(&identify_config, workspace_root, current_exe_dir)?;
    let children = spawn_setup_children(&specs, log_dir)?;
    let airbot_identify_interface = if is_airbot_identify_driver(&available.driver) {
        available.current.interface.clone()
    } else {
        None
    };
    if let Some(interface) = airbot_identify_interface.as_deref() {
        let _ = set_airbot_identify_led(interface, true);
    }
    session.message = Some(format!(
        "Identify active for {} ({})",
        available.display_name, available.id
    ));
    Ok(SetupRuntimeState {
        children,
        temp_config_path: Some(temp_config_path),
        identify_device_name: Some(target_name.to_owned()),
        airbot_identify_interface,
    })
}

fn start_setup_bridge_runtime(
    session: &SetupSession,
    websocket_port: u16,
    workspace_root: &Path,
    current_exe_dir: &Path,
    log_dir: &Path,
) -> Result<SetupRuntimeState, Box<dyn Error>> {
    let bridge_config = build_setup_bridge_config(&session.config, websocket_port);
    let spec = build_visualizer_spec(&bridge_config, workspace_root, current_exe_dir)?;
    let children = spawn_setup_children(std::slice::from_ref(&spec), log_dir)?;
    Ok(SetupRuntimeState {
        children,
        temp_config_path: None,
        identify_device_name: None,
        airbot_identify_interface: None,
    })
}

fn stop_setup_runtime(runtime_state: &mut Option<SetupRuntimeState>) -> Result<(), Box<dyn Error>> {
    let Some(mut runtime) = runtime_state.take() else {
        return Ok(());
    };

    terminate_children(
        &mut runtime.children,
        SETUP_SHUTDOWN_TIMEOUT,
        SETUP_POLL_INTERVAL,
    )?;
    if let Some(interface) = runtime.airbot_identify_interface.as_deref() {
        let _ = set_airbot_identify_led(interface, false);
    }
    if let Some(temp_config_path) = runtime.temp_config_path.as_deref() {
        cleanup_preview_temp_config(temp_config_path);
    }
    Ok(())
}

fn build_setup_bridge_config(config: &Config, websocket_port: u16) -> Config {
    let mut bridge_config = config.clone();
    bridge_config.visualizer.port = websocket_port;
    bridge_config
}

fn build_preview_config(config: &Config, websocket_port: u16, websocket_url: &str) -> Config {
    let mut preview_config = config.clone();
    preview_config.visualizer.port = websocket_port;
    preview_config.ui.websocket_url = Some(websocket_url.into());
    preview_config
}

fn build_identify_config(
    target: &AvailableDevice,
    config: &Config,
    websocket_port: u16,
    websocket_url: &str,
) -> Result<Config, Box<dyn Error>> {
    let mut identify_config = config.clone();
    identify_config.visualizer.port = websocket_port;
    identify_config.ui.websocket_url = Some(websocket_url.into());
    identify_config.mode = CollectionMode::Intervention;
    identify_config.pairing.clear();
    identify_config.devices = vec![{
        let mut device = target.current.clone();
        if device.device_type == DeviceType::Robot {
            device.mode = Some(RobotMode::FreeDrive);
        }
        device
    }];
    identify_config.validate()?;
    Ok(identify_config)
}

fn write_setup_temp_config(
    config: &Config,
    log_dir: &Path,
    filename: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let path = log_dir.join(filename);
    fs::write(&path, toml::to_string_pretty(config)?)?;
    Ok(path)
}

fn cleanup_preview_temp_config(path: &Path) {
    let _ = fs::remove_file(path);
}

fn build_setup_preview_specs(
    config: &Config,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Result<Vec<ChildSpec>, Box<dyn Error>> {
    build_preview_specs(config, workspace_root, current_exe_dir)
}

fn sanitize_temp_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "device".into()
    } else {
        sanitized
    }
}

fn is_airbot_identify_driver(driver: &str) -> bool {
    matches!(driver, "airbot-play" | "airbot-e2" | "airbot-g2")
}

fn set_airbot_identify_led(interface: &str, enabled: bool) -> Result<(), Box<dyn Error>> {
    let frames =
        PlayLedProtocol::new(0x00).generate_led_effect(if enabled { 0x22 } else { 0x1F })?;
    let interface = interface.trim().to_owned();
    if interface.is_empty() {
        return Ok(());
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let io = SocketCanIo::open(interface)?;
        for frame in &frames {
            io.send(frame).await?;
        }
        Ok::<(), Box<dyn Error>>(())
    })?;
    Ok(())
}

fn build_setup_ui_spec(
    workspace_root: &Path,
    websocket_url: &str,
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
                OsString::from("--ws"),
                OsString::from(websocket_url),
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

        Ok(Self {
            _node: node,
            setup_command_subscriber: command_service.subscriber_builder().create()?,
            setup_state_publisher: state_service.publisher_builder().create()?,
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
}

fn available_devices_from_config(config: &Config) -> Vec<AvailableDevice> {
    config
        .devices
        .iter()
        .map(|device| AvailableDevice {
            name: available_device_key_from_config(device),
            display_name: canonical_device_display_name(
                device.device_type,
                &device.driver,
                device.dof,
                device.stream.as_deref(),
                device.channel,
            ),
            device_type: device.device_type,
            driver: device.driver.clone(),
            id: device.id.clone(),
            camera_profiles: if device.device_type == DeviceType::Camera {
                vec![CameraProfile {
                    width: device.width.unwrap_or_default(),
                    height: device.height.unwrap_or_default(),
                    fps: device.fps.unwrap_or_default(),
                    pixel_format: device.pixel_format.unwrap_or(PixelFormat::Rgb24),
                    stream: device.stream.clone(),
                    channel: device.channel,
                }]
            } else {
                Vec::new()
            },
            supported_modes: device.mode.into_iter().collect(),
            current: device.clone(),
        })
        .collect()
}

fn available_devices_from_discoveries(
    discoveries: &[DiscoveredDevice],
    config: &Config,
) -> Result<Vec<AvailableDevice>, Box<dyn Error>> {
    let mut available = Vec::new();

    for discovery in discoveries {
        match discovery.device_type {
            DeviceType::Camera => {
                let profile = discovery.camera_profiles.first().cloned().ok_or_else(|| {
                    format!("camera \"{}\" exposed no supported profiles", discovery.id)
                })?;
                let current = config
                    .devices
                    .iter()
                    .find(|device| {
                        device_matches_discovery(
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
                available.push(AvailableDevice {
                    name: available_device_key_from_config(&current),
                    display_name: discovery.display_name.clone(),
                    device_type: DeviceType::Camera,
                    driver: discovery.driver.clone(),
                    id: discovery.id.clone(),
                    camera_profiles: discovery.camera_profiles.clone(),
                    supported_modes: Vec::new(),
                    current,
                });
            }
            DeviceType::Robot => {
                let current = config
                    .devices
                    .iter()
                    .find(|device| device_matches_discovery(device, discovery, None, None))
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "missing setup device for discovered robot {} ({})",
                            discovery.display_name, discovery.id
                        )
                    })?;
                available.push(AvailableDevice {
                    name: available_device_key_from_config(&current),
                    display_name: discovery.display_name.clone(),
                    device_type: DeviceType::Robot,
                    driver: discovery.driver.clone(),
                    id: discovery.id.clone(),
                    camera_profiles: Vec::new(),
                    supported_modes: discovery.supported_modes.clone(),
                    current,
                });
            }
        }
    }

    Ok(available)
}

fn discover_devices(
    workspace_root: &Path,
    current_exe_dir: &Path,
    options: DiscoveryOptions,
) -> Result<(Vec<DiscoveredDevice>, Vec<String>), Box<dyn Error>> {
    let mut discoveries = Vec::new();
    let mut probe_errors = Vec::new();

    for driver in known_drivers() {
        extend_driver_discoveries(
            *driver,
            &[],
            workspace_root,
            current_exe_dir,
            &mut discoveries,
            &mut probe_errors,
        );
    }

    if options.simulated_cameras > 0 {
        let simulated_camera_args = vec![
            OsString::from("--count"),
            OsString::from(options.simulated_cameras.to_string()),
        ];
        extend_driver_discoveries(
            KnownDriver {
                device_type: DeviceType::Camera,
                driver: "pseudo",
                probe_args: &[],
            },
            &simulated_camera_args,
            workspace_root,
            current_exe_dir,
            &mut discoveries,
            &mut probe_errors,
        );
    }

    if options.simulated_arms > 0 {
        let simulated_robot_args = vec![
            OsString::from("--count"),
            OsString::from(options.simulated_arms.to_string()),
        ];
        extend_driver_discoveries(
            KnownDriver {
                device_type: DeviceType::Robot,
                driver: "pseudo",
                probe_args: &[],
            },
            &simulated_robot_args,
            workspace_root,
            current_exe_dir,
            &mut discoveries,
            &mut probe_errors,
        );
    }

    if discoveries.is_empty() && !probe_errors.is_empty() {
        return Err(probe_errors.join("; ").into());
    }

    Ok((discoveries, probe_errors))
}

fn extend_driver_discoveries(
    driver: KnownDriver,
    extra_probe_args: &[OsString],
    workspace_root: &Path,
    current_exe_dir: &Path,
    discoveries: &mut Vec<DiscoveredDevice>,
    probe_errors: &mut Vec<String>,
) {
    let program = resolve_device_program(
        driver.device_type,
        driver.driver,
        workspace_root,
        current_exe_dir,
    );
    let mut probe_args = vec![OsString::from("probe")];
    probe_args.extend(driver.probe_args.iter().map(OsString::from));
    probe_args.extend(extra_probe_args.iter().cloned());

    let probe_output =
        match run_driver_json(&program, &probe_args, workspace_root, DISCOVERY_TIMEOUT) {
            Ok(value) => value,
            Err(DriverCommandError::NotFound { .. }) => return,
            Err(error) => {
                probe_errors.push(format!("{}: {error}", driver.driver));
                return;
            }
        };

    let Some(entries) = probe_output.as_array() else {
        probe_errors.push(format!(
            "{}: probe output must be a JSON array, got {}",
            driver.driver, probe_output
        ));
        return;
    };

    for entry in entries {
        match build_discovered_device(driver, entry, &program, workspace_root, DISCOVERY_TIMEOUT) {
            Ok(device) => {
                discoveries.push(device.clone());
                if let Some(derived) = derive_attached_airbot_eef_discovery(&device) {
                    discoveries.push(derived);
                }
            }
            Err(error) => probe_errors.push(format!("{}: {error}", driver.driver)),
        }
    }
}

fn build_discovered_device(
    driver: KnownDriver,
    probe_entry: &Value,
    program: &OsString,
    workspace_root: &Path,
    timeout: Duration,
) -> Result<DiscoveredDevice, Box<dyn Error>> {
    let id = value_as_string(probe_entry.get("id"))
        .ok_or_else(|| format!("probe entry missing id: {probe_entry}"))?;
    let capabilities = run_driver_json(
        program,
        &[OsString::from("capabilities"), OsString::from(&id)],
        workspace_root,
        timeout,
    )?;

    match driver.device_type {
        DeviceType::Camera => {
            let camera_profiles =
                normalize_camera_profiles(driver.driver, parse_camera_capabilities(&capabilities));
            let display_name = canonical_device_display_name(
                DeviceType::Camera,
                driver.driver,
                None,
                camera_profiles
                    .first()
                    .and_then(|profile| profile.stream.as_deref()),
                camera_profiles.first().and_then(|profile| profile.channel),
            );
            Ok(DiscoveredDevice {
                device_type: DeviceType::Camera,
                driver: driver.driver.to_owned(),
                id,
                display_name,
                camera_profiles,
                dof: None,
                supported_modes: Vec::new(),
                default_frequency_hz: None,
                transport: value_as_string(capabilities.get("transport"))
                    .or_else(|| value_as_string(probe_entry.get("transport"))),
                interface: value_as_string(capabilities.get("interface"))
                    .or_else(|| value_as_string(probe_entry.get("interface"))),
                product_variant: value_as_string(capabilities.get("product_variant"))
                    .or_else(|| value_as_string(probe_entry.get("product_variant"))),
                end_effector: value_as_string(capabilities.get("end_effector"))
                    .or_else(|| value_as_string(probe_entry.get("end_effector"))),
            })
        }
        DeviceType::Robot => {
            let dof = value_as_u32(capabilities.get("dof"))
                .or_else(|| value_as_u32(probe_entry.get("dof")));
            let display_name =
                canonical_device_display_name(DeviceType::Robot, driver.driver, dof, None, None);
            Ok(DiscoveredDevice {
                device_type: DeviceType::Robot,
                driver: driver.driver.to_owned(),
                id,
                display_name,
                camera_profiles: Vec::new(),
                dof,
                supported_modes: parse_robot_modes(
                    capabilities
                        .get("supported_modes")
                        .or_else(|| probe_entry.get("supported_modes")),
                ),
                default_frequency_hz: value_as_f64(capabilities.get("default_frequency_hz"))
                    .or_else(|| value_as_f64(capabilities.get("control_frequency_hz"))),
                transport: value_as_string(capabilities.get("transport"))
                    .or_else(|| value_as_string(probe_entry.get("transport"))),
                interface: value_as_string(capabilities.get("interface"))
                    .or_else(|| value_as_string(probe_entry.get("interface"))),
                product_variant: value_as_string(capabilities.get("product_variant"))
                    .or_else(|| value_as_string(probe_entry.get("product_variant"))),
                end_effector: value_as_string(capabilities.get("end_effector"))
                    .or_else(|| value_as_string(probe_entry.get("end_effector"))),
            })
        }
    }
}

fn validate_existing_config(
    config: &Config,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    for device in &config.devices {
        validate_device_hardware(device, workspace_root, current_exe_dir)?;
    }
    Ok(())
}

fn validate_device_hardware(
    device: &DeviceConfig,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    let program = resolve_device_program(
        device.device_type,
        &device.driver,
        workspace_root,
        current_exe_dir,
    );
    let report = run_driver_json(
        &program,
        &[OsString::from("validate"), OsString::from(&device.id)],
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

fn build_discovery_config(discoveries: &[DiscoveredDevice]) -> Result<Config, Box<dyn Error>> {
    let mut config = Config::draft_setup_template();
    let mut default_name_counts = BTreeMap::new();
    let mut arm_index = 0usize;
    let mut eef_index = 0usize;

    for discovery in discoveries {
        match discovery.device_type {
            DeviceType::Camera => {
                let profile = discovery.camera_profiles.first().cloned().ok_or_else(|| {
                    format!("camera \"{}\" exposed no supported profiles", discovery.id)
                })?;
                let name = next_default_device_name(
                    default_device_name_base(
                        discovery.device_type,
                        &discovery.driver,
                        discovery.dof,
                        profile.stream.as_deref(),
                        profile.channel,
                    ),
                    &mut default_name_counts,
                );
                config.devices.push(DeviceConfig {
                    name,
                    device_type: DeviceType::Camera,
                    driver: discovery.driver.clone(),
                    id: discovery.id.clone(),
                    width: Some(profile.width),
                    height: Some(profile.height),
                    fps: Some(profile.fps),
                    pixel_format: Some(profile.pixel_format),
                    stream: profile.stream,
                    channel: profile.channel,
                    dof: None,
                    mode: None,
                    control_frequency_hz: None,
                    transport: discovery.transport.clone(),
                    interface: discovery.interface.clone(),
                    product_variant: discovery.product_variant.clone(),
                    end_effector: discovery.end_effector.clone(),
                    model_path: None,
                    gravity_comp_torque_scales: None,
                    mit_kp: None,
                    mit_kd: None,
                    command_latency_ms: None,
                    state_noise_stddev: None,
                    extra: toml::Table::new(),
                });
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
                config.devices.push(DeviceConfig {
                    name,
                    device_type: DeviceType::Robot,
                    driver: discovery.driver.clone(),
                    id: discovery.id.clone(),
                    width: None,
                    height: None,
                    fps: None,
                    pixel_format: None,
                    stream: None,
                    channel: None,
                    dof: discovery.dof.or(Some(6)),
                    mode: Some(select_supported_mode(
                        &discovery.supported_modes,
                        preferred_mode,
                    )),
                    control_frequency_hz: discovery.default_frequency_hz,
                    transport: discovery.transport.clone(),
                    interface: discovery.interface.clone(),
                    product_variant: discovery.product_variant.clone(),
                    end_effector: discovery.end_effector.clone(),
                    model_path: None,
                    gravity_comp_torque_scales: None,
                    mit_kp: None,
                    mit_kd: None,
                    command_latency_ms: Some(20),
                    state_noise_stddev: Some(0.0),
                    extra: toml::Table::new(),
                });
            }
        }
    }

    config.pairing = build_default_pairings(&config.devices);
    config.mode = if config.pairing.is_empty() {
        CollectionMode::Intervention
    } else {
        CollectionMode::Teleop
    };
    config.validate()?;
    Ok(config)
}

fn build_default_pairings(devices: &[DeviceConfig]) -> Vec<PairConfig> {
    let mut pairings = Vec::new();
    let arms = devices
        .iter()
        .filter(|device| {
            device.device_type == DeviceType::Robot && device.dof.unwrap_or_default() != 1
        })
        .collect::<Vec<_>>();
    let eefs = devices
        .iter()
        .filter(|device| device.device_type == DeviceType::Robot && device.dof == Some(1))
        .collect::<Vec<_>>();

    for (leader, follower) in [pair_devices_by_order(&arms), pair_devices_by_order(&eefs)]
        .into_iter()
        .flatten()
    {
        let dof = follower.dof.unwrap_or_default();
        pairings.push(PairConfig {
            leader: leader.name.clone(),
            follower: follower.name.clone(),
            mapping: MappingStrategy::DirectJoint,
            joint_index_map: (0..dof).collect(),
            joint_scales: vec![1.0; dof as usize],
        });
    }
    pairings
}

fn save_config(config: &Config, output_path: &Path) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(output_path, toml::to_string_pretty(config)?)?;
    Ok(())
}

fn known_drivers() -> &'static [KnownDriver] {
    &[
        KnownDriver {
            device_type: DeviceType::Camera,
            driver: "realsense",
            probe_args: &[],
        },
        KnownDriver {
            device_type: DeviceType::Camera,
            driver: "v4l2",
            probe_args: &[],
        },
        KnownDriver {
            device_type: DeviceType::Robot,
            driver: "airbot-play",
            probe_args: &[],
        },
    ]
}

fn derive_attached_airbot_eef_discovery(arm: &DiscoveredDevice) -> Option<DiscoveredDevice> {
    if arm.device_type != DeviceType::Robot || arm.driver != "airbot-play" {
        return None;
    }

    let interface = arm.interface.as_deref()?.trim();
    if interface.is_empty() {
        return None;
    }

    let (driver, id_suffix, product_variant, end_effector) =
        match normalize_attached_airbot_eef(arm.end_effector.as_deref()) {
            Some(values) => values,
            None => return None,
        };

    Some(DiscoveredDevice {
        device_type: DeviceType::Robot,
        driver: driver.to_owned(),
        id: format!("{interface}:{id_suffix}"),
        display_name: canonical_device_display_name(DeviceType::Robot, driver, Some(1), None, None),
        camera_profiles: Vec::new(),
        dof: Some(1),
        supported_modes: vec![RobotMode::FreeDrive, RobotMode::CommandFollowing],
        default_frequency_hz: Some(250.0),
        transport: arm.transport.clone().or_else(|| Some("can".to_owned())),
        interface: Some(interface.to_owned()),
        product_variant: Some(product_variant.to_owned()),
        end_effector: Some(end_effector.to_owned()),
    })
}

fn normalize_attached_airbot_eef(
    value: Option<&str>,
) -> Option<(&'static str, &'static str, &'static str, &'static str)> {
    match value?.trim().to_ascii_lowercase().as_str() {
        "e2" | "e2b" => Some(("airbot-e2", "e2", "e2", "e2")),
        "g2" => Some(("airbot-g2", "g2", "g2", "g2")),
        _ => None,
    }
}

fn run_driver_json(
    program: &OsString,
    args: &[OsString],
    working_directory: &Path,
    timeout: Duration,
) -> Result<Value, DriverCommandError> {
    let program_name = os_string_lossy(program);
    let args_display = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    let mut child = Command::new(program)
        .args(args)
        .current_dir(working_directory)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                DriverCommandError::NotFound {
                    program: program_name.clone(),
                }
            } else {
                DriverCommandError::Io {
                    program: program_name.clone(),
                    source,
                }
            }
        })?;

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(DriverCommandError::Timeout {
                        program: program_name,
                        args: args_display,
                    });
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(source) => {
                return Err(DriverCommandError::Io {
                    program: program_name,
                    source,
                });
            }
        }
    };

    let stdout = read_child_pipe(child.stdout.take()).map_err(|source| DriverCommandError::Io {
        program: program_name.clone(),
        source,
    })?;
    let stderr = read_child_pipe(child.stderr.take()).map_err(|source| DriverCommandError::Io {
        program: program_name.clone(),
        source,
    })?;

    if !status.success() {
        let details = if stderr.trim().is_empty() {
            stdout.trim().to_owned()
        } else {
            stderr.trim().to_owned()
        };
        return Err(DriverCommandError::Failed {
            program: program_name,
            args: args_display,
            details,
        });
    }

    serde_json::from_str(stdout.trim()).map_err(|source| DriverCommandError::InvalidJson {
        program: program_name,
        source,
        stdout,
    })
}

fn read_child_pipe(mut pipe: Option<impl Read>) -> Result<String, std::io::Error> {
    let mut output = String::new();
    if let Some(pipe) = pipe.as_mut() {
        pipe.read_to_string(&mut output)?;
    }
    Ok(output)
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
                stream,
                channel: channel.filter(|channel| *channel > 0),
            })
        })
        .collect()
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
            _ => None,
        })
        .collect()
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

fn device_identity_from_available(device: &AvailableDevice) -> DeviceIdentity {
    device_identity_from_config(&device.current)
}

fn device_identity_from_config(device: &DeviceConfig) -> DeviceIdentity {
    DeviceIdentity {
        device_type: device.device_type,
        driver: device.driver.clone(),
        id: device.id.clone(),
        stream: device.stream.clone(),
        channel: device.channel,
    }
}

fn device_matches_discovery(
    device: &DeviceConfig,
    discovery: &DiscoveredDevice,
    stream: Option<&str>,
    channel: Option<u32>,
) -> bool {
    device_identity_from_config(device)
        == DeviceIdentity {
            device_type: discovery.device_type,
            driver: discovery.driver.clone(),
            id: discovery.id.clone(),
            stream: stream.map(ToOwned::to_owned),
            channel,
        }
}

fn available_device_key_from_config(device: &DeviceConfig) -> String {
    let kind = match device.device_type {
        DeviceType::Camera => "camera",
        DeviceType::Robot => "robot",
    };
    let stream = device.stream.as_deref().unwrap_or("-");
    let channel = device
        .channel
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".into());
    format!("{kind}|{}|{}|{stream}|{channel}", device.driver, device.id)
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

fn pair_devices_by_order<'a>(
    devices: &[&'a DeviceConfig],
) -> Option<(&'a DeviceConfig, &'a DeviceConfig)> {
    match devices {
        [leader, follower, ..] => Some((leader, follower)),
        _ => None,
    }
}

fn group_default_mode(index: usize) -> RobotMode {
    match index {
        0 => RobotMode::FreeDrive,
        1 => RobotMode::CommandFollowing,
        _ => RobotMode::FreeDrive,
    }
}

fn os_string_lossy(value: &OsStr) -> String {
    value.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;

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
                stream: Some("color".into()),
                channel: None,
            }],
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
            dof: Some(dof),
            supported_modes: vec![RobotMode::FreeDrive, RobotMode::CommandFollowing],
            default_frequency_hz: Some(60.0),
            transport: Some("simulated".into()),
            interface: None,
            product_variant: None,
            end_effector: None,
        }
    }

    fn airbot_play_discovery(end_effector: Option<&str>) -> DiscoveredDevice {
        DiscoveredDevice {
            device_type: DeviceType::Robot,
            driver: "airbot-play".into(),
            id: "PZ123".into(),
            display_name: "AIRBOT Play".into(),
            camera_profiles: Vec::new(),
            dof: Some(6),
            supported_modes: vec![RobotMode::FreeDrive, RobotMode::CommandFollowing],
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
            config.camera_names(),
            vec!["pseudo_camera", "pseudo_camera_2"]
        );
        assert_eq!(
            config.robot_names(),
            vec!["pseudo_arm", "pseudo_arm_2", "pseudo_eef", "pseudo_eef_2"]
        );
        assert_eq!(config.pairing.len(), 2);
        assert_eq!(config.pairing[0].leader, "pseudo_arm");
        assert_eq!(config.pairing[0].follower, "pseudo_arm_2");
        assert_eq!(config.pairing[1].leader, "pseudo_eef");
        assert_eq!(config.pairing[1].follower, "pseudo_eef_2");
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
                    stream: None,
                    channel: None,
                },
                CameraProfile {
                    width: 640,
                    height: 480,
                    fps: 30,
                    pixel_format: PixelFormat::Yuyv,
                    stream: None,
                    channel: None,
                },
                CameraProfile {
                    width: 1280,
                    height: 720,
                    fps: 30,
                    pixel_format: PixelFormat::Yuyv,
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
                    stream: Some("color".into()),
                    channel: None,
                },
                CameraProfile {
                    width: 1280,
                    height: 720,
                    fps: 30,
                    pixel_format: PixelFormat::Rgb24,
                    stream: Some("color".into()),
                    channel: None,
                },
            ]
        );
    }

    #[test]
    fn build_setup_bridge_config_preserves_visible_devices_and_overrides_port() {
        let config = build_discovery_config(&[
            camera_discovery("cam0"),
            camera_discovery("cam1"),
            robot_discovery("robot0", 6),
            robot_discovery("robot1", 6),
        ])
        .expect("config should build");

        let bridge = build_setup_bridge_config(&config, 42424);

        assert_eq!(bridge.visualizer.port, 42424);
        assert_eq!(bridge.camera_names(), config.camera_names());
        assert_eq!(bridge.robot_names(), config.robot_names());
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
        assert!(session.set_identify_device(Some(&device_name)));
        assert_eq!(session.identify_device_name.as_deref(), Some(device_name.as_str()));

        assert!(
            session
                .toggle_device_selection(&device_name)
                .expect("deselect should succeed")
        );
        assert!(!session.is_device_selected(&device_name));
        assert!(session.identify_device_name.is_none());
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

    #[test]
    fn derive_attached_airbot_eef_discovery_matches_reported_mount() {
        let e2 = derive_attached_airbot_eef_discovery(&airbot_play_discovery(Some("E2B")))
            .expect("E2B mount should produce a derived discovery");
        assert_eq!(e2.driver, "airbot-e2");
        assert_eq!(e2.id, "can0:e2");
        assert_eq!(e2.display_name, "AIRBOT E2");
        assert_eq!(e2.dof, Some(1));
        assert_eq!(e2.end_effector.as_deref(), Some("e2"));

        let g2 = derive_attached_airbot_eef_discovery(&airbot_play_discovery(Some("g2")))
            .expect("G2 mount should produce a derived discovery");
        assert_eq!(g2.driver, "airbot-g2");
        assert_eq!(g2.id, "can0:g2");

        assert!(derive_attached_airbot_eef_discovery(&airbot_play_discovery(None)).is_none());
        assert!(
            derive_attached_airbot_eef_discovery(&airbot_play_discovery(Some("none"))).is_none()
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
}
