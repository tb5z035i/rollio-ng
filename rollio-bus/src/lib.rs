pub const CONTROL_EVENTS_SERVICE: &str = "control/events";
pub const EPISODE_COMMAND_SERVICE: &str = "control/episode-command";
pub const EPISODE_STATUS_SERVICE: &str = "control/episode-status";
pub const SETUP_COMMAND_SERVICE: &str = "setup/command";
pub const SETUP_STATE_SERVICE: &str = "setup/state";
pub const VIDEO_READY_SERVICE: &str = "encoder/video-ready";
pub const EPISODE_READY_SERVICE: &str = "assembler/episode-ready";
pub const EPISODE_STORED_SERVICE: &str = "storage/episode-stored";
pub const BACKPRESSURE_SERVICE: &str = "encoder/backpressure";

pub fn camera_frames_service_name(device_name: &str) -> String {
    format!("camera/{device_name}/frames")
}

pub fn robot_state_service_name(device_name: &str) -> String {
    format!("robot/{device_name}/state")
}

pub fn robot_command_service_name(device_name: &str) -> String {
    format!("robot/{device_name}/command")
}

pub fn device_info_service_name(bus_root: &str) -> String {
    format!("{bus_root}/info")
}

pub fn device_shutdown_service_name(bus_root: &str) -> String {
    format!("{bus_root}/shutdown")
}

pub fn channel_status_service_name(bus_root: &str, channel_type: &str) -> String {
    format!("{bus_root}/{channel_type}/status")
}

pub fn channel_mode_info_service_name(bus_root: &str, channel_type: &str) -> String {
    format!("{bus_root}/{channel_type}/info/mode")
}

pub fn channel_mode_control_service_name(bus_root: &str, channel_type: &str) -> String {
    format!("{bus_root}/{channel_type}/control/mode")
}

pub fn channel_profile_info_service_name(bus_root: &str, channel_type: &str) -> String {
    format!("{bus_root}/{channel_type}/info/profile")
}

pub fn channel_profile_control_service_name(bus_root: &str, channel_type: &str) -> String {
    format!("{bus_root}/{channel_type}/control/profile")
}

pub fn channel_frames_service_name(bus_root: &str, channel_type: &str) -> String {
    format!("{bus_root}/{channel_type}/frames")
}

pub fn channel_state_service_name(bus_root: &str, channel_type: &str, state_kind: &str) -> String {
    format!("{bus_root}/{channel_type}/states/{state_kind}")
}

pub fn channel_command_service_name(
    bus_root: &str,
    channel_type: &str,
    command_kind: &str,
) -> String {
    format!("{bus_root}/{channel_type}/commands/{command_kind}")
}
