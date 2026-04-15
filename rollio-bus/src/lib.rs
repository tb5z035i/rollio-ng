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
