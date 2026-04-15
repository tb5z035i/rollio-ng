mod runtime;

use airbot_play_rust::can::worker::CanWorkerBackend;
use airbot_play_rust::model::MountedEefType;
use airbot_play_rust::probe::discover::{ProbeError, probe_all};
use airbot_play_rust::types::DiscoveredInstance;
use async_trait::async_trait;
use clap::{Args, Parser, Subcommand};
use rollio_types::config::{ConfigError, DeviceConfig, DeviceType};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;

pub use runtime::{RollioRuntimeConfig, RollioRuntimeError, run_rollio_runtime};

const DEFAULT_DOF: u32 = 1;
const DEFAULT_CONTROL_FREQUENCY_HZ: f64 = 250.0;
const DEFAULT_PROBE_TIMEOUT_MS: u64 = 1000;

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
        #[arg(long)]
        interface: Option<String>,
        #[arg(long, default_value_t = DEFAULT_PROBE_TIMEOUT_MS)]
        timeout_ms: u64,
    },
    Validate {
        id: String,
        #[arg(long, default_value_t = DEFAULT_PROBE_TIMEOUT_MS)]
        timeout_ms: u64,
    },
    Capabilities {
        id: String,
        #[arg(long, default_value_t = DEFAULT_PROBE_TIMEOUT_MS)]
        timeout_ms: u64,
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
    #[error("probe error: {0}")]
    Probe(#[from] ProbeError),
    #[error("runtime transport error: {0}")]
    Transport(#[from] RollioRuntimeError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid {driver} probe id: {id}")]
    InvalidProbeId { driver: &'static str, id: String },
    #[error("unknown {driver} device id: {id}")]
    UnknownProbeId { driver: &'static str, id: String },
    #[error("{0}")]
    InvalidDevice(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedProbeDevice {
    id: String,
    interface: String,
    product_variant: String,
}

#[async_trait(?Send)]
trait ProbeProvider: Send + Sync {
    async fn probe(&self, timeout: Duration) -> Result<Vec<DiscoveredInstance>, AirbotEefError>;
}

struct LibraryProbeProvider;

#[async_trait(?Send)]
impl ProbeProvider for LibraryProbeProvider {
    async fn probe(&self, timeout: Duration) -> Result<Vec<DiscoveredInstance>, AirbotEefError> {
        Ok(probe_all(timeout).await?.instances)
    }
}

pub async fn run_with_profile(profile: DriverProfile) -> Result<(), AirbotEefError> {
    run_cli(Cli::parse(), profile).await
}

pub async fn run_cli(cli: Cli, profile: DriverProfile) -> Result<(), AirbotEefError> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    execute_command_with(&mut handle, cli.command, profile, &LibraryProbeProvider).await
}

pub async fn execute_command<W: Write>(
    writer: &mut W,
    command: Command,
    profile: DriverProfile,
) -> Result<(), AirbotEefError> {
    execute_command_with(writer, command, profile, &LibraryProbeProvider).await
}

async fn execute_command_with<W, P>(
    writer: &mut W,
    command: Command,
    profile: DriverProfile,
    probe_provider: &P,
) -> Result<(), AirbotEefError>
where
    W: Write,
    P: ProbeProvider,
{
    match command {
        Command::Probe {
            interface,
            timeout_ms,
        } => {
            let devices = probe_devices_with_provider(
                probe_provider,
                profile,
                interface.as_deref(),
                probe_timeout(timeout_ms),
            )
            .await?;
            serde_json::to_writer_pretty(&mut *writer, &devices)?;
            writer.write_all(b"\n")?;
        }
        Command::Validate { id, timeout_ms } => {
            let device = require_probe_device_with_provider(
                probe_provider,
                profile,
                &id,
                probe_timeout(timeout_ms),
            )
            .await?;
            let report = ValidateReport {
                valid: true,
                id: device.id,
            };
            serde_json::to_writer_pretty(&mut *writer, &report)?;
            writer.write_all(b"\n")?;
        }
        Command::Capabilities { id, timeout_ms } => {
            let device = require_probe_device_with_provider(
                probe_provider,
                profile,
                &id,
                probe_timeout(timeout_ms),
            )
            .await?;
            let report = CapabilitiesReport {
                id: device.id,
                driver: profile.driver_name().to_owned(),
                dof: DEFAULT_DOF,
                supported_modes: ["free-drive".to_owned(), "command-following".to_owned()],
                transport: "can".to_owned(),
                interface: device.interface,
                product_variant: device.product_variant,
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

async fn probe_devices_with_provider<P>(
    probe_provider: &P,
    profile: DriverProfile,
    interface: Option<&str>,
    timeout: Duration,
) -> Result<Vec<ProbeDevice>, AirbotEefError>
where
    P: ProbeProvider,
{
    let resolved = resolve_probe_devices(profile, probe_provider.probe(timeout).await?, interface);
    Ok(resolved
        .into_iter()
        .map(|device| ProbeDevice {
            id: device.id,
            driver: profile.driver_name().to_owned(),
            interface: device.interface,
            product_variant: device.product_variant,
            dof: DEFAULT_DOF,
        })
        .collect())
}

async fn require_probe_device_with_provider<P>(
    probe_provider: &P,
    profile: DriverProfile,
    device_id: &str,
    timeout: Duration,
) -> Result<ResolvedProbeDevice, AirbotEefError>
where
    P: ProbeProvider,
{
    let interface = parse_probe_interface(profile, device_id)?;
    resolve_probe_devices(
        profile,
        probe_provider.probe(timeout).await?,
        Some(interface.as_str()),
    )
    .into_iter()
    .find(|device| device.interface == interface)
    .ok_or_else(|| AirbotEefError::UnknownProbeId {
        driver: profile.driver_name(),
        id: device_id.to_owned(),
    })
}

fn parse_probe_interface(
    profile: DriverProfile,
    device_id: &str,
) -> Result<String, AirbotEefError> {
    let normalized = device_id.trim();
    let Some((interface, suffix)) = normalized.split_once(':') else {
        return Err(AirbotEefError::InvalidProbeId {
            driver: profile.driver_name(),
            id: device_id.to_owned(),
        });
    };
    let interface = interface.trim();
    if interface.is_empty() || !suffix.trim().eq_ignore_ascii_case(profile.label()) {
        return Err(AirbotEefError::InvalidProbeId {
            driver: profile.driver_name(),
            id: device_id.to_owned(),
        });
    }
    Ok(interface.to_owned())
}

fn resolve_probe_devices(
    profile: DriverProfile,
    instances: Vec<DiscoveredInstance>,
    interface_filter: Option<&str>,
) -> Vec<ResolvedProbeDevice> {
    let mut devices = Vec::new();
    let mut seen_interfaces = HashSet::new();
    let interface_filter = interface_filter
        .map(str::trim)
        .filter(|value| !value.is_empty());

    for instance in instances {
        let interface = instance.interface.trim();
        if interface.is_empty() {
            continue;
        }
        if interface_filter.is_some_and(|value| value != interface) {
            continue;
        }
        let Some(mounted_eef) = instance
            .mounted_eef
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        if !profile.matches_end_effector(mounted_eef) {
            continue;
        }
        if !seen_interfaces.insert(interface.to_owned()) {
            continue;
        }

        devices.push(ResolvedProbeDevice {
            id: format!("{interface}:{}", profile.label()),
            interface: interface.to_owned(),
            product_variant: profile.label().to_owned(),
        });
    }

    devices.sort_unstable_by(|left, right| left.interface.cmp(&right.interface));
    devices
}

fn probe_timeout(timeout_ms: u64) -> Duration {
    Duration::from_millis(timeout_ms)
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

    if let Some(product_variant) = device.product_variant.as_deref() {
        if !profile.matches_end_effector(product_variant) {
            return Err(AirbotEefError::InvalidDevice(format!(
                "device \"{}\": product_variant \"{product_variant}\" does not match driver {}",
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
    use std::collections::BTreeMap;

    struct FixtureProbeProvider {
        instances: Vec<DiscoveredInstance>,
    }

    #[async_trait(?Send)]
    impl ProbeProvider for FixtureProbeProvider {
        async fn probe(
            &self,
            _timeout: Duration,
        ) -> Result<Vec<DiscoveredInstance>, AirbotEefError> {
            Ok(self.instances.clone())
        }
    }

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
    fn validate_device_rejects_wrong_product_variant() {
        let mut device = device_for(DriverProfile::E2);
        device.product_variant = Some("g2".to_owned());
        let err = validate_device(DriverProfile::E2, device)
            .expect_err("product variant should mismatch");
        assert!(err.to_string().contains("product_variant"));
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

    #[tokio::test]
    async fn probe_command_returns_only_matching_profile_devices() {
        let provider = FixtureProbeProvider {
            instances: vec![
                fixture_instance("can0", "E2B"),
                fixture_instance("can1", "G2"),
                fixture_instance("can2", "E2B"),
            ],
        };
        let mut output = Vec::new();

        execute_command_with(
            &mut output,
            Command::Probe {
                interface: None,
                timeout_ms: 250,
            },
            DriverProfile::G2,
            &provider,
        )
        .await
        .expect("probe command should succeed");

        let devices: Vec<ProbeDevice> =
            serde_json::from_slice(&output).expect("probe output should be valid JSON");
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "can1:g2");
        assert_eq!(devices[0].interface, "can1");
        assert_eq!(devices[0].driver, "airbot-g2");
        assert_eq!(devices[0].product_variant, "g2");
    }

    #[tokio::test]
    async fn probe_command_honors_interface_filter() {
        let provider = FixtureProbeProvider {
            instances: vec![
                fixture_instance("can0", "G2"),
                fixture_instance("can1", "G2"),
            ],
        };
        let mut output = Vec::new();

        execute_command_with(
            &mut output,
            Command::Probe {
                interface: Some("can1".to_owned()),
                timeout_ms: 250,
            },
            DriverProfile::G2,
            &provider,
        )
        .await
        .expect("probe command should succeed");

        let devices: Vec<ProbeDevice> =
            serde_json::from_slice(&output).expect("probe output should be valid JSON");
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "can1:g2");
    }

    #[tokio::test]
    async fn validate_command_rejects_wrong_profile_suffix() {
        let provider = FixtureProbeProvider {
            instances: vec![fixture_instance("can0", "E2B")],
        };
        let mut output = Vec::new();

        let error = execute_command_with(
            &mut output,
            Command::Validate {
                id: "can0:e2".to_owned(),
                timeout_ms: 250,
            },
            DriverProfile::G2,
            &provider,
        )
        .await
        .expect_err("validate should reject mismatched suffix");

        assert_eq!(error.to_string(), "invalid airbot-g2 probe id: can0:e2");
    }

    #[tokio::test]
    async fn capabilities_command_uses_resolved_interface_from_probe_id() {
        let provider = FixtureProbeProvider {
            instances: vec![fixture_instance("can7", "G2")],
        };
        let mut output = Vec::new();

        execute_command_with(
            &mut output,
            Command::Capabilities {
                id: "can7:g2".to_owned(),
                timeout_ms: 250,
            },
            DriverProfile::G2,
            &provider,
        )
        .await
        .expect("capabilities command should succeed");

        let report: CapabilitiesReport =
            serde_json::from_slice(&output).expect("capabilities output should be JSON");
        assert_eq!(report.id, "can7:g2");
        assert_eq!(report.interface, "can7");
        assert_eq!(report.product_variant, "g2");
        assert_eq!(report.driver, "airbot-g2");
        assert_eq!(
            report.supported_modes,
            ["free-drive".to_owned(), "command-following".to_owned()]
        );
    }

    fn fixture_instance(interface: &str, mounted_eef: &str) -> DiscoveredInstance {
        DiscoveredInstance {
            interface: interface.to_owned(),
            identified_as: Some("AIRBOT Play".to_owned()),
            product_sn: Some(format!("SN-{interface}")),
            pcba_sn: Some(format!("PCBA-{interface}")),
            mounted_eef: Some(mounted_eef.to_owned()),
            base_board_software_version: Some("01 02 03 04".to_owned()),
            end_board_software_version: Some("05 06 07 08".to_owned()),
            motor_software_versions: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }
}
