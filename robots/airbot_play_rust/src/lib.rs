mod rollio_runtime;

use airbot_play_rust::can::worker::CanWorkerBackend;
use airbot_play_rust::model::ModelBackendKind;
use airbot_play_rust::probe::discover::{probe_all, ProbeError};
use airbot_play_rust::types::DiscoveredInstance;
use async_trait::async_trait;
use clap::{Args, Parser, Subcommand};
use rollio_runtime::{run_rollio_runtime, RollioRuntimeConfig, RollioRuntimeError};
use rollio_types::config::{DeviceConfig, DeviceType};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;

pub const DRIVER_NAME: &str = "airbot-play";
pub const DEFAULT_DOF: u32 = 6;
pub const DEFAULT_PRODUCT_VARIANT: &str = "play-e2";
pub const DEFAULT_PROBE_TIMEOUT_MS: u64 = 1000;
pub const SUPPORTED_MODES: [&str; 2] = ["free-drive", "command-following"];

#[derive(Debug, Parser)]
#[command(name = "rollio-robot-airbot-play")]
#[command(about = "AIRBOT Play driver for Rollio")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    Probe {
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
    pub end_effector: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidateReport {
    pub valid: bool,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilitiesReport {
    pub id: String,
    pub driver: String,
    pub dof: u32,
    pub supported_modes: [String; 2],
    pub default_frequency_hz: f64,
    pub transport: String,
    pub interface: String,
    pub product_variant: String,
    pub serial_number: String,
    pub end_effector: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedProbeDevice {
    id: String,
    interface: String,
    product_variant: String,
    end_effector: Option<String>,
}

#[derive(Debug, Error)]
pub enum AirbotCliError {
    #[error("failed to read config: {0}")]
    Config(#[from] rollio_types::config::ConfigError),
    #[error("probe error: {0}")]
    Probe(#[from] ProbeError),
    #[error("runtime transport error: {0}")]
    Transport(#[from] RollioRuntimeError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    InvalidDevice(String),
    #[error("invalid AIRBOT probe id: {0}")]
    InvalidProbeId(String),
    #[error("no AIRBOT devices with readable serial numbers were detected")]
    NoDetectedDevices,
    #[error("unknown AIRBOT device id: {0}")]
    UnknownProbeId(String),
}

#[async_trait(?Send)]
pub trait ProbeProvider: Send + Sync {
    async fn probe(&self, timeout: Duration) -> Result<Vec<DiscoveredInstance>, AirbotCliError>;
}

#[async_trait(?Send)]
pub trait TransportRunner: Send + Sync {
    async fn run(&self, config: RollioRuntimeConfig) -> Result<(), AirbotCliError>;
}

pub struct LibraryProbeProvider;
pub struct LibraryTransportRunner;

#[async_trait(?Send)]
impl ProbeProvider for LibraryProbeProvider {
    async fn probe(&self, timeout: Duration) -> Result<Vec<DiscoveredInstance>, AirbotCliError> {
        Ok(probe_all(timeout).await?.instances)
    }
}

#[async_trait(?Send)]
impl TransportRunner for LibraryTransportRunner {
    async fn run(&self, config: RollioRuntimeConfig) -> Result<(), AirbotCliError> {
        run_rollio_runtime(config).await?;
        Ok(())
    }
}

pub async fn run_cli(cli: Cli) -> Result<(), AirbotCliError> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    execute_command_with(
        cli.command,
        &mut handle,
        &LibraryProbeProvider,
        &LibraryTransportRunner,
    )
    .await
}

pub async fn execute_command_with<W, P, R>(
    command: Command,
    writer: &mut W,
    probe_provider: &P,
    transport_runner: &R,
) -> Result<(), AirbotCliError>
where
    W: Write,
    P: ProbeProvider,
    R: TransportRunner,
{
    match command {
        Command::Probe { timeout_ms } => {
            let devices =
                probe_devices_with_provider(probe_provider, probe_timeout(timeout_ms)).await?;
            serde_json::to_writer_pretty(&mut *writer, &devices)?;
            writer.write_all(b"\n")?;
        }
        Command::Validate { id, timeout_ms } => {
            let device =
                require_probe_device_with_provider(probe_provider, &id, probe_timeout(timeout_ms))
                    .await?;
            let report = ValidateReport {
                valid: true,
                id: device.id,
            };
            serde_json::to_writer_pretty(&mut *writer, &report)?;
            writer.write_all(b"\n")?;
        }
        Command::Capabilities { id, timeout_ms } => {
            let report = capabilities_for_probe_id_with_provider(
                probe_provider,
                &id,
                probe_timeout(timeout_ms),
            )
            .await?;
            serde_json::to_writer_pretty(&mut *writer, &report)?;
            writer.write_all(b"\n")?;
        }
        Command::Run(args) => {
            let device = load_device_config(&args)?;
            run_device_with_runner(device, transport_runner).await?;
        }
    }

    Ok(())
}

pub fn load_device_config(args: &RunArgs) -> Result<DeviceConfig, AirbotCliError> {
    let device = if let Some(config_path) = &args.config {
        DeviceConfig::from_file(config_path)?
    } else if let Some(config_inline) = &args.config_inline {
        config_inline.parse::<DeviceConfig>()?
    } else {
        return Err(AirbotCliError::InvalidDevice(
            "run requires either --config or --config-inline".into(),
        ));
    };

    validate_device(device)
}

pub fn validate_device(device: DeviceConfig) -> Result<DeviceConfig, AirbotCliError> {
    if device.device_type != DeviceType::Robot {
        return Err(AirbotCliError::InvalidDevice(format!(
            "device \"{}\" is not a robot",
            device.name
        )));
    }
    if device.driver != DRIVER_NAME {
        return Err(AirbotCliError::InvalidDevice(format!(
            "device \"{}\" uses driver \"{}\", expected {DRIVER_NAME}",
            device.name, device.driver
        )));
    }

    let dof = device.dof.unwrap_or_default();
    if dof != DEFAULT_DOF {
        return Err(AirbotCliError::InvalidDevice(format!(
            "device \"{}\": AIRBOT Play requires dof = {DEFAULT_DOF}, got {dof}",
            device.name
        )));
    }

    if device.interface.as_deref().is_none() {
        return Err(AirbotCliError::InvalidDevice(format!(
            "device \"{}\": AIRBOT Play requires interface",
            device.name
        )));
    }
    if device.product_variant.as_deref().is_none() {
        return Err(AirbotCliError::InvalidDevice(format!(
            "device \"{}\": AIRBOT Play requires product_variant",
            device.name
        )));
    }

    if let Some(transport) = device.transport.as_deref() {
        if transport != "can" {
            return Err(AirbotCliError::InvalidDevice(format!(
                "device \"{}\": AIRBOT Play requires transport = \"can\", got \"{transport}\"",
                device.name
            )));
        }
    }

    Ok(device)
}

pub async fn run_device(device: DeviceConfig) -> Result<(), AirbotCliError> {
    run_device_with_runner(device, &LibraryTransportRunner).await
}

pub async fn run_device_with_runner<R>(
    device: DeviceConfig,
    transport_runner: &R,
) -> Result<(), AirbotCliError>
where
    R: TransportRunner,
{
    let device = validate_device(device)?;
    let config = RollioRuntimeConfig {
        device_name: device.name.clone(),
        interface: device
            .interface
            .clone()
            .unwrap_or_else(|| "can0".to_owned()),
        dof: device.dof.unwrap_or(DEFAULT_DOF) as usize,
        initial_mode: device.mode.expect("validated device must include mode"),
        publish_rate_hz: device.control_frequency_hz.unwrap_or(250.0),
        can_backend: CanWorkerBackend::AsyncFd,
        model_backend: ModelBackendKind::PlayAnalytical,
    };
    transport_runner.run(config).await
}

async fn probe_devices_with_provider<P>(
    probe_provider: &P,
    timeout: Duration,
) -> Result<Vec<ProbeDevice>, AirbotCliError>
where
    P: ProbeProvider,
{
    let resolved = resolve_probe_devices(probe_provider.probe(timeout).await?);
    Ok(resolved
        .into_iter()
        .map(|device| ProbeDevice {
            id: device.id,
            driver: DRIVER_NAME.to_owned(),
            interface: device.interface,
            product_variant: device.product_variant,
            end_effector: device.end_effector,
        })
        .collect())
}

async fn capabilities_for_probe_id_with_provider<P>(
    probe_provider: &P,
    device_id: &str,
    timeout: Duration,
) -> Result<CapabilitiesReport, AirbotCliError>
where
    P: ProbeProvider,
{
    let device = require_probe_device_with_provider(probe_provider, device_id, timeout).await?;
    Ok(CapabilitiesReport {
        id: device.id.clone(),
        driver: DRIVER_NAME.to_owned(),
        dof: DEFAULT_DOF,
        supported_modes: SUPPORTED_MODES.map(str::to_owned),
        default_frequency_hz: 250.0,
        transport: "can".to_owned(),
        interface: device.interface,
        product_variant: device.product_variant,
        serial_number: device.id,
        end_effector: device.end_effector,
    })
}

async fn require_probe_device_with_provider<P>(
    probe_provider: &P,
    device_id: &str,
    timeout: Duration,
) -> Result<ResolvedProbeDevice, AirbotCliError>
where
    P: ProbeProvider,
{
    let normalized = parse_probe_id(device_id)?;
    let devices = resolve_probe_devices(probe_provider.probe(timeout).await?);
    if devices.is_empty() {
        return Err(AirbotCliError::NoDetectedDevices);
    }

    if let Some(device) = devices.into_iter().find(|device| device.id == normalized) {
        return Ok(device);
    }

    Err(AirbotCliError::UnknownProbeId(device_id.to_owned()))
}

fn parse_probe_id(device_id: &str) -> Result<String, AirbotCliError> {
    let normalized = device_id.trim();
    if normalized.is_empty() || normalized.starts_with("airbot-play@") {
        return Err(AirbotCliError::InvalidProbeId(device_id.to_owned()));
    }
    Ok(normalized.to_owned())
}

fn resolve_probe_devices(instances: Vec<DiscoveredInstance>) -> Vec<ResolvedProbeDevice> {
    let mut devices = Vec::new();
    let mut seen_ids = HashSet::new();

    for instance in instances {
        let Some(id) = instance
            .product_sn
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

        if !seen_ids.insert(id.to_owned()) {
            continue;
        }

        devices.push(ResolvedProbeDevice {
            id: id.to_owned(),
            interface: instance.interface,
            // Keep the existing Rollio-facing product variant stable until the
            // wrapper grows a stronger AIRBOT model/variant contract.
            product_variant: DEFAULT_PRODUCT_VARIANT.to_owned(),
            end_effector: instance.mounted_eef.map(|value| value.to_ascii_lowercase()),
        });
    }

    devices
}

fn probe_timeout(timeout_ms: u64) -> Duration {
    Duration::from_millis(timeout_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    struct FixtureProbeProvider {
        instances: Vec<DiscoveredInstance>,
    }

    #[async_trait(?Send)]
    impl ProbeProvider for FixtureProbeProvider {
        async fn probe(
            &self,
            _timeout: Duration,
        ) -> Result<Vec<DiscoveredInstance>, AirbotCliError> {
            Ok(self.instances.clone())
        }
    }

    #[derive(Default)]
    struct RecordingTransportRunner {
        configs: Mutex<Vec<RollioRuntimeConfig>>,
    }

    #[async_trait(?Send)]
    impl TransportRunner for RecordingTransportRunner {
        async fn run(&self, config: RollioRuntimeConfig) -> Result<(), AirbotCliError> {
            self.configs
                .lock()
                .expect("configs lock poisoned")
                .push(config);
            Ok(())
        }
    }

    #[tokio::test]
    async fn probe_command_uses_serial_numbers_as_ids() {
        let provider = FixtureProbeProvider {
            instances: vec![
                fixture_instance("can0", Some("SN12345678")),
                fixture_instance("can1", None),
                fixture_instance("can2", Some("SN12345678")),
            ],
        };
        let runner = RecordingTransportRunner::default();
        let mut output = Vec::new();

        execute_command_with(
            Command::Probe { timeout_ms: 250 },
            &mut output,
            &provider,
            &runner,
        )
        .await
        .expect("probe command should succeed");

        let parsed: Vec<serde_json::Value> =
            serde_json::from_slice(&output).expect("probe output should be valid JSON");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["id"], "SN12345678");
        assert_eq!(parsed[0]["driver"], DRIVER_NAME);
        assert_eq!(parsed[0]["interface"], "can0");
        assert_eq!(parsed[0]["product_variant"], DEFAULT_PRODUCT_VARIANT);
        assert_eq!(parsed[0]["end_effector"], "g2");
    }

    #[tokio::test]
    async fn validate_and_capabilities_resolve_serial_numbers() {
        let provider = FixtureProbeProvider {
            instances: vec![fixture_instance("can0", Some("SN12345678"))],
        };
        let runner = RecordingTransportRunner::default();

        let mut validate_output = Vec::new();
        execute_command_with(
            Command::Validate {
                id: "SN12345678".to_owned(),
                timeout_ms: 250,
            },
            &mut validate_output,
            &provider,
            &runner,
        )
        .await
        .expect("validate command should succeed");
        let validate: ValidateReport =
            serde_json::from_slice(&validate_output).expect("validate output should be JSON");
        assert!(validate.valid);
        assert_eq!(validate.id, "SN12345678");

        let mut capabilities_output = Vec::new();
        execute_command_with(
            Command::Capabilities {
                id: "SN12345678".to_owned(),
                timeout_ms: 250,
            },
            &mut capabilities_output,
            &provider,
            &runner,
        )
        .await
        .expect("capabilities command should succeed");
        let capabilities: CapabilitiesReport = serde_json::from_slice(&capabilities_output)
            .expect("capabilities output should be JSON");
        assert_eq!(capabilities.id, "SN12345678");
        assert_eq!(capabilities.driver, DRIVER_NAME);
        assert_eq!(capabilities.dof, DEFAULT_DOF);
        assert_eq!(capabilities.default_frequency_hz, 250.0);
        assert_eq!(capabilities.transport, "can");
        assert_eq!(capabilities.interface, "can0");
        assert_eq!(capabilities.product_variant, DEFAULT_PRODUCT_VARIANT);
        assert_eq!(capabilities.serial_number, "SN12345678");
        assert_eq!(capabilities.end_effector.as_deref(), Some("g2"));
        assert_eq!(
            capabilities.supported_modes,
            SUPPORTED_MODES.map(str::to_owned)
        );
    }

    #[tokio::test]
    async fn run_command_uses_transport_runner_with_inline_config() {
        let provider = FixtureProbeProvider {
            instances: Vec::new(),
        };
        let runner = RecordingTransportRunner::default();
        let mut output = Vec::new();

        let config_inline = toml::to_string(&airbot_device()).expect("device should serialize");
        execute_command_with(
            Command::Run(RunArgs {
                config: None,
                config_inline: Some(config_inline),
            }),
            &mut output,
            &provider,
            &runner,
        )
        .await
        .expect("run command should succeed");

        let configs = runner.configs.lock().expect("configs lock poisoned");
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].device_name, "airbot_leader");
        assert_eq!(configs[0].interface, "can0");
        assert_eq!(configs[0].dof, DEFAULT_DOF as usize);
        assert_eq!(
            configs[0].initial_mode,
            rollio_types::config::RobotMode::FreeDrive
        );
        assert_eq!(configs[0].publish_rate_hz, 250.0);
        assert_eq!(configs[0].can_backend, CanWorkerBackend::AsyncFd);
        assert_eq!(configs[0].model_backend, ModelBackendKind::PlayAnalytical);
    }

    #[test]
    fn invalid_probe_ids_are_rejected() {
        let error = parse_probe_id("airbot-play@can0").expect_err("expected invalid probe id");
        assert_eq!(
            error.to_string(),
            "invalid AIRBOT probe id: airbot-play@can0"
        );
    }

    #[test]
    fn validate_device_rejects_wrong_dof() {
        let mut device = airbot_device();
        device.dof = Some(5);
        let error = validate_device(device).expect_err("expected dof validation error");
        assert!(
            error
                .to_string()
                .contains("AIRBOT Play requires dof = 6, got 5"),
            "unexpected error: {error}"
        );
    }

    fn airbot_device() -> DeviceConfig {
        DeviceConfig {
            name: "airbot_leader".to_owned(),
            device_type: DeviceType::Robot,
            driver: DRIVER_NAME.to_owned(),
            id: "SN12345678".to_owned(),
            width: None,
            height: None,
            fps: None,
            pixel_format: None,
            stream: None,
            channel: None,
            dof: Some(DEFAULT_DOF),
            mode: Some(rollio_types::config::RobotMode::FreeDrive),
            control_frequency_hz: Some(250.0),
            transport: Some("can".to_owned()),
            interface: Some("can0".to_owned()),
            product_variant: Some(DEFAULT_PRODUCT_VARIANT.to_owned()),
            end_effector: Some("g2".to_owned()),
            model_path: None,
            gravity_comp_torque_scales: None,
            mit_kp: None,
            mit_kd: None,
            command_latency_ms: None,
            state_noise_stddev: None,
            extra: toml::Table::new(),
        }
    }

    fn fixture_instance(interface: &str, product_sn: Option<&str>) -> DiscoveredInstance {
        DiscoveredInstance {
            interface: interface.to_owned(),
            identified_as: Some("AIRBOT Play".to_owned()),
            product_sn: product_sn.map(str::to_owned),
            pcba_sn: Some("PCBA-123".to_owned()),
            mounted_eef: Some("G2".to_owned()),
            base_board_software_version: Some("01 02 03 04".to_owned()),
            end_board_software_version: Some("05 06 07 08".to_owned()),
            motor_software_versions: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }
}
