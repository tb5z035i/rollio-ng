use super::devices::select_supported_mode;
use super::overview::camera_channel_type_for_profile;
use super::pairings::{build_default_channel_pairings, default_publish_states_for_meta};
use super::state::{AvailableDevice, CameraProfile, DiscoveredChannelMeta, DiscoveredDevice};
use crate::discovery::{discover_probe_entries, run_driver_json, DiscoveryOptions};
use crate::runtime_paths::{default_device_executable_name, resolve_registered_program};
use rollio_types::config::{
    BinaryDeviceConfig, CameraChannelProfile, ChannelCommandDefaults, CollectionMode,
    DeviceChannelConfigV2, DeviceType, ProjectConfig, RobotMode, RobotStateKind,
};
use rollio_types::messages::PixelFormat;
use serde_json::Value;
use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

pub(super) const DISCOVERY_TIMEOUT: Duration = Duration::from_millis(2_000);
pub(super) const VALIDATION_TIMEOUT: Duration = Duration::from_millis(1_000);

/// One channel + chosen profile + final user-visible name for a camera
/// discovery row. Built by `build_discovery_config` so multi-stream cameras
/// (e.g. RealSense color + depth + infrared) collapse into a single
/// Build a `BinaryDeviceConfig` from one discovered device by iterating
/// every channel returned in the driver's `query --json`. There is no
/// per-driver branching: cameras, robots, and mixed devices all flow
/// through the same loop. Each channel's `kind`, `dof`, `modes`,
/// `profiles`, `defaults`, `value_limits`, `direct_joint_compatibility`,
/// and `supported_commands` are taken straight from the channel meta.
pub(super) fn binary_device_from_discovery(
    discovery: &DiscoveredDevice,
    name: String,
    preferred_mode: RobotMode,
    name_counts: &mut BTreeMap<String, usize>,
) -> BinaryDeviceConfig {
    let mut extra = toml::Table::new();
    if let Some(transport) = &discovery.transport {
        extra.insert("transport".into(), toml::Value::String(transport.clone()));
    }
    if let Some(interface) = &discovery.interface {
        extra.insert("interface".into(), toml::Value::String(interface.clone()));
    }
    if let Some(product_variant) = &discovery.product_variant {
        extra.insert(
            "product_variant".into(),
            toml::Value::String(product_variant.clone()),
        );
    }
    if let Some(end_effector) = &discovery.end_effector {
        extra.insert(
            "end_effector".into(),
            toml::Value::String(end_effector.clone()),
        );
    }
    let single_channel = discovery.channel_meta_by_channel.len() == 1;
    let channels = discovery
        .channel_meta_by_channel
        .iter()
        .map(|(channel_type, meta)| {
            let channel_name = if single_channel {
                // Avoid double-deduping: a single-channel device uses the
                // device-level name (already deduped in `build_discovery_config`)
                // as the channel name. Multi-channel devices need per-channel
                // names so the wizard can tell rows apart.
                Some(name.clone())
            } else {
                dedup_channel_default_name(meta.default_name.as_deref(), name_counts)
            };
            build_channel_config_from_meta(channel_type, meta, preferred_mode, channel_name)
        })
        .collect();
    BinaryDeviceConfig {
        name: name.clone(),
        executable: Some(default_device_executable_name(&discovery.driver)),
        driver: discovery.driver.clone(),
        id: discovery.id.clone(),
        bus_root: name,
        channels,
        extra,
    }
}

pub(super) fn build_channel_config_from_meta(
    channel_type: &str,
    meta: &DiscoveredChannelMeta,
    preferred_mode: RobotMode,
    channel_name: Option<String>,
) -> DeviceChannelConfigV2 {
    match meta.kind {
        DeviceType::Camera => {
            let profile = pick_default_camera_profile(&meta.profiles);
            DeviceChannelConfigV2 {
                channel_type: channel_type.to_owned(),
                kind: DeviceType::Camera,
                enabled: true,
                name: channel_name,
                channel_label: meta.channel_label.clone(),
                mode: None,
                dof: None,
                publish_states: Vec::new(),
                recorded_states: Vec::new(),
                control_frequency_hz: None,
                profile,
                preview_enabled: true,
                record_enabled: true,
                record: None,
                preview_settings: None,
                command_defaults: meta.defaults.clone(),
                value_limits: meta.value_limits.clone(),
                direct_joint_compatibility: meta.direct_joint_compatibility.clone(),
                supported_commands: meta.supported_commands.clone(),
                publish_sensors: Vec::new(),
                sample_rate_hz: None,
                sensor_shape_hints: Default::default(),
                extra: toml::Table::new(),
            }
        }
        DeviceType::Robot => {
            let mode = Some(select_supported_mode(&meta.modes, preferred_mode));
            let publish_states =
                default_publish_states_for_meta(meta, &robot_publish_states_fallback(channel_type));
            let recorded_states = publish_states.clone();
            DeviceChannelConfigV2 {
                channel_type: channel_type.to_owned(),
                kind: DeviceType::Robot,
                enabled: true,
                name: channel_name,
                channel_label: meta.channel_label.clone(),
                mode,
                dof: meta.dof,
                publish_states,
                recorded_states,
                control_frequency_hz: meta.default_control_frequency_hz,
                profile: None,
                preview_enabled: true,
                record_enabled: true,
                record: None,
                preview_settings: None,
                command_defaults: meta.defaults.clone(),
                value_limits: meta.value_limits.clone(),
                direct_joint_compatibility: meta.direct_joint_compatibility.clone(),
                supported_commands: meta.supported_commands.clone(),
                publish_sensors: Vec::new(),
                sample_rate_hz: None,
                sensor_shape_hints: Default::default(),
                extra: toml::Table::new(),
            }
        }
        DeviceType::Sensor => DeviceChannelConfigV2 {
            channel_type: channel_type.to_owned(),
            kind: DeviceType::Sensor,
            enabled: true,
            name: channel_name,
            channel_label: meta.channel_label.clone(),
            mode: None,
            dof: None,
            publish_states: Vec::new(),
            recorded_states: Vec::new(),
            control_frequency_hz: None,
            profile: None,
            preview_enabled: false,
            record_enabled: true,
            record: None,
            preview_settings: None,
            command_defaults: ChannelCommandDefaults::default(),
            value_limits: Vec::new(),
            direct_joint_compatibility: Default::default(),
            supported_commands: Vec::new(),
            publish_sensors: meta.supported_sensor_kinds.clone(),
            sample_rate_hz: meta.default_sample_rate_hz,
            sensor_shape_hints: meta.sensor_shape_hints.clone(),
            extra: toml::Table::new(),
        },
    }
}

/// Channel-shape-generic fallback for `publish_states` when the driver
/// neither populates `supported_states` nor `value_limits`. Picks
/// joint-shaped defaults for typical arm channel names; parallel-shaped
/// defaults for grippers / end-effectors. Driver-specific tables are NOT
/// consulted — the driver should populate `supported_states` properly.
pub(super) fn robot_publish_states_fallback(channel_type: &str) -> Vec<RobotStateKind> {
    let lower = channel_type.to_ascii_lowercase();
    if lower.contains("gripper")
        || lower.contains("eef")
        || lower == "e2"
        || lower == "g2"
        || lower == "e2b"
    {
        vec![
            RobotStateKind::ParallelPosition,
            RobotStateKind::ParallelVelocity,
            RobotStateKind::ParallelEffort,
        ]
    } else {
        vec![
            RobotStateKind::JointPosition,
            RobotStateKind::JointVelocity,
            RobotStateKind::JointEffort,
        ]
    }
}

pub(super) fn pick_default_camera_profile(
    profiles: &[CameraProfile],
) -> Option<CameraChannelProfile> {
    profiles
        .iter()
        .max_by_key(|profile| camera_profile_quality_key(profile))
        .map(|profile| CameraChannelProfile {
            width: profile.width,
            height: profile.height,
            fps: profile.fps,
            pixel_format: profile.pixel_format,
            native_pixel_format: profile.native_pixel_format.clone(),
            mjpeg_quality: None,
            h264_bitrate_bps: None,
            h264_gop: None,
            h264_preset: None,
            h264_tune: None,
            h264_profile: None,
        })
}

/// Quality ordering key for `CameraProfile` defaults: higher pixel count
/// first, ties broken by higher fps. Returned as a tuple so the caller
/// can compare with `>` directly.
pub(super) fn camera_profile_quality_key(profile: &CameraProfile) -> (u64, u32) {
    let pixels = (profile.width as u64) * (profile.height as u64);
    (pixels, profile.fps)
}

pub(super) fn available_devices_from_project(
    project: &ProjectConfig,
    runtime_meta: &crate::device_query::DeviceRuntimeMetaMap,
) -> Vec<AvailableDevice> {
    project
        .devices
        .iter()
        .flat_map(|device| {
            device.channels.iter().filter_map(|channel| {
                let current = row_current_from_binary_channel(device, channel)?;
                let device_type = channel.kind;
                let camera_profiles = if device_type == DeviceType::Camera {
                    channel
                        .profile
                        .as_ref()
                        .map(|profile| CameraProfile {
                            width: profile.width,
                            height: profile.height,
                            fps: profile.fps,
                            pixel_format: profile.pixel_format,
                            native_pixel_format: profile.native_pixel_format.clone(),
                            stream: split_camera_channel_type(&channel.channel_type)
                                .0
                                .map(ToOwned::to_owned),
                            channel: split_camera_channel_type(&channel.channel_type).1,
                        })
                        .into_iter()
                        .collect()
                } else {
                    Vec::new()
                };
                let supported_modes = supported_modes_from_project_channel(device, channel);
                let supported_states = if device_type == DeviceType::Robot {
                    runtime_meta
                        .get(&(device.name.clone(), channel.channel_type.clone()))
                        .map(|meta| meta.supported_states.clone())
                        .unwrap_or_else(|| {
                            // Older drivers may not advertise supported_states;
                            // fall back to whatever value_limits the latest
                            // refresh populated.
                            channel
                                .value_limits
                                .iter()
                                .map(|entry| entry.state_kind)
                                .collect()
                        })
                } else {
                    Vec::new()
                };
                let runtime_meta_for_channel =
                    runtime_meta.get(&(device.name.clone(), channel.channel_type.clone()));
                let supported_commands = runtime_meta_for_channel
                    .map(|meta| meta.supported_commands.clone())
                    .unwrap_or_default();
                let direct_joint_compatibility = runtime_meta_for_channel
                    .map(|meta| meta.direct_joint_compatibility.clone())
                    .unwrap_or_default();
                Some(AvailableDevice {
                    name: available_device_key_from_binary(&current),
                    display_name: display_name_for_binary_channel(device, channel),
                    device_type,
                    driver: device.driver.clone(),
                    id: device.id.clone(),
                    camera_profiles,
                    supported_modes,
                    supported_states,
                    supported_commands,
                    direct_joint_compatibility,
                    current,
                })
            })
        })
        .collect()
}

pub(super) fn available_devices_from_discoveries(
    discoveries: &[DiscoveredDevice],
    project: &ProjectConfig,
) -> Result<Vec<AvailableDevice>, Box<dyn Error>> {
    let mut available = Vec::new();

    for discovery in discoveries {
        let mut current = project
            .devices
            .iter()
            .find(|device| device_matches_discovery_binary(device, discovery, None, None))
            .cloned()
            .ok_or_else(|| {
                format!(
                    "missing setup device for discovered device {} ({})",
                    discovery.display_name, discovery.id
                )
            })?;
        enrich_current_device_from_discovery(&mut current, discovery);
        for row in available_rows_from_discovery(&current, discovery) {
            available.push(row);
        }
    }

    Ok(available)
}

pub(super) fn available_rows_from_discovery(
    current: &BinaryDeviceConfig,
    discovery: &DiscoveredDevice,
) -> Vec<AvailableDevice> {
    current
        .channels
        .iter()
        .filter_map(|channel| {
            let row_current = row_current_from_binary_channel(current, channel)?;
            let device_type = channel.kind;
            let camera_profiles = if device_type == DeviceType::Camera {
                discovery
                    .channel_meta_by_channel
                    .get(&channel.channel_type)
                    .map(|meta| meta.profiles.clone())
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let meta = discovery.channel_meta_by_channel.get(&channel.channel_type);
            let supported_states = if device_type == DeviceType::Robot {
                meta.map(|m| m.supported_states.clone()).unwrap_or_default()
            } else {
                Vec::new()
            };
            let supported_commands = if device_type == DeviceType::Robot {
                meta.map(|m| m.supported_commands.clone())
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let direct_joint_compatibility = if device_type == DeviceType::Robot {
                meta.map(|m| m.direct_joint_compatibility.clone())
                    .unwrap_or_default()
            } else {
                rollio_types::config::DirectJointCompatibility::default()
            };
            Some(AvailableDevice {
                name: available_device_key_from_binary(&row_current),
                display_name: display_name_for_binary_channel(current, channel),
                device_type,
                driver: current.driver.clone(),
                id: current.id.clone(),
                camera_profiles,
                supported_modes: supported_modes_from_discovery(discovery, channel),
                supported_states,
                supported_commands,
                direct_joint_compatibility,
                current: row_current,
            })
        })
        .collect()
}

pub(super) fn row_current_from_binary_channel(
    device: &BinaryDeviceConfig,
    channel: &DeviceChannelConfigV2,
) -> Option<BinaryDeviceConfig> {
    let mut current = device.clone();
    current.channels = vec![channel.clone()];
    Some(current)
}

pub(super) fn supported_modes_from_project_channel(
    _device: &BinaryDeviceConfig,
    channel: &DeviceChannelConfigV2,
) -> Vec<RobotMode> {
    if channel.kind != DeviceType::Robot {
        return Vec::new();
    }
    // Without a live driver session we can only echo the persisted mode.
    // The discovery path uses `supported_modes_from_discovery` which does
    // consult the driver's `query --json` `modes` array directly.
    channel.mode.into_iter().collect()
}

pub(super) fn supported_modes_from_discovery(
    discovery: &DiscoveredDevice,
    channel: &DeviceChannelConfigV2,
) -> Vec<RobotMode> {
    if channel.kind != DeviceType::Robot {
        return Vec::new();
    }
    discovery
        .channel_meta_by_channel
        .get(&channel.channel_type)
        .map(|meta| meta.modes.clone())
        .unwrap_or_else(|| channel.mode.into_iter().collect())
}

pub(super) fn split_camera_channel_type(channel_type: &str) -> (Option<&str>, Option<u32>) {
    channel_type
        .rsplit_once('_')
        .and_then(|(stream, suffix)| suffix.parse::<u32>().ok().map(|channel| (stream, channel)))
        .map(|(stream, channel)| (Some(stream), Some(channel)))
        .unwrap_or((Some(channel_type), None))
}

pub(super) fn display_name_for_binary_channel(
    device: &BinaryDeviceConfig,
    channel: &DeviceChannelConfigV2,
) -> String {
    // Prefer the device-provided per-channel label so the controller stays
    // driver-agnostic. Falls back to a generic `{driver_label} ({channel_type})`
    // format only when no driver-supplied label exists; new device
    // executables MUST set `channel_label` (or `device_label`) in their
    // `query --json` to control display strings.
    if let Some(label) = channel.channel_label.as_deref() {
        if !label.trim().is_empty() {
            return label.to_owned();
        }
    }
    let driver_label = driver_to_label_fallback(&device.driver);
    if channel.channel_type.is_empty() {
        driver_label
    } else {
        format!("{driver_label} ({})", channel.channel_type)
    }
}

/// Build a list of human-readable warnings for every robot channel whose
/// `publish_states` includes a kind that the driver did not report
/// `value_limits` for. The renderer paints these cells with `?` placeholder
/// bars (no fallback envelope), so flagging the misconfiguration during
/// setup tells the operator they need to update their device executable
/// rather than diagnose missing bars at run time.
pub(super) fn missing_value_limit_warnings(config: &ProjectConfig) -> Vec<String> {
    let mut warnings = Vec::new();
    for device in &config.devices {
        for channel in &device.channels {
            if channel.kind != DeviceType::Robot || !channel.enabled {
                continue;
            }
            for state_kind in &channel.publish_states {
                let entry = channel
                    .value_limits
                    .iter()
                    .find(|entry| entry.state_kind == *state_kind);
                let needs_warning = match entry {
                    None => true,
                    Some(entry) => entry.min.is_empty() || entry.max.is_empty(),
                };
                if needs_warning {
                    warnings.push(format!(
                        "device \"{}\" channel \"{}\": driver did not report value_limits for {}; bars will render as ??? until the device executable provides them",
                        device.name,
                        channel.channel_type,
                        state_kind.topic_suffix()
                    ));
                }
            }
        }
    }
    warnings
}

pub(super) fn discover_devices(
    workspace_root: &Path,
    process_working_dir: &Path,
    current_exe_dir: &Path,
    options: DiscoveryOptions,
) -> Result<(Vec<DiscoveredDevice>, Vec<String>), Box<dyn Error>> {
    let (probe_entries, mut probe_errors) = discover_probe_entries(
        workspace_root,
        process_working_dir,
        current_exe_dir,
        options,
        DISCOVERY_TIMEOUT,
    )?;
    let mut discoveries = Vec::new();

    for entry in probe_entries {
        match build_discovered_device(
            &entry.executable,
            &entry.probe_entry,
            &entry.program,
            process_working_dir,
            DISCOVERY_TIMEOUT,
        ) {
            Ok(device) => discoveries.push(device),
            Err(error) => probe_errors.push(format!("{}: {error}", entry.executable)),
        }
    }

    Ok((discoveries, probe_errors))
}

/// Single, device-type-free discovery entry. Every channel reported by the
/// driver's `query --json` is parsed into a `DiscoveredChannelMeta`,
/// preserving its `kind` (camera or robot). The wizard later builds one
/// `DeviceChannelConfigV2` per channel without any per-driver branching.
pub(super) fn build_discovered_device(
    executable: &str,
    probe_entry: &Value,
    program: &OsString,
    process_working_dir: &Path,
    timeout: Duration,
) -> Result<DiscoveredDevice, Box<dyn Error>> {
    let id = probe_entry
        .as_str()
        .map(ToOwned::to_owned)
        .or_else(|| value_as_string(probe_entry.get("id")))
        .ok_or_else(|| format!("probe entry missing id: {probe_entry}"))?;
    let query = run_driver_json(
        program,
        &[
            OsString::from("query"),
            OsString::from("--json"),
            OsString::from(&id),
        ],
        process_working_dir,
        timeout,
    )?;
    let query_device = query
        .get("devices")
        .and_then(Value::as_array)
        .and_then(|devices| {
            devices
                .iter()
                .find(|device| value_as_string(device.get("id")).as_deref() == Some(id.as_str()))
                .or_else(|| devices.first())
        })
        .ok_or_else(|| format!("query returned no devices for id {id}: {query}"))?;

    // Driver name is authoritative from the query response. Fall back to
    // stripping the well-known `rollio-device-` prefix off the executable
    // name only if the response didn't include one (e.g. older drivers).
    let driver = value_as_string(query.get("driver"))
        .or_else(|| {
            executable
                .strip_prefix("rollio-device-")
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| executable.to_owned());

    let channel_meta_by_channel = parse_query_channel_meta(query_device);
    let device_label = value_as_string(query_device.get("device_label"));
    let default_device_name = value_as_string(query_device.get("default_device_name"));
    let display_name = device_label
        .clone()
        .unwrap_or_else(|| driver_to_label_fallback(&driver));

    Ok(DiscoveredDevice {
        driver,
        id,
        display_name,
        default_device_name,
        channel_meta_by_channel,
        transport: query_metadata_string(query_device, "transport")
            .or_else(|| value_as_string(probe_entry.get("transport"))),
        interface: query_metadata_string(query_device, "interface")
            .or_else(|| value_as_string(probe_entry.get("interface"))),
        product_variant: query_metadata_string(query_device, "product_variant")
            .or_else(|| value_as_string(probe_entry.get("product_variant"))),
        end_effector: query_metadata_string(query_device, "end_effector")
            .or_else(|| value_as_string(probe_entry.get("end_effector"))),
    })
}

/// "airbot-play" -> "Airbot Play" fallback when a driver doesn't supply its
/// own `device_label` in `query --json`. Generic, no per-driver lookup.
pub(super) fn driver_to_label_fallback(driver: &str) -> String {
    driver
        .split('-')
        .filter(|s| !s.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn validate_existing_project(
    project: &ProjectConfig,
    workspace_root: &Path,
    process_working_dir: &Path,
    current_exe_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    for device in &project.devices {
        validate_binary_device_hardware(
            device,
            workspace_root,
            process_working_dir,
            current_exe_dir,
        )?;
    }
    Ok(())
}

pub(super) fn validate_binary_device_hardware(
    device: &BinaryDeviceConfig,
    workspace_root: &Path,
    process_working_dir: &Path,
    current_exe_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    let executable_name = device
        .executable
        .clone()
        .unwrap_or_else(|| default_device_executable_name(&device.driver));
    let program = resolve_registered_program(&executable_name, workspace_root, current_exe_dir);
    let mut args = vec![OsString::from("validate"), OsString::from(&device.id)];
    for channel in device.channels.iter().filter(|channel| channel.enabled) {
        args.push(OsString::from("--channel-type"));
        args.push(OsString::from(&channel.channel_type));
    }
    args.push(OsString::from("--json"));
    let report = run_driver_json(&program, &args, process_working_dir, VALIDATION_TIMEOUT)?;
    if report
        .get("valid")
        .and_then(Value::as_bool)
        .is_some_and(|valid| !valid)
    {
        return Err(format!(
            "device \"{}\" ({}) is no longer valid",
            device.name, device.id
        )
        .into());
    }
    Ok(())
}

pub(super) fn build_discovery_config(
    discoveries: &[DiscoveredDevice],
) -> Result<ProjectConfig, Box<dyn Error>> {
    let mut config = ProjectConfig::draft_setup_template();
    let mut default_name_counts = BTreeMap::new();
    let mut arm_index = 0usize;
    let mut eef_index = 0usize;

    for discovery in discoveries {
        if discovery.channel_meta_by_channel.is_empty() {
            return Err(format!(
                "device \"{}\" ({}) exposed no channels in its query response",
                discovery.display_name, discovery.id
            )
            .into());
        }
        // Pick a "preferred mode" per device. The legacy wizard alternated
        // `FreeDrive` / `CommandFollowing` between leader/follower groups
        // based on whether a device was detected as an EEF (dof == 1) or
        // arm. We approximate that here by checking the first robot
        // channel's dof; cameras don't care about mode and take None.
        let preferred_mode = if discovery
            .channel_meta_by_channel
            .values()
            .any(|meta| meta.kind == DeviceType::Robot && meta.dof != Some(1))
        {
            let mode = group_default_mode(arm_index);
            arm_index += 1;
            mode
        } else if discovery
            .channel_meta_by_channel
            .values()
            .any(|meta| meta.kind == DeviceType::Robot)
        {
            let mode = group_default_mode(eef_index);
            eef_index += 1;
            mode
        } else {
            // Pure camera devices don't use the mode field but the unified
            // builder still needs a placeholder value.
            RobotMode::FreeDrive
        };
        let name_base = discovery
            .default_device_name
            .clone()
            .unwrap_or_else(|| discovery.driver.replace('-', "_"));
        let device_name = next_default_device_name(name_base, &mut default_name_counts);
        config.devices.push(binary_device_from_discovery(
            discovery,
            device_name,
            preferred_mode,
            &mut default_name_counts,
        ));
    }

    // Auto-build the default pairings once on discovery to seed the
    // pairing step, but leave the operator free to delete them via `d`
    // and add their own with `m`. Teleop is now the only collection mode
    // the wizard exposes (intervention is removed from the cycle), and
    // teleop with zero pairings is a valid intermediate state — the
    // operator may save the config before they assemble pairings.
    config.pairings = build_default_channel_pairings(&config.devices);
    config.mode = CollectionMode::Teleop;
    config
        .validate()
        .map_err(|e| -> Box<dyn Error> { e.to_string().into() })?;
    Ok(config)
}

pub(super) fn parse_camera_capabilities(capabilities: &Value) -> Vec<CameraProfile> {
    let pixel_formats = capabilities
        .get("pixel_formats")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value_as_string(Some(value)))
        .filter_map(|value| parse_pixel_format(&value))
        .collect::<Vec<_>>();
    let streams = capabilities
        .get("streams")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value_as_string(Some(value)))
        .collect::<Vec<_>>();

    capabilities
        .get("profiles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|profile| {
            let width = value_as_u32(profile.get("width"))?;
            let height = value_as_u32(profile.get("height"))?;
            let fps = value_as_fps_u32(profile.get("fps"))?;
            let stream =
                value_as_string(profile.get("stream")).or_else(|| streams.first().cloned());
            let channel =
                value_as_u32(profile.get("channel")).or_else(|| value_as_u32(profile.get("index")));
            let pixel_format = value_as_string(profile.get("pixel_format"))
                .or_else(|| value_as_string(profile.get("native_pixel_format")))
                .and_then(|value| parse_pixel_format(&value))
                .or_else(|| stream.as_deref().and_then(infer_stream_pixel_format))
                .or_else(|| pixel_formats.first().copied())
                .unwrap_or(PixelFormat::Rgb24);
            Some(CameraProfile {
                width,
                height,
                fps,
                pixel_format,
                native_pixel_format: value_as_string(profile.get("native_pixel_format")),
                stream,
                channel: channel.filter(|channel| *channel > 0),
            })
        })
        .collect()
}

pub(super) fn parse_query_robot_modes(channel: &Value) -> Vec<RobotMode> {
    channel
        .get("modes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .filter_map(|mode| match mode {
            "free-drive" => Some(RobotMode::FreeDrive),
            "command-following" => Some(RobotMode::CommandFollowing),
            "identifying" => Some(RobotMode::Identifying),
            "disabled" => Some(RobotMode::Disabled),
            _ => None,
        })
        .collect()
}

pub(super) fn parse_query_channel_meta(device: &Value) -> BTreeMap<String, DiscoveredChannelMeta> {
    device
        .get("channels")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|channel| {
            let channel_type = value_as_string(channel.get("channel_type"))?;
            let kind = value_as_string(channel.get("kind"))
                .and_then(|s| match s.as_str() {
                    "camera" => Some(DeviceType::Camera),
                    "robot" => Some(DeviceType::Robot),
                    "sensor" => Some(DeviceType::Sensor),
                    _ => None,
                })
                .unwrap_or(DeviceType::Robot);
            let channel_label = value_as_string(channel.get("channel_label"));
            let default_name = value_as_string(channel.get("default_name"));
            let modes = parse_query_robot_modes(channel);
            let dof = value_as_u32(channel.get("dof"));
            let default_control_frequency_hz =
                value_as_f64(channel.get("default_control_frequency_hz"))
                    .or_else(|| value_as_f64(channel.get("control_frequency_hz")));
            let defaults = parse_query_command_defaults(channel.get("defaults"));
            let profiles = parse_channel_camera_profiles(channel);
            let value_limits =
                crate::device_query::parse_query_value_limits(channel.get("value_limits"));
            let mut supported_states =
                crate::device_query::parse_query_supported_states(channel.get("supported_states"));
            // Fall back to the kinds enumerated by value_limits so older
            // drivers that only populate value_limits still expose a
            // supported-state list to the wizard.
            if supported_states.is_empty() {
                supported_states = value_limits.iter().map(|entry| entry.state_kind).collect();
            }
            let supported_commands = crate::device_query::parse_query_supported_commands(
                channel.get("supported_commands"),
            );
            let direct_joint_compatibility =
                crate::device_query::parse_query_direct_joint_compatibility(
                    channel.get("direct_joint_compatibility"),
                );
            let supported_sensor_kinds =
                parse_query_supported_sensor_kinds(channel.get("supported_sensor_kinds"));
            let default_sample_rate_hz = value_as_f64(channel.get("default_sample_rate_hz"));
            let sensor_shape_hints =
                parse_query_sensor_shape_hints(channel.get("sensor_shape_hints"));
            Some((
                channel_type,
                DiscoveredChannelMeta {
                    kind,
                    channel_label,
                    default_name,
                    modes,
                    dof,
                    profiles,
                    default_control_frequency_hz,
                    defaults,
                    value_limits,
                    supported_states,
                    supported_commands,
                    direct_joint_compatibility,
                    supported_sensor_kinds,
                    default_sample_rate_hz,
                    sensor_shape_hints,
                },
            ))
        })
        .collect()
}

pub(super) fn parse_channel_camera_profiles(channel: &Value) -> Vec<CameraProfile> {
    let stream = value_as_string(channel.get("channel_type"));
    channel
        .get("profiles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|profile| {
            let width = value_as_u32(profile.get("width"))?;
            let height = value_as_u32(profile.get("height"))?;
            let fps = value_as_u32(profile.get("fps"))
                .or_else(|| value_as_f64(profile.get("fps")).map(|fps| fps.round() as u32))?;
            let pixel_format = value_as_string(profile.get("pixel_format"))
                .and_then(|value| parse_pixel_format_name(&value))
                .unwrap_or(PixelFormat::Rgb24);
            Some(CameraProfile {
                width,
                height,
                fps,
                pixel_format,
                native_pixel_format: value_as_string(profile.get("native_pixel_format")),
                stream: stream.clone(),
                channel: None,
            })
        })
        .collect()
}

fn parse_sensor_kind_name(name: &str) -> Option<rollio_types::config::SensorStateKind> {
    use rollio_types::config::SensorStateKind;
    match name {
        "imu_accel_gyro" => Some(SensorStateKind::ImuAccelGyro),
        "tactile_point_cloud2" => Some(SensorStateKind::TactilePointCloud2),
        _ => None,
    }
}

pub(super) fn parse_query_supported_sensor_kinds(
    value: Option<&Value>,
) -> Vec<rollio_types::config::SensorStateKind> {
    value
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().and_then(parse_sensor_kind_name))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn parse_query_sensor_shape_hints(
    value: Option<&Value>,
) -> BTreeMap<rollio_types::config::SensorStateKind, Vec<u32>> {
    let Some(obj) = value.and_then(|v| v.as_object()) else {
        return BTreeMap::new();
    };
    obj.iter()
        .filter_map(|(key, dims_value)| {
            let kind = parse_sensor_kind_name(key)?;
            let dims = dims_value.as_array()?;
            let shape: Vec<u32> = dims.iter().filter_map(|d| value_as_u32(Some(d))).collect();
            if shape.is_empty() {
                None
            } else {
                Some((kind, shape))
            }
        })
        .collect()
}

pub(super) fn parse_query_command_defaults(
    value: Option<&Value>,
) -> rollio_types::config::ChannelCommandDefaults {
    let parse_array = |key: &str| -> Vec<f64> {
        value
            .and_then(|v| v.get(key))
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default()
    };
    rollio_types::config::ChannelCommandDefaults {
        joint_mit_kp: parse_array("joint_mit_kp"),
        joint_mit_kd: parse_array("joint_mit_kd"),
        parallel_mit_kp: parse_array("parallel_mit_kp"),
        parallel_mit_kd: parse_array("parallel_mit_kd"),
    }
}

pub(super) fn query_metadata_string(device: &Value, key: &str) -> Option<String> {
    value_as_string(device.get(key)).or_else(|| {
        device
            .get("optional_info")
            .and_then(|optional| optional.get(key))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    })
}

pub(super) fn enrich_current_device_from_discovery(
    current: &mut BinaryDeviceConfig,
    discovery: &DiscoveredDevice,
) {
    merge_discovery_extra(&mut current.extra, discovery);
    // Re-merge per-state value_limits from the latest query so a project
    // saved before the driver started reporting limits picks them up on the
    // next setup pass without manual editing.
    for channel in current.channels.iter_mut() {
        if let Some(meta) = discovery.channel_meta_by_channel.get(&channel.channel_type) {
            if !meta.value_limits.is_empty() {
                channel.value_limits = meta.value_limits.clone();
            }
        }
    }
    if !discovery
        .channel_meta_by_channel
        .values()
        .any(|meta| meta.kind == DeviceType::Camera)
    {
        return;
    }
    let camera_profiles = discovery.all_camera_profiles();
    for channel in current.channels.iter_mut() {
        if channel.kind != DeviceType::Camera {
            continue;
        }
        let Some(profile) = channel.profile.as_mut() else {
            continue;
        };
        if profile.native_pixel_format.is_some() {
            continue;
        }
        let matched = camera_profiles.iter().find(|candidate| {
            candidate.width == profile.width
                && candidate.height == profile.height
                && candidate.fps == profile.fps
                && candidate.pixel_format == profile.pixel_format
                && camera_channel_type_for_profile(candidate) == channel.channel_type
        });
        if let Some(matched) = matched {
            profile.native_pixel_format = matched.native_pixel_format.clone();
        }
    }
}

pub(super) fn merge_discovery_extra(extra: &mut toml::Table, discovery: &DiscoveredDevice) {
    if let Some(transport) = &discovery.transport {
        extra.insert("transport".into(), toml::Value::String(transport.clone()));
    }
    if let Some(interface) = &discovery.interface {
        extra.insert("interface".into(), toml::Value::String(interface.clone()));
    }
    if let Some(product_variant) = &discovery.product_variant {
        extra.insert(
            "product_variant".into(),
            toml::Value::String(product_variant.clone()),
        );
    }
    if let Some(end_effector) = &discovery.end_effector {
        extra.insert(
            "end_effector".into(),
            toml::Value::String(end_effector.clone()),
        );
    }
}

pub(super) fn parse_pixel_format_name(value: &str) -> Option<PixelFormat> {
    match value {
        "rgb24" => Some(PixelFormat::Rgb24),
        "bgr24" => Some(PixelFormat::Bgr24),
        "yuyv" => Some(PixelFormat::Yuyv),
        "mjpeg" => Some(PixelFormat::Mjpeg),
        "depth16" => Some(PixelFormat::Depth16),
        "gray8" => Some(PixelFormat::Gray8),
        "h264-annex-b" => Some(PixelFormat::H264AnnexB),
        _ => None,
    }
}

pub(super) fn parse_robot_modes(value: Option<&Value>) -> Vec<RobotMode> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| value_as_string(Some(entry)))
        .filter_map(|mode| match mode.as_str() {
            "free-drive" => Some(RobotMode::FreeDrive),
            "command-following" => Some(RobotMode::CommandFollowing),
            "identifying" => Some(RobotMode::Identifying),
            "disabled" => Some(RobotMode::Disabled),
            _ => None,
        })
        .collect()
}

pub(super) fn default_supported_robot_modes() -> Vec<RobotMode> {
    vec![
        RobotMode::FreeDrive,
        RobotMode::CommandFollowing,
        RobotMode::Identifying,
        RobotMode::Disabled,
    ]
}

pub(super) fn parse_pixel_format(value: &str) -> Option<PixelFormat> {
    match value.trim().to_ascii_lowercase().as_str() {
        "rgb24" | "rgb3" => Some(PixelFormat::Rgb24),
        "bgr24" | "bgr3" => Some(PixelFormat::Bgr24),
        "yuyv" | "yuy2" => Some(PixelFormat::Yuyv),
        "mjpeg" | "mjpg" => Some(PixelFormat::Mjpeg),
        "depth16" | "z16" => Some(PixelFormat::Depth16),
        "gray8" | "grey" | "gray" | "y8" => Some(PixelFormat::Gray8),
        "h264-annex-b" => Some(PixelFormat::H264AnnexB),
        _ => None,
    }
}

pub(super) fn infer_stream_pixel_format(stream: &str) -> Option<PixelFormat> {
    match stream {
        "color" => Some(PixelFormat::Rgb24),
        "depth" => Some(PixelFormat::Depth16),
        "infrared" => Some(PixelFormat::Gray8),
        _ => None,
    }
}

pub(super) fn value_as_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(ToOwned::to_owned)
}

pub(super) fn value_as_u32(value: Option<&Value>) -> Option<u32> {
    value
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

pub(super) fn value_as_f64(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64)
}

pub(super) fn value_as_fps_u32(value: Option<&Value>) -> Option<u32> {
    value_as_u32(value).or_else(|| {
        let fps = value_as_f64(value)?;
        if !fps.is_finite() || fps <= 0.0 || fps > u32::MAX as f64 {
            return None;
        }
        Some(fps.round() as u32)
    })
}

pub(super) fn discovery_camera_channel_key(stream: Option<&str>, channel: Option<u32>) -> String {
    let base = stream.unwrap_or("color").to_string();
    match channel {
        Some(ch) if ch > 0 => format!("{base}_{ch}"),
        _ => base,
    }
}

/// Match the project-side `BinaryDeviceConfig` to a fresh discovery by
/// `(driver, id)` only. Devices may now carry mixed-kind channels under a
/// single config, so matching on a "primary channel type" no longer makes
/// sense; the unified discovery loop emits one row per discovered device
/// regardless of which channels it exposes.
pub(super) fn device_matches_discovery_binary(
    device: &BinaryDeviceConfig,
    discovery: &DiscoveredDevice,
    _stream: Option<&str>,
    _channel: Option<u32>,
) -> bool {
    device.driver == discovery.driver && device.id == discovery.id
}

pub(super) fn available_device_key_from_binary(device: &BinaryDeviceConfig) -> String {
    let ch = device
        .channels
        .first()
        .expect("setup devices always include a primary channel");
    let kind = match ch.kind {
        DeviceType::Camera => "camera",
        DeviceType::Robot => "robot",
        DeviceType::Sensor => "sensor",
    };
    format!(
        "{kind}|{}|{}|{}|-",
        device.driver, device.id, ch.channel_type
    )
}

pub(super) fn next_default_device_name(
    base: String,
    counts: &mut BTreeMap<String, usize>,
) -> String {
    let next_index = counts.entry(base.clone()).or_insert(0);
    let resolved = if *next_index == 0 {
        base.clone()
    } else {
        format!("{base}_{}", *next_index + 1)
    };
    *next_index += 1;
    resolved
}

/// Dedupe the per-channel `default_name` reported by a driver query (e.g.
/// "airbot_play_arm", "airbot_e2") against the same `counts` map the
/// device-name path uses, so two physical AIRBOT Play arms become
/// "airbot_play_arm" and "airbot_play_arm_2" instead of two rows with the
/// same name. Returns `None` when the driver did not advertise a default
/// channel name (callers fall back to the channel_type).
pub(super) fn dedup_channel_default_name(
    default_name: Option<&str>,
    counts: &mut BTreeMap<String, usize>,
) -> Option<String> {
    let base = default_name?.trim();
    if base.is_empty() {
        return None;
    }
    Some(next_default_device_name(base.to_owned(), counts))
}

pub(super) fn group_default_mode(index: usize) -> RobotMode {
    match index {
        0 => RobotMode::FreeDrive,
        1 => RobotMode::CommandFollowing,
        _ => RobotMode::FreeDrive,
    }
}
