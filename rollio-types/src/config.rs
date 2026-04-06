use crate::messages::{PixelFormat, MAX_JOINTS};
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
pub struct Config {
    pub episode: EpisodeConfig,
    pub devices: Vec<DeviceConfig>,
    #[serde(default)]
    pub pairing: Vec<PairConfig>,
    pub encoder: EncoderConfig,
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

impl Config {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
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
        }

        self.encoder.validate()?;
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
        if config.websocket_url.is_none() {
            config.websocket_url = Some(format!("ws://127.0.0.1:{}", self.visualizer.port));
        }
        config
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
}

impl RobotMode {
    pub fn control_mode_value(self) -> u32 {
        match self {
            Self::FreeDrive => 0,
            Self::CommandFollowing => 1,
        }
    }

    pub fn from_control_mode_value(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::FreeDrive),
            1 => Some(Self::CommandFollowing),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Pairing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairConfig {
    pub leader: String,
    pub follower: String,
    #[serde(default = "default_mapping")]
    pub mapping: MappingStrategy,
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

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncoderConfig {
    pub codec: String,
    #[serde(default = "default_queue_size")]
    pub queue_size: u32,
}

fn default_queue_size() -> u32 {
    32
}

impl EncoderConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        const KNOWN_CODECS: &[&str] = &[
            "libx264",
            "libx265",
            "h264_nvenc",
            "hevc_nvenc",
            "ffv1",
            "mjpeg",
        ];
        if !KNOWN_CODECS.contains(&self.codec.as_str()) {
            return Err(ConfigError::Validation(format!(
                "encoder: unknown codec \"{}\", expected one of: {}",
                self.codec,
                KNOWN_CODECS.join(", ")
            )));
        }
        if self.queue_size == 0 {
            return Err(ConfigError::Validation(
                "encoder: queue_size must be > 0".into(),
            ));
        }
        Ok(())
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
}

impl StorageConfig {
    fn validate(&self) -> Result<(), ConfigError> {
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
    3_000
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
    9090
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiRuntimeConfig {
    pub websocket_url: Option<String>,
}

impl UiRuntimeConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if let Some(websocket_url) = &self.websocket_url {
            if websocket_url.trim().is_empty() {
                return Err(ConfigError::Validation(
                    "ui: websocket_url must not be empty".into(),
                ));
            }
        }
        Ok(())
    }
}

impl FromStr for UiRuntimeConfig {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: UiRuntimeConfig = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MonitorConfig {
    #[serde(default = "default_metrics_freq")]
    pub metrics_frequency_hz: f64,
    #[serde(default)]
    pub thresholds: std::collections::HashMap<String, ThresholdGroup>,
}

fn default_metrics_freq() -> f64 {
    1.0
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
