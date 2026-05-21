//! Live-discovery helpers shared by `probe` and `query` for imu-cora.
//!
//! The driver discovers DDS topics by msg type rather than by hard-coded
//! topic names so renaming the publisher path doesn't break setup. The id
//! encodes the full topic (slashes → `__`) so `query --json <id>` can
//! recover the original wire name without a separate cache.

pub const DEVICE_TYPE: &str = "sensor";

/// Match Fast-DDS / ROS2 type-name strings for `sensor_msgs/Imu`.
/// FastDDS reports the IDL-derived name (e.g. `sensor_msgs::msg::dds_::Imu_`),
/// substring match keeps us robust to minor variations.
pub fn is_supported_type(type_name: &str) -> bool {
    type_name.contains("Imu")
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
