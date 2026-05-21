//! WebSocket protocol encoding/decoding for the visualizer bridge.
//!
//! Two binary message kinds are sent to the UI:
//!
//! ```text
//! 0x01 JPEG_FRAME        — legacy JPEG path (jpeg output mode)
//! 0x03 ENCODED_PACKET    — encoded access unit (encoded output mode).
//!                          Self-contained Annex B AU; keyframes carry
//!                          inline SPS/PPS so the frontend's WebCodecs
//!                          decoder auto-configures from the first key
//!                          packet alone — no separate config message.
//! ```
//!
//! Common header layout for all three kinds:
//!
//! ```text
//! [0]            kind tag (one of the constants above)
//! [1..3]         camera name length (u16 LE)
//! [3..3+N]       camera name (UTF-8)
//! [3+N..]        kind-specific body (see fns below)
//! ```
//!
//! Text/JSON messages (both directions) carry stream metadata,
//! commands, and robot state — see `encode_robot_state` and
//! `encode_stream_info`.

use serde::{Deserialize, Serialize};

use crate::stream_info::StreamInfoSnapshot;

pub const KIND_JPEG_FRAME: u8 = 0x01;
pub const KIND_ENCODED_PACKET: u8 = 0x03;

/// Encode a JPEG frame (preview output mode = jpeg).
pub fn encode_jpeg_frame(
    name: &str,
    timestamp_us: u64,
    frame_index: u64,
    width: u32,
    height: u32,
    jpeg_data: &[u8],
) -> Vec<u8> {
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len();
    let total = 1 + 2 + name_len + 8 + 8 + 4 + 4 + jpeg_data.len();
    let mut buf = Vec::with_capacity(total);
    buf.push(KIND_JPEG_FRAME);
    buf.extend_from_slice(&(name_len as u16).to_le_bytes());
    buf.extend_from_slice(name_bytes);
    buf.extend_from_slice(&timestamp_us.to_le_bytes());
    buf.extend_from_slice(&frame_index.to_le_bytes());
    buf.extend_from_slice(&width.to_le_bytes());
    buf.extend_from_slice(&height.to_le_bytes());
    buf.extend_from_slice(jpeg_data);
    buf
}

/// Encode an encoded preview packet (preview output mode = encoded).
///
/// Body layout (after the common name header):
///
/// ```text
/// [0]      codec id
/// [1]      flags (bit 0 = keyframe)
/// [2..10]  pts (u64 LE, microseconds, monotonic from recording start)
/// [10..18] sequence (u64 LE)
/// [18..26] source_timestamp_us (u64 LE, camera capture wall-clock µs
///          since UNIX epoch) — for capture-to-display latency metrics
/// [26..30] width (u32 LE) — coded width, lets the frontend configure
///          its WebCodecs decoder from the first keyframe alone
/// [30..34] height (u32 LE)
/// [34..38] payload len (u32 LE)
/// [38..]   payload bytes (Annex B AU for h264, etc.)
/// ```
#[allow(clippy::too_many_arguments)]
pub fn encode_packet(
    name: &str,
    codec_id: u8,
    flags: u8,
    pts_us: i64,
    sequence: u64,
    source_timestamp_us: u64,
    width: u32,
    height: u32,
    payload: &[u8],
) -> Vec<u8> {
    let name_bytes = name.as_bytes();
    let total = 1 + 2 + name_bytes.len() + 1 + 1 + 8 + 8 + 8 + 4 + 4 + 4 + payload.len();
    let mut buf = Vec::with_capacity(total);
    buf.push(KIND_ENCODED_PACKET);
    buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    buf.extend_from_slice(name_bytes);
    buf.push(codec_id);
    buf.push(flags);
    buf.extend_from_slice(&(pts_us as u64).to_le_bytes());
    buf.extend_from_slice(&sequence.to_le_bytes());
    buf.extend_from_slice(&source_timestamp_us.to_le_bytes());
    buf.extend_from_slice(&width.to_le_bytes());
    buf.extend_from_slice(&height.to_le_bytes());
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(payload);
    buf
}

// ---------------------------------------------------------------------------
// Robot-state JSON
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct RobotStateJson<'a> {
    #[serde(rename = "type")]
    msg_type: &'static str,
    name: &'a str,
    timestamp_us: u64,
    num_joints: u32,
    values: &'a [f64],
    state_kind: &'a str,
    #[serde(skip_serializing_if = "<[f64]>::is_empty")]
    value_min: &'a [f64],
    #[serde(skip_serializing_if = "<[f64]>::is_empty")]
    value_max: &'a [f64],
}

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

pub fn encode_stream_info(snapshot: &StreamInfoSnapshot) -> String {
    serde_json::to_string(snapshot).unwrap_or_default()
}

#[derive(Debug, Deserialize)]
pub struct Command {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub action: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

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
    fn jpeg_frame_round_trips_basic_layout() {
        let bytes = encode_jpeg_frame("cam1", 1234, 5, 320, 240, &[0xFF, 0xD8, 0xFF, 0xD9]);
        assert_eq!(bytes[0], KIND_JPEG_FRAME);
        let name_len = u16::from_le_bytes([bytes[1], bytes[2]]) as usize;
        assert_eq!(&bytes[3..3 + name_len], b"cam1");
    }

    #[test]
    fn encoded_packet_carries_flags_and_seq() {
        let bytes = encode_packet(
            "cam1",
            0,
            1,
            1234,
            7,
            1_700_000_000_000_000,
            1920,
            1080,
            b"AU",
        );
        assert_eq!(bytes[0], KIND_ENCODED_PACKET);
        let name_len = u16::from_le_bytes([bytes[1], bytes[2]]) as usize;
        let body = &bytes[3 + name_len..];
        assert_eq!(body[0], 0); // codec id
        assert_eq!(body[1], 1); // flags = keyframe
        let seq = u64::from_le_bytes([
            body[10], body[11], body[12], body[13], body[14], body[15], body[16], body[17],
        ]);
        assert_eq!(seq, 7);
        let source_ts = u64::from_le_bytes([
            body[18], body[19], body[20], body[21], body[22], body[23], body[24], body[25],
        ]);
        assert_eq!(source_ts, 1_700_000_000_000_000);
        let width = u32::from_le_bytes([body[26], body[27], body[28], body[29]]);
        assert_eq!(width, 1920);
        let height = u32::from_le_bytes([body[30], body[31], body[32], body[33]]);
        assert_eq!(height, 1080);
        let payload_len = u32::from_le_bytes([body[34], body[35], body[36], body[37]]) as usize;
        assert_eq!(payload_len, 2);
        assert_eq!(&body[38..38 + payload_len], b"AU");
    }
}
