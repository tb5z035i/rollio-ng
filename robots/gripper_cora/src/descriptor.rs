//! Live-discovery helpers shared by `probe` and `query` for gripper-cora.

pub const DEVICE_TYPE: &str = "robot";

/// Match Fast-DDS / ROS2 type-name strings for `sensor_msgs/JointState`.
pub fn is_supported_type(type_name: &str) -> bool {
    type_name.contains("JointState")
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
    let s = s
        .strip_suffix("/joint_state")
        .or_else(|| s.strip_suffix("/state"))
        .unwrap_or(s);
    s.replace('/', "_")
}

pub fn name_from_id(id: &str) -> String {
    name_from_topic(&topic_from_id(id))
}
