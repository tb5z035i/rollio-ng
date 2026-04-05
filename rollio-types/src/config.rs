use serde::Deserialize;
use std::collections::HashSet;
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

#[derive(Debug, Deserialize)]
pub struct Config {
    pub episode: EpisodeConfig,
    pub devices: Vec<DeviceConfig>,
    #[serde(default)]
    pub pairing: Vec<PairConfig>,
    pub encoder: EncoderConfig,
    pub storage: StorageConfig,
    #[serde(default)]
    pub monitor: MonitorConfig,
}

impl Config {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
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

        Ok(())
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

#[derive(Debug, Deserialize)]
pub struct EpisodeConfig {
    pub format: EpisodeFormat,
    pub fps: u32,
    #[serde(default = "default_chunk_size")]
    pub chunk_size: u32,
}

fn default_chunk_size() -> u32 {
    1000
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Deserialize)]
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
    pub pixel_format: Option<String>,

    // Robot-specific (optional)
    pub dof: Option<u32>,
    pub mode: Option<RobotMode>,
}

impl DeviceConfig {
    fn validate(&self) -> Result<(), ConfigError> {
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
        Ok(())
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeviceType {
    Camera,
    Robot,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RobotMode {
    FreeDrive,
    CommandFollowing,
}

// ---------------------------------------------------------------------------
// Pairing
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct PairConfig {
    pub leader: String,
    pub follower: String,
    #[serde(default = "default_mapping")]
    pub mapping: MappingStrategy,
}

fn default_mapping() -> MappingStrategy {
    MappingStrategy::DirectJoint
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MappingStrategy {
    DirectJoint,
    Cartesian,
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
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
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct StorageConfig {
    pub backend: StorageBackend,
    pub output_path: Option<String>,
    pub endpoint: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    Local,
    Http,
}

// ---------------------------------------------------------------------------
// Monitor
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
pub struct MonitorConfig {
    #[serde(default = "default_metrics_freq")]
    pub metrics_frequency_hz: f64,
    #[serde(default)]
    pub thresholds: std::collections::HashMap<String, ThresholdGroup>,
}

fn default_metrics_freq() -> f64 {
    1.0
}

/// Thresholds keyed by metric name within a given process.
pub type ThresholdGroup = std::collections::HashMap<String, ThresholdDef>;

#[derive(Debug, Deserialize)]
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
