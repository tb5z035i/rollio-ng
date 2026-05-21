//! Shared discovery helpers for Cora SDK drivers (imu/tactile/gripper).
//!
//! Centralises the bits every cora driver got wrong before:
//!
//! - DDS domain id resolution: operator env → Cora robot's
//!   `framework_config.json` → ROS2 convention env → 0 fallback. Reading
//!   the robot's canonical config means a fresh deployment auto-aligns
//!   with whatever domain the rest of the stack is using; no operator
//!   plumbing required.
//! - `dds_leaf_type()`: strip the IDL namespace + trailing `_` so we
//!   compare semantic identity (`Imu` == `Imu`) instead of substring
//!   accidents (`Imu` matches `ImuStamped`, `ImuCalibration`, ...).
//! - PDP cycle timeout: 3 s default. The 800 ms used previously fell
//!   inside the Fast-DDS PDP burst window on busy robots, hiding live
//!   publishers from probe.
//! - Topic `/`-prefix filter: Fast-DDS occasionally surfaces internal
//!   builtin topics without the ROS2 leading slash; skip them.

use std::path::PathBuf;
use std::time::Duration;

/// Default `probe --json` wait window. Covers the Fast-DDS PDP cycle on
/// production cora robots; under-sampling here hides publishers entirely.
pub const DEFAULT_PROBE_DURATION: Duration = Duration::from_millis(3_000);

/// Canonical Cora robot framework config path. Override via
/// `ROLLIO_CORA_FRAMEWORK_CONFIG` for tests / non-standard installs.
pub const FRAMEWORK_CONFIG_PATH: &str = "/opt/robot_app/configs/framework_config.json";

/// Strip the Fast-DDS / ROS2 IDL namespace and the trailing `_` so that
/// `sensor_msgs::msg::dds_::Imu_` reduces to `Imu`. Returns the input
/// unchanged when no namespace separator is present.
pub fn dds_leaf_type(type_name: &str) -> &str {
    type_name
        .rsplit("::")
        .next()
        .unwrap_or(type_name)
        .trim_end_matches('_')
}

/// True when the topic is a regular ROS2 / Fast-DDS topic (leading `/`)
/// rather than an internal builtin that occasionally leaks into the
/// discovery callback.
pub fn is_external_topic(topic: &str) -> bool {
    topic.starts_with('/')
}

/// Resolve the DDS domain id every cora driver should sit on. Order:
///
/// 1. `ROLLIO_DDS_DOMAIN_ID` — explicit operator / controller override.
/// 2. `/opt/robot_app/configs/framework_config.json::domain_id` — the
///    Cora robot's own deployment config; reading it means a fresh
///    `rollio setup` joins whatever domain the robot stack already uses.
/// 3. `ROS_DOMAIN_ID` — ROS2 ecosystem convention.
/// 4. `0` — Fast-DDS / cora SDK default.
pub fn resolve_dds_domain_id() -> i32 {
    if let Some(domain) = parse_env_i32("ROLLIO_DDS_DOMAIN_ID") {
        return domain;
    }
    if let Some(domain) = read_framework_config_domain_id() {
        return domain;
    }
    if let Some(domain) = parse_env_i32("ROS_DOMAIN_ID") {
        return domain;
    }
    0
}

fn parse_env_i32(name: &str) -> Option<i32> {
    std::env::var(name).ok()?.trim().parse::<i32>().ok()
}

fn read_framework_config_domain_id() -> Option<i32> {
    let path = std::env::var("ROLLIO_CORA_FRAMEWORK_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(FRAMEWORK_CONFIG_PATH));
    let raw = std::fs::read_to_string(&path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value.get("domain_id")?.as_i64()?.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dds_leaf_type_strips_namespace_and_trailing_underscore() {
        assert_eq!(dds_leaf_type("sensor_msgs::msg::dds_::Imu_"), "Imu");
        assert_eq!(dds_leaf_type("sensor_msgs::msg::Imu"), "Imu");
        assert_eq!(dds_leaf_type("Imu"), "Imu");
        // No false positives from substring overlap.
        assert_ne!(dds_leaf_type("sensor_msgs::msg::dds_::ImuStamped_"), "Imu");
    }

    #[test]
    fn is_external_topic_filters_builtin_leakage() {
        assert!(is_external_topic("/imu"));
        assert!(is_external_topic("/cora/left_arm/imu"));
        assert!(!is_external_topic("rt/Heartbeat"));
        assert!(!is_external_topic(""));
    }

    #[test]
    fn resolve_dds_domain_id_prefers_env() {
        // SAFETY: tests in this module are single-threaded; rust runs
        // them serially when they share env mutation.
        // Mock framework_config away so the env path wins regardless of
        // the host's /opt state.
        unsafe {
            std::env::set_var("ROLLIO_CORA_FRAMEWORK_CONFIG", "/nonexistent");
            std::env::set_var("ROLLIO_DDS_DOMAIN_ID", "7");
        }
        assert_eq!(resolve_dds_domain_id(), 7);
        unsafe {
            std::env::remove_var("ROLLIO_DDS_DOMAIN_ID");
            std::env::remove_var("ROLLIO_CORA_FRAMEWORK_CONFIG");
        }
    }
}
