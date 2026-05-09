//! WebSocket protocol encoding/decoding for the visualizer bridge.
//!
//! Three binary message kinds are sent to the UI:
//!
//! ```text
//! 0x01 JPEG_FRAME        — legacy JPEG path (jpeg output mode)
//! 0x02 ENCODED_CONFIG    — codec config (encoded output mode)
//! 0x03 ENCODED_PACKET    — encoded access unit (encoded output mode)
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
pub const KIND_ENCODED_CONFIG: u8 = 0x02;
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

/// Encode a codec stream-config message (preview output mode =
/// encoded). The body carries the codec id, dims, and the AVCC bytes
/// that the web UI hands to `VideoDecoder.configure({description})`.
///
/// Body layout (after the common name header):
///
/// ```text
/// [0]      codec id (matches EncodedCodecId discriminant value)
/// [1..5]   width (u32 LE)
/// [5..9]   height (u32 LE)
/// [9..13]  avcc len (u32 LE)
/// [13..]   avcc bytes
/// ```
pub fn encode_stream_config(
    name: &str,
    codec_id: u8,
    width: u32,
    height: u32,
    avcc: &[u8],
) -> Vec<u8> {
    let name_bytes = name.as_bytes();
    let total = 1 + 2 + name_bytes.len() + 1 + 4 + 4 + 4 + avcc.len();
    let mut buf = Vec::with_capacity(total);
    buf.push(KIND_ENCODED_CONFIG);
    buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    buf.extend_from_slice(name_bytes);
    buf.push(codec_id);
    buf.extend_from_slice(&width.to_le_bytes());
    buf.extend_from_slice(&height.to_le_bytes());
    buf.extend_from_slice(&(avcc.len() as u32).to_le_bytes());
    buf.extend_from_slice(avcc);
    buf
}

/// Encode an encoded preview packet (preview output mode = encoded).
///
/// Body layout (after the common name header):
///
/// ```text
/// [0]      codec id
/// [1]      flags (bit 0 = keyframe)
/// [2..10]  pts (u64 LE, microseconds)
/// [10..18] sequence (u64 LE)
/// [18..22] payload len (u32 LE)
/// [22..]   payload bytes (Annex B AU for h264, etc.)
/// ```
#[allow(clippy::too_many_arguments)]
pub fn encode_packet(
    name: &str,
    codec_id: u8,
    flags: u8,
    pts_us: i64,
    sequence: u64,
    payload: &[u8],
) -> Vec<u8> {
    let name_bytes = name.as_bytes();
    let total = 1 + 2 + name_bytes.len() + 1 + 1 + 8 + 8 + 4 + payload.len();
    let mut buf = Vec::with_capacity(total);
    buf.push(KIND_ENCODED_PACKET);
    buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    buf.extend_from_slice(name_bytes);
    buf.push(codec_id);
    buf.push(flags);
    buf.extend_from_slice(&(pts_us as u64).to_le_bytes());
    buf.extend_from_slice(&sequence.to_le_bytes());
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

// ---------------------------------------------------------------------------
// Annex B -> AVCC conversion (H.264 only)
//
// WebCodecs `VideoDecoder.configure({description})` expects the AVCC
// configuration record (also called avcDecoderConfigurationRecord),
// not raw Annex B SPS/PPS. The encoder ships SPS+PPS as Annex B
// extradata; this helper converts them once at session-open time so
// the web UI can configure its decoder with the bytes verbatim.
// ---------------------------------------------------------------------------

/// Convert Annex B SPS+PPS extradata into the AVCC configuration
/// record format. Returns `None` if SPS or PPS NALUs cannot be found
/// in the input.
pub fn annex_b_to_avcc(extradata: &[u8]) -> Option<Vec<u8>> {
    let nalus = split_annex_b_nalus(extradata);
    let mut sps: Option<&[u8]> = None;
    let mut pps: Option<&[u8]> = None;
    for nalu in nalus {
        if nalu.is_empty() {
            continue;
        }
        let nal_type = nalu[0] & 0x1F;
        match nal_type {
            7 => sps.get_or_insert(nalu),
            8 => pps.get_or_insert(nalu),
            _ => continue,
        };
    }
    let sps = sps?;
    let pps = pps?;
    if sps.len() < 4 {
        return None;
    }
    let mut avcc = Vec::with_capacity(7 + 2 + sps.len() + 1 + 2 + pps.len());
    avcc.push(0x01); // configurationVersion
    avcc.push(sps[1]); // AVCProfileIndication
    avcc.push(sps[2]); // profile_compatibility
    avcc.push(sps[3]); // AVCLevelIndication
    avcc.push(0xFF); // reserved (6 bits) | lengthSizeMinusOne (2 bits, 0xFC | 3)
    avcc.push(0xE1); // reserved (3 bits) | numOfSequenceParameterSets (5 bits, 1)
    avcc.extend_from_slice(&(sps.len() as u16).to_be_bytes());
    avcc.extend_from_slice(sps);
    avcc.push(0x01); // numOfPictureParameterSets
    avcc.extend_from_slice(&(pps.len() as u16).to_be_bytes());
    avcc.extend_from_slice(pps);
    Some(avcc)
}

/// Split an Annex B byte slice into its constituent NALU bodies
/// (start codes stripped). Handles both 3-byte (`0x000001`) and
/// 4-byte (`0x00000001`) start codes.
fn split_annex_b_nalus(data: &[u8]) -> Vec<&[u8]> {
    let mut nalus = Vec::new();
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 2 < data.len() {
        if data[i] == 0x00 && data[i + 1] == 0x00 {
            if data[i + 2] == 0x01 {
                starts.push((i, 3));
                i += 3;
                continue;
            } else if i + 3 < data.len() && data[i + 2] == 0x00 && data[i + 3] == 0x01 {
                starts.push((i, 4));
                i += 4;
                continue;
            }
        }
        i += 1;
    }
    for (idx, (start_offset, code_len)) in starts.iter().enumerate() {
        let body_start = start_offset + code_len;
        let body_end = if idx + 1 < starts.len() {
            starts[idx + 1].0
        } else {
            data.len()
        };
        if body_start <= body_end {
            nalus.push(&data[body_start..body_end]);
        }
    }
    nalus
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
    fn encoded_config_carries_codec_id_and_avcc() {
        let bytes = encode_stream_config("cam1", 0, 320, 240, &[0x01, 0x02]);
        assert_eq!(bytes[0], KIND_ENCODED_CONFIG);
        let name_len = u16::from_le_bytes([bytes[1], bytes[2]]) as usize;
        let body = &bytes[3 + name_len..];
        assert_eq!(body[0], 0); // codec id (H.264 = 0)
        let width = u32::from_le_bytes([body[1], body[2], body[3], body[4]]);
        assert_eq!(width, 320);
        let avcc_len = u32::from_le_bytes([body[9], body[10], body[11], body[12]]) as usize;
        assert_eq!(avcc_len, 2);
        assert_eq!(&body[13..13 + avcc_len], &[0x01, 0x02]);
    }

    #[test]
    fn encoded_packet_carries_flags_and_seq() {
        let bytes = encode_packet("cam1", 0, 1, 1234, 7, b"AU");
        assert_eq!(bytes[0], KIND_ENCODED_PACKET);
        let name_len = u16::from_le_bytes([bytes[1], bytes[2]]) as usize;
        let body = &bytes[3 + name_len..];
        assert_eq!(body[0], 0); // codec id
        assert_eq!(body[1], 1); // flags = keyframe
        let seq = u64::from_le_bytes([
            body[10], body[11], body[12], body[13], body[14], body[15], body[16], body[17],
        ]);
        assert_eq!(seq, 7);
    }

    #[test]
    fn avcc_conversion_recovers_sps_pps_from_annex_b() {
        let mut extradata = Vec::new();
        extradata.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]); // start
        extradata.extend_from_slice(&[0x67, 0x42, 0xC0, 0x1F]); // SPS NAL: type=7, profile/level bytes
        extradata.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]); // start
        extradata.extend_from_slice(&[0x68, 0xCE, 0x3C, 0x80]); // PPS NAL: type=8
        let avcc = annex_b_to_avcc(&extradata).expect("avcc conversion");
        assert_eq!(avcc[0], 0x01); // configurationVersion
        assert_eq!(avcc[1], 0x42); // profile
        assert_eq!(avcc[2], 0xC0); // profile_compat
        assert_eq!(avcc[3], 0x1F); // level
        assert_eq!(avcc[4], 0xFF); // reserved | lengthSizeMinusOne
        assert_eq!(avcc[5], 0xE1); // reserved | numSps=1
        let sps_len = u16::from_be_bytes([avcc[6], avcc[7]]) as usize;
        assert_eq!(sps_len, 4);
        assert_eq!(&avcc[8..12], &[0x67, 0x42, 0xC0, 0x1F]);
        assert_eq!(avcc[12], 0x01); // numPps=1
        let pps_len = u16::from_be_bytes([avcc[13], avcc[14]]) as usize;
        assert_eq!(pps_len, 4);
        assert_eq!(&avcc[15..19], &[0x68, 0xCE, 0x3C, 0x80]);
    }

    #[test]
    fn avcc_conversion_returns_none_for_missing_pps() {
        let mut extradata = Vec::new();
        extradata.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        extradata.extend_from_slice(&[0x67, 0x42, 0xC0, 0x1F]);
        assert!(annex_b_to_avcc(&extradata).is_none());
    }
}
