/// WebSocket binary/JSON protocol encoding and decoding.
///
/// Binary messages (Visualizer → UI) — camera frames:
///   Byte 0:      frame encoding type (0x01 = JPEG)
///   Bytes 1-2:   camera name length (u16 LE)
///   Bytes 3..N:  camera name (UTF-8)
///   Bytes N+1..N+8:   source timestamp_ns (u64 LE)
///   Bytes N+9..N+16:  source frame_index (u64 LE)
///   Bytes N+17..N+20: encoded preview width (u32 LE)
///   Bytes N+21..N+24: encoded preview height (u32 LE)
///   Remaining:   encoded frame data (JPEG payload)
///
/// Text/JSON messages (both directions):
///   Visualizer → UI: {"type":"robot_state","name":"...","timestamp_ns":...,...}
///   Visualizer → UI: {"type":"episode_status","state":"idle","episode_count":0,...}
///   UI → Visualizer:  {"type":"command","action":"...","width":...,"height":...}
use rollio_types::messages::{EndEffectorStatus, EpisodeCommand, EpisodeStatus, RobotState};
use serde::{Deserialize, Serialize};

use crate::stream_info::StreamInfoSnapshot;

/// Frame encoding type tags.
pub const FRAME_TYPE_JPEG: u8 = 0x01;

/// Encode a camera frame into the binary WebSocket protocol.
///
/// Pre-allocates the exact output capacity to avoid reallocation.
pub fn encode_camera_frame(
    name: &str,
    timestamp_ns: u64,
    frame_index: u64,
    width: u32,
    height: u32,
    jpeg_data: &[u8],
) -> Vec<u8> {
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len();
    // 1 (type) + 2 (name len) + name + 8 (timestamp) + 8 (frame index)
    // + 4 (width) + 4 (height) + jpeg payload
    let total = 1 + 2 + name_len + 8 + 8 + 4 + 4 + jpeg_data.len();
    let mut buf = Vec::with_capacity(total);

    buf.push(FRAME_TYPE_JPEG);
    buf.extend_from_slice(&(name_len as u16).to_le_bytes());
    buf.extend_from_slice(name_bytes);
    buf.extend_from_slice(&timestamp_ns.to_le_bytes());
    buf.extend_from_slice(&frame_index.to_le_bytes());
    buf.extend_from_slice(&width.to_le_bytes());
    buf.extend_from_slice(&height.to_le_bytes());
    buf.extend_from_slice(jpeg_data);

    buf
}

/// JSON message for robot state sent to the UI.
#[derive(Serialize)]
struct RobotStateJson<'a> {
    #[serde(rename = "type")]
    msg_type: &'static str,
    name: &'a str,
    timestamp_ns: u64,
    num_joints: u32,
    positions: &'a [f64],
    velocities: &'a [f64],
    efforts: &'a [f64],
    #[serde(skip_serializing_if = "Option::is_none")]
    end_effector_status: Option<EndEffectorStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_effector_feedback_valid: Option<bool>,
}

/// Encode a robot state into a JSON string for WebSocket text message.
///
/// Only serializes `num_joints` values from each array, not the full 16.
pub fn encode_robot_state(name: &str, state: &RobotState) -> String {
    let n = state.num_joints as usize;
    let msg = RobotStateJson {
        msg_type: "robot_state",
        name,
        timestamp_ns: state.timestamp_ns,
        num_joints: state.num_joints,
        positions: &state.positions[..n],
        velocities: &state.velocities[..n],
        efforts: &state.efforts[..n],
        end_effector_status: state
            .has_end_effector_status
            .then_some(state.end_effector_status),
        end_effector_feedback_valid: state
            .has_end_effector_status
            .then_some(state.end_effector_feedback_valid),
    };
    // serde_json::to_string is infallible for this struct
    serde_json::to_string(&msg).unwrap_or_default()
}

#[derive(Serialize)]
struct EpisodeStatusJson {
    #[serde(rename = "type")]
    msg_type: &'static str,
    state: &'static str,
    episode_count: u32,
    elapsed_ms: u64,
}

pub fn encode_episode_status(status: &EpisodeStatus) -> String {
    serde_json::to_string(&EpisodeStatusJson {
        msg_type: "episode_status",
        state: status.state.as_str(),
        episode_count: status.episode_count,
        elapsed_ms: status.elapsed_ms,
    })
    .unwrap_or_default()
}

/// Encode stream metadata into a JSON string for WebSocket text message.
pub fn encode_stream_info(snapshot: &StreamInfoSnapshot) -> String {
    serde_json::to_string(snapshot).unwrap_or_default()
}

/// A command received from the UI via WebSocket.
#[derive(Debug, Deserialize)]
pub struct Command {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub action: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Attempt to parse a JSON text message as a command from the UI.
pub fn decode_command(text: &str) -> Option<Command> {
    let command: Command = serde_json::from_str(text).ok()?;
    if command.msg_type == "command" {
        Some(command)
    } else {
        None
    }
}

pub fn decode_episode_command(command: &Command) -> Option<EpisodeCommand> {
    match command.action.as_str() {
        "episode_start" => Some(EpisodeCommand::Start),
        "episode_stop" => Some(EpisodeCommand::Stop),
        "episode_keep" => Some(EpisodeCommand::Keep),
        "episode_discard" => Some(EpisodeCommand::Discard),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollio_types::messages::EpisodeState;

    #[test]
    fn encode_episode_status_uses_expected_json_shape() {
        let json = encode_episode_status(&EpisodeStatus {
            state: EpisodeState::Recording,
            episode_count: 2,
            elapsed_ms: 5_000,
        });
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("episode status should be valid JSON");
        assert_eq!(value["type"], "episode_status");
        assert_eq!(value["state"], "recording");
        assert_eq!(value["episode_count"], 2);
        assert_eq!(value["elapsed_ms"], 5_000);
    }

    #[test]
    fn decode_episode_command_maps_ui_actions() {
        let command = decode_command(r#"{"type":"command","action":"episode_keep"}"#)
            .expect("command should parse");
        assert_eq!(decode_episode_command(&command), Some(EpisodeCommand::Keep));
    }

    #[test]
    fn encode_robot_state_includes_optional_end_effector_fields() {
        let mut state = RobotState {
            timestamp_ns: 123,
            num_joints: 1,
            ..RobotState::default()
        };
        state.positions[0] = 0.042;
        state.velocities[0] = -0.1;
        state.efforts[0] = 1.25;
        state.has_end_effector_status = true;
        state.end_effector_status = EndEffectorStatus::Enabled;
        state.end_effector_feedback_valid = true;

        let json = encode_robot_state("eef_g2", &state);
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("robot state should be valid JSON");
        assert_eq!(value["type"], "robot_state");
        assert_eq!(value["name"], "eef_g2");
        assert_eq!(value["positions"][0], 0.042);
        assert_eq!(value["end_effector_status"], "enabled");
        assert_eq!(value["end_effector_feedback_valid"], true);
    }

    #[test]
    fn encode_robot_state_omits_optional_end_effector_fields_for_regular_robots() {
        let mut state = RobotState {
            timestamp_ns: 456,
            num_joints: 6,
            ..RobotState::default()
        };
        state.positions[..6].copy_from_slice(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);

        let json = encode_robot_state("leader_arm", &state);
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("robot state should be valid JSON");
        assert_eq!(value["type"], "robot_state");
        assert!(value.get("end_effector_status").is_none());
        assert!(value.get("end_effector_feedback_valid").is_none());
    }
}
