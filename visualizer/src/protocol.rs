/// WebSocket binary/JSON protocol encoding and decoding.
///
/// Binary messages (Visualizer → UI) — camera frames:
///   Byte 0:      frame encoding type (0x01 = JPEG)
///   Bytes 1-2:   camera name length (u16 LE)
///   Bytes 3..N:  camera name (UTF-8)
///   Bytes N+1..N+8:   source timestamp_us (u64 LE)
///   Bytes N+9..N+16:  source frame_index (u64 LE)
///   Bytes N+17..N+20: encoded preview width (u32 LE)
///   Bytes N+21..N+24: encoded preview height (u32 LE)
///   Remaining:   encoded frame data (JPEG payload)
///
/// Text/JSON messages (both directions):
///   Visualizer → UI: {"type":"robot_state","name":"...","timestamp_us":...,...}
///   Visualizer → UI: {"type":"stream_info",...}
///   UI → Visualizer:  {"type":"command","action":"set_preview_size","width":...,"height":...}
use serde::{Deserialize, Serialize};

use crate::stream_info::StreamInfoSnapshot;

/// Frame encoding type tags.
pub const FRAME_TYPE_JPEG: u8 = 0x01;

/// Encode a camera frame into the binary WebSocket protocol.
///
/// Pre-allocates the exact output capacity to avoid reallocation.
pub fn encode_camera_frame(
    name: &str,
    timestamp_us: u64,
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
    buf.extend_from_slice(&timestamp_us.to_le_bytes());
    buf.extend_from_slice(&frame_index.to_le_bytes());
    buf.extend_from_slice(&width.to_le_bytes());
    buf.extend_from_slice(&height.to_le_bytes());
    buf.extend_from_slice(jpeg_data);

    buf
}

/// JSON message for a single robot state-kind sample sent to the UI.
///
/// The visualizer emits one of these per (channel, state_kind) update so the
/// UI can group them per channel and lay out joint position / velocity /
/// effort rows independently. `value_min` / `value_max` carry the device's
/// reported envelope (or empty arrays when the driver does not expose limits).
#[derive(Serialize)]
struct RobotStateJson<'a> {
    #[serde(rename = "type")]
    msg_type: &'static str,
    name: &'a str,
    timestamp_us: u64,
    num_joints: u32,
    /// Element values for the named `state_kind`. The field name is kept as
    /// `values` (not "positions") so it accurately describes velocity and
    /// effort payloads without misleading readers.
    values: &'a [f64],
    state_kind: &'a str,
    #[serde(skip_serializing_if = "<[f64]>::is_empty")]
    value_min: &'a [f64],
    #[serde(skip_serializing_if = "<[f64]>::is_empty")]
    value_max: &'a [f64],
}

/// Encode a robot state into a JSON string for WebSocket text message.
pub fn encode_robot_state(
    name: &str,
    timestamp_us: u64,
    values: &[f64],
    state_kind: &str,
    value_min: &[f64],
    value_max: &[f64],
) -> String {
    let msg = RobotStateJson {
        msg_type: "robot_state",
        name,
        timestamp_us,
        num_joints: values.len() as u32,
        values,
        state_kind,
        value_min,
        value_max,
    };
    serde_json::to_string(&msg).unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_command_parses_set_preview_size() {
        let cmd = decode_command(
            r#"{"type":"command","action":"set_preview_size","width":640,"height":480}"#,
        )
        .expect("set_preview_size should parse");
        assert_eq!(cmd.action, "set_preview_size");
        assert_eq!(cmd.width, Some(640));
        assert_eq!(cmd.height, Some(480));
    }

    #[test]
    fn decode_command_rejects_non_command_envelopes() {
        assert!(decode_command(r#"{"type":"setup_state","step":"devices"}"#).is_none());
    }

    #[test]
    fn encode_robot_state_includes_state_kind() {
        let json = encode_robot_state(
            "eef_g2",
            123,
            &[0.042],
            "parallel_position",
            &[0.0],
            &[0.07],
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("robot state should be valid JSON");
        assert_eq!(value["type"], "robot_state");
        assert_eq!(value["name"], "eef_g2");
        assert_eq!(value["values"][0], 0.042);
        assert_eq!(value["state_kind"], "parallel_position");
        assert_eq!(value["value_min"][0], 0.0);
        assert_eq!(value["value_max"][0], 0.07);
    }

    #[test]
    fn encode_robot_state_omits_value_envelope_when_not_set() {
        let json = encode_robot_state(
            "leader_arm",
            456,
            &[0.1, 0.2, 0.3, 0.4, 0.5, 0.6],
            "joint_position",
            &[],
            &[],
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("robot state should be valid JSON");
        assert_eq!(value["type"], "robot_state");
        assert_eq!(value["state_kind"], "joint_position");
        assert!(value.get("value_min").is_none());
        assert!(value.get("value_max").is_none());
    }
}
