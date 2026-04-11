mod runtime;

use airbot_play_rust::can::worker::CanWorkerBackend;
use airbot_play_rust::model::MountedEefType;
use clap::{Args, Parser, Subcommand};
use rollio_types::config::{ConfigError, DeviceConfig, DeviceType};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use thiserror::Error;

pub use runtime::{RollioRuntimeConfig, RollioRuntimeError, run_rollio_runtime};

const DEFAULT_DOF: u32 = 1;
const DEFAULT_CONTROL_FREQUENCY_HZ: f64 = 250.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverProfile {
    E2,
    G2,
}

impl DriverProfile {
    pub fn driver_name(self) -> &'static str {
        match self {
            Self::E2 => "airbot-e2",
            Self::G2 => "airbot-g2",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::E2 => "e2",
            Self::G2 => "g2",
        }
    }

    pub fn mounted_eef(self) -> MountedEefType {
        match self {
            Self::E2 => MountedEefType::E2B,
            Self::G2 => MountedEefType::G2,
        }
    }

    pub fn default_mit_kp(self) -> f64 {
        match self {
            Self::E2 => 0.0,
            Self::G2 => 10.0,
        }
    }

    pub fn default_mit_kd(self) -> f64 {
        match self {
            Self::E2 => 0.0,
            Self::G2 => 0.5,
        }
    }

    pub fn matches_end_effector(self, value: &str) -> bool {
        match self {
            Self::E2 => matches!(value.trim().to_ascii_lowercase().as_str(), "e2" | "e2b"),
            Self::G2 => value.trim().eq_ignore_ascii_case("g2"),
        }
    }
}

#[derive(Debug, Parser)]
#[command(about = "Standalone AIRBOT end-effector driver for Rollio")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    Probe {
        #[arg(long, default_value = "can0")]
        interface: String,
    },
    Validate {
        id: String,
    },
    Capabilities {
        id: String,
        #[arg(long, default_value = "can0")]
        interface: String,
    },
    Run(RunArgs),
}

#[derive(Debug, Clone, Args)]
pub struct RunArgs {
    #[arg(long, value_name = "PATH", conflicts_with = "config_inline")]
    pub config: Option<PathBuf>,
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    pub config_inline: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeDevice {
    pub id: String,
    pub driver: String,
    pub interface: String,
    pub product_variant: String,
    pub dof: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidateReport {
    pub valid: bool,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitiesReport {
    pub id: String,
    pub driver: String,
    pub dof: u32,
    pub supported_modes: [String; 2],
    pub transport: String,
    pub interface: String,
    pub product_variant: String,
}

#[derive(Debug, Error)]
pub enum AirbotEefError {
    #[error("failed to read config: {0}")]
    Config(#[from] ConfigError),
    #[error("runtime transport error: {0}")]
    Transport(#[from] RollioRuntimeError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    InvalidDevice(String),
}

pub async fn run_with_profile(profile: DriverProfile) -> Result<(), AirbotEefError> {
    run_cli(Cli::parse(), profile).await
}

pub async fn run_cli(cli: Cli, profile: DriverProfile) -> Result<(), AirbotEefError> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    execute_command(&mut handle, cli.command, profile).await
}

pub async fn execute_command<W: Write>(
    writer: &mut W,
    command: Command,
    profile: DriverProfile,
) -> Result<(), AirbotEefError> {
    match command {
        Command::Probe { interface } => {
            let devices = vec![ProbeDevice {
                id: format!("{interface}:{}", profile.label()),
                driver: profile.driver_name().to_owned(),
                interface,
                product_variant: profile.label().to_owned(),
                dof: DEFAULT_DOF,
            }];
            serde_json::to_writer_pretty(&mut *writer, &devices)?;
            writer.write_all(b"\n")?;
        }
        Command::Validate { id } => {
            let report = ValidateReport { valid: true, id };
            serde_json::to_writer_pretty(&mut *writer, &report)?;
            writer.write_all(b"\n")?;
        }
        Command::Capabilities { id, interface } => {
            let report = CapabilitiesReport {
                id,
                driver: profile.driver_name().to_owned(),
                dof: DEFAULT_DOF,
                supported_modes: [
                    "free-drive".to_owned(),
                    "command-following".to_owned(),
                ],
                transport: "can".to_owned(),
                interface,
                product_variant: profile.label().to_owned(),
            };
            serde_json::to_writer_pretty(&mut *writer, &report)?;
            writer.write_all(b"\n")?;
        }
        Command::Run(args) => {
            let device = load_device_config(&args, profile)?;
            run_device(profile, &device).await?;
        }
    }

    Ok(())
}

pub fn load_device_config(
    args: &RunArgs,
    profile: DriverProfile,
) -> Result<DeviceConfig, AirbotEefError> {
    let device = if let Some(config_path) = &args.config {
        DeviceConfig::from_file(config_path)?
    } else if let Some(config_inline) = &args.config_inline {
        config_inline.parse::<DeviceConfig>()?
    } else {
        return Err(AirbotEefError::InvalidDevice(
            "run requires either --config or --config-inline".into(),
        ));
    };

    validate_device(profile, device)
}

pub fn validate_device(
    profile: DriverProfile,
    device: DeviceConfig,
) -> Result<DeviceConfig, AirbotEefError> {
    if device.device_type != DeviceType::Robot {
        return Err(AirbotEefError::InvalidDevice(format!(
            "device \"{}\" is not a robot",
            device.name
        )));
    }
    if device.driver != profile.driver_name() {
        return Err(AirbotEefError::InvalidDevice(format!(
            "device \"{}\" uses driver \"{}\", expected {}",
            device.name,
            device.driver,
            profile.driver_name()
        )));
    }

    let dof = device.dof.unwrap_or_default();
    if dof != DEFAULT_DOF {
        return Err(AirbotEefError::InvalidDevice(format!(
            "device \"{}\": {} requires dof = {DEFAULT_DOF}, got {dof}",
            device.name,
            profile.driver_name()
        )));
    }

    if device.interface.as_deref().is_none() {
        return Err(AirbotEefError::InvalidDevice(format!(
            "device \"{}\": {} requires interface",
            device.name,
            profile.driver_name()
        )));
    }

    if let Some(transport) = device.transport.as_deref() {
        if transport != "can" {
            return Err(AirbotEefError::InvalidDevice(format!(
                "device \"{}\": {} requires transport = \"can\", got \"{transport}\"",
                device.name,
                profile.driver_name()
            )));
        }
    }

    if let Some(end_effector) = device.end_effector.as_deref() {
        if !profile.matches_end_effector(end_effector) {
            return Err(AirbotEefError::InvalidDevice(format!(
                "device \"{}\": end_effector \"{end_effector}\" does not match driver {}",
                device.name,
                profile.driver_name()
            )));
        }
    }

    Ok(device)
}

pub async fn run_device(
    profile: DriverProfile,
    device: &DeviceConfig,
) -> Result<(), AirbotEefError> {
    let config = RollioRuntimeConfig {
        device_name: device.name.clone(),
        interface: device
            .interface
            .clone()
            .expect("validated AIRBOT EEF device should include interface"),
        initial_mode: device.mode.expect("validated robot mode should be present"),
        publish_rate_hz: device
            .control_frequency_hz
            .unwrap_or(DEFAULT_CONTROL_FREQUENCY_HZ),
        profile,
        can_backend: CanWorkerBackend::AsyncFd,
        mit_kp: device
            .mit_kp
            .as_ref()
            .and_then(|values| values.first().copied())
            .unwrap_or_else(|| profile.default_mit_kp()),
        mit_kd: device
            .mit_kd
            .as_ref()
            .and_then(|values| values.first().copied())
            .unwrap_or_else(|| profile.default_mit_kd()),
        command_velocity: 0.0,
        command_effort: 0.0,
        current_threshold: 0.0,
    };
    run_rollio_runtime(config).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollio_types::config::RobotMode;

    fn device_for(profile: DriverProfile) -> DeviceConfig {
        DeviceConfig {
            name: "eef".to_owned(),
            device_type: DeviceType::Robot,
            driver: profile.driver_name().to_owned(),
            id: "eef0".to_owned(),
            width: None,
            height: None,
            fps: None,
            pixel_format: None,
            stream: None,
            channel: None,
            dof: Some(1),
            mode: Some(RobotMode::CommandFollowing),
            control_frequency_hz: Some(100.0),
            transport: Some("can".to_owned()),
            interface: Some("can0".to_owned()),
            product_variant: None,
            end_effector: Some(profile.label().to_owned()),
            model_path: None,
            gravity_comp_torque_scales: None,
            mit_kp: None,
            mit_kd: None,
            command_latency_ms: None,
            state_noise_stddev: None,
            extra: toml::Table::new(),
        }
    }

    #[test]
    fn validate_device_rejects_wrong_driver_name() {
        let mut device = device_for(DriverProfile::E2);
        device.driver = DriverProfile::G2.driver_name().to_owned();
        let err = validate_device(DriverProfile::E2, device).expect_err("driver should mismatch");
        assert!(err.to_string().contains("expected airbot-e2"));
    }

    #[test]
    fn validate_device_rejects_wrong_end_effector_label() {
        let mut device = device_for(DriverProfile::G2);
        device.end_effector = Some("e2".to_owned());
        let err = validate_device(DriverProfile::G2, device).expect_err("eef should mismatch");
        assert!(err.to_string().contains("does not match driver"));
    }

    #[test]
    fn run_device_uses_profile_specific_default_gains() {
        let e2_device = device_for(DriverProfile::E2);
        let g2_device = device_for(DriverProfile::G2);

        let e2_config = RollioRuntimeConfig {
            device_name: e2_device.name.clone(),
            interface: e2_device.interface.clone().unwrap(),
            initial_mode: e2_device.mode.unwrap(),
            publish_rate_hz: e2_device.control_frequency_hz.unwrap(),
            profile: DriverProfile::E2,
            can_backend: CanWorkerBackend::AsyncFd,
            mit_kp: e2_device
                .mit_kp
                .as_ref()
                .and_then(|values| values.first().copied())
                .unwrap_or_else(|| DriverProfile::E2.default_mit_kp()),
            mit_kd: e2_device
                .mit_kd
                .as_ref()
                .and_then(|values| values.first().copied())
                .unwrap_or_else(|| DriverProfile::E2.default_mit_kd()),
            command_velocity: 0.0,
            command_effort: 0.0,
            current_threshold: 0.0,
        };
        let g2_config = RollioRuntimeConfig {
            device_name: g2_device.name.clone(),
            interface: g2_device.interface.clone().unwrap(),
            initial_mode: g2_device.mode.unwrap(),
            publish_rate_hz: g2_device.control_frequency_hz.unwrap(),
            profile: DriverProfile::G2,
            can_backend: CanWorkerBackend::AsyncFd,
            mit_kp: g2_device
                .mit_kp
                .as_ref()
                .and_then(|values| values.first().copied())
                .unwrap_or_else(|| DriverProfile::G2.default_mit_kp()),
            mit_kd: g2_device
                .mit_kd
                .as_ref()
                .and_then(|values| values.first().copied())
                .unwrap_or_else(|| DriverProfile::G2.default_mit_kd()),
            command_velocity: 0.0,
            command_effort: 0.0,
            current_threshold: 0.0,
        };

        assert_eq!(e2_config.mit_kp, 0.0);
        assert_eq!(e2_config.mit_kd, 0.0);
        assert_eq!(g2_config.mit_kp, 10.0);
        assert_eq!(g2_config.mit_kd, 0.5);
    }
}
