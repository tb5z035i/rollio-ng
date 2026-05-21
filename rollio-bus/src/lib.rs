pub const CONTROL_EVENTS_SERVICE: &str = "control/events";
pub const EPISODE_COMMAND_SERVICE: &str = "control/episode-command";
pub const EPISODE_STATUS_SERVICE: &str = "control/episode-status";
pub const SETUP_COMMAND_SERVICE: &str = "setup/command";
pub const SETUP_STATE_SERVICE: &str = "setup/state";
pub const EPISODE_READY_SERVICE: &str = "assembler/episode-ready";
pub const EPISODE_STORED_SERVICE: &str = "storage/episode-stored";
pub const EPISODE_DROPPED_SERVICE: &str = "assembler/episode-dropped";
pub const BACKPRESSURE_SERVICE: &str = "encoder/backpressure";

/// Default ring buffer depth for every state and command publish_subscribe
/// service.
///
/// Robot drivers publish states / commands at their `control_frequency_hz`
/// (250 Hz on AIRBOT and Nero by default — one sample every 4 ms). The
/// iceoryx2 default `subscriber_max_buffer_size` is `2`, which gives the
/// consumer just ~8 ms of headroom before silent overwrites — far less
/// than the worst-case work that consumers (notably `rollio-episode-lerobot`
/// during `stage_episode`) can take. With a 1024-slot ring the consumer
/// can be unresponsive for ~4 s at 250 Hz before any sample is lost.
///
/// Both the producer side (`robots/*`) and the consumer side
/// (`rollio-episode-lerobot`, `teleop-router`, `visualizer`) must request at
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

/// Subscriber buffer for the strict per-camera recording packet topic.
/// Recording packets cannot be dropped without corrupting the resulting
/// video container, so producers configure these services with safe
/// overflow disabled and `UnableToDeliverStrategy::Block`. The buffer
/// must therefore be deep enough that a brief assembler stall (parquet
/// flush, video file fsync) doesn't push back on the encoder hot path
/// long enough to drop camera frames upstream.
pub const RECORDING_PACKET_BUFFER: usize = STATE_BUFFER;

/// Subscriber buffer for the best-effort preview packet / preview JPEG
/// topics. Preview is intentionally loss-tolerant: the visualizer/UI
/// recovers at the next keyframe, so a small ring keeps memory bounded.
pub const PREVIEW_PACKET_BUFFER: usize = 8;

/// History size for the per-camera recording-config and preview-config
/// topics. iceoryx2 retains the most recent `N` samples per publisher
/// and replays them to subscribers that connect afterwards, which is
/// exactly the late-join semantics codec stream config needs (a
/// subscriber that misses the one-shot config message can't decode any
/// packet until the encoder is restarted). Keeping the value at 1
/// keeps the cost minimal while still surviving a visualizer restart.
pub const STREAM_CONFIG_HISTORY_SIZE: usize = 1;

/// Camera-frame topic max_subscribers. Bumped from the iceoryx2 default
/// of 2 to accommodate the new packet-mode topology where every camera
/// has both a recording-role encoder and a preview-role encoder
/// subscribing simultaneously, plus headroom for diagnostics
/// (`test/bus-tap`) and future tools.
pub const CAMERA_FRAMES_MAX_SUBSCRIBERS: usize = 4;

pub const EPISODE_METADATA_ENTRIES_SERVICE: &str = "episode/metadata/entries";
pub const EPISODE_METADATA_HISTORY_SIZE: usize = 64;
pub const EPISODE_METADATA_BUFFER: usize = 256;

/// Capacity caps for the `control/events` fan-out service.
/// Every device driver, encoder, teleop router, visualizer and assembler
/// subscribes here for `Shutdown` / `RecordingStart` / etc.  A moderately
/// large rig (2 robots + 3-stream RealSense + 2 webcams + encoder pool)
/// exhausts the iceoryx2 default of 16 nodes, so these are set generously.
pub const CONTROL_EVENTS_MAX_PUBLISHERS: usize = 4;
pub const CONTROL_EVENTS_MAX_SUBSCRIBERS: usize = 32;
pub const CONTROL_EVENTS_MAX_NODES: usize = 32;

/// Capacity caps for the `control/episode-command` service.
/// Published by the station, consumed by the controller and a small number
/// of interested observers (e.g. storage backends).
pub const EPISODE_COMMAND_MAX_PUBLISHERS: usize = 4;
pub const EPISODE_COMMAND_MAX_SUBSCRIBERS: usize = 8;
pub const EPISODE_COMMAND_MAX_NODES: usize = 8;

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

// ---------------------------------------------------------------------------
// Encoded packet topics
// ---------------------------------------------------------------------------

/// Per-channel codec-config topic for the recording role. Carries
/// `EncodedPacketHeader` user header with `kind = Config`. iceoryx2
/// `history_size = STREAM_CONFIG_HISTORY_SIZE` so the latest config is
/// replayed to subscribers that connect after the encoder.
pub fn recording_config_service_name(bus_root: &str, channel_type: &str) -> String {
    format!("{bus_root}/{channel_type}/recording-config")
}

/// Per-channel encoded recording packets. Strict delivery (no overflow,
/// publisher blocks). One `Packet` per encoded access unit; one
/// `EndOfStream` at session finish.
pub fn recording_packet_service_name(bus_root: &str, channel_type: &str) -> String {
    format!("{bus_root}/{channel_type}/recording-packets")
}

/// Per-channel codec-config topic for the preview role. Same shape as
/// recording-config but on a separate name so subscribers can apply
/// loss-tolerant delivery semantics.
pub fn preview_config_service_name(bus_root: &str, channel_type: &str) -> String {
    format!("{bus_root}/{channel_type}/preview-config")
}

/// Per-channel encoded preview packets. Best-effort delivery; the UI
/// recovers at the next keyframe after a drop.
pub fn preview_packet_service_name(bus_root: &str, channel_type: &str) -> String {
    format!("{bus_root}/{channel_type}/preview-packets")
}

/// Per-channel preview JPEG bytes. Used when the project's
/// `[encoder.preview] output_mode = "jpeg"`. The encoder publishes
/// `CameraFrameHeader` user header (so it reuses the existing
/// camera-frame plumbing on the visualizer side) with the JPEG
/// payload.
pub fn preview_jpeg_service_name(bus_root: &str, channel_type: &str) -> String {
    format!("{bus_root}/{channel_type}/preview-jpeg")
}

/// Per-channel preview control topic. The visualizer publishes
/// `PreviewControl::SetSize` here when an operator changes the preview
/// raster dims; the preview encoder restarts its session at the new
/// dims and emits a fresh `Config` + first keyframe.
pub fn preview_control_service_name(bus_root: &str, channel_type: &str) -> String {
    format!("{bus_root}/{channel_type}/preview-control")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_topic_names_use_purpose_suffix() {
        assert_eq!(
            recording_config_service_name("cam1", "color"),
            "cam1/color/recording-config"
        );
        assert_eq!(
            recording_packet_service_name("cam1", "color"),
            "cam1/color/recording-packets"
        );
    }

    #[test]
    fn preview_topic_names_use_purpose_suffix() {
        assert_eq!(
            preview_config_service_name("cam1", "color"),
            "cam1/color/preview-config"
        );
        assert_eq!(
            preview_packet_service_name("cam1", "color"),
            "cam1/color/preview-packets"
        );
        assert_eq!(
            preview_jpeg_service_name("cam1", "color"),
            "cam1/color/preview-jpeg"
        );
        assert_eq!(
            preview_control_service_name("cam1", "color"),
            "cam1/color/preview-control"
        );
    }
}
