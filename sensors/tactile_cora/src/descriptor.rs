//! Live-discovery helpers shared by `probe` and `query` for tactile-cora.

use rollio_bus::cora_discovery::dds_leaf_type;

pub const DEVICE_TYPE: &str = "sensor";

/// Exact-match against the Fast-DDS leaf type. `sensor_msgs::msg::dds_::PointCloud2_`
/// reduces to `PointCloud2`.
pub fn is_supported_type(type_name: &str) -> bool {
    dds_leaf_type(type_name) == "PointCloud2"
}

pub fn id_from_topic(topic: &str) -> String {
    topic.trim_start_matches('/').replace('/', "__")
}

pub fn topic_from_id(id: &str) -> String {
    let mut s = id.replace("__", "/");
    if !s.starts_with('/') {
        s.insert(0, '/');
    }
    s
}

pub fn name_from_topic(topic: &str) -> String {
    let s = topic.trim_start_matches('/');
    let s = s.strip_prefix("robot/").unwrap_or(s);
    let s = s.strip_suffix("/points").unwrap_or(s);
    s.replace('/', "_")
}

pub fn name_from_id(id: &str) -> String {
    name_from_topic(&topic_from_id(id))
}
