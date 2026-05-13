//! Annex B H.264 passthrough.
//!
//! For cameras that publish pre-encoded H.264 directly, decoding and
//! re-encoding wastes CPU/GPU. This backend forwards the camera's NAL
//! units verbatim to the sink, rewriting only the per-packet envelope
//! fields (PTS, sequence number, source timestamp) that the encoder
//! protocol expects to be stable across the wire.
//!
//! Constraints:
//! - Output codec must be H.264 (`ColorCodec::H264`). Codec mismatch
//!   (camera publishes H.264 but config requests H.265) is *not* this
//!   backend's job; it returns false from `supports()` so the
//!   registry's `Auto` walk falls through to NVIDIA / CPU for the
//!   transcode.
//! - **No scaling.** The session's output dims must equal the
//!   first-frame source dims; mismatch is a hard error at session
//!   open. This is the architectural promise that propagates through
//!   the visualizer's `scaling_locked` flag in phase 5.
//!
//! Wire shape of one Annex B AU from a typical camera encoder
//! configured without `GLOBAL_HEADER`:
//!
//! ```text
//! Keyframe AU: [start][SPS][start][PPS][start][SEI…][start][IDR slice]
//! Delta AU:    [start][slice]                       (+ optional SEI)
//! ```
//!
//! On the first keyframe we extract SPS (NAL type 7) and PPS (NAL
//! type 8) bytes, ship them once via `write_config` (matching the
//! LibavCodecSession protocol so the visualizer's existing
//! cached-config plumbing works unchanged), and forward the remaining
//! NAL units as the packet payload. Subsequent keyframes also carry
//! in-band SPS/PPS in their AU; we leave them in place since the
//! visualizer's prepend-on-keyframe logic tolerates duplicates and a
//! truly minimal-touch passthrough is easier to reason about than
//! per-frame NAL surgery.

use rollio_types::config::EncoderCodec;
use rollio_types::messages::{EncodedCodecId, EncodedPacketHeader, EncodedPacketKind, PixelFormat};

use super::{ColorBackendId, ColorCodec, ColorEncoderBackend};
use crate::codec::{CodecSession, CodecSessionParams, EncodedPacketSink, OwnedFrame};
use crate::error::{EncoderError, Result};
use crate::media::EncodeMetrics;

pub struct PassthroughBackend;

impl ColorEncoderBackend for PassthroughBackend {
    fn id(&self) -> ColorBackendId {
        ColorBackendId::Passthrough
    }

    fn priority(&self) -> u32 {
        // Highest priority under `Auto`. The `supports()` gate is
        // strict (only `H264 + H264AnnexB`), so this only "wins" when
        // the camera is already producing what the config requested.
        // Anything else falls through to the libav backends.
        1000
    }

    fn available(&self) -> bool {
        // Pure-Rust byte relay; nothing to probe.
        true
    }

    fn supports(&self, codec: ColorCodec, input: PixelFormat) -> bool {
        codec == ColorCodec::H264 && input == PixelFormat::H264AnnexB
    }

    fn open_session(
        &self,
        params: &CodecSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Box<dyn CodecSession>> {
        if params.codec != EncoderCodec::H264 {
            return Err(EncoderError::message(format!(
                "passthrough backend requires codec=h264, got {}",
                params.codec.as_str()
            )));
        }
        if first_frame.header.pixel_format != PixelFormat::H264AnnexB {
            return Err(EncoderError::message(format!(
                "passthrough backend requires input pixel_format=h264-annex-b, got {:?}",
                first_frame.header.pixel_format
            )));
        }
        // Architectural promise: passthrough cannot rescale. The
        // upstream runtimes (recording_runtime / preview_runtime) must
        // pass the source dims as the configured output dims.
        if params.output_width != first_frame.header.width
            || params.output_height != first_frame.header.height
        {
            return Err(EncoderError::message(format!(
                "passthrough backend cannot rescale: source dims {}x{} != configured output dims {}x{}. \
                 Set the preview/recording dims equal to the camera's native dims when using a \
                 pre-encoded source, or pick a libav backend to transcode.",
                first_frame.header.width,
                first_frame.header.height,
                params.output_width,
                params.output_height,
            )));
        }

        Ok(Box::new(PassthroughCodecSession {
            width: params.output_width,
            height: params.output_height,
            episode_index: params.episode_index,
            recording_start_us: params.recording_start_us,
            config_sent: false,
            next_sequence: 0,
            metrics: EncodeMetrics::default(),
        }))
    }
}

/// Bytes-verbatim H.264 codec session. Splits the first keyframe's
/// SPS/PPS NAL units out for a one-shot `write_config`; forwards every
/// AU (including any subsequent in-band SPS/PPS) as a `Packet`.
struct PassthroughCodecSession {
    width: u32,
    height: u32,
    episode_index: u32,
    recording_start_us: u64,
    config_sent: bool,
    next_sequence: u64,
    metrics: EncodeMetrics,
}

impl PassthroughCodecSession {
    /// Locate SPS (NAL type 7) + PPS (NAL type 8) NAL units in an
    /// Annex B byte slice and rebuild them into a single
    /// start-code-prefixed buffer suitable for the `Config` packet.
    /// Returns `None` if either is missing — the caller postpones the
    /// `Config` write until a frame with both arrives.
    fn extract_sps_pps(data: &[u8]) -> Option<Vec<u8>> {
        let nalus = split_annex_b_nalus(data);
        let mut sps: Option<&[u8]> = None;
        let mut pps: Option<&[u8]> = None;
        for nalu in nalus {
            if nalu.is_empty() {
                continue;
            }
            match nalu[0] & 0x1F {
                7 => sps.get_or_insert(nalu),
                8 => pps.get_or_insert(nalu),
                _ => continue,
            };
        }
        let sps = sps?;
        let pps = pps?;
        let mut out = Vec::with_capacity(4 + sps.len() + 4 + pps.len());
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        out.extend_from_slice(sps);
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        out.extend_from_slice(pps);
        Some(out)
    }

    /// Detect whether an Annex B AU contains an IDR slice (NAL type
    /// 5). Falls back to checking for SPS/PPS as a weaker signal —
    /// some camera encoders emit SPS+PPS only at keyframes, so seeing
    /// either is a strong hint we're at an IDR boundary.
    fn is_keyframe(data: &[u8]) -> bool {
        let nalus = split_annex_b_nalus(data);
        for nalu in nalus {
            if nalu.is_empty() {
                continue;
            }
            let nal_type = nalu[0] & 0x1F;
            if matches!(nal_type, 5 | 7 | 8) {
                return true;
            }
        }
        false
    }

    fn build_header(
        &self,
        kind: EncodedPacketKind,
        frame: &OwnedFrame,
        pts_us: i64,
        sequence: u64,
        payload_len: u32,
        keyframe: bool,
    ) -> EncodedPacketHeader {
        let mut header = EncodedPacketHeader {
            kind,
            codec: EncodedCodecId::H264,
            flags: 0,
            width: self.width,
            height: self.height,
            pixel_format: PixelFormat::H264AnnexB,
            _reserved0: 0,
            time_base_num: 1,
            time_base_den: 1_000_000,
            pts_us,
            dts_us: pts_us,
            duration_us: 0,
            sequence_number: sequence,
            source_timestamp_us: frame.header.timestamp_us,
            source_frame_index: frame.header.frame_index,
            episode_index: self.episode_index,
            payload_len,
        };
        header.set_keyframe(keyframe);
        header
    }
}

impl CodecSession for PassthroughCodecSession {
    fn encode(&mut self, frame: &OwnedFrame, sink: &mut dyn EncodedPacketSink) -> Result<()> {
        if frame.header.pixel_format != PixelFormat::H264AnnexB {
            return Err(EncoderError::message(format!(
                "passthrough session received non-H264AnnexB frame: {:?}",
                frame.header.pixel_format
            )));
        }
        if frame.header.width != self.width || frame.header.height != self.height {
            return Err(EncoderError::message(format!(
                "passthrough session: source dim drift {}x{} -> {}x{} (passthrough cannot rescale)",
                self.width, self.height, frame.header.width, frame.header.height
            )));
        }

        // Send the codec-config (SPS+PPS) packet once, on the first
        // frame that carries both NAL units. A camera that emits SPS/
        // PPS only on keyframes will block here until its first IDR;
        // delta-only frames before the first IDR are dropped.
        if !self.config_sent {
            if let Some(extradata) = Self::extract_sps_pps(&frame.payload) {
                let header = self.build_header(
                    EncodedPacketKind::Config,
                    frame,
                    0,
                    self.next_sequence,
                    extradata.len() as u32,
                    false,
                );
                self.next_sequence += 1;
                sink.write_config(header, &extradata)?;
                self.config_sent = true;
            } else {
                // No SPS/PPS yet — drop the frame; the next keyframe
                // will populate the config and stream resumes.
                self.metrics.dropped_frames = self.metrics.dropped_frames.saturating_add(1);
                return Ok(());
            }
        }

        // Pass-through packet. Reuse the camera-side wall-clock
        // timestamp (now relative to `recording_start_us`, so PTS
        // monotonically increases from session start) for the codec
        // PTS; sequence numbers come from this session.
        let pts_us = frame
            .header
            .timestamp_us
            .saturating_sub(self.recording_start_us) as i64;
        let keyframe = Self::is_keyframe(&frame.payload);
        let header = self.build_header(
            EncodedPacketKind::Packet,
            frame,
            pts_us,
            self.next_sequence,
            frame.payload.len() as u32,
            keyframe,
        );
        self.next_sequence += 1;
        self.metrics.encoded_bytes += frame.payload.len();
        sink.write_packet(header, &frame.payload)?;
        Ok(())
    }

    fn finish(self: Box<Self>, sink: &mut dyn EncodedPacketSink) -> Result<()> {
        let header = EncodedPacketHeader {
            kind: EncodedPacketKind::EndOfStream,
            codec: EncodedCodecId::H264,
            flags: 0,
            width: self.width,
            height: self.height,
            pixel_format: PixelFormat::H264AnnexB,
            _reserved0: 0,
            time_base_num: 1,
            time_base_den: 1_000_000,
            pts_us: 0,
            dts_us: 0,
            duration_us: 0,
            sequence_number: self.next_sequence,
            source_timestamp_us: self.recording_start_us,
            source_frame_index: 0,
            episode_index: self.episode_index,
            payload_len: 0,
        };
        sink.write_eos(header)
    }

    fn metrics(&self) -> &EncodeMetrics {
        &self.metrics
    }

    fn record_dropped(&mut self) {
        self.metrics.dropped_frames = self.metrics.dropped_frames.saturating_add(1);
    }
}

/// Split an Annex B byte slice into its constituent NALU bodies
/// (start codes stripped). Handles both 3-byte (`0x000001`) and
/// 4-byte (`0x00000001`) start codes.
///
/// Local to the passthrough backend so its NAL surgery is self-
/// contained. The visualizer had a similar helper for the
/// (now-deleted) AVCC conversion; if a third caller needs it we can
/// promote to a shared `nal_b` module under `backend/`.
fn split_annex_b_nalus(bytes: &[u8]) -> Vec<&[u8]> {
    let mut starts: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == 0x00 && bytes[i + 1] == 0x00 {
            if bytes[i + 2] == 0x01 {
                starts.push((i, 3));
                i += 3;
                continue;
            }
            if i + 3 < bytes.len() && bytes[i + 2] == 0x00 && bytes[i + 3] == 0x01 {
                starts.push((i, 4));
                i += 4;
                continue;
            }
        }
        i += 1;
    }
    let mut out = Vec::with_capacity(starts.len());
    for (idx, &(offset, prefix)) in starts.iter().enumerate() {
        let body_start = offset + prefix;
        let body_end = if idx + 1 < starts.len() {
            starts[idx + 1].0
        } else {
            bytes.len()
        };
        if body_start <= body_end {
            out.push(&bytes[body_start..body_end]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollio_types::config::{ChromaSubsampling, EncoderBackend, EncoderColorSpace};
    use rollio_types::messages::CameraFrameHeader;

    fn frame(width: u32, height: u32, payload: Vec<u8>, ts_us: u64) -> OwnedFrame {
        OwnedFrame {
            header: CameraFrameHeader {
                timestamp_us: ts_us,
                width,
                height,
                pixel_format: PixelFormat::H264AnnexB,
                frame_index: 0,
            },
            payload,
        }
    }

    fn params<'a>(process_id: &'a str, width: u32, height: u32) -> CodecSessionParams<'a> {
        CodecSessionParams {
            codec: EncoderCodec::H264,
            backend: EncoderBackend::Passthrough,
            fps: 30,
            crf: None,
            preset: None,
            tune: None,
            bit_depth: 8,
            chroma_subsampling: ChromaSubsampling::S420,
            color_space: EncoderColorSpace::Auto,
            process_id,
            episode_index: 0,
            recording_start_us: 0,
            output_width: width,
            output_height: height,
            allow_rescale: false,
        }
    }

    /// Capture every sink call so tests can assert the order and
    /// contents (config bytes, packet bytes, keyframe flag).
    #[derive(Default)]
    struct CaptureSink {
        calls: Vec<SinkCall>,
    }

    #[derive(Debug)]
    enum SinkCall {
        Config { header: EncodedPacketHeader, extradata: Vec<u8> },
        Packet { header: EncodedPacketHeader, payload: Vec<u8> },
        Eos { header: EncodedPacketHeader },
    }

    impl EncodedPacketSink for CaptureSink {
        fn write_config(
            &mut self,
            header: EncodedPacketHeader,
            extradata: &[u8],
        ) -> Result<()> {
            self.calls.push(SinkCall::Config {
                header,
                extradata: extradata.to_vec(),
            });
            Ok(())
        }
        fn write_packet(
            &mut self,
            header: EncodedPacketHeader,
            payload: &[u8],
        ) -> Result<()> {
            self.calls.push(SinkCall::Packet {
                header,
                payload: payload.to_vec(),
            });
            Ok(())
        }
        fn write_eos(&mut self, header: EncodedPacketHeader) -> Result<()> {
            self.calls.push(SinkCall::Eos { header });
            Ok(())
        }
    }

    fn synthetic_keyframe_au() -> Vec<u8> {
        let mut au = Vec::new();
        // SPS NAL (type 7): nal_ref_idc=3, nal_type=7 → 0x67
        au.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        au.extend_from_slice(&[0x67, 0x42, 0xC0, 0x1E, 0xAA, 0xBB]);
        // PPS NAL (type 8): 0x68
        au.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        au.extend_from_slice(&[0x68, 0xCE, 0x3C, 0x80]);
        // IDR slice (type 5): 0x65
        au.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        au.extend_from_slice(&[0x65, 0x88, 0x88, 0x80, 0x00, 0x10]);
        au
    }

    fn synthetic_delta_au() -> Vec<u8> {
        let mut au = Vec::new();
        // P slice (type 1): 0x41
        au.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        au.extend_from_slice(&[0x41, 0x9A, 0x00, 0x42]);
        au
    }

    #[test]
    fn open_session_rejects_codec_mismatch() {
        let backend = PassthroughBackend;
        let mut p = params("test", 1920, 1080);
        p.codec = EncoderCodec::H265;
        let f = frame(1920, 1080, synthetic_keyframe_au(), 1_000_000);
        let err = match backend.open_session(&p, &f) {
            Ok(_) => panic!("h265 must not pass passthrough"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("codec=h264"));
    }

    #[test]
    fn open_session_rejects_scaling() {
        let backend = PassthroughBackend;
        let p = params("test", 1280, 720); // != 1920x1080
        let f = frame(1920, 1080, synthetic_keyframe_au(), 1_000_000);
        let err = match backend.open_session(&p, &f) {
            Ok(_) => panic!("dim mismatch must not pass passthrough"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("cannot rescale"));
    }

    #[test]
    fn config_then_packet_then_eos_forwards_bytes_verbatim() {
        let backend = PassthroughBackend;
        let p = params("test", 1920, 1080);
        let f = frame(1920, 1080, synthetic_keyframe_au(), 1_000_000);
        let mut session = backend.open_session(&p, &f).expect("open");
        let mut sink = CaptureSink::default();

        session.encode(&f, &mut sink).expect("encode keyframe");
        let delta = frame(1920, 1080, synthetic_delta_au(), 1_033_000);
        session.encode(&delta, &mut sink).expect("encode delta");
        session.finish(&mut sink).expect("finish");

        // Order: Config, Packet (keyframe), Packet (delta), EOS.
        assert!(matches!(sink.calls[0], SinkCall::Config { .. }));
        match &sink.calls[0] {
            SinkCall::Config {
                header,
                extradata,
            } => {
                assert_eq!(header.codec, EncodedCodecId::H264);
                assert_eq!(header.width, 1920);
                assert_eq!(header.height, 1080);
                assert_eq!(extradata.len() as u32, header.payload_len);
                // Should contain SPS and PPS, each prefixed with the
                // 4-byte Annex B start code.
                assert!(extradata.starts_with(&[0x00, 0x00, 0x00, 0x01]));
                assert!(extradata.windows(5).any(|w| w == [0x00, 0x00, 0x00, 0x01, 0x67]));
                assert!(extradata.windows(5).any(|w| w == [0x00, 0x00, 0x00, 0x01, 0x68]));
            }
            _ => unreachable!(),
        }
        match &sink.calls[1] {
            SinkCall::Packet { header, payload } => {
                assert!(header.is_keyframe(), "first packet must be keyframe");
                assert_eq!(payload, &synthetic_keyframe_au(),
                    "passthrough must forward the AU bytes verbatim");
            }
            other => panic!("expected packet, got {other:?}"),
        }
        match &sink.calls[2] {
            SinkCall::Packet { header, payload } => {
                assert!(!header.is_keyframe(), "second packet must not be keyframe");
                assert_eq!(payload, &synthetic_delta_au());
            }
            other => panic!("expected delta packet, got {other:?}"),
        }
        assert!(matches!(sink.calls[3], SinkCall::Eos { .. }));
    }

    #[test]
    fn delta_before_first_keyframe_is_dropped_until_sps_arrives() {
        let backend = PassthroughBackend;
        let p = params("test", 1920, 1080);
        let first_delta = frame(1920, 1080, synthetic_delta_au(), 1_000_000);
        let mut session = backend.open_session(&p, &first_delta).expect("open");
        let mut sink = CaptureSink::default();

        // A delta-only AU before SPS arrives must not surface a
        // Packet (the visualizer would have nothing to decode it
        // against). Session silently drops and waits for the next
        // keyframe.
        session.encode(&first_delta, &mut sink).expect("encode");
        assert!(sink.calls.is_empty(),
            "delta before SPS/PPS must not emit Config or Packet");

        let key = frame(1920, 1080, synthetic_keyframe_au(), 1_033_000);
        session.encode(&key, &mut sink).expect("encode keyframe");
        assert_eq!(sink.calls.len(), 2);
        assert!(matches!(sink.calls[0], SinkCall::Config { .. }));
        match &sink.calls[1] {
            SinkCall::Packet { header, .. } => {
                assert!(header.is_keyframe());
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn mid_stream_dim_drift_errors() {
        let backend = PassthroughBackend;
        let p = params("test", 1920, 1080);
        let key = frame(1920, 1080, synthetic_keyframe_au(), 1_000_000);
        let mut session = backend.open_session(&p, &key).expect("open");
        let mut sink = CaptureSink::default();
        session.encode(&key, &mut sink).expect("encode keyframe");

        // Camera changes resolution mid-stream — passthrough cannot
        // adapt.
        let resized = frame(1280, 720, synthetic_keyframe_au(), 1_033_000);
        let err = session
            .encode(&resized, &mut sink)
            .err()
            .expect("dim drift must error");
        assert!(err.to_string().contains("cannot rescale"));
    }
}
