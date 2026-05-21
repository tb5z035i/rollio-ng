use super::runtime::{
    spawn_setup_children, SetupIpc, SetupRuntimeState, SETUP_POLL_INTERVAL, SETUP_SHUTDOWN_TIMEOUT,
};
use super::state::{AvailableDevice, CameraProfile, SetupSession, SetupStep};
use crate::cli::SetupBackend;
use crate::process::{terminate_children, ChildSpec};
use crate::runtime_plan::build_preview_specs;
use rollio_types::config::{
    ChannelPreviewConfig, CollectionMode, DeviceChannelConfigV2, DeviceType, PreviewOutputMode,
    ProjectConfig, RobotMode,
};
use rollio_types::messages::DeviceChannelMode;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Instant;

pub(super) fn should_run_preview_runtime(session: &SetupSession) -> bool {
    session.exit_kind.is_none()
        && (session.current_step == SetupStep::Preview
            || (session.current_step == SetupStep::Devices
                && session.identify_device_name.is_some()))
}

pub(super) fn robot_mode_to_channel_mode(mode: RobotMode) -> DeviceChannelMode {
    match mode {
        RobotMode::FreeDrive => DeviceChannelMode::FreeDrive,
        RobotMode::CommandFollowing => DeviceChannelMode::CommandFollowing,
        RobotMode::Identifying => DeviceChannelMode::Identifying,
        RobotMode::Disabled => DeviceChannelMode::Disabled,
    }
}

pub(super) fn available_primary_channel(
    available: &AvailableDevice,
) -> Option<&DeviceChannelConfigV2> {
    available.current.channels.first()
}

pub(super) fn publish_available_device_mode(
    ipc: &SetupIpc,
    available: &AvailableDevice,
    mode: DeviceChannelMode,
) -> Result<(), Box<dyn Error>> {
    let Some(channel) = available_primary_channel(available) else {
        return Ok(());
    };
    ipc.publish_channel_mode(&available.current.bus_root, &channel.channel_type, mode)
}

pub(super) fn configured_channel_mode_for_available(
    available: &AvailableDevice,
) -> Option<DeviceChannelMode> {
    let channel = available_primary_channel(available)?;
    if channel.kind != DeviceType::Robot || !channel.enabled {
        return None;
    }
    channel.mode.map(robot_mode_to_channel_mode)
}

pub(super) fn sync_identify_mode(
    session: &SetupSession,
    ipc: &SetupIpc,
    active_identify_target: &mut Option<String>,
    preview_runtime_restarted: bool,
) -> Result<(), Box<dyn Error>> {
    let desired_target = if session.current_step == SetupStep::Devices {
        session.identify_device_name.clone()
    } else {
        None
    };

    if active_identify_target.as_ref() != desired_target.as_ref() {
        if let Some(previous_name) = active_identify_target.as_deref() {
            if let Some(previous_available) = session.available_device(previous_name) {
                if let Some(mode) = configured_channel_mode_for_available(previous_available) {
                    publish_available_device_mode(ipc, previous_available, mode)?;
                }
            }
        }
    }

    if preview_runtime_restarted || active_identify_target.as_ref() != desired_target.as_ref() {
        if let Some(target_name) = desired_target.as_deref() {
            if let Some(target_available) = session.available_device(target_name) {
                if available_primary_channel(target_available)
                    .is_some_and(|channel| channel.kind == DeviceType::Robot)
                {
                    publish_available_device_mode(
                        ipc,
                        target_available,
                        DeviceChannelMode::Identifying,
                    )?;
                }
            }
        }
    }

    *active_identify_target = desired_target;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn start_preview_runtime(
    session: &mut SetupSession,
    preview_port: u16,
    preview_websocket_url: &str,
    workspace_root: &Path,
    child_working_dir: &Path,
    current_exe_dir: &Path,
    log_dir: &Path,
    backend: SetupBackend,
) -> Result<SetupRuntimeState, Box<dyn Error>> {
    let preview_config =
        build_preview_project_config(session, preview_port, preview_websocket_url, backend)?;
    let temp_config_path = write_setup_temp_config(
        &preview_config,
        log_dir,
        &format!("setup-preview-{preview_port}.toml"),
    )?;
    let specs = build_setup_preview_specs(
        &preview_config,
        workspace_root,
        child_working_dir,
        current_exe_dir,
    )?;
    let children = spawn_setup_children(&specs, log_dir)?;
    if session.current_step == SetupStep::Preview {
        session.message = Some(format!(
            "Preview running from {}",
            temp_config_path.display()
        ));
    }
    Ok(SetupRuntimeState {
        children,
        temp_config_path: Some(temp_config_path),
        preview_target_name: session.identify_device_name.clone(),
    })
}

pub(super) fn stop_setup_runtime(
    runtime_state: &mut Option<SetupRuntimeState>,
    ipc: &SetupIpc,
) -> Result<(), Box<dyn Error>> {
    let Some(mut runtime) = runtime_state.take() else {
        return Ok(());
    };

    ipc.send_shutdown()?;

    let deadline = Instant::now() + SETUP_SHUTDOWN_TIMEOUT;
    loop {
        let mut remaining_children = 0usize;
        for child in runtime.children.iter_mut() {
            if child.child.try_wait()?.is_none() {
                remaining_children += 1;
            }
        }
        if remaining_children == 0 {
            break;
        }
        if Instant::now() >= deadline {
            terminate_children(
                &mut runtime.children,
                SETUP_SHUTDOWN_TIMEOUT,
                SETUP_POLL_INTERVAL,
            )?;
            break;
        }
        thread::sleep(SETUP_POLL_INTERVAL);
    }
    if let Some(temp_config_path) = runtime.temp_config_path.as_deref() {
        cleanup_preview_temp_config(temp_config_path);
    }
    Ok(())
}

pub(super) fn build_preview_project_config(
    session: &SetupSession,
    websocket_port: u16,
    websocket_url: &str,
    backend: SetupBackend,
) -> Result<ProjectConfig, Box<dyn Error>> {
    let mut preview = if session.current_step == SetupStep::Devices {
        if let Some(target_name) = session.identify_device_name.as_deref() {
            let target = session
                .available_device(target_name)
                .ok_or_else(|| format!("missing identify target {target_name}"))?;
            let mut preview = session.config.clone();
            preview.mode = CollectionMode::Intervention;
            preview.pairings.clear();
            // Boot the identify target's robot channels directly into
            // RobotMode::Identifying so the device process never has the
            // chance to start in FreeDrive and miss a late-arriving mode
            // event from `sync_identify_mode` (race confirmed in debug
            // session 8d351b: device booted ~21 ms AFTER controller
            // published Identifying, so the publish landed before the
            // subscriber existed).
            let mut device = target.current.clone();
            for channel in device.channels.iter_mut() {
                if channel.kind == DeviceType::Robot && channel.enabled {
                    channel.mode = Some(RobotMode::Identifying);
                }
            }
            preview.devices = vec![device];
            preview
        } else {
            session.config.clone()
        }
    } else {
        session.config.clone()
    };
    preview.visualizer.port = websocket_port;
    preview.ui.preview_websocket_url = Some(websocket_url.into());
    // The Ink terminal UI only decodes the legacy JPEG binary message kind;
    // H.264 / Annex B packets from `PreviewOutputMode::Encoded` are silently
    // dropped at `parseBinaryMessage`. Force JPEG only on the TUI path so
    // camera identify / Step-4 live preview show anything at all in the
    // terminal. The web SPA has a WebCodecs decoder and honors whatever
    // `preview_output_mode` the project config specifies.
    if matches!(backend, SetupBackend::Tui) {
        force_jpeg_preview_for_terminal_ui(&mut preview);
    }
    Ok(preview)
}

/// Override every camera channel's `preview_settings.output_mode` to
/// `Jpeg`. The setup wizard's terminal UI only handles the JPEG binary
/// message kind from the visualizer; H.264 / encoded packets are
/// silently dropped at `parseBinaryMessage`. The recording pipeline
/// (which writes to disk) is unaffected — only `preview_settings` is
/// touched.
fn force_jpeg_preview_for_terminal_ui(preview: &mut ProjectConfig) {
    for device in preview.devices.iter_mut() {
        for channel in device.channels.iter_mut() {
            if channel.kind != DeviceType::Camera || !channel.preview_enabled {
                continue;
            }
            let mut settings = channel
                .preview_settings
                .clone()
                .unwrap_or_else(ChannelPreviewConfig::default);
            settings.output_mode = Some(PreviewOutputMode::Jpeg);
            channel.preview_settings = Some(settings);
        }
    }
}

pub(super) fn write_setup_temp_config(
    project: &ProjectConfig,
    log_dir: &Path,
    filename: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let path = log_dir.join(filename);
    fs::write(&path, toml::to_string_pretty(project)?)?;
    Ok(path)
}

pub(super) fn cleanup_preview_temp_config(path: &Path) {
    let _ = fs::remove_file(path);
}

pub(super) fn build_setup_preview_specs(
    project: &ProjectConfig,
    workspace_root: &Path,
    child_working_dir: &Path,
    current_exe_dir: &Path,
) -> Result<Vec<ChildSpec>, Box<dyn Error>> {
    build_preview_specs(project, workspace_root, child_working_dir, current_exe_dir)
}

pub(super) fn camera_channel_type_for_profile(profile: &CameraProfile) -> String {
    let base = profile
        .stream
        .clone()
        .unwrap_or_else(|| "color".to_string());
    match profile.channel {
        Some(ch) if ch > 0 => format!("{base}_{ch}"),
        _ => base,
    }
}

pub(super) fn robot_default_channel_type(_driver: &str) -> String {
    "arm".into()
}
