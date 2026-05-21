//! `query --json <id>` emits a `DeviceQueryResponse` describing this driver's
//! IMU sensor channel capability. cora-* drivers are static-config; the device
//! has no live config at query time, only the device id passed on the command
//! line. We respond with a synthetic single-channel descriptor:
//! `kind=Sensor`, supported_sensor_kinds=["imu_accel_gyro"], shape=[6].
//!
//! The controller's `device_query.rs` parses the returned `channels[*]` and
//! matches them to the user-declared `[devices.channels]` rows by
//! `channel_type`. To keep that match working without per-installation
//! coordination, this driver always reports `channel_type = "imu"` — users
//! must set the same string in their `config.toml`.

use rollio_types::config::{
    ChannelCommandDefaults, DeviceQueryChannel, DeviceQueryDevice, DeviceQueryResponse, DeviceType,
    DirectJointCompatibility, SensorStateKind,
};

use crate::{descriptor, driver_name};

pub const DEFAULT_CHANNEL_TYPE: &str = "imu";

pub fn run(id: &str, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut shape_hints = std::collections::BTreeMap::new();
    shape_hints.insert(SensorStateKind::ImuAccelGyro, vec![6u32]);

    let default_name = descriptor::name_from_id(id);

    let channel = DeviceQueryChannel {
        channel_type: DEFAULT_CHANNEL_TYPE.to_string(),
        kind: DeviceType::Sensor,
        available: true,
        channel_label: Some("Cora IMU (accel + gyro)".to_string()),
        default_name: Some(DEFAULT_CHANNEL_TYPE.to_string()),
        modes: vec!["enabled".to_string(), "disabled".to_string()],
        profiles: Vec::new(),
        supported_states: Vec::new(),
        supported_commands: Vec::new(),
        supports_fk: false,
        supports_ik: false,
        dof: None,
        default_control_frequency_hz: None,
        direct_joint_compatibility: DirectJointCompatibility::default(),
        defaults: ChannelCommandDefaults::default(),
        value_limits: Vec::new(),
        supported_sensor_kinds: vec![SensorStateKind::ImuAccelGyro],
        default_sample_rate_hz: Some(200.0),
        sensor_shape_hints: shape_hints,
        optional_info: toml::Table::new(),
    };
    let device = DeviceQueryDevice {
        id: id.to_string(),
        device_class: "sensor".to_string(),
        device_label: "Cora IMU passthrough".to_string(),
        default_device_name: Some(default_name.clone()),
        optional_info: toml::Table::new(),
        channels: vec![channel],
    };
    let response = DeviceQueryResponse {
        driver: driver_name().to_string(),
        devices: vec![device],
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        println!(
            "{}: device id={} name={} class=sensor channel_type={} kind=sensor sensor_kinds=[imu_accel_gyro] shape=[6]",
            driver_name(),
            id,
            default_name,
            DEFAULT_CHANNEL_TYPE
        );
    }
    Ok(())
}
