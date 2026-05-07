pub const CONTROL_EVENTS_SERVICE: &str = "control/events";
pub const EPISODE_COMMAND_SERVICE: &str = "control/episode-command";
pub const EPISODE_STATUS_SERVICE: &str = "control/episode-status";
pub const SETUP_COMMAND_SERVICE: &str = "setup/command";
pub const SETUP_STATE_SERVICE: &str = "setup/state";
pub const VIDEO_READY_SERVICE: &str = "encoder/video-ready";
pub const EPISODE_READY_SERVICE: &str = "assembler/episode-ready";
pub const EPISODE_STORED_SERVICE: &str = "storage/episode-stored";
pub const BACKPRESSURE_SERVICE: &str = "encoder/backpressure";

/// Default ring buffer depth for every state and command publish_subscribe
/// service.
///
/// Robot drivers publish states / commands at their `control_frequency_hz`
/// (250 Hz on AIRBOT and Nero by default — one sample every 4 ms). The
/// iceoryx2 default `subscriber_max_buffer_size` is `2`, which gives the
/// consumer just ~8 ms of headroom before silent overwrites — far less
/// than the worst-case work that consumers (notably the episode-assembler
/// during `stage_episode`) can take. With a 1024-slot ring the consumer
/// can be unresponsive for ~4 s at 250 Hz before any sample is lost.
///
/// Both the producer side (`robots/*`) and the consumer side
/// (`episode-assembler`, `teleop-router`, `visualizer`) must request at
/// least this depth — `open_or_create` rejects mismatches.
pub const STATE_BUFFER: usize = 1024;

/// Default `max_publishers` cap for state / command services.
///
/// Each state/command topic typically has one publisher (the device driver
/// owning the channel) and a small number of subscribers. Lifting the
/// default of 2 to 16 keeps the topology composable for multi-device
/// setups (e.g. teleop-router fanning multiple followers off one leader)
/// without any practical memory cost.
pub const STATE_MAX_PUBLISHERS: usize = 16;

/// Default `max_subscribers` cap for state / command services.
pub const STATE_MAX_SUBSCRIBERS: usize = 16;

/// Default `max_nodes` cap for state / command services.
pub const STATE_MAX_NODES: usize = 16;

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

/// Topic for the always-on RGB24 preview tap that the encoder publishes for
/// the visualizer to subscribe to. Mirrors `channel_frames_service_name` but
/// carries downsized frames at the visualizer's preview cadence regardless
/// of whether an episode is being recorded.
pub fn channel_preview_service_name(bus_root: &str, channel_type: &str) -> String {
    format!("{bus_root}/{channel_type}/preview")
}

pub fn channel_state_service_name(bus_root: &str, channel_type: &str, state_kind: &str) -> String {
    format!("{bus_root}/{channel_type}/states/{state_kind}")
}

/// IMU state service. Sibling of [`channel_state_service_name`] but with the
/// state kind pinned to `imu` since IMU streams aren't enumerated as a
/// `RobotStateKind` (they're a separate `DeviceType::Imu`).
pub fn channel_imu_service_name(bus_root: &str, channel_type: &str) -> String {
    format!("{bus_root}/{channel_type}/states/imu")
}

pub fn channel_command_service_name(
    bus_root: &str,
    channel_type: &str,
    command_kind: &str,
) -> String {
    format!("{bus_root}/{channel_type}/commands/{command_kind}")
}
