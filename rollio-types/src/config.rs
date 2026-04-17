use crate::messages::{MAX_DOF, MAX_JOINTS, MAX_PARALLEL, PixelFormat};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::str::FromStr;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("validation error: {0}")]
    Validation(String),
}

// ---------------------------------------------------------------------------
// Top-level config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "ConfigSerde")]
pub struct Config {
    pub project_name: String,
    pub mode: CollectionMode,
    pub episode: EpisodeConfig,
    pub devices: Vec<DeviceConfig>,
    #[serde(default)]
    pub pairing: Vec<PairConfig>,
    pub encoder: EncoderConfig,
    #[serde(default)]
    pub assembler: AssemblerConfig,
    pub storage: StorageConfig,
    #[serde(default)]
    pub monitor: MonitorConfig,
    #[serde(default)]
    pub controller: ControllerConfig,
    #[serde(default)]
    pub visualizer: VisualizerRuntimeConfig,
    #[serde(default)]
    pub ui: UiRuntimeConfig,
}

#[derive(Debug, Deserialize)]
struct ConfigSerde {
    #[serde(default = "default_project_name")]
    project_name: String,
    #[serde(default)]
    mode: Option<CollectionMode>,
    episode: EpisodeConfig,
    devices: Vec<DeviceConfig>,
    #[serde(default)]
    pairing: Vec<PairConfig>,
    encoder: EncoderConfig,
    #[serde(default)]
    assembler: AssemblerConfig,
    storage: StorageConfig,
    #[serde(default)]
    monitor: MonitorConfig,
    #[serde(default)]
    controller: ControllerConfig,
    #[serde(default)]
    visualizer: VisualizerRuntimeConfig,
    #[serde(default)]
    ui: UiRuntimeConfig,
}

impl From<ConfigSerde> for Config {
    fn from(value: ConfigSerde) -> Self {
        let mode = value
            .mode
            .unwrap_or_else(|| infer_collection_mode(&value.pairing));
        Self {
            project_name: value.project_name,
            mode,
            episode: value.episode,
            devices: value.devices,
            pairing: value.pairing,
            encoder: value.encoder,
            assembler: value.assembler,
            storage: value.storage,
            monitor: value.monitor,
            controller: value.controller,
            visualizer: value.visualizer,
            ui: value.ui,
        }
    }
}

fn default_project_name() -> String {
    "default".into()
}

fn infer_collection_mode(pairing: &[PairConfig]) -> CollectionMode {
    if pairing.is_empty() {
        CollectionMode::Intervention
    } else {
        CollectionMode::Teleop
    }
}

impl Config {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn draft_setup_template() -> Self {
        Self {
            project_name: default_project_name(),
            mode: CollectionMode::Intervention,
            episode: EpisodeConfig::default(),
            devices: Vec::new(),
            pairing: Vec::new(),
            encoder: EncoderConfig::default(),
            assembler: AssemblerConfig::default(),
            storage: StorageConfig::default(),
            monitor: MonitorConfig::default(),
            controller: ControllerConfig::default(),
            visualizer: VisualizerRuntimeConfig::default(),
            ui: UiRuntimeConfig::default(),
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.project_name.trim().is_empty() {
            return Err(ConfigError::Validation(
                "project_name must not be empty".into(),
            ));
        }

        self.episode.validate()?;

        if self.devices.is_empty() {
            return Err(ConfigError::Validation(
                "at least one [[devices]] entry is required".into(),
            ));
        }

        let mut names = HashSet::new();
        for dev in &self.devices {
            if !names.insert(&dev.name) {
                return Err(ConfigError::Validation(format!(
                    "duplicate device name: \"{}\"",
                    dev.name
                )));
            }
            dev.validate()?;
        }

        let device_names: HashSet<&str> = self.devices.iter().map(|d| d.name.as_str()).collect();
        for pair in &self.pairing {
            if !device_names.contains(pair.leader.as_str()) {
                return Err(ConfigError::Validation(format!(
                    "pairing references unknown device: \"{}\"",
                    pair.leader
                )));
            }
            if !device_names.contains(pair.follower.as_str()) {
                return Err(ConfigError::Validation(format!(
                    "pairing references unknown device: \"{}\"",
                    pair.follower
                )));
            }
            let leader = self
                .device_named(&pair.leader)
                .expect("validated leader should exist");
            let follower = self
                .device_named(&pair.follower)
                .expect("validated follower should exist");
            pair.validate_with_devices(leader, follower)?;
        }

        match self.mode {
            CollectionMode::Teleop => {
                if self.pairing.is_empty() {
                    return Err(ConfigError::Validation(
                        "mode=teleop requires at least one [[pairing]] entry".into(),
                    ));
                }
            }
            CollectionMode::Intervention => {
                if !self.pairing.is_empty() {
                    return Err(ConfigError::Validation(
                        "mode=intervention does not allow [[pairing]] entries".into(),
                    ));
                }
            }
        }

        self.encoder.validate()?;
        self.assembler.validate()?;
        self.storage.validate()?;
        self.monitor.validate()?;
        self.controller.validate()?;
        self.visualizer.validate()?;
        self.ui.validate()?;

        Ok(())
    }

    pub fn device_named(&self, name: &str) -> Option<&DeviceConfig> {
        self.devices.iter().find(|device| device.name == name)
    }

    pub fn camera_devices(&self) -> impl Iterator<Item = &DeviceConfig> {
        self.devices
            .iter()
            .filter(|device| device.device_type == DeviceType::Camera)
    }

    pub fn robot_devices(&self) -> impl Iterator<Item = &DeviceConfig> {
        self.devices
            .iter()
            .filter(|device| device.device_type == DeviceType::Robot)
    }

    pub fn camera_names(&self) -> Vec<String> {
        self.camera_devices()
            .map(|device| device.name.clone())
            .collect()
    }

    pub fn robot_names(&self) -> Vec<String> {
        self.robot_devices()
            .map(|device| device.name.clone())
            .collect()
    }

    pub fn visualizer_runtime_config(&self) -> VisualizerRuntimeConfig {
        let mut config = self.visualizer.clone();
        config.cameras = self.camera_names();
        config.robots = self.robot_names();
        config
    }

    pub fn ui_runtime_config(&self) -> UiRuntimeConfig {
        let mut config = self.ui.clone();
        if config.preview_websocket_url.is_none() {
            config.preview_websocket_url =
                Some(format!("ws://127.0.0.1:{}", self.visualizer.port));
        }
        config
    }

    pub fn encoder_runtime_configs(&self) -> Vec<EncoderRuntimeConfig> {
        self.camera_devices()
            .map(|camera| {
                let codec = self.encoder.codec_for_camera(camera);
                EncoderRuntimeConfig {
                    process_id: encoder_process_id(&camera.name),
                    camera_name: Some(camera.name.clone()),
                    frame_topic: Some(camera_frames_topic(&camera.name)),
                    output_dir: encoder_output_dir(&self.assembler.staging_dir, &camera.name),
                    codec,
                    backend: self.encoder.backend,
                    artifact_format: self.encoder.resolved_artifact_format_for(codec),
                    queue_size: self.encoder.queue_size,
                    fps: camera.fps.unwrap_or(self.episode.fps),
                }
            })
            .collect()
    }

    pub fn assembler_runtime_config(&self, embedded_config_toml: String) -> AssemblerRuntimeConfig {
        AssemblerRuntimeConfig {
            process_id: "episode-assembler".into(),
            format: self.episode.format,
            fps: self.episode.fps,
            chunk_size: self.episode.chunk_size,
            missing_video_timeout_ms: self.assembler.missing_video_timeout_ms,
            staging_dir: episode_staging_root(&self.assembler.staging_dir),
            encoded_handoff: self.assembler.encoded_handoff,
            cameras: self
                .camera_devices()
                .map(|camera| {
                    let codec = self.encoder.codec_for_camera(camera);
                    AssemblerCameraRuntimeConfig {
                        camera_name: camera.name.clone(),
                        encoder_process_id: encoder_process_id(&camera.name),
                        width: camera.width.unwrap_or_default(),
                        height: camera.height.unwrap_or_default(),
                        fps: camera.fps.unwrap_or(self.episode.fps),
                        pixel_format: camera.pixel_format.unwrap_or(PixelFormat::Rgb24),
                        codec,
                        artifact_format: self.encoder.resolved_artifact_format_for(codec),
                    }
                })
                .collect(),
            robots: self
                .robot_devices()
                .map(|robot| AssemblerRobotRuntimeConfig {
                    robot_name: robot.name.clone(),
                    state_topic: robot_state_topic(&robot.name),
                    dof: robot.dof.unwrap_or_default(),
                })
                .collect(),
            actions: self
                .pairing
                .iter()
                .filter_map(|pair| {
                    self.device_named(&pair.follower)
                        .map(|follower| AssemblerActionRuntimeConfig {
                            source_name: pair.follower.clone(),
                            command_topic: robot_command_topic(&pair.follower),
                            dof: follower.dof.unwrap_or_default(),
                        })
                })
                .collect(),
            embedded_config_toml,
        }
    }

    pub fn storage_runtime_config(&self) -> StorageRuntimeConfig {
        StorageRuntimeConfig {
            process_id: "storage".into(),
            backend: self.storage.backend,
            output_path: self.storage.output_path.clone(),
            endpoint: self.storage.endpoint.clone(),
            queue_size: self.storage.queue_size,
        }
    }
}

impl FromStr for Config {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: Config = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionMode {
    Teleop,
    Intervention,
}

// ---------------------------------------------------------------------------
// Episode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeConfig {
    pub format: EpisodeFormat,
    pub fps: u32,
    #[serde(default = "default_chunk_size")]
    pub chunk_size: u32,
}

fn default_chunk_size() -> u32 {
    1000
}

impl Default for EpisodeConfig {
    fn default() -> Self {
        Self {
            format: EpisodeFormat::default(),
            fps: 30,
            chunk_size: default_chunk_size(),
        }
    }
}

impl EpisodeConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.fps == 0 || self.fps > 1000 {
            return Err(ConfigError::Validation(format!(
                "episode: fps must be 1..1000, got {}",
                self.fps
            )));
        }
        if self.chunk_size == 0 {
            return Err(ConfigError::Validation(
                "episode: chunk_size must be > 0".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EpisodeFormat {
    #[serde(rename = "lerobot-v2.1")]
    LeRobotV2_1,
    #[serde(rename = "lerobot-v3.0")]
    LeRobotV3_0,
    Mcap,
}

impl Default for EpisodeFormat {
    fn default() -> Self {
        Self::LeRobotV2_1
    }
}

// ---------------------------------------------------------------------------
// Device
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub device_type: DeviceType,
    pub driver: String,
    pub id: String,

    // Camera-specific (optional)
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u32>,
    pub pixel_format: Option<PixelFormat>,
    pub stream: Option<String>,
    pub channel: Option<u32>,

    // Robot-specific (optional)
    pub dof: Option<u32>,
    pub mode: Option<RobotMode>,
    pub control_frequency_hz: Option<f64>,
    pub transport: Option<String>,
    pub interface: Option<String>,
    pub product_variant: Option<String>,
    pub end_effector: Option<String>,
    pub model_path: Option<String>,
    pub gravity_comp_torque_scales: Option<Vec<f64>>,
    pub mit_kp: Option<Vec<f64>>,
    pub mit_kd: Option<Vec<f64>>,
    pub command_latency_ms: Option<u64>,
    pub state_noise_stddev: Option<f64>,
    #[serde(flatten, default)]
    pub extra: toml::Table,
}

impl DeviceConfig {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.name.trim().is_empty() {
            return Err(ConfigError::Validation(
                "device: name must not be empty".into(),
            ));
        }
        if self.driver.trim().is_empty() {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": driver must not be empty",
                self.name
            )));
        }
        if self.id.trim().is_empty() {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": id must not be empty",
                self.name
            )));
        }
        if let Some(fps) = self.fps {
            if fps == 0 || fps > 1000 {
                return Err(ConfigError::Validation(format!(
                    "device \"{}\": fps must be 1..1000, got {fps}",
                    self.name
                )));
            }
        }
        if let Some(w) = self.width {
            if w == 0 {
                return Err(ConfigError::Validation(format!(
                    "device \"{}\": width must be > 0",
                    self.name
                )));
            }
        }
        if let Some(h) = self.height {
            if h == 0 {
                return Err(ConfigError::Validation(format!(
                    "device \"{}\": height must be > 0",
                    self.name
                )));
            }
        }
        if let Some(channel) = self.channel {
            if channel == 0 {
                return Err(ConfigError::Validation(format!(
                    "device \"{}\": channel must be > 0",
                    self.name
                )));
            }
        }
        if self
            .stream
            .as_deref()
            .is_some_and(|stream| stream.trim().is_empty())
        {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": stream must not be empty",
                self.name
            )));
        }
        if self
            .transport
            .as_deref()
            .is_some_and(|transport| transport.trim().is_empty())
        {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": transport must not be empty",
                self.name
            )));
        }
        if let Some(control_frequency_hz) = self.control_frequency_hz {
            if !control_frequency_hz.is_finite() || control_frequency_hz <= 0.0 {
                return Err(ConfigError::Validation(format!(
                    "device \"{}\": control_frequency_hz must be a positive finite number",
                    self.name
                )));
            }
        }
        if let Some(state_noise_stddev) = self.state_noise_stddev {
            if !state_noise_stddev.is_finite() || state_noise_stddev < 0.0 {
                return Err(ConfigError::Validation(format!(
                    "device \"{}\": state_noise_stddev must be a non-negative finite number",
                    self.name
                )));
            }
        }

        if let Some(path) = &self.model_path {
            if path.trim().is_empty() {
                return Err(ConfigError::Validation(format!(
                    "device \"{}\": model_path must not be empty",
                    self.name
                )));
            }
        }
        if let Some(interface) = &self.interface {
            if interface.trim().is_empty() {
                return Err(ConfigError::Validation(format!(
                    "device \"{}\": interface must not be empty",
                    self.name
                )));
            }
        }
        if let Some(product_variant) = &self.product_variant {
            if product_variant.trim().is_empty() {
                return Err(ConfigError::Validation(format!(
                    "device \"{}\": product_variant must not be empty",
                    self.name
                )));
            }
        }
        if let Some(end_effector) = &self.end_effector {
            if end_effector.trim().is_empty() {
                return Err(ConfigError::Validation(format!(
                    "device \"{}\": end_effector must not be empty",
                    self.name
                )));
            }
        }

        match self.device_type {
            DeviceType::Camera => self.validate_camera_fields()?,
            DeviceType::Robot => self.validate_robot_fields()?,
        }

        Ok(())
    }

    pub fn executable_name(&self) -> String {
        let driver_name = self.driver.replace('_', "-");
        match self.device_type {
            DeviceType::Camera => format!("rollio-camera-{driver_name}"),
            DeviceType::Robot => format!("rollio-robot-{driver_name}"),
        }
    }

    pub fn uses_depth_codec(&self) -> bool {
        if self.device_type != DeviceType::Camera {
            return false;
        }

        if matches!(
            self.pixel_format,
            Some(PixelFormat::Depth16 | PixelFormat::Gray8)
        ) {
            return true;
        }

        self.stream.as_ref().is_some_and(|stream| {
            let normalized = stream.trim().to_ascii_lowercase();
            matches!(
                normalized.as_str(),
                "depth" | "infrared" | "ir" | "gray" | "grayscale"
            )
        })
    }

    fn validate_camera_fields(&self) -> Result<(), ConfigError> {
        let width = self.width.ok_or_else(|| {
            ConfigError::Validation(format!(
                "device \"{}\": camera width is required",
                self.name
            ))
        })?;
        if width == 0 {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": width must be > 0",
                self.name
            )));
        }

        let height = self.height.ok_or_else(|| {
            ConfigError::Validation(format!(
                "device \"{}\": camera height is required",
                self.name
            ))
        })?;
        if height == 0 {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": height must be > 0",
                self.name
            )));
        }

        let fps = self.fps.ok_or_else(|| {
            ConfigError::Validation(format!("device \"{}\": camera fps is required", self.name))
        })?;
        if fps == 0 || fps > 1000 {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": fps must be 1..1000, got {fps}",
                self.name
            )));
        }

        if self.pixel_format.is_none() {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": camera pixel_format is required",
                self.name
            )));
        }

        if self.dof.is_some() || self.mode.is_some() {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": robot-only fields set on a camera device",
                self.name
            )));
        }

        Ok(())
    }

    fn validate_robot_fields(&self) -> Result<(), ConfigError> {
        let dof = self.dof.ok_or_else(|| {
            ConfigError::Validation(format!("device \"{}\": robot dof is required", self.name))
        })?;
        if dof == 0 || dof as usize > MAX_JOINTS {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": dof must be 1..{}, got {dof}",
                self.name, MAX_JOINTS
            )));
        }

        if self.mode.is_none() {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": robot mode is required",
                self.name
            )));
        }

        self.validate_joint_array_len(
            "gravity_comp_torque_scales",
            self.gravity_comp_torque_scales.as_deref(),
            dof,
        )?;
        self.validate_joint_array_len("mit_kp", self.mit_kp.as_deref(), dof)?;
        self.validate_joint_array_len("mit_kd", self.mit_kd.as_deref(), dof)?;

        Ok(())
    }

    fn validate_joint_array_len(
        &self,
        field_name: &str,
        values: Option<&[f64]>,
        dof: u32,
    ) -> Result<(), ConfigError> {
        let Some(values) = values else {
            return Ok(());
        };

        if values.len() != dof as usize {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": {field_name} must contain exactly {dof} values, got {}",
                self.name,
                values.len()
            )));
        }
        if values.iter().any(|value| !value.is_finite()) {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": {field_name} must contain only finite values",
                self.name
            )));
        }
        Ok(())
    }
}

impl FromStr for DeviceConfig {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let device: DeviceConfig = toml::from_str(s)?;
        device.validate()?;
        Ok(device)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeviceType {
    Camera,
    Robot,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RobotMode {
    FreeDrive,
    CommandFollowing,
    Identifying,
    Disabled,
}

impl RobotMode {
    pub fn control_mode_value(self) -> u32 {
        match self {
            Self::FreeDrive => 0,
            Self::CommandFollowing => 1,
            Self::Identifying => 2,
            Self::Disabled => 3,
        }
    }

    pub fn from_control_mode_value(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::FreeDrive),
            1 => Some(Self::CommandFollowing),
            2 => Some(Self::Identifying),
            3 => Some(Self::Disabled),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::FreeDrive => "free-drive",
            Self::CommandFollowing => "command-following",
            Self::Identifying => "identifying",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectJointMappingKind {
    AirbotPlay,
    AirbotE2,
    AirbotG2,
}

impl DirectJointMappingKind {
    fn from_device(device: &DeviceConfig) -> Option<Self> {
        match device.driver.as_str() {
            "airbot-play" => Some(Self::AirbotPlay),
            "airbot-e2" | "airbot-e2b" => Some(Self::AirbotE2),
            "airbot-g2" => Some(Self::AirbotG2),
            _ => None,
        }
    }

    fn allows_peer(self, peer: Self) -> bool {
        match self {
            Self::AirbotPlay => matches!(peer, Self::AirbotPlay),
            Self::AirbotE2 => matches!(peer, Self::AirbotG2),
            Self::AirbotG2 => matches!(peer, Self::AirbotE2 | Self::AirbotG2),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::AirbotPlay => "airbot-play",
            Self::AirbotE2 => "airbot-e2",
            Self::AirbotG2 => "airbot-g2",
        }
    }
}

// ---------------------------------------------------------------------------
// Pairing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PairConfig {
    pub leader: String,
    pub follower: String,
    #[serde(default = "default_mapping")]
    pub mapping: MappingStrategy,
    #[serde(default)]
    pub joint_index_map: Vec<u32>,
    #[serde(default)]
    pub joint_scales: Vec<f64>,
}

fn default_mapping() -> MappingStrategy {
    MappingStrategy::DirectJoint
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MappingStrategy {
    DirectJoint,
    Cartesian,
}

impl PairConfig {
    fn validate_with_devices(
        &self,
        leader: &DeviceConfig,
        follower: &DeviceConfig,
    ) -> Result<(), ConfigError> {
        if leader.device_type != DeviceType::Robot {
            return Err(ConfigError::Validation(format!(
                "pairing leader \"{}\" must be a robot device",
                leader.name
            )));
        }
        if follower.device_type != DeviceType::Robot {
            return Err(ConfigError::Validation(format!(
                "pairing follower \"{}\" must be a robot device",
                follower.name
            )));
        }
        if leader.name == follower.name {
            return Err(ConfigError::Validation(format!(
                "pairing leader and follower must be different devices: \"{}\"",
                leader.name
            )));
        }

        match self.mapping {
            MappingStrategy::DirectJoint => self.validate_direct_joint_mapping(leader, follower),
            MappingStrategy::Cartesian => {
                if !self.joint_index_map.is_empty() {
                    return Err(ConfigError::Validation(format!(
                        "pairing {} -> {}: joint_index_map is only valid for direct-joint mapping",
                        leader.name, follower.name
                    )));
                }
                if !self.joint_scales.is_empty() {
                    return Err(ConfigError::Validation(format!(
                        "pairing {} -> {}: joint_scales is only valid for direct-joint mapping",
                        leader.name, follower.name
                    )));
                }
                Ok(())
            }
        }
    }

    fn validate_direct_joint_mapping(
        &self,
        leader: &DeviceConfig,
        follower: &DeviceConfig,
    ) -> Result<(), ConfigError> {
        let leader_dof = leader.dof.unwrap_or(0);
        let follower_dof = follower.dof.unwrap_or(0);

        if self.joint_index_map.is_empty() {
            if leader_dof < follower_dof {
                return Err(ConfigError::Validation(format!(
                    "pairing {} -> {}: direct-joint identity mapping requires leader dof ({leader_dof}) >= follower dof ({follower_dof})",
                    leader.name, follower.name
                )));
            }
        } else {
            if self.joint_index_map.len() != follower_dof as usize {
                return Err(ConfigError::Validation(format!(
                    "pairing {} -> {}: joint_index_map length must match follower dof ({follower_dof})",
                    leader.name, follower.name
                )));
            }
            for (index, leader_joint) in self.joint_index_map.iter().enumerate() {
                if *leader_joint >= leader_dof {
                    return Err(ConfigError::Validation(format!(
                        "pairing {} -> {}: joint_index_map[{index}]={} exceeds leader dof ({leader_dof})",
                        leader.name, follower.name, leader_joint
                    )));
                }
            }
        }

        if !self.joint_scales.is_empty() {
            let expected_len = if self.joint_index_map.is_empty() {
                follower_dof as usize
            } else {
                self.joint_index_map.len()
            };
            if self.joint_scales.len() != expected_len {
                return Err(ConfigError::Validation(format!(
                    "pairing {} -> {}: joint_scales length must match {} mapped follower joints",
                    leader.name, follower.name, expected_len
                )));
            }
            for (index, scale) in self.joint_scales.iter().enumerate() {
                if !scale.is_finite() {
                    return Err(ConfigError::Validation(format!(
                        "pairing {} -> {}: joint_scales[{index}] must be finite",
                        leader.name, follower.name
                    )));
                }
            }
        }

        self.validate_airbot_direct_joint_compatibility(leader, follower)?;

        Ok(())
    }

    fn validate_airbot_direct_joint_compatibility(
        &self,
        leader: &DeviceConfig,
        follower: &DeviceConfig,
    ) -> Result<(), ConfigError> {
        let (Some(leader_kind), Some(follower_kind)) = (
            DirectJointMappingKind::from_device(leader),
            DirectJointMappingKind::from_device(follower),
        ) else {
            return Ok(());
        };

        if leader_kind.allows_peer(follower_kind) && follower_kind.allows_peer(leader_kind) {
            return Ok(());
        }

        Err(ConfigError::Validation(format!(
            "pairing {} -> {}: direct-joint mapping is not supported between AIRBOT device kinds \"{}\" and \"{}\"",
            leader.name,
            follower.name,
            leader_kind.as_str(),
            follower_kind.as_str()
        )))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeleopRuntimeConfig {
    pub process_id: String,
    pub leader_name: String,
    pub follower_name: String,
    pub leader_state_topic: String,
    pub follower_state_topic: String,
    pub follower_command_topic: String,
    #[serde(default = "default_mapping")]
    pub mapping: MappingStrategy,
    #[serde(default)]
    pub joint_index_map: Vec<u32>,
    #[serde(default)]
    pub joint_scales: Vec<f64>,
}

impl TeleopRuntimeConfig {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.process_id.trim().is_empty() {
            return Err(ConfigError::Validation(
                "teleop runtime: process_id must not be empty".into(),
            ));
        }
        if self.leader_name.trim().is_empty() {
            return Err(ConfigError::Validation(
                "teleop runtime: leader_name must not be empty".into(),
            ));
        }
        if self.follower_name.trim().is_empty() {
            return Err(ConfigError::Validation(
                "teleop runtime: follower_name must not be empty".into(),
            ));
        }
        if self.leader_name == self.follower_name {
            return Err(ConfigError::Validation(
                "teleop runtime: leader_name and follower_name must differ".into(),
            ));
        }
        if self.leader_state_topic.trim().is_empty() {
            return Err(ConfigError::Validation(
                "teleop runtime: leader_state_topic must not be empty".into(),
            ));
        }
        if self.follower_state_topic.trim().is_empty() {
            return Err(ConfigError::Validation(
                "teleop runtime: follower_state_topic must not be empty".into(),
            ));
        }
        if self.follower_command_topic.trim().is_empty() {
            return Err(ConfigError::Validation(
                "teleop runtime: follower_command_topic must not be empty".into(),
            ));
        }
        match self.mapping {
            MappingStrategy::DirectJoint => {
                if self.joint_scales.iter().any(|scale| !scale.is_finite()) {
                    return Err(ConfigError::Validation(
                        "teleop runtime: joint_scales must be finite".into(),
                    ));
                }
                if !self.joint_scales.is_empty()
                    && !self.joint_index_map.is_empty()
                    && self.joint_scales.len() != self.joint_index_map.len()
                {
                    return Err(ConfigError::Validation(
                        "teleop runtime: joint_scales length must match joint_index_map length"
                            .into(),
                    ));
                }
            }
            MappingStrategy::Cartesian => {
                if !self.joint_index_map.is_empty() || !self.joint_scales.is_empty() {
                    return Err(ConfigError::Validation(
                        "teleop runtime: Cartesian mapping does not use joint_index_map or joint_scales"
                            .into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

impl FromStr for TeleopRuntimeConfig {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: TeleopRuntimeConfig = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

// ---------------------------------------------------------------------------
// Assembler
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum EncodedHandoffMode {
    #[default]
    File,
    Iceoryx2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblerConfig {
    #[serde(default = "default_missing_video_timeout_ms")]
    pub missing_video_timeout_ms: u64,
    #[serde(default = "default_staging_dir")]
    pub staging_dir: String,
    #[serde(default)]
    pub encoded_handoff: EncodedHandoffMode,
}

fn default_missing_video_timeout_ms() -> u64 {
    5_000
}

fn default_staging_dir() -> String {
    #[cfg(target_os = "linux")]
    {
        return "/dev/shm/rollio".into();
    }

    #[allow(unreachable_code)]
    std::env::temp_dir()
        .join("rollio")
        .to_string_lossy()
        .into_owned()
}

impl Default for AssemblerConfig {
    fn default() -> Self {
        Self {
            missing_video_timeout_ms: default_missing_video_timeout_ms(),
            staging_dir: default_staging_dir(),
            encoded_handoff: EncodedHandoffMode::File,
        }
    }
}

impl AssemblerConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.missing_video_timeout_ms == 0 {
            return Err(ConfigError::Validation(
                "assembler: missing_video_timeout_ms must be > 0".into(),
            ));
        }
        if self.staging_dir.trim().is_empty() {
            return Err(ConfigError::Validation(
                "assembler: staging_dir must not be empty".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblerCameraRuntimeConfig {
    pub camera_name: String,
    pub encoder_process_id: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub pixel_format: PixelFormat,
    pub codec: EncoderCodec,
    pub artifact_format: EncoderArtifactFormat,
}

impl AssemblerCameraRuntimeConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.camera_name.trim().is_empty() {
            return Err(ConfigError::Validation(
                "assembler runtime: camera_name must not be empty".into(),
            ));
        }
        if self.encoder_process_id.trim().is_empty() {
            return Err(ConfigError::Validation(
                "assembler runtime: camera encoder_process_id must not be empty".into(),
            ));
        }
        if self.width == 0 || self.height == 0 {
            return Err(ConfigError::Validation(format!(
                "assembler runtime: camera {} must declare non-zero dimensions",
                self.camera_name
            )));
        }
        if self.fps == 0 || self.fps > 1000 {
            return Err(ConfigError::Validation(format!(
                "assembler runtime: camera {} fps must be 1..1000, got {}",
                self.camera_name, self.fps
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblerRobotRuntimeConfig {
    pub robot_name: String,
    pub state_topic: String,
    pub dof: u32,
}

impl AssemblerRobotRuntimeConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.robot_name.trim().is_empty() {
            return Err(ConfigError::Validation(
                "assembler runtime: robot_name must not be empty".into(),
            ));
        }
        if self.state_topic.trim().is_empty() {
            return Err(ConfigError::Validation(
                "assembler runtime: robot state_topic must not be empty".into(),
            ));
        }
        if self.dof == 0 || self.dof as usize > MAX_JOINTS {
            return Err(ConfigError::Validation(format!(
                "assembler runtime: robot {} dof must be 1..{}, got {}",
                self.robot_name, MAX_JOINTS, self.dof
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblerActionRuntimeConfig {
    pub source_name: String,
    pub command_topic: String,
    pub dof: u32,
}

impl AssemblerActionRuntimeConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.source_name.trim().is_empty() {
            return Err(ConfigError::Validation(
                "assembler runtime: action source_name must not be empty".into(),
            ));
        }
        if self.command_topic.trim().is_empty() {
            return Err(ConfigError::Validation(
                "assembler runtime: action command_topic must not be empty".into(),
            ));
        }
        if self.dof == 0 || self.dof as usize > MAX_JOINTS {
            return Err(ConfigError::Validation(format!(
                "assembler runtime: action source {} dof must be 1..{}, got {}",
                self.source_name, MAX_JOINTS, self.dof
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblerRuntimeConfig {
    pub process_id: String,
    pub format: EpisodeFormat,
    pub fps: u32,
    pub chunk_size: u32,
    pub missing_video_timeout_ms: u64,
    pub staging_dir: String,
    #[serde(default)]
    pub encoded_handoff: EncodedHandoffMode,
    #[serde(default)]
    pub cameras: Vec<AssemblerCameraRuntimeConfig>,
    #[serde(default)]
    pub robots: Vec<AssemblerRobotRuntimeConfig>,
    #[serde(default)]
    pub actions: Vec<AssemblerActionRuntimeConfig>,
    pub embedded_config_toml: String,
}

impl AssemblerRuntimeConfig {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.process_id.trim().is_empty() {
            return Err(ConfigError::Validation(
                "assembler runtime: process_id must not be empty".into(),
            ));
        }
        if self.fps == 0 || self.fps > 1000 {
            return Err(ConfigError::Validation(format!(
                "assembler runtime: fps must be 1..1000, got {}",
                self.fps
            )));
        }
        if self.chunk_size == 0 {
            return Err(ConfigError::Validation(
                "assembler runtime: chunk_size must be > 0".into(),
            ));
        }
        if self.missing_video_timeout_ms == 0 {
            return Err(ConfigError::Validation(
                "assembler runtime: missing_video_timeout_ms must be > 0".into(),
            ));
        }
        if self.staging_dir.trim().is_empty() {
            return Err(ConfigError::Validation(
                "assembler runtime: staging_dir must not be empty".into(),
            ));
        }
        if self.cameras.is_empty() {
            return Err(ConfigError::Validation(
                "assembler runtime: at least one camera is required".into(),
            ));
        }
        if self.embedded_config_toml.trim().is_empty() {
            return Err(ConfigError::Validation(
                "assembler runtime: embedded_config_toml must not be empty".into(),
            ));
        }
        for camera in &self.cameras {
            camera.validate()?;
        }
        for robot in &self.robots {
            robot.validate()?;
        }
        for action in &self.actions {
            action.validate()?;
        }
        Ok(())
    }
}

impl FromStr for AssemblerRuntimeConfig {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: AssemblerRuntimeConfig = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EncoderCodec {
    #[serde(alias = "libx264", alias = "h264_nvenc", alias = "h264_vaapi")]
    H264,
    #[serde(alias = "libx265", alias = "hevc_nvenc", alias = "hevc_vaapi")]
    H265,
    #[serde(
        alias = "libsvtav1",
        alias = "librav1e",
        alias = "av1_nvenc",
        alias = "av1_vaapi"
    )]
    Av1,
    Rvl,
}

impl Default for EncoderCodec {
    fn default() -> Self {
        Self::H264
    }
}

impl EncoderCodec {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::H265 => "h265",
            Self::Av1 => "av1",
            Self::Rvl => "rvl",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum EncoderBackend {
    #[default]
    Auto,
    Cpu,
    Nvidia,
    Vaapi,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum EncoderArtifactFormat {
    #[default]
    Auto,
    Mp4,
    Mkv,
    Rvl,
}

impl EncoderArtifactFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Auto => "bin",
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::Rvl => "rvl",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EncoderImplementationFamily {
    Libav,
    Rvl,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EncoderCapabilityDirection {
    Encode,
    Decode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncoderCapability {
    pub codec: EncoderCodec,
    pub implementation: EncoderImplementationFamily,
    pub direction: EncoderCapabilityDirection,
    pub backend: EncoderBackend,
    pub pixel_formats: Vec<PixelFormat>,
    pub artifact_formats: Vec<EncoderArtifactFormat>,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EncoderCapabilityReport {
    #[serde(default)]
    pub codecs: Vec<EncoderCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "EncoderConfigSerde")]
pub struct EncoderConfig {
    pub video_codec: EncoderCodec,
    pub depth_codec: EncoderCodec,
    #[serde(default)]
    pub backend: EncoderBackend,
    #[serde(default)]
    pub artifact_format: EncoderArtifactFormat,
    #[serde(default = "default_queue_size")]
    pub queue_size: u32,
}

#[derive(Debug, Deserialize)]
struct EncoderConfigSerde {
    #[serde(default)]
    codec: Option<EncoderCodec>,
    #[serde(default)]
    video_codec: Option<EncoderCodec>,
    #[serde(default)]
    depth_codec: Option<EncoderCodec>,
    #[serde(default)]
    backend: EncoderBackend,
    #[serde(default)]
    artifact_format: EncoderArtifactFormat,
    #[serde(default = "default_queue_size")]
    queue_size: u32,
}

impl From<EncoderConfigSerde> for EncoderConfig {
    fn from(value: EncoderConfigSerde) -> Self {
        let legacy_codec = value.codec;
        let video_codec = value.video_codec.or(legacy_codec).unwrap_or_default();
        let depth_codec = value
            .depth_codec
            .or(legacy_codec)
            .unwrap_or_else(default_depth_codec);
        Self {
            video_codec,
            depth_codec,
            backend: value.backend,
            artifact_format: value.artifact_format,
            queue_size: value.queue_size,
        }
    }
}

fn default_queue_size() -> u32 {
    32
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            video_codec: EncoderCodec::default(),
            depth_codec: default_depth_codec(),
            backend: EncoderBackend::default(),
            artifact_format: EncoderArtifactFormat::default(),
            queue_size: default_queue_size(),
        }
    }
}

impl EncoderConfig {
    pub fn codec_for_camera(&self, camera: &DeviceConfig) -> EncoderCodec {
        if let Some(pixel_format) = camera.pixel_format {
            return self.codec_for_pixel_format(pixel_format);
        }
        if camera.uses_depth_codec() {
            self.depth_codec_with_gray8_fallback()
        } else {
            self.video_codec
        }
    }

    pub fn codec_for_pixel_format(&self, pixel_format: PixelFormat) -> EncoderCodec {
        match pixel_format {
            PixelFormat::Depth16 => self.depth_codec,
            // Gray8 (e.g. RealSense infrared) is plain monochrome video.
            // It nominally shares depth_codec, but the RVL encoder is
            // depth-specific and physically rejects non-depth16 frames, so
            // fall back to video_codec when depth_codec=rvl. Without this
            // fallback the infrared encoder process exits at episode start
            // with `rvl requires depth16 frames, got Gray8`.
            PixelFormat::Gray8 => self.depth_codec_with_gray8_fallback(),
            _ => self.video_codec,
        }
    }

    fn depth_codec_with_gray8_fallback(&self) -> EncoderCodec {
        if self.depth_codec == EncoderCodec::Rvl {
            self.video_codec
        } else {
            self.depth_codec
        }
    }

    pub fn resolved_artifact_format(&self) -> EncoderArtifactFormat {
        self.resolved_artifact_format_for(self.video_codec)
    }

    pub fn resolved_depth_artifact_format(&self) -> EncoderArtifactFormat {
        self.resolved_artifact_format_for(self.depth_codec)
    }

    pub fn resolved_artifact_format_for(&self, codec: EncoderCodec) -> EncoderArtifactFormat {
        if self.artifact_format != EncoderArtifactFormat::Auto {
            return self.artifact_format;
        }

        match codec {
            EncoderCodec::H264 | EncoderCodec::H265 => EncoderArtifactFormat::Mp4,
            EncoderCodec::Av1 => EncoderArtifactFormat::Mkv,
            EncoderCodec::Rvl => EncoderArtifactFormat::Rvl,
        }
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.queue_size == 0 {
            return Err(ConfigError::Validation(
                "encoder: queue_size must be > 0".into(),
            ));
        }
        self.validate_codec("video_codec", self.video_codec)?;
        self.validate_codec("depth_codec", self.depth_codec)?;
        Ok(())
    }

    fn validate_codec(&self, field_name: &str, codec: EncoderCodec) -> Result<(), ConfigError> {
        if codec == EncoderCodec::Rvl && self.backend == EncoderBackend::Vaapi {
            return Err(ConfigError::Validation(format!(
                "encoder: {field_name}=rvl only supports cpu or auto backends"
            )));
        }
        if codec == EncoderCodec::Rvl && self.backend == EncoderBackend::Nvidia {
            return Err(ConfigError::Validation(format!(
                "encoder: {field_name}=rvl only supports cpu or auto backends"
            )));
        }

        match (codec, self.resolved_artifact_format_for(codec)) {
            (EncoderCodec::Rvl, EncoderArtifactFormat::Rvl)
            | (EncoderCodec::H264, EncoderArtifactFormat::Mp4)
            | (EncoderCodec::H265, EncoderArtifactFormat::Mp4)
            | (EncoderCodec::Av1, EncoderArtifactFormat::Mkv) => Ok(()),
            (EncoderCodec::Rvl, other) => Err(ConfigError::Validation(format!(
                "encoder: {field_name}=rvl requires artifact_format=rvl, got {:?}",
                other
            ))),
            (_, EncoderArtifactFormat::Rvl) => Err(ConfigError::Validation(format!(
                "encoder: artifact_format=rvl requires {field_name}=rvl"
            ))),
            (codec, other) => Err(ConfigError::Validation(format!(
                "encoder: {field_name}={} does not support artifact_format {:?}",
                codec.as_str(),
                other
            ))),
        }
    }
}

fn default_depth_codec() -> EncoderCodec {
    EncoderCodec::Rvl
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncoderRuntimeConfig {
    pub process_id: String,
    #[serde(default)]
    pub camera_name: Option<String>,
    #[serde(default)]
    pub frame_topic: Option<String>,
    pub output_dir: String,
    pub codec: EncoderCodec,
    #[serde(default)]
    pub backend: EncoderBackend,
    #[serde(default)]
    pub artifact_format: EncoderArtifactFormat,
    #[serde(default = "default_queue_size")]
    pub queue_size: u32,
    pub fps: u32,
}

impl EncoderRuntimeConfig {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn resolved_artifact_format(&self) -> EncoderArtifactFormat {
        EncoderConfig {
            video_codec: self.codec,
            depth_codec: self.codec,
            backend: self.backend,
            artifact_format: self.artifact_format,
            queue_size: self.queue_size,
        }
        .resolved_artifact_format()
    }

    pub fn output_extension(&self) -> &'static str {
        self.resolved_artifact_format().extension()
    }

    pub fn output_file_name(&self, episode_index: u32) -> String {
        let stem = self
            .process_id
            .chars()
            .map(|ch| match ch {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
                _ => '_',
            })
            .collect::<String>();
        format!(
            "{stem}_episode_{episode_index:06}.{}",
            self.output_extension()
        )
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.process_id.trim().is_empty() {
            return Err(ConfigError::Validation(
                "encoder runtime: process_id must not be empty".into(),
            ));
        }
        if self
            .camera_name
            .as_deref()
            .is_none_or(|camera_name| camera_name.trim().is_empty())
            && self
                .frame_topic
                .as_deref()
                .is_none_or(|frame_topic| frame_topic.trim().is_empty())
        {
            return Err(ConfigError::Validation(
                "encoder runtime: either camera_name or frame_topic is required".into(),
            ));
        }
        if self.output_dir.trim().is_empty() {
            return Err(ConfigError::Validation(
                "encoder runtime: output_dir must not be empty".into(),
            ));
        }
        if self.fps == 0 || self.fps > 1000 {
            return Err(ConfigError::Validation(format!(
                "encoder runtime: fps must be 1..1000, got {}",
                self.fps
            )));
        }

        EncoderConfig {
            video_codec: self.codec,
            depth_codec: self.codec,
            backend: self.backend,
            artifact_format: self.artifact_format,
            queue_size: self.queue_size,
        }
        .validate()
    }
}

impl FromStr for EncoderRuntimeConfig {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: EncoderRuntimeConfig = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub backend: StorageBackend,
    pub output_path: Option<String>,
    pub endpoint: Option<String>,
    #[serde(default = "default_storage_queue_size")]
    pub queue_size: u32,
}

fn default_storage_queue_size() -> u32 {
    32
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: StorageBackend::default(),
            output_path: Some("./output".into()),
            endpoint: None,
            queue_size: default_storage_queue_size(),
        }
    }
}

impl StorageConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.queue_size == 0 {
            return Err(ConfigError::Validation(
                "storage: queue_size must be > 0".into(),
            ));
        }
        match self.backend {
            StorageBackend::Local => {
                if self
                    .output_path
                    .as_deref()
                    .is_none_or(|path| path.trim().is_empty())
                {
                    return Err(ConfigError::Validation(
                        "storage: local backend requires output_path".into(),
                    ));
                }
            }
            StorageBackend::Http => {
                if self
                    .endpoint
                    .as_deref()
                    .is_none_or(|endpoint| endpoint.trim().is_empty())
                {
                    return Err(ConfigError::Validation(
                        "storage: http backend requires endpoint".into(),
                    ));
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    Local,
    Http,
}

impl Default for StorageBackend {
    fn default() -> Self {
        Self::Local
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageRuntimeConfig {
    pub process_id: String,
    pub backend: StorageBackend,
    pub output_path: Option<String>,
    pub endpoint: Option<String>,
    #[serde(default = "default_storage_queue_size")]
    pub queue_size: u32,
}

impl StorageRuntimeConfig {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.process_id.trim().is_empty() {
            return Err(ConfigError::Validation(
                "storage runtime: process_id must not be empty".into(),
            ));
        }
        StorageConfig {
            backend: self.backend,
            output_path: self.output_path.clone(),
            endpoint: self.endpoint.clone(),
            queue_size: self.queue_size,
        }
        .validate()
    }
}

impl FromStr for StorageRuntimeConfig {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: StorageRuntimeConfig = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

// ---------------------------------------------------------------------------
// Monitor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControllerConfig {
    #[serde(default = "default_shutdown_timeout_ms")]
    pub shutdown_timeout_ms: u64,
    #[serde(default = "default_child_poll_interval_ms")]
    pub child_poll_interval_ms: u64,
}

fn default_shutdown_timeout_ms() -> u64 {
    30_000
}

fn default_child_poll_interval_ms() -> u64 {
    100
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            shutdown_timeout_ms: default_shutdown_timeout_ms(),
            child_poll_interval_ms: default_child_poll_interval_ms(),
        }
    }
}

impl ControllerConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.shutdown_timeout_ms == 0 {
            return Err(ConfigError::Validation(
                "controller: shutdown_timeout_ms must be > 0".into(),
            ));
        }
        if self.child_poll_interval_ms == 0 {
            return Err(ConfigError::Validation(
                "controller: child_poll_interval_ms must be > 0".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizerRuntimeConfig {
    #[serde(default = "default_visualizer_port")]
    pub port: u16,
    #[serde(default)]
    pub cameras: Vec<String>,
    #[serde(default)]
    pub robots: Vec<String>,
    #[serde(default = "default_max_preview_width")]
    pub max_preview_width: u32,
    #[serde(default = "default_max_preview_height")]
    pub max_preview_height: u32,
    #[serde(default = "default_jpeg_quality")]
    pub jpeg_quality: i32,
    #[serde(default = "default_preview_fps")]
    pub preview_fps: u32,
    #[serde(default)]
    pub preview_workers: Option<usize>,
}

fn default_visualizer_port() -> u16 {
    19090
}

fn default_max_preview_width() -> u32 {
    320
}

fn default_max_preview_height() -> u32 {
    240
}

fn default_jpeg_quality() -> i32 {
    30
}

fn default_preview_fps() -> u32 {
    60
}

impl Default for VisualizerRuntimeConfig {
    fn default() -> Self {
        Self {
            port: default_visualizer_port(),
            cameras: Vec::new(),
            robots: Vec::new(),
            max_preview_width: default_max_preview_width(),
            max_preview_height: default_max_preview_height(),
            jpeg_quality: default_jpeg_quality(),
            preview_fps: default_preview_fps(),
            preview_workers: None,
        }
    }
}

impl VisualizerRuntimeConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.port == 0 {
            return Err(ConfigError::Validation(
                "visualizer: port must be > 0".into(),
            ));
        }
        if self.max_preview_width == 0 || self.max_preview_height == 0 {
            return Err(ConfigError::Validation(
                "visualizer: preview dimensions must be > 0".into(),
            ));
        }
        if !(1..=100).contains(&self.jpeg_quality) {
            return Err(ConfigError::Validation(
                "visualizer: jpeg_quality must be 1..100".into(),
            ));
        }
        if self.preview_fps > 1000 {
            return Err(ConfigError::Validation(
                "visualizer: preview_fps must be <= 1000".into(),
            ));
        }
        Ok(())
    }
}

impl FromStr for VisualizerRuntimeConfig {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: VisualizerRuntimeConfig = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiRuntimeConfig {
    /// Optional explicit upstream URL for the control plane WebSocket
    /// (proxied at `/ws/control`). Defaults to
    /// `ws://127.0.0.1:<controller-control-port>` when the controller
    /// derives the runtime config; runtime callers must populate it before
    /// passing into the UI server.
    pub control_websocket_url: Option<String>,
    /// Optional explicit upstream URL for the preview plane WebSocket
    /// (proxied at `/ws/preview`). Defaults to
    /// `ws://127.0.0.1:<visualizer-port>`.
    pub preview_websocket_url: Option<String>,
    #[serde(default = "default_ui_http_host")]
    pub http_host: String,
    #[serde(default = "default_ui_http_port")]
    pub http_port: u16,
    #[serde(default = "default_ui_start_key")]
    pub start_key: String,
    #[serde(default = "default_ui_stop_key")]
    pub stop_key: String,
    #[serde(default = "default_ui_keep_key")]
    pub keep_key: String,
    #[serde(default = "default_ui_discard_key")]
    pub discard_key: String,
}

fn default_ui_http_host() -> String {
    "127.0.0.1".into()
}

fn default_ui_http_port() -> u16 {
    3000
}

fn default_ui_start_key() -> String {
    "s".into()
}

fn default_ui_stop_key() -> String {
    "e".into()
}

fn default_ui_keep_key() -> String {
    "k".into()
}

fn default_ui_discard_key() -> String {
    "x".into()
}

impl Default for UiRuntimeConfig {
    fn default() -> Self {
        Self {
            control_websocket_url: None,
            preview_websocket_url: None,
            http_host: default_ui_http_host(),
            http_port: default_ui_http_port(),
            start_key: default_ui_start_key(),
            stop_key: default_ui_stop_key(),
            keep_key: default_ui_keep_key(),
            discard_key: default_ui_discard_key(),
        }
    }
}

impl UiRuntimeConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        for (label, url) in [
            ("control_websocket_url", &self.control_websocket_url),
            ("preview_websocket_url", &self.preview_websocket_url),
        ] {
            if let Some(url) = url {
                if url.trim().is_empty() {
                    return Err(ConfigError::Validation(format!(
                        "ui: {label} must not be empty"
                    )));
                }
            }
        }

        if self.http_host.trim().is_empty() {
            return Err(ConfigError::Validation(
                "ui: http_host must not be empty".into(),
            ));
        }
        if self.http_port == 0 {
            return Err(ConfigError::Validation(
                "ui: http_port must be greater than zero".into(),
            ));
        }

        let mut seen = HashSet::new();
        for (label, key) in [
            ("start_key", &self.start_key),
            ("stop_key", &self.stop_key),
            ("keep_key", &self.keep_key),
            ("discard_key", &self.discard_key),
        ] {
            let normalized = normalize_ui_key(label, key)?;
            if normalized == "d" || normalized == "r" {
                return Err(ConfigError::Validation(format!(
                    "ui: {label} conflicts with reserved UI shortcut \"{normalized}\""
                )));
            }
            if !seen.insert(normalized.clone()) {
                return Err(ConfigError::Validation(format!(
                    "ui: duplicate key binding \"{normalized}\""
                )));
            }
        }
        Ok(())
    }
}

fn normalize_ui_key(label: &str, raw: &str) -> Result<String, ConfigError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ConfigError::Validation(format!(
            "ui: {label} must not be empty"
        )));
    }

    let mut chars = trimmed.chars();
    let ch = chars.next().ok_or_else(|| {
        ConfigError::Validation(format!("ui: {label} must be a single printable character"))
    })?;
    if chars.next().is_some() {
        return Err(ConfigError::Validation(format!(
            "ui: {label} must be a single printable character"
        )));
    }
    if ch.is_control() {
        return Err(ConfigError::Validation(format!(
            "ui: {label} must be a printable character"
        )));
    }
    Ok(ch.to_ascii_lowercase().to_string())
}

impl FromStr for UiRuntimeConfig {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: UiRuntimeConfig = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

fn encoder_process_id(camera_name: &str) -> String {
    format!("encoder.{camera_name}")
}

fn camera_frames_topic(camera_name: &str) -> String {
    format!("camera/{camera_name}/frames")
}

fn robot_state_topic(robot_name: &str) -> String {
    format!("robot/{robot_name}/state")
}

fn robot_command_topic(robot_name: &str) -> String {
    format!("robot/{robot_name}/command")
}

fn encoder_output_dir(staging_root: &str, camera_name: &str) -> String {
    Path::new(staging_root)
        .join("encoders")
        .join(camera_name)
        .to_string_lossy()
        .into_owned()
}

fn episode_staging_root(staging_root: &str) -> String {
    Path::new(staging_root)
        .join("episodes")
        .to_string_lossy()
        .into_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorConfig {
    #[serde(default = "default_metrics_freq")]
    pub metrics_frequency_hz: f64,
    #[serde(default)]
    pub thresholds: std::collections::HashMap<String, ThresholdGroup>,
}

fn default_metrics_freq() -> f64 {
    1.0
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            metrics_frequency_hz: default_metrics_freq(),
            thresholds: HashMap::new(),
        }
    }
}

impl MonitorConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if !self.metrics_frequency_hz.is_finite() || self.metrics_frequency_hz <= 0.0 {
            return Err(ConfigError::Validation(
                "monitor: metrics_frequency_hz must be a positive finite number".into(),
            ));
        }

        for (process_id, thresholds) in &self.thresholds {
            if process_id.trim().is_empty() {
                return Err(ConfigError::Validation(
                    "monitor: threshold group names must not be empty".into(),
                ));
            }
            for (metric_name, threshold) in thresholds {
                if metric_name.trim().is_empty() {
                    return Err(ConfigError::Validation(format!(
                        "monitor: threshold metric name must not be empty in group \"{process_id}\""
                    )));
                }
                threshold.validate(process_id, metric_name)?;
            }
        }

        Ok(())
    }
}

/// Thresholds keyed by metric name within a given process.
pub type ThresholdGroup = HashMap<String, ThresholdDef>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdDef {
    pub explanation: String,
    pub gt: Option<f64>,
    pub lt: Option<f64>,
    pub gte: Option<f64>,
    pub lte: Option<f64>,
    pub outside: Option<[f64; 2]>,
    pub inside: Option<[f64; 2]>,
    pub occurred: Option<bool>,
    pub gap: Option<f64>,
    pub repeated: Option<bool>,
}

impl ThresholdDef {
    fn validate(&self, process_id: &str, metric_name: &str) -> Result<(), ConfigError> {
        if self.explanation.trim().is_empty() {
            return Err(ConfigError::Validation(format!(
                "monitor: threshold \"{process_id}.{metric_name}\" requires an explanation"
            )));
        }

        let configured_conditions = [
            self.gt.is_some(),
            self.lt.is_some(),
            self.gte.is_some(),
            self.lte.is_some(),
            self.outside.is_some(),
            self.inside.is_some(),
            self.occurred.is_some(),
            self.gap.is_some(),
            self.repeated.is_some(),
        ]
        .into_iter()
        .filter(|value| *value)
        .count();

        if configured_conditions == 0 {
            return Err(ConfigError::Validation(format!(
                "monitor: threshold \"{process_id}.{metric_name}\" must declare at least one condition"
            )));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Sprint Extra A hierarchical device-binary config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "ProjectConfigSerde")]
pub struct ProjectConfig {
    pub project_name: String,
    pub mode: CollectionMode,
    pub episode: EpisodeConfig,
    pub devices: Vec<BinaryDeviceConfig>,
    #[serde(default)]
    pub pairings: Vec<ChannelPairingConfig>,
    pub encoder: EncoderConfig,
    #[serde(default)]
    pub assembler: AssemblerConfig,
    pub storage: StorageConfig,
    #[serde(default)]
    pub monitor: MonitorConfig,
    #[serde(default)]
    pub controller: ControllerConfig,
    #[serde(default)]
    pub visualizer: VisualizerConfig,
    #[serde(default)]
    pub ui: UiRuntimeConfig,
}

#[derive(Debug, Deserialize)]
struct ProjectConfigSerde {
    #[serde(default = "default_project_name")]
    project_name: String,
    #[serde(default)]
    mode: Option<CollectionMode>,
    episode: EpisodeConfig,
    devices: Vec<BinaryDeviceConfig>,
    #[serde(default)]
    pairings: Vec<ChannelPairingConfig>,
    encoder: EncoderConfig,
    #[serde(default)]
    assembler: AssemblerConfig,
    storage: StorageConfig,
    #[serde(default)]
    monitor: MonitorConfig,
    #[serde(default)]
    controller: ControllerConfig,
    #[serde(default)]
    visualizer: VisualizerConfig,
    #[serde(default)]
    ui: UiRuntimeConfig,
}

impl From<ProjectConfigSerde> for ProjectConfig {
    fn from(value: ProjectConfigSerde) -> Self {
        let mode = value
            .mode
            .unwrap_or_else(|| infer_collection_mode_v2(&value.pairings));
        Self {
            project_name: value.project_name,
            mode,
            episode: value.episode,
            devices: value.devices,
            pairings: value.pairings,
            encoder: value.encoder,
            assembler: value.assembler,
            storage: value.storage,
            monitor: value.monitor,
            controller: value.controller,
            visualizer: value.visualizer,
            ui: value.ui,
        }
    }
}

fn infer_collection_mode_v2(pairings: &[ChannelPairingConfig]) -> CollectionMode {
    if pairings.is_empty() {
        CollectionMode::Intervention
    } else {
        CollectionMode::Teleop
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryDeviceConfig {
    pub name: String,
    #[serde(default)]
    pub executable: Option<String>,
    pub driver: String,
    pub id: String,
    pub bus_root: String,
    #[serde(default)]
    pub channels: Vec<DeviceChannelConfigV2>,
    #[serde(flatten, default)]
    pub extra: toml::Table,
}

impl BinaryDeviceConfig {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.name.trim().is_empty() {
            return Err(ConfigError::Validation(
                "device: name must not be empty".into(),
            ));
        }
        if self.driver.trim().is_empty() {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": driver must not be empty",
                self.name
            )));
        }
        if matches!(self.driver.as_str(), "airbot-e2" | "airbot-e2b" | "airbot-g2") {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": {} is no longer configured as a standalone device; use driver \"airbot-play\" with an enabled \"e2\" or \"g2\" channel instead",
                self.name, self.driver
            )));
        }
        if self
            .executable
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": executable must not be empty when set",
                self.name
            )));
        }
        if self.id.trim().is_empty() {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": id must not be empty",
                self.name
            )));
        }
        if self.bus_root.trim().is_empty() {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": bus_root must not be empty",
                self.name
            )));
        }
        if self.channels.is_empty() {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": at least one [[devices.channels]] entry is required",
                self.name
            )));
        }
        let mut channel_types = HashSet::new();
        for channel in &self.channels {
            if !channel_types.insert(channel.channel_type.as_str()) {
                return Err(ConfigError::Validation(format!(
                    "device \"{}\": duplicate channel_type \"{}\"",
                    self.name, channel.channel_type
                )));
            }
            channel.validate(self)?;
        }
        Ok(())
    }

    pub fn channel_named(&self, channel_type: &str) -> Option<&DeviceChannelConfigV2> {
        self.channels
            .iter()
            .find(|channel| channel.channel_type == channel_type)
    }
}

impl FromStr for BinaryDeviceConfig {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: BinaryDeviceConfig = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceChannelConfigV2 {
    pub channel_type: String,
    pub kind: DeviceType,
    #[serde(default = "default_enabled_true")]
    pub enabled: bool,
    /// User-facing name for this channel (independent per channel even when
    /// multiple channels share one device process / bus_root). Defaults to
    /// the device-supplied `default_name` when present, otherwise the
    /// channel_type. Renaming a row in the wizard mutates *only* this field.
    #[serde(default)]
    pub name: Option<String>,
    /// Human-readable label for this channel as reported by the device
    /// executable's `query --json` response (e.g. "AIRBOT E2", "V4L2 Camera").
    /// Used purely for display; the controller no longer derives this from
    /// the driver name.
    #[serde(default)]
    pub channel_label: Option<String>,
    #[serde(default)]
    pub mode: Option<RobotMode>,
    #[serde(default)]
    pub dof: Option<u32>,
    #[serde(default)]
    pub publish_states: Vec<RobotStateKind>,
    #[serde(default)]
    pub recorded_states: Vec<RobotStateKind>,
    #[serde(default)]
    pub control_frequency_hz: Option<f64>,
    #[serde(default)]
    pub profile: Option<CameraChannelProfile>,
    #[serde(default)]
    pub command_defaults: ChannelCommandDefaults,
    #[serde(flatten, default)]
    pub extra: toml::Table,
}

fn default_enabled_true() -> bool {
    true
}

impl DeviceChannelConfigV2 {
    fn validate(&self, device: &BinaryDeviceConfig) -> Result<(), ConfigError> {
        if self.channel_type.trim().is_empty() {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": channel_type must not be empty",
                device.name
            )));
        }
        match self.kind {
            DeviceType::Camera => {
                if self.mode.is_some() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": camera channels do not accept mode",
                        device.name, self.channel_type
                    )));
                }
                if self.dof.is_some() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": camera channels do not accept dof",
                        device.name, self.channel_type
                    )));
                }
                if !self.publish_states.is_empty() || !self.recorded_states.is_empty() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": camera channels do not accept publish_states or recorded_states",
                        device.name, self.channel_type
                    )));
                }
                if self.control_frequency_hz.is_some() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": camera channels do not accept control_frequency_hz",
                        device.name, self.channel_type
                    )));
                }
                self.profile.as_ref().ok_or_else(|| {
                    ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": enabled camera channels require profile",
                        device.name, self.channel_type
                    ))
                })?;
                if let Some(profile) = &self.profile {
                    profile.validate(device, &self.channel_type)?;
                }
                self.command_defaults
                    .validate(device, &self.channel_type, 0)?;
            }
            DeviceType::Robot => {
                let dof = self.dof.ok_or_else(|| {
                    ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": robot channels require dof",
                        device.name, self.channel_type
                    ))
                })?;
                if dof == 0 || dof as usize > MAX_DOF {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": dof must be 1..{}, got {}",
                        device.name, self.channel_type, MAX_DOF, dof
                    )));
                }
                if self.enabled && self.mode.is_none() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": enabled robot channels require mode",
                        device.name, self.channel_type
                    )));
                }
                if self.enabled && self.publish_states.is_empty() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": enabled robot channels require publish_states",
                        device.name, self.channel_type
                    )));
                }
                if self
                    .recorded_states
                    .iter()
                    .any(|state| !self.publish_states.contains(state))
                {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": recorded_states must be a subset of publish_states",
                        device.name, self.channel_type
                    )));
                }
                if let Some(profile) = &self.profile {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": robot channels do not accept profile ({:?})",
                        device.name, self.channel_type, profile
                    )));
                }
                if let Some(freq) = self.control_frequency_hz {
                    if !freq.is_finite() || freq <= 0.0 {
                        return Err(ConfigError::Validation(format!(
                            "device \"{}\" channel \"{}\": control_frequency_hz must be a positive finite number",
                            device.name, self.channel_type
                        )));
                    }
                }
                self.command_defaults
                    .validate(device, &self.channel_type, dof as usize)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CameraChannelProfile {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub pixel_format: PixelFormat,
    #[serde(default)]
    pub native_pixel_format: Option<String>,
}

impl CameraChannelProfile {
    fn validate(
        &self,
        device: &BinaryDeviceConfig,
        channel_type: &str,
    ) -> Result<(), ConfigError> {
        if self.width == 0 || self.height == 0 {
            return Err(ConfigError::Validation(format!(
                "device \"{}\" channel \"{}\": camera profile requires non-zero width and height",
                device.name, channel_type
            )));
        }
        if self.fps == 0 || self.fps > 1000 {
            return Err(ConfigError::Validation(format!(
                "device \"{}\" channel \"{}\": camera profile fps must be 1..1000, got {}",
                device.name, channel_type, self.fps
            )));
        }
        if self
            .native_pixel_format
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ConfigError::Validation(format!(
                "device \"{}\" channel \"{}\": native_pixel_format must not be empty",
                device.name, channel_type
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChannelCommandDefaults {
    #[serde(default)]
    pub joint_mit_kp: Vec<f64>,
    #[serde(default)]
    pub joint_mit_kd: Vec<f64>,
    #[serde(default)]
    pub parallel_mit_kp: Vec<f64>,
    #[serde(default)]
    pub parallel_mit_kd: Vec<f64>,
}

impl ChannelCommandDefaults {
    fn validate(
        &self,
        device: &BinaryDeviceConfig,
        channel_type: &str,
        dof: usize,
    ) -> Result<(), ConfigError> {
        for (label, values, limit) in [
            ("joint_mit_kp", &self.joint_mit_kp, MAX_DOF),
            ("joint_mit_kd", &self.joint_mit_kd, MAX_DOF),
            ("parallel_mit_kp", &self.parallel_mit_kp, MAX_PARALLEL),
            ("parallel_mit_kd", &self.parallel_mit_kd, MAX_PARALLEL),
        ] {
            if values.iter().any(|value| !value.is_finite()) {
                return Err(ConfigError::Validation(format!(
                    "device \"{}\" channel \"{}\": {} values must be finite",
                    device.name, channel_type, label
                )));
            }
            if values.len() > limit {
                return Err(ConfigError::Validation(format!(
                    "device \"{}\" channel \"{}\": {} may contain at most {} values",
                    device.name, channel_type, label, limit
                )));
            }
        }
        if !self.joint_mit_kp.is_empty() && dof != 0 && self.joint_mit_kp.len() != dof {
            return Err(ConfigError::Validation(format!(
                "device \"{}\" channel \"{}\": joint_mit_kp length must match dof {}",
                device.name, channel_type, dof
            )));
        }
        if !self.joint_mit_kd.is_empty() && dof != 0 && self.joint_mit_kd.len() != dof {
            return Err(ConfigError::Validation(format!(
                "device \"{}\" channel \"{}\": joint_mit_kd length must match dof {}",
                device.name, channel_type, dof
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RobotStateKind {
    JointPosition,
    JointVelocity,
    JointEffort,
    EndEffectorPose,
    EndEffectorTwist,
    EndEffectorWrench,
    ParallelPosition,
    ParallelVelocity,
    ParallelEffort,
}

impl RobotStateKind {
    pub fn topic_suffix(self) -> &'static str {
        match self {
            Self::JointPosition => "joint_position",
            Self::JointVelocity => "joint_velocity",
            Self::JointEffort => "joint_effort",
            Self::EndEffectorPose => "end_effector_pose",
            Self::EndEffectorTwist => "end_effector_twist",
            Self::EndEffectorWrench => "end_effector_wrench",
            Self::ParallelPosition => "parallel_position",
            Self::ParallelVelocity => "parallel_velocity",
            Self::ParallelEffort => "parallel_effort",
        }
    }

    pub fn value_len(self, dof: u32) -> u32 {
        match self {
            Self::JointPosition | Self::JointVelocity | Self::JointEffort => dof,
            Self::ParallelPosition | Self::ParallelVelocity | Self::ParallelEffort => {
                dof.min(MAX_PARALLEL as u32)
            }
            Self::EndEffectorPose => 7,
            Self::EndEffectorTwist | Self::EndEffectorWrench => 6,
        }
    }

    pub fn uses_pose_payload(self) -> bool {
        matches!(self, Self::EndEffectorPose)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RobotCommandKind {
    JointPosition,
    JointMit,
    EndPose,
    ParallelPosition,
    ParallelMit,
}

impl RobotCommandKind {
    pub fn topic_suffix(self) -> &'static str {
        match self {
            Self::JointPosition => "joint_position",
            Self::JointMit => "joint_mit",
            Self::EndPose => "end_pose",
            Self::ParallelPosition => "parallel_position",
            Self::ParallelMit => "parallel_mit",
        }
    }

    pub fn uses_pose_payload(self) -> bool {
        matches!(self, Self::EndPose)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelPairingConfig {
    pub leader_device: String,
    pub leader_channel_type: String,
    pub follower_device: String,
    pub follower_channel_type: String,
    #[serde(default = "default_mapping")]
    pub mapping: MappingStrategy,
    pub leader_state: RobotStateKind,
    pub follower_command: RobotCommandKind,
    #[serde(default)]
    pub joint_index_map: Vec<u32>,
    #[serde(default)]
    pub joint_scales: Vec<f64>,
}

impl ChannelPairingConfig {
    fn validate(&self, config: &ProjectConfig) -> Result<(), ConfigError> {
        let leader_device = config.device_named(&self.leader_device).ok_or_else(|| {
            ConfigError::Validation(format!(
                "pairing references unknown leader_device \"{}\"",
                self.leader_device
            ))
        })?;
        let follower_device = config.device_named(&self.follower_device).ok_or_else(|| {
            ConfigError::Validation(format!(
                "pairing references unknown follower_device \"{}\"",
                self.follower_device
            ))
        })?;
        let leader = leader_device
            .channel_named(&self.leader_channel_type)
            .ok_or_else(|| {
                ConfigError::Validation(format!(
                    "pairing references unknown leader channel {}:{}",
                    self.leader_device, self.leader_channel_type
                ))
            })?;
        let follower = follower_device
            .channel_named(&self.follower_channel_type)
            .ok_or_else(|| {
                ConfigError::Validation(format!(
                    "pairing references unknown follower channel {}:{}",
                    self.follower_device, self.follower_channel_type
                ))
            })?;
        if leader.kind != DeviceType::Robot || follower.kind != DeviceType::Robot {
            return Err(ConfigError::Validation(format!(
                "pairing {}:{} -> {}:{} must target robot channels",
                self.leader_device,
                self.leader_channel_type,
                self.follower_device,
                self.follower_channel_type
            )));
        }
        if self.leader_device == self.follower_device
            && self.leader_channel_type == self.follower_channel_type
        {
            return Err(ConfigError::Validation(
                "pairing leader and follower channel must differ".into(),
            ));
        }
        if !leader.publish_states.contains(&self.leader_state) {
            return Err(ConfigError::Validation(format!(
                "pairing {}:{} leader_state {:?} is not present in publish_states",
                self.leader_device, self.leader_channel_type, self.leader_state
            )));
        }
        match self.mapping {
            MappingStrategy::DirectJoint => {
                if matches!(self.leader_state, RobotStateKind::EndEffectorPose)
                    || matches!(self.follower_command, RobotCommandKind::EndPose)
                {
                    return Err(ConfigError::Validation(
                        "direct-joint mapping does not allow end-effector pose commands".into(),
                    ));
                }
            }
            MappingStrategy::Cartesian => {
                if self.leader_state != RobotStateKind::EndEffectorPose
                    || self.follower_command != RobotCommandKind::EndPose
                {
                    return Err(ConfigError::Validation(
                        "cartesian mapping requires leader_state=end_effector_pose and follower_command=end_pose"
                            .into(),
                    ));
                }
                if !self.joint_index_map.is_empty() || !self.joint_scales.is_empty() {
                    return Err(ConfigError::Validation(
                        "cartesian mapping does not use joint_index_map or joint_scales".into(),
                    ));
                }
            }
        }
        if self.joint_scales.iter().any(|value| !value.is_finite()) {
            return Err(ConfigError::Validation(
                "pairing joint_scales must be finite".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VisualizerConfig {
    #[serde(default = "default_visualizer_port")]
    pub port: u16,
    #[serde(default = "default_max_preview_width")]
    pub max_preview_width: u32,
    #[serde(default = "default_max_preview_height")]
    pub max_preview_height: u32,
    #[serde(default = "default_jpeg_quality")]
    pub jpeg_quality: i32,
    #[serde(default = "default_preview_fps")]
    pub preview_fps: u32,
    #[serde(default)]
    pub preview_workers: Option<usize>,
}

impl VisualizerConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        VisualizerRuntimeConfig {
            port: self.port,
            cameras: Vec::new(),
            robots: Vec::new(),
            max_preview_width: self.max_preview_width,
            max_preview_height: self.max_preview_height,
            jpeg_quality: self.jpeg_quality,
            preview_fps: self.preview_fps,
            preview_workers: self.preview_workers,
        }
        .validate()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizerCameraSourceConfig {
    pub channel_id: String,
    pub frame_topic: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizerRobotSourceConfig {
    pub channel_id: String,
    pub state_kind: RobotStateKind,
    pub state_topic: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizerRuntimeConfigV2 {
    pub port: u16,
    #[serde(default)]
    pub camera_sources: Vec<VisualizerCameraSourceConfig>,
    #[serde(default)]
    pub robot_sources: Vec<VisualizerRobotSourceConfig>,
    pub max_preview_width: u32,
    pub max_preview_height: u32,
    pub jpeg_quality: i32,
    pub preview_fps: u32,
    #[serde(default)]
    pub preview_workers: Option<usize>,
}

impl VisualizerRuntimeConfigV2 {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        VisualizerRuntimeConfig {
            port: self.port,
            cameras: self
                .camera_sources
                .iter()
                .map(|source| source.channel_id.clone())
                .collect(),
            robots: self
                .robot_sources
                .iter()
                .map(|source| source.channel_id.clone())
                .collect(),
            max_preview_width: self.max_preview_width,
            max_preview_height: self.max_preview_height,
            jpeg_quality: self.jpeg_quality,
            preview_fps: self.preview_fps,
            preview_workers: self.preview_workers,
        }
        .validate()?;
        Ok(())
    }
}

impl FromStr for VisualizerRuntimeConfigV2 {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: VisualizerRuntimeConfigV2 = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeleopRuntimeConfigV2 {
    pub process_id: String,
    pub leader_channel_id: String,
    pub follower_channel_id: String,
    pub leader_state_kind: RobotStateKind,
    pub leader_state_topic: String,
    pub follower_command_kind: RobotCommandKind,
    pub follower_command_topic: String,
    #[serde(default = "default_mapping")]
    pub mapping: MappingStrategy,
    #[serde(default)]
    pub joint_index_map: Vec<u32>,
    #[serde(default)]
    pub joint_scales: Vec<f64>,
    #[serde(default)]
    pub command_defaults: ChannelCommandDefaults,
}

impl TeleopRuntimeConfigV2 {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.process_id.trim().is_empty() {
            return Err(ConfigError::Validation(
                "teleop runtime v2: process_id must not be empty".into(),
            ));
        }
        if self.leader_channel_id.trim().is_empty() || self.follower_channel_id.trim().is_empty() {
            return Err(ConfigError::Validation(
                "teleop runtime v2: leader_channel_id and follower_channel_id must not be empty"
                    .into(),
            ));
        }
        if self.leader_state_topic.trim().is_empty()
            || self.follower_command_topic.trim().is_empty()
        {
            return Err(ConfigError::Validation(
                "teleop runtime v2: state and command topics must not be empty".into(),
            ));
        }
        if self.joint_scales.iter().any(|scale| !scale.is_finite()) {
            return Err(ConfigError::Validation(
                "teleop runtime v2: joint_scales must be finite".into(),
            ));
        }
        Ok(())
    }
}

impl FromStr for TeleopRuntimeConfigV2 {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: TeleopRuntimeConfigV2 = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncoderRuntimeConfigV2 {
    pub process_id: String,
    pub channel_id: String,
    pub frame_topic: String,
    pub output_dir: String,
    pub codec: EncoderCodec,
    #[serde(default)]
    pub backend: EncoderBackend,
    #[serde(default)]
    pub artifact_format: EncoderArtifactFormat,
    #[serde(default = "default_queue_size")]
    pub queue_size: u32,
    pub fps: u32,
}

impl EncoderRuntimeConfigV2 {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn resolved_artifact_format(&self) -> EncoderArtifactFormat {
        EncoderConfig {
            video_codec: self.codec,
            depth_codec: self.codec,
            backend: self.backend,
            artifact_format: self.artifact_format,
            queue_size: self.queue_size,
        }
        .resolved_artifact_format()
    }

    pub fn output_extension(&self) -> &'static str {
        self.resolved_artifact_format().extension()
    }

    pub fn output_file_name(&self, episode_index: u32) -> String {
        let stem = self
            .process_id
            .chars()
            .map(|ch| match ch {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
                _ => '_',
            })
            .collect::<String>();
        format!(
            "{stem}_episode_{episode_index:06}.{}",
            self.output_extension()
        )
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.process_id.trim().is_empty()
            || self.channel_id.trim().is_empty()
            || self.frame_topic.trim().is_empty()
            || self.output_dir.trim().is_empty()
        {
            return Err(ConfigError::Validation(
                "encoder runtime v2: process_id, channel_id, frame_topic, and output_dir must not be empty"
                    .into(),
            ));
        }
        if self.fps == 0 || self.fps > 1000 {
            return Err(ConfigError::Validation(format!(
                "encoder runtime v2: fps must be 1..1000, got {}",
                self.fps
            )));
        }
        EncoderConfig {
            video_codec: self.codec,
            depth_codec: self.codec,
            backend: self.backend,
            artifact_format: self.artifact_format,
            queue_size: self.queue_size,
        }
        .validate()
    }
}

impl FromStr for EncoderRuntimeConfigV2 {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: EncoderRuntimeConfigV2 = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblerCameraRuntimeConfigV2 {
    pub channel_id: String,
    pub encoder_process_id: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub pixel_format: PixelFormat,
    pub codec: EncoderCodec,
    pub artifact_format: EncoderArtifactFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblerObservationRuntimeConfigV2 {
    pub channel_id: String,
    pub state_kind: RobotStateKind,
    pub state_topic: String,
    pub value_len: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblerActionRuntimeConfigV2 {
    pub channel_id: String,
    pub command_kind: RobotCommandKind,
    pub command_topic: String,
    pub value_len: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblerRuntimeConfigV2 {
    pub process_id: String,
    pub format: EpisodeFormat,
    pub fps: u32,
    pub chunk_size: u32,
    pub missing_video_timeout_ms: u64,
    pub staging_dir: String,
    #[serde(default)]
    pub encoded_handoff: EncodedHandoffMode,
    #[serde(default)]
    pub cameras: Vec<AssemblerCameraRuntimeConfigV2>,
    #[serde(default)]
    pub observations: Vec<AssemblerObservationRuntimeConfigV2>,
    #[serde(default)]
    pub actions: Vec<AssemblerActionRuntimeConfigV2>,
    pub embedded_config_toml: String,
}

impl AssemblerRuntimeConfigV2 {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.process_id.trim().is_empty() || self.staging_dir.trim().is_empty() {
            return Err(ConfigError::Validation(
                "assembler runtime v2: process_id and staging_dir must not be empty".into(),
            ));
        }
        if self.fps == 0 || self.fps > 1000 || self.chunk_size == 0 {
            return Err(ConfigError::Validation(
                "assembler runtime v2: fps must be 1..1000 and chunk_size must be > 0".into(),
            ));
        }
        if self.cameras.is_empty() {
            return Err(ConfigError::Validation(
                "assembler runtime v2: at least one camera is required".into(),
            ));
        }
        if self.embedded_config_toml.trim().is_empty() {
            return Err(ConfigError::Validation(
                "assembler runtime v2: embedded_config_toml must not be empty".into(),
            ));
        }
        Ok(())
    }
}

impl FromStr for AssemblerRuntimeConfigV2 {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: AssemblerRuntimeConfigV2 = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectJointCompatibilityPeer {
    pub driver: String,
    pub channel_type: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectJointCompatibility {
    #[serde(default)]
    pub can_lead: Vec<DirectJointCompatibilityPeer>,
    #[serde(default)]
    pub can_follow: Vec<DirectJointCompatibilityPeer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceQueryChannel {
    pub channel_type: String,
    pub kind: DeviceType,
    pub available: bool,
    /// Display label for this channel (e.g., "AIRBOT E2", "V4L2 Camera").
    /// Falls back to the parent device's `device_label` when None.
    #[serde(default)]
    pub channel_label: Option<String>,
    /// Default user-facing name to use when this channel is first added to
    /// a project (e.g., "airbot_play_arm", "airbot_e2", "camera"). The
    /// controller stores this in `DeviceChannelConfigV2.name`. Falls back
    /// to channel_type when None.
    #[serde(default)]
    pub default_name: Option<String>,
    #[serde(default)]
    pub modes: Vec<String>,
    #[serde(default)]
    pub profiles: Vec<CameraChannelProfile>,
    #[serde(default)]
    pub supported_states: Vec<RobotStateKind>,
    #[serde(default)]
    pub supported_commands: Vec<RobotCommandKind>,
    #[serde(default)]
    pub supports_fk: bool,
    #[serde(default)]
    pub supports_ik: bool,
    pub dof: Option<u32>,
    pub default_control_frequency_hz: Option<f64>,
    #[serde(default)]
    pub direct_joint_compatibility: DirectJointCompatibility,
    #[serde(default)]
    pub defaults: ChannelCommandDefaults,
    #[serde(default)]
    pub optional_info: toml::Table,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceQueryDevice {
    pub id: String,
    pub device_class: String,
    pub device_label: String,
    #[serde(default)]
    pub optional_info: toml::Table,
    #[serde(default)]
    pub channels: Vec<DeviceQueryChannel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceQueryResponse {
    pub driver: String,
    #[serde(default)]
    pub devices: Vec<DeviceQueryDevice>,
}

#[derive(Debug, Clone)]
pub struct ResolvedCameraChannel {
    pub channel_id: String,
    pub device_name: String,
    pub bus_root: String,
    pub channel_type: String,
    pub frame_topic: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub pixel_format: PixelFormat,
}

#[derive(Debug, Clone)]
pub struct ResolvedRobotChannel {
    pub channel_id: String,
    pub device_name: String,
    pub driver: String,
    pub bus_root: String,
    pub channel_type: String,
    pub dof: u32,
    pub state_topics: Vec<(RobotStateKind, String)>,
    pub recorded_states: Vec<RobotStateKind>,
    pub control_frequency_hz: f64,
    pub command_defaults: ChannelCommandDefaults,
}

impl ProjectConfig {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    /// Empty project shell used by interactive setup before devices are finalized.
    pub fn draft_setup_template() -> Self {
        Self {
            project_name: default_project_name(),
            mode: CollectionMode::Intervention,
            episode: EpisodeConfig::default(),
            devices: Vec::new(),
            pairings: Vec::new(),
            encoder: EncoderConfig::default(),
            assembler: AssemblerConfig::default(),
            storage: StorageConfig::default(),
            monitor: MonitorConfig::default(),
            controller: ControllerConfig::default(),
            // `VisualizerConfig` uses `#[derive(Default)]` with serde defaults only for
            // deserialization — explicit values keep draft templates valid before TOML parse.
            visualizer: VisualizerConfig {
                port: 19090,
                max_preview_width: 320,
                max_preview_height: 240,
                jpeg_quality: 30,
                preview_fps: 60,
                preview_workers: None,
            },
            ui: UiRuntimeConfig::default(),
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.project_name.trim().is_empty() {
            return Err(ConfigError::Validation(
                "project_name must not be empty".into(),
            ));
        }
        self.episode.validate()?;
        if self.devices.is_empty() {
            return Err(ConfigError::Validation(
                "at least one [[devices]] entry is required".into(),
            ));
        }
        let mut names = HashSet::new();
        let mut bus_roots = HashSet::new();
        for device in &self.devices {
            if !names.insert(device.name.as_str()) {
                return Err(ConfigError::Validation(format!(
                    "duplicate device name: \"{}\"",
                    device.name
                )));
            }
            if !bus_roots.insert(device.bus_root.as_str()) {
                return Err(ConfigError::Validation(format!(
                    "duplicate bus_root: \"{}\"",
                    device.bus_root
                )));
            }
            device.validate()?;
        }
        for pairing in &self.pairings {
            pairing.validate(self)?;
        }
        match self.mode {
            CollectionMode::Teleop => {
                if self.pairings.is_empty() {
                    return Err(ConfigError::Validation(
                        "mode=teleop requires at least one [[pairings]] entry".into(),
                    ));
                }
            }
            CollectionMode::Intervention => {
                if !self.pairings.is_empty() {
                    return Err(ConfigError::Validation(
                        "mode=intervention does not allow [[pairings]] entries".into(),
                    ));
                }
            }
        }
        self.encoder.validate()?;
        self.assembler.validate()?;
        self.storage.validate()?;
        self.monitor.validate()?;
        self.controller.validate()?;
        self.visualizer.validate()?;
        self.ui.validate()?;
        Ok(())
    }

    pub fn device_named(&self, name: &str) -> Option<&BinaryDeviceConfig> {
        self.devices.iter().find(|device| device.name == name)
    }

    pub fn resolved_camera_channels(&self) -> Vec<ResolvedCameraChannel> {
        let mut channels = Vec::new();
        for device in &self.devices {
            for channel in &device.channels {
                if channel.kind != DeviceType::Camera || !channel.enabled {
                    continue;
                }
                let Some(profile) = channel.profile.as_ref() else {
                    continue;
                };
                let channel_id = device_channel_id(&device.name, &channel.channel_type);
                channels.push(ResolvedCameraChannel {
                    channel_id,
                    device_name: device.name.clone(),
                    bus_root: device.bus_root.clone(),
                    channel_type: channel.channel_type.clone(),
                    frame_topic: camera_frames_topic_v2(&device.bus_root, &channel.channel_type),
                    width: profile.width,
                    height: profile.height,
                    fps: profile.fps,
                    pixel_format: profile.pixel_format,
                });
            }
        }
        channels
    }

    pub fn resolved_robot_channels(&self) -> Vec<ResolvedRobotChannel> {
        let mut channels = Vec::new();
        for device in &self.devices {
            for channel in &device.channels {
                if channel.kind != DeviceType::Robot || !channel.enabled {
                    continue;
                }
                let dof = channel.dof.unwrap_or_default();
                let recorded_states = if channel.recorded_states.is_empty() {
                    channel.publish_states.clone()
                } else {
                    channel.recorded_states.clone()
                };
                let state_topics = channel
                    .publish_states
                    .iter()
                    .copied()
                    .map(|state| {
                        (
                            state,
                            robot_state_topic_v2(&device.bus_root, &channel.channel_type, state),
                        )
                    })
                    .collect::<Vec<_>>();
                channels.push(ResolvedRobotChannel {
                    channel_id: device_channel_id(&device.name, &channel.channel_type),
                    device_name: device.name.clone(),
                    driver: device.driver.clone(),
                    bus_root: device.bus_root.clone(),
                    channel_type: channel.channel_type.clone(),
                    dof,
                    state_topics,
                    recorded_states,
                    control_frequency_hz: channel.control_frequency_hz.unwrap_or(60.0),
                    command_defaults: channel.command_defaults.clone(),
                });
            }
        }
        channels
    }

    pub fn ui_runtime_config(&self) -> UiRuntimeConfig {
        let mut config = self.ui.clone();
        if config.preview_websocket_url.is_none() {
            config.preview_websocket_url =
                Some(format!("ws://127.0.0.1:{}", self.visualizer.port));
        }
        config
    }

    pub fn visualizer_runtime_config_v2(&self) -> VisualizerRuntimeConfigV2 {
        let camera_sources = self
            .resolved_camera_channels()
            .into_iter()
            .map(|camera| VisualizerCameraSourceConfig {
                channel_id: camera.channel_id,
                frame_topic: camera.frame_topic,
            })
            .collect();
        let robot_sources = self
            .resolved_robot_channels()
            .into_iter()
            .flat_map(|robot| {
                let channel_id = robot.channel_id.clone();
                robot.state_topics
                    .into_iter()
                    .map(move |(state_kind, state_topic)| VisualizerRobotSourceConfig {
                        channel_id: channel_id.clone(),
                        state_kind,
                        state_topic,
                    })
            })
            .collect();
        VisualizerRuntimeConfigV2 {
            port: self.visualizer.port,
            camera_sources,
            robot_sources,
            max_preview_width: self.visualizer.max_preview_width,
            max_preview_height: self.visualizer.max_preview_height,
            jpeg_quality: self.visualizer.jpeg_quality,
            preview_fps: self.visualizer.preview_fps,
            preview_workers: self.visualizer.preview_workers,
        }
    }

    pub fn encoder_runtime_configs_v2(&self) -> Vec<EncoderRuntimeConfigV2> {
        self.resolved_camera_channels()
            .into_iter()
            .map(|camera| {
                let codec = self.encoder.codec_for_pixel_format(camera.pixel_format);
                EncoderRuntimeConfigV2 {
                    process_id: encoder_process_id_v2(&camera.channel_id),
                    channel_id: camera.channel_id.clone(),
                    frame_topic: camera.frame_topic,
                    output_dir: encoder_output_dir_v2(&self.assembler.staging_dir, &camera.channel_id),
                    codec,
                    backend: self.encoder.backend,
                    artifact_format: self.encoder.resolved_artifact_format_for(codec),
                    queue_size: self.encoder.queue_size,
                    fps: camera.fps,
                }
            })
            .collect()
    }

    pub fn assembler_runtime_config_v2(
        &self,
        embedded_config_toml: String,
    ) -> AssemblerRuntimeConfigV2 {
        let cameras = self
            .resolved_camera_channels()
            .into_iter()
            .map(|camera| {
                let codec = self.encoder.codec_for_pixel_format(camera.pixel_format);
                AssemblerCameraRuntimeConfigV2 {
                    channel_id: camera.channel_id.clone(),
                    encoder_process_id: encoder_process_id_v2(&camera.channel_id),
                    width: camera.width,
                    height: camera.height,
                    fps: camera.fps,
                    pixel_format: camera.pixel_format,
                    codec,
                    artifact_format: self.encoder.resolved_artifact_format_for(codec),
                }
            })
            .collect();
        let observations = self
            .resolved_robot_channels()
            .into_iter()
            .flat_map(|robot| {
                let recorded = robot.recorded_states.clone();
                robot.state_topics
                    .into_iter()
                    .filter(move |(state_kind, _)| recorded.contains(state_kind))
                    .map(move |(state_kind, state_topic)| AssemblerObservationRuntimeConfigV2 {
                        channel_id: robot.channel_id.clone(),
                        state_kind,
                        state_topic,
                        value_len: state_kind.value_len(robot.dof),
                    })
            })
            .collect();
        let actions = self
            .pairings
            .iter()
            .filter_map(|pairing| {
                let follower = self
                    .device_named(&pairing.follower_device)?
                    .channel_named(&pairing.follower_channel_type)?;
                let dof = follower.dof.unwrap_or_default();
                let bus_root = &self.device_named(&pairing.follower_device)?.bus_root;
                Some(AssemblerActionRuntimeConfigV2 {
                    channel_id: device_channel_id(&pairing.follower_device, &pairing.follower_channel_type),
                    command_kind: pairing.follower_command,
                    command_topic: robot_command_topic_v2(
                        bus_root,
                        &pairing.follower_channel_type,
                        pairing.follower_command,
                    ),
                    value_len: match pairing.follower_command {
                        RobotCommandKind::JointPosition | RobotCommandKind::JointMit => dof,
                        RobotCommandKind::ParallelPosition | RobotCommandKind::ParallelMit => {
                            dof.min(MAX_PARALLEL as u32)
                        }
                        RobotCommandKind::EndPose => 7,
                    },
                })
            })
            .collect();
        AssemblerRuntimeConfigV2 {
            process_id: "episode-assembler".into(),
            format: self.episode.format,
            fps: self.episode.fps,
            chunk_size: self.episode.chunk_size,
            missing_video_timeout_ms: self.assembler.missing_video_timeout_ms,
            staging_dir: episode_staging_root_v2(&self.assembler.staging_dir),
            encoded_handoff: self.assembler.encoded_handoff,
            cameras,
            observations,
            actions,
            embedded_config_toml,
        }
    }

    pub fn teleop_runtime_configs_v2(&self) -> Vec<TeleopRuntimeConfigV2> {
        self.pairings
            .iter()
            .filter_map(|pairing| {
                let leader_device = self.device_named(&pairing.leader_device)?;
                let follower_device = self.device_named(&pairing.follower_device)?;
                let follower_channel = follower_device.channel_named(&pairing.follower_channel_type)?;
                Some(TeleopRuntimeConfigV2 {
                    process_id: format!(
                        "teleop.{}.{}.to.{}.{}",
                        pairing.leader_device,
                        pairing.leader_channel_type,
                        pairing.follower_device,
                        pairing.follower_channel_type
                    ),
                    leader_channel_id: device_channel_id(
                        &pairing.leader_device,
                        &pairing.leader_channel_type,
                    ),
                    follower_channel_id: device_channel_id(
                        &pairing.follower_device,
                        &pairing.follower_channel_type,
                    ),
                    leader_state_kind: pairing.leader_state,
                    leader_state_topic: robot_state_topic_v2(
                        &leader_device.bus_root,
                        &pairing.leader_channel_type,
                        pairing.leader_state,
                    ),
                    follower_command_kind: pairing.follower_command,
                    follower_command_topic: robot_command_topic_v2(
                        &follower_device.bus_root,
                        &pairing.follower_channel_type,
                        pairing.follower_command,
                    ),
                    mapping: pairing.mapping,
                    joint_index_map: pairing.joint_index_map.clone(),
                    joint_scales: pairing.joint_scales.clone(),
                    command_defaults: follower_channel.command_defaults.clone(),
                })
            })
            .collect()
    }

    pub fn storage_runtime_config(&self) -> StorageRuntimeConfig {
        StorageRuntimeConfig {
            process_id: "storage".into(),
            backend: self.storage.backend,
            output_path: self.storage.output_path.clone(),
            endpoint: self.storage.endpoint.clone(),
            queue_size: self.storage.queue_size,
        }
    }
}

impl FromStr for ProjectConfig {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: ProjectConfig = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

fn device_channel_id(device_name: &str, channel_type: &str) -> String {
    format!("{device_name}/{channel_type}")
}

fn channel_prefix_v2(bus_root: &str, channel_type: &str) -> String {
    format!("{bus_root}/{channel_type}")
}

fn camera_frames_topic_v2(bus_root: &str, channel_type: &str) -> String {
    format!("{}/frames", channel_prefix_v2(bus_root, channel_type))
}

fn robot_state_topic_v2(bus_root: &str, channel_type: &str, state: RobotStateKind) -> String {
    format!(
        "{}/states/{}",
        channel_prefix_v2(bus_root, channel_type),
        state.topic_suffix()
    )
}

fn robot_command_topic_v2(
    bus_root: &str,
    channel_type: &str,
    command: RobotCommandKind,
) -> String {
    format!(
        "{}/commands/{}",
        channel_prefix_v2(bus_root, channel_type),
        command.topic_suffix()
    )
}

fn encoder_process_id_v2(channel_id: &str) -> String {
    format!("encoder.{}", channel_id.replace('/', "."))
}

fn encoder_output_dir_v2(staging_root: &str, channel_id: &str) -> String {
    Path::new(staging_root)
        .join("encoders")
        .join(channel_id.replace('/', "__"))
        .to_string_lossy()
        .into_owned()
}

fn episode_staging_root_v2(staging_root: &str) -> String {
    Path::new(staging_root)
        .join("episodes")
        .to_string_lossy()
        .into_owned()
}
