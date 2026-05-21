//! Per-camera packet stream buffer used by the assembler.
//!
//! The assembler subscribes to two iceoryx2 topics per camera:
//!
//! * `…/recording-config` — one `EncodedPacketHeader` with `kind =
//!   Config` per session-open. Carries codec extradata (Annex B SPS/PPS
//!   for H.264/H.265, AV1 sequence header, RVL container preamble).
//!   Opened with `history_size = 1` so the assembler can subscribe
//!   after the encoder has started and still pick up the cached config.
//! * `…/recording-packets` — `kind = Packet` per encoded access unit
//!   plus a terminating `kind = EndOfStream` per episode.
//!
//! The assembler folds those into a `RecordingStreamBuffer` per
//! `(channel_id, episode_index)` pair, validates sequence numbers,
//! and feeds the result into the muxer when EOS arrives.

use rollio_types::config::EncoderCodec;
#[allow(unused_imports)]
use rollio_types::messages::EncodedPacketKind;
use rollio_types::messages::{EncodedCodecId, EncodedPacketHeader, PixelFormat};
use std::error::Error;
use std::fmt;

/// Codec configuration parsed from a `Config` packet. The muxer uses
/// `extradata` to populate AVCodecParameters / RVL preamble bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedStreamConfig {
    pub codec: EncoderCodec,
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
    pub time_base_num: u32,
    pub time_base_den: u32,
    pub extradata: Vec<u8>,
}

impl EncodedStreamConfig {
    pub fn from_header(header: &EncodedPacketHeader, extradata: &[u8]) -> Self {
        Self {
            codec: codec_from_id(header.codec),
            width: header.width,
            height: header.height,
            pixel_format: header.pixel_format,
            time_base_num: header.time_base_num,
            time_base_den: header.time_base_den,
            extradata: extradata.to_vec(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EncodedPacketRecord {
    pub header: EncodedPacketHeader,
    pub payload: Vec<u8>,
}

/// Per-camera, per-episode packet accumulator. Sequence numbers are
/// validated as packets arrive; an out-of-order or missing seq tags
/// the buffer as failed and the assembler removes the in-flight
/// episode artifacts.
#[derive(Debug, Clone, Default)]
pub struct RecordingStreamBuffer {
    pub config: Option<EncodedStreamConfig>,
    pub packets: Vec<EncodedPacketRecord>,
    pub eos_received: bool,
    pub failed: Option<String>,
    pub seen_keyframe: bool,
    last_sequence: Option<u64>,
}

impl RecordingStreamBuffer {
    pub fn observe_config(&mut self, header: &EncodedPacketHeader, extradata: &[u8]) {
        if self.failed.is_some() {
            return;
        }
        if let Err(error) = self.update_sequence(header.sequence_number) {
            self.failed = Some(error.to_string());
            return;
        }
        let new_config = EncodedStreamConfig::from_header(header, extradata);
        if let Some(existing) = &self.config {
            if existing != &new_config {
                self.failed = Some(format!(
                    "config changed mid-recording: old codec={:?}, new codec={:?}",
                    existing.codec, new_config.codec
                ));
                return;
            }
        }
        self.config = Some(new_config);
    }

    pub fn observe_packet(&mut self, header: &EncodedPacketHeader, payload: &[u8]) {
        if self.failed.is_some() {
            return;
        }
        if !self.seen_keyframe {
            if !header.is_keyframe() {
                return;
            }
            self.seen_keyframe = true;
        }
        if let Err(error) = self.update_sequence(header.sequence_number) {
            self.failed = Some(error.to_string());
            return;
        }
        self.packets.push(EncodedPacketRecord {
            header: *header,
            payload: payload.to_vec(),
        });
    }

    pub fn observe_eos(&mut self, header: &EncodedPacketHeader) {
        if self.failed.is_some() {
            return;
        }
        if let Err(error) = self.update_sequence(header.sequence_number) {
            self.failed = Some(error.to_string());
            return;
        }
        self.eos_received = true;
    }

    pub fn is_complete(&self) -> bool {
        self.failed.is_none() && self.eos_received && self.seen_keyframe
    }

    fn update_sequence(&mut self, sequence: u64) -> Result<(), SequenceError> {
        match self.last_sequence {
            None => {
                self.last_sequence = Some(sequence);
                Ok(())
            }
            Some(last) if sequence == last + 1 => {
                self.last_sequence = Some(sequence);
                Ok(())
            }
            Some(last) => Err(SequenceError {
                expected: last + 1,
                actual: sequence,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceError {
    pub expected: u64,
    pub actual: u64,
}

impl fmt::Display for SequenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "packet sequence gap: expected {}, got {}",
            self.expected, self.actual
        )
    }
}

impl Error for SequenceError {}

pub fn codec_from_id(id: EncodedCodecId) -> EncoderCodec {
    match id {
        EncodedCodecId::H264 => EncoderCodec::H264,
        EncodedCodecId::H265 => EncoderCodec::H265,
        EncodedCodecId::Av1 => EncoderCodec::Av1,
        EncodedCodecId::Rvl => EncoderCodec::Rvl,
        EncodedCodecId::Mjpg => EncoderCodec::Mjpg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollio_types::messages::EncodedCodecId;

    fn header(kind: EncodedPacketKind, seq: u64) -> EncodedPacketHeader {
        EncodedPacketHeader {
            kind,
            codec: EncodedCodecId::H264,
            flags: 0,
            width: 96,
            height: 64,
            pixel_format: PixelFormat::Rgb24,
            _reserved0: 0,
            time_base_num: 1,
            time_base_den: 1_000_000,
            pts_us: 0,
            dts_us: 0,
            duration_us: 0,
            sequence_number: seq,
            source_timestamp_us: 0,
            source_frame_index: 0,
            episode_index: 1,
            payload_len: 0,
        }
    }

    #[test]
    fn in_order_sequences_accepted() {
        let mut buf = RecordingStreamBuffer::default();
        buf.observe_packet(&header(EncodedPacketKind::Packet, 0), b"frame_a");
        buf.observe_packet(&header(EncodedPacketKind::Packet, 1), b"frame_b");
        buf.observe_eos(&header(EncodedPacketKind::EndOfStream, 2));
        assert!(buf.failed.is_none());
        assert!(buf.is_complete());
        assert_eq!(buf.packets.len(), 2);
    }

    #[test]
    fn sequence_gap_marks_buffer_failed() {
        let mut buf = RecordingStreamBuffer::default();
        buf.observe_packet(&header(EncodedPacketKind::Packet, 0), b"frame_a");
        buf.observe_packet(&header(EncodedPacketKind::Packet, 2), b"frame_skip");
        assert!(buf.failed.is_some(), "gap must mark the buffer as failed");
    }

    #[test]
    fn out_of_order_marks_buffer_failed() {
        let mut buf = RecordingStreamBuffer::default();
        buf.observe_packet(&header(EncodedPacketKind::Packet, 0), b"frame_a");
        buf.observe_packet(&header(EncodedPacketKind::Packet, 0), b"duplicate");
        assert!(buf.failed.is_some());
    }

    #[test]
    fn buffer_incomplete_until_eos_arrives() {
        let mut buf = RecordingStreamBuffer::default();
        buf.observe_packet(&header(EncodedPacketKind::Packet, 0), b"frame_a");
        assert!(!buf.is_complete());
        buf.observe_eos(&header(EncodedPacketKind::EndOfStream, 1));
        assert!(buf.is_complete());
    }
}
