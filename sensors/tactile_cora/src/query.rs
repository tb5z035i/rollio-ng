//! `query --json <id>` reports the single tactile-cora capability:
//! kind=Sensor, supported_sensor_kinds=["tactile_point_cloud2"]. The
//! `tactile_point_count` for shape_hints is config-driven, but `query` is
//! invoked with only an id, so we emit a default shape `[1024, 6]` as a
//! placeholder; the controller already merges this with the user's per-channel
//! `tactile_point_count` (via `resolved_sensor_channels`) at runtime, so the
//! number here is informational only.

use rollio_types::config::{
    ChannelKindInfo, DeviceQueryChannel, DeviceQueryDevice, DeviceQueryResponse, SensorChannelInfo,
    SensorStateKind,
};

use crate::driver_name;

pub const DEFAULT_CHANNEL_TYPE: &str = "tactile";
const DEFAULT_POINT_COUNT_HINT: u32 = 1024;

pub fn run(id: &str, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut shape_hints = std::collections::BTreeMap::new();
    shape_hints.insert(
        SensorStateKind::TactilePointCloud2,
        vec![DEFAULT_POINT_COUNT_HINT, 6u32],
    );

    let channel = DeviceQueryChannel {
        channel_type: DEFAULT_CHANNEL_TYPE.to_string(),
        available: true,
        channel_label: Some("Cora PointCloud2 (tactile)".to_string()),
        default_name: Some("tactile".to_string()),
        info: ChannelKindInfo::Sensor(SensorChannelInfo {
            modes: vec!["enabled".to_string(), "disabled".to_string()],
            supported_sensor_kinds: vec![SensorStateKind::TactilePointCloud2],
            sensor_shape_hints: shape_hints,
            default_sample_rate_hz: None,
        }),
        optional_info: toml::Table::new(),
    };
    let device = DeviceQueryDevice {
        id: id.to_string(),
        device_class: "sensor".to_string(),
        device_label: "Cora PointCloud2 passthrough".to_string(),
        default_device_name: Some("tactile_cora".to_string()),
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
            "{}: device id={} class=sensor channel_type={} kind=sensor sensor_kinds=[tactile_point_cloud2] shape_hint=[{},6]",
            driver_name(),
            id,
            DEFAULT_CHANNEL_TYPE,
            DEFAULT_POINT_COUNT_HINT,
        );
    }
    Ok(())
}
