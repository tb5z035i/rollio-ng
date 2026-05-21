//! Live-discovery helpers shared by `probe` and `query` for imu-cora.
//!
//! The driver discovers DDS topics by msg type rather than by hard-coded
//! topic names so renaming the publisher path doesn't break setup. The id
//! encodes the full topic (slashes → `__`) so `query --json <id>` can
//! recover the original wire name without a separate cache.

use rollio_bus::cora_discovery::dds_leaf_type;

pub const DEVICE_TYPE: &str = "sensor";

/// Exact-match against the Fast-DDS leaf type. `sensor_msgs::msg::dds_::Imu_`
/// reduces to `Imu`; subtypes like `ImuStamped` / `ImuCalibration` are
/// rejected on purpose.
pub fn is_supported_type(type_name: &str) -> bool {
    dds_leaf_type(type_name) == "Imu"
}

/// id = topic stripped of leading `/`, with `/` replaced by `__`.
pub fn id_from_topic(topic: &str) -> String {
    topic.trim_start_matches('/').replace('/', "__")
}

/// Inverse of `id_from_topic`: `__` → `/`, prepend leading `/`.
pub fn topic_from_id(id: &str) -> String {
    let mut s = id.replace("__", "/");
    if !s.starts_with('/') {
        s.insert(0, '/');
    }
    s
}

/// Human-friendly default device name. Strips `/`, leading `robot/`, and
/// trailing `/data`, then replaces `/` with `_`.
pub fn name_from_topic(topic: &str) -> String {
    let s = topic.trim_start_matches('/');
    let s = s.strip_prefix("robot/").unwrap_or(s);
    let s = s.strip_suffix("/data").unwrap_or(s);
    s.replace('/', "_")
}

pub fn name_from_id(id: &str) -> String {
    name_from_topic(&topic_from_id(id))
}
