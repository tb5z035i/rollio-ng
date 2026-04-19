//! Helpers for re-querying device executables to refresh runtime-only
//! metadata (currently `value_limits`) on every controller startup.
//!
//! The persisted `config.toml` no longer carries `value_limits` — operators
//! never edit them and stale values silently broke the visualizer. Instead,
//! both `setup` (resume path) and `collect` (run path) call
//! [`refresh_value_limits_from_devices`] to re-run each device's `query
//! --json` invocation and copy the freshly reported limits into the
//! in-memory channel config before downstream consumers (visualizer
//! runtime config) see it.

use crate::discovery::run_driver_json;
use crate::runtime_paths::{default_device_executable_name, resolve_registered_program};
use rollio_types::config::{
    BinaryDeviceConfig, DeviceType, DirectJointCompatibility, DirectJointCompatibilityPeer,
    ProjectConfig, RobotCommandKind, RobotStateKind, StateValueLimitsEntry,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

const DEVICE_QUERY_TIMEOUT: Duration = Duration::from_millis(2_000);

/// Per-channel runtime metadata pulled from a fresh `query --json`
/// invocation. Indexed by `(device_name, channel_type)` so callers can
/// match it back to entries in the project config.
#[derive(Debug, Clone, Default)]
pub(crate) struct ChannelRuntimeMeta {
    pub(crate) value_limits: Vec<StateValueLimitsEntry>,
    pub(crate) supported_states: Vec<RobotStateKind>,
    pub(crate) supported_commands: Vec<RobotCommandKind>,
    pub(crate) direct_joint_compatibility: DirectJointCompatibility,
}

pub(crate) type DeviceRuntimeMetaMap = BTreeMap<(String, String), ChannelRuntimeMeta>;

/// Re-runs `<executable> query --json <id>` for every device in `config`
/// that exposes a robot channel and overwrites each robot channel's
/// `value_limits` from the response. Camera-only devices are skipped
/// because cameras do not advertise value limits.
///
/// Returns a map of fresh per-channel runtime metadata so callers can
/// also surface supported-state lists to the wizard UI.
///
/// Returns an error the first time a device query fails. The visualizer
/// treats absent limits as a hard error (renders `???` placeholder bars),
/// so we surface the underlying driver/path error early instead of
/// limping forward with empty envelopes.
pub(crate) fn refresh_value_limits_from_devices(
    config: &mut ProjectConfig,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Result<DeviceRuntimeMetaMap, Box<dyn Error>> {
    let mut runtime_meta = DeviceRuntimeMetaMap::new();
    for device in config.devices.iter_mut() {
        if !device
            .channels
            .iter()
            .any(|channel| channel.kind == DeviceType::Robot && channel.enabled)
        {
            continue;
        }
        let meta_by_channel = refresh_device_value_limits(device, workspace_root, current_exe_dir)?;
        for (channel_type, meta) in meta_by_channel {
            runtime_meta.insert((device.name.clone(), channel_type), meta);
        }
    }
    Ok(runtime_meta)
}

fn refresh_device_value_limits(
    device: &mut BinaryDeviceConfig,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> Result<BTreeMap<String, ChannelRuntimeMeta>, Box<dyn Error>> {
    let executable_name = device
        .executable
        .clone()
        .unwrap_or_else(|| default_device_executable_name(&device.driver));
    let program = resolve_registered_program(&executable_name, workspace_root, current_exe_dir);
    let response = run_driver_json(
        &program,
        &[
            OsString::from("query"),
            OsString::from("--json"),
            OsString::from(&device.id),
        ],
        workspace_root,
        DEVICE_QUERY_TIMEOUT,
    )
    .map_err(|error| -> Box<dyn Error> {
        format!(
            "rollio: failed to refresh value_limits for device \"{}\" (driver \"{}\", id \"{}\"): {}",
            device.name, device.driver, device.id, error
        )
        .into()
    })?;

    let query_device = response
        .get("devices")
        .and_then(Value::as_array)
        .and_then(|devices| {
            devices
                .iter()
                .find(|d| value_as_string(d.get("id")).as_deref() == Some(device.id.as_str()))
                .or_else(|| devices.first())
        })
        .ok_or_else(|| -> Box<dyn Error> {
            format!(
                "rollio: device \"{}\" query returned no devices for id \"{}\"",
                device.name, device.id
            )
            .into()
        })?;

    let meta_by_channel = parse_channel_runtime_meta(query_device);

    for channel in device.channels.iter_mut() {
        if channel.kind != DeviceType::Robot {
            continue;
        }
        if let Some(meta) = meta_by_channel.get(&channel.channel_type) {
            channel.value_limits = meta.value_limits.clone();
            channel.supported_commands = meta.supported_commands.clone();
            channel.direct_joint_compatibility = meta.direct_joint_compatibility.clone();
        }
    }

    Ok(meta_by_channel)
}

fn parse_channel_runtime_meta(device: &Value) -> BTreeMap<String, ChannelRuntimeMeta> {
    device
        .get("channels")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|channel| {
            let channel_type = value_as_string(channel.get("channel_type"))?;
            let value_limits = parse_query_value_limits(channel.get("value_limits"));
            let mut supported_states =
                parse_query_supported_states(channel.get("supported_states"));
            if supported_states.is_empty() {
                supported_states = value_limits.iter().map(|entry| entry.state_kind).collect();
            }
            let supported_commands =
                parse_query_supported_commands(channel.get("supported_commands"));
            let direct_joint_compatibility =
                parse_query_direct_joint_compatibility(channel.get("direct_joint_compatibility"));
            Some((
                channel_type,
                ChannelRuntimeMeta {
                    value_limits,
                    supported_states,
                    supported_commands,
                    direct_joint_compatibility,
                },
            ))
        })
        .collect()
}

/// Public so the setup discovery path can reuse the same parser when
/// building `DiscoveredChannelMeta` from the initial device query.
pub(crate) fn parse_query_value_limits(value: Option<&Value>) -> Vec<StateValueLimitsEntry> {
    let Some(entries) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let state_kind_str = value_as_string(entry.get("state_kind"))?;
            let state_kind: RobotStateKind =
                serde_json::from_value(serde_json::Value::String(state_kind_str)).ok()?;
            let min = value_as_f64_array(entry.get("min"));
            let max = value_as_f64_array(entry.get("max"));
            Some(StateValueLimitsEntry {
                state_kind,
                min,
                max,
            })
        })
        .collect()
}

/// Read the `supported_states` array from a per-channel query entry.
/// New drivers populate this explicitly; older ones implicitly enumerate
/// supported kinds via `value_limits` (handled by callers as a fallback).
pub(crate) fn parse_query_supported_states(value: Option<&Value>) -> Vec<RobotStateKind> {
    let Some(entries) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let kind_str = entry.as_str()?;
            serde_json::from_value(serde_json::Value::String(kind_str.to_owned())).ok()
        })
        .collect()
}

/// Read the `supported_commands` array from a per-channel query entry. The
/// controller persists this on `DeviceChannelConfigV2.supported_commands`
/// so downstream teleop/pairing logic stays driver-agnostic.
pub(crate) fn parse_query_supported_commands(value: Option<&Value>) -> Vec<RobotCommandKind> {
    let Some(entries) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let kind_str = entry.as_str()?;
            serde_json::from_value(serde_json::Value::String(kind_str.to_owned())).ok()
        })
        .collect()
}

/// Read the `direct_joint_compatibility` object from a per-channel query
/// entry. Each peer is `{ "driver": "...", "channel_type": "..." }`.
pub(crate) fn parse_query_direct_joint_compatibility(
    value: Option<&Value>,
) -> DirectJointCompatibility {
    let Some(map) = value.and_then(Value::as_object) else {
        return DirectJointCompatibility::default();
    };
    let parse_peers = |key: &str| -> Vec<DirectJointCompatibilityPeer> {
        let Some(entries) = map.get(key).and_then(Value::as_array) else {
            return Vec::new();
        };
        entries
            .iter()
            .filter_map(|entry| {
                let driver = value_as_string(entry.get("driver"))?;
                let channel_type = value_as_string(entry.get("channel_type"))?;
                Some(DirectJointCompatibilityPeer {
                    driver,
                    channel_type,
                })
            })
            .collect()
    };
    DirectJointCompatibility {
        can_lead: parse_peers("can_lead"),
        can_follow: parse_peers("can_follow"),
    }
}

fn value_as_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(ToOwned::to_owned)
}

fn value_as_f64_array(value: Option<&Value>) -> Vec<f64> {
    value
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default()
}
