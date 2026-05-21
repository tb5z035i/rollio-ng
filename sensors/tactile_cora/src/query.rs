//! `query --json <id>` reports the single tactile-cora capability:
//! kind=Sensor, supported_sensor_kinds=["tactile_point_cloud2"]. The
//! `tactile_point_count` for shape_hints is config-driven, but `query` is
//! invoked with only an id, so we emit a default shape `[1024, 6]` as a
//! placeholder; the controller already merges this with the user's per-channel
//! `tactile_point_count` (via `resolved_sensor_channels`) at runtime, so the
//! number here is informational only.

use rollio_types::config::{
    ChannelCommandDefaults, DeviceQueryChannel, DeviceQueryDevice, DeviceQueryResponse, DeviceType,
    DirectJointCompatibility, SensorStateKind,
};

use crate::{descriptor, driver_name};

pub const DEFAULT_CHANNEL_TYPE: &str = "tactile";
const DEFAULT_POINT_COUNT_HINT: u32 = 1024;

pub fn run(id: &str, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut shape_hints = std::collections::BTreeMap::new();
    shape_hints.insert(
        SensorStateKind::TactilePointCloud2,
        vec![DEFAULT_POINT_COUNT_HINT, 6u32],
    );

    let default_name = descriptor::name_from_id(id);
    let cora_topic = descriptor::topic_from_id(id);

    let mut channel_extra = toml::Table::new();
    channel_extra.insert("cora_topic".to_string(), toml::Value::String(cora_topic));
    channel_extra.insert(
        "tactile_point_count".to_string(),
        toml::Value::Integer(DEFAULT_POINT_COUNT_HINT as i64),
    );
    let field_map = ["x", "y", "z", "fx", "fy", "fz"]
        .into_iter()
        .map(|s| toml::Value::String(s.to_string()))
        .collect::<Vec<_>>();
    channel_extra.insert(
        "pointcloud_field_map".to_string(),
        toml::Value::Array(field_map),
    );

    let channel = DeviceQueryChannel {
        channel_type: DEFAULT_CHANNEL_TYPE.to_string(),
        kind: DeviceType::Sensor,
        available: true,
        channel_label: Some("Cora PointCloud2 (tactile)".to_string()),
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
        supported_sensor_kinds: vec![SensorStateKind::TactilePointCloud2],
        default_sample_rate_hz: Some(100.0),
        sensor_shape_hints: shape_hints,
        optional_info: channel_extra,
    };
    let device = DeviceQueryDevice {
        id: id.to_string(),
        device_class: "sensor".to_string(),
        device_label: "Cora PointCloud2 passthrough".to_string(),
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
            "{}: device id={} name={} class=sensor channel_type={} kind=sensor sensor_kinds=[tactile_point_cloud2] shape_hint=[{},6]",
            driver_name(),
            id,
            default_name,
            DEFAULT_CHANNEL_TYPE,
            DEFAULT_POINT_COUNT_HINT,
        );
    }
    Ok(())
}
