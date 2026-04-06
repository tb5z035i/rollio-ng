pub const CONTROL_EVENTS_SERVICE: &str = "control/events";

pub fn camera_frames_service_name(device_name: &str) -> String {
    format!("camera/{device_name}/frames")
}

pub fn robot_state_service_name(device_name: &str) -> String {
    format!("robot/{device_name}/state")
}

pub fn robot_command_service_name(device_name: &str) -> String {
    format!("robot/{device_name}/command")
}
