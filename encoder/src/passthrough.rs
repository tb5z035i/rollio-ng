//! H264 passthrough session. Mux'es Annex-B byte streams into MP4 without
//! re-encoding. Selected by [`media::open_session`] when the first frame's
//! `pixel_format` is `H264` (i.e. when the upstream device is the UMI
//! bridge republishing cora's `CompressedVideo` topics).
//!
//! Behaviour:
//! - On open, the session defers MP4 header writing until the first IDR
//!   frame with SPS/PPS is observed. SPS/PPS are extracted from the
//!   Annex-B byte stream and written into the output stream's `extradata`
//!   in AVCC `avcC` format so MP4 players can decode the resulting file.
//! - Per encode_frame, Annex-B start codes (`00 00 00 01` / `00 00 01`)
//!   are converted to 4-byte big-endian length prefixes before
//!   `av_interleaved_write_frame` so the resulting MP4 is AVCC-conformant.
//! - SPS (NAL 7) and PPS (NAL 8) are stripped from the packet bytes since
//!   they live in extradata.
//! - AUD (NAL 9) is stripped: each AVPacket is one access unit, so the
//!   delimiter is redundant and only bloats the file.
//! - SEI (NAL 6) is passed through.
//! - PTS in microseconds, anchored at `recording_start_us` like the libav
//!   path. IDR frames flag `AV_PKT_FLAG_KEY`.

use crate::error::{EncoderError, Result};
use crate::media::{EncodeMetrics, EncodedArtifact, OwnedFrame};
use ffmpeg_next as ffmpeg;
use rollio_types::config::{
    EncoderArtifactFormat, EncoderBackend, EncoderCodec, EncoderRuntimeConfigV2,
};
use rollio_types::messages::PixelFormat;
use std::path::PathBuf;

// Documented for completeness; only the IDR/SPS/PPS/AUD types are
// actively switched on. The non-IDR + SEI constants are reference values
// used by the comments in this module.
#[allow(dead_code)]
const NAL_TYPE_SLICE_NON_IDR: u8 = 1;
const NAL_TYPE_SLICE_IDR: u8 = 5;
#[allow(dead_code)]
const NAL_TYPE_SEI: u8 = 6;
const NAL_TYPE_SPS: u8 = 7;
const NAL_TYPE_PPS: u8 = 8;
const NAL_TYPE_AUD: u8 = 9;

/// Annex-B → AVCC passthrough mux session.
pub(crate) struct PassthroughSession {
    /// Kept for telemetry parity with `LibavSession`; not used today, but
    /// future per-channel passthrough metrics or codec-name display can
    /// pull from here without a signature change.
    #[allow(dead_code)]
    config: EncoderRuntimeConfigV2,
    output_path: PathBuf,
    output: Option<ffmpeg::format::context::Output>,
    stream_index: usize,
    stream_time_base: ffmpeg::Rational,
    /// SPS/PPS captured from the Annex-B stream. Latched on the first
    /// frame that carries them; required to produce AVCC `extradata`.
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
    /// True after `output.write_header()` has been called (i.e. once we've
    /// observed enough SPS/PPS to populate extradata).
    header_written: bool,
    width: u32,
    height: u32,
    recording_start_us: u64,
    last_pts_us: Option<i64>,
    nonmonotonic_warning_logged: bool,
    pre_header_warning_logged: bool,
    metrics: EncodeMetrics,
}

impl PassthroughSession {
    pub(crate) fn new(
        config: EncoderRuntimeConfigV2,
        output_path: PathBuf,
        recording_start_us: u64,
        first_frame: &OwnedFrame,
    ) -> Result<Self> {
        if first_frame.header.pixel_format != PixelFormat::H264 {
            return Err(EncoderError::message(format!(
                "passthrough session requires H264 frames, got {:?}",
                first_frame.header.pixel_format
            )));
        }
        std::fs::create_dir_all(
            output_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".")),
        )?;

        // Open the MP4 output context but DON'T add the stream or write
        // the header yet — we need SPS/PPS for the extradata, and the
        // first frame may not contain them (e.g. only an SPS+PPS+IDR
        // frame triggers a clean session).
        let output = ffmpeg::format::output_as(&output_path, "mp4")
            .map_err(|e| EncoderError::message(format!("ffmpeg output_as(mp4) failed: {e}")))?;

        let session = Self {
            config,
            output_path,
            output: Some(output),
            stream_index: 0,
            // The mp4 muxer will rewrite the time base after write_header;
            // start with 1/1_000_000 (microseconds) which matches our
            // pts source. Real value is captured after write_header.
            stream_time_base: ffmpeg::Rational::new(1, 1_000_000),
            sps: None,
            pps: None,
            header_written: false,
            width: first_frame.header.width,
            height: first_frame.header.height,
            recording_start_us,
            last_pts_us: None,
            nonmonotonic_warning_logged: false,
            pre_header_warning_logged: false,
            metrics: EncodeMetrics::default(),
        };
        Ok(session)
    }

    pub(crate) fn encode_frame(&mut self, frame: &OwnedFrame) -> Result<()> {
        if frame.header.pixel_format != PixelFormat::H264 {
            return Err(EncoderError::message(format!(
                "passthrough session received non-H264 frame ({:?})",
                frame.header.pixel_format
            )));
        }

        let nals = parse_annex_b(&frame.payload);
        if nals.is_empty() {
            return Ok(());
        }

        let mut is_idr = false;
        for nal in &nals {
            let nal_type = nal_unit_type(nal);
            match nal_type {
                NAL_TYPE_SPS => self.sps = Some(nal.to_vec()),
                NAL_TYPE_PPS => self.pps = Some(nal.to_vec()),
                NAL_TYPE_SLICE_IDR => is_idr = true,
                _ => {}
            }
        }

        if !self.header_written {
            // We need both SPS and PPS before we can construct the
            // AVCC `avcC` extradata. Drop frames until we have them.
            if self.sps.is_none() || self.pps.is_none() {
                if !self.pre_header_warning_logged {
                    eprintln!(
                        "rollio-encoder: passthrough waiting for SPS+PPS NAL units \
                         before writing MP4 header (file={})",
                        self.output_path.display()
                    );
                    self.pre_header_warning_logged = true;
                }
                self.metrics.dropped_frames = self.metrics.dropped_frames.saturating_add(1);
                return Ok(());
            }
            self.write_header()?;
        }

        let pts_us = self.compute_pts_us(frame.header.timestamp_us);
        let Some(pts_us) = pts_us else {
            self.metrics.dropped_frames = self.metrics.dropped_frames.saturating_add(1);
            return Ok(());
        };
        let pts_us = self.bump_if_nonmonotonic(pts_us);

        // Build the AVCC payload by replacing each kept NAL's start code
        // with a 4-byte big-endian length prefix. SPS/PPS/AUD are
        // dropped per the comments in the module header.
        let mut payload_bytes: Vec<u8> = Vec::with_capacity(frame.payload.len());
        for nal in &nals {
            let nal_type = nal_unit_type(nal);
            match nal_type {
                NAL_TYPE_SPS | NAL_TYPE_PPS | NAL_TYPE_AUD => continue,
                _ => {}
            }
            payload_bytes.extend_from_slice(&(nal.len() as u32).to_be_bytes());
            payload_bytes.extend_from_slice(nal);
        }
        if payload_bytes.is_empty() {
            return Ok(());
        }

        // Build and write the AVPacket via raw FFI so we can set
        // pts/dts/duration/flags without bringing in higher-level
        // helpers that the ffmpeg-next crate doesn't expose for raw byte
        // packets. ffmpeg's `av_rescale_q` performs `pts_us * src/dst`
        // exactly as we want; rationals are passed by value across the
        // FFI boundary.
        let src_tb = ffmpeg::Rational::new(1, 1_000_000);
        let pts_in_stream =
            unsafe { ffmpeg::ffi::av_rescale_q(pts_us, src_tb.into(), self.stream_time_base.into()) };

        let mut packet = ffmpeg::Packet::copy(&payload_bytes);
        packet.set_stream(self.stream_index);
        packet.set_pts(Some(pts_in_stream));
        packet.set_dts(Some(pts_in_stream));
        if is_idr {
            packet.set_flags(ffmpeg::packet::Flags::KEY);
        }

        let output = self
            .output
            .as_mut()
            .ok_or_else(|| EncoderError::message("passthrough output context missing"))?;
        if let Err(e) = packet.write_interleaved(output) {
            return Err(EncoderError::message(format!(
                "ffmpeg av_interleaved_write_frame failed: {e}"
            )));
        }
        self.metrics.frames = self.metrics.frames.saturating_add(1);
        self.metrics.encoded_bytes = self.metrics.encoded_bytes.saturating_add(payload_bytes.len());
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<EncodedArtifact> {
        if let Some(mut output) = self.output.take() {
            if self.header_written {
                output
                    .write_trailer()
                    .map_err(|e| EncoderError::message(format!("av_write_trailer failed: {e}")))?;
            }
            // Drop the output context to flush+close.
            drop(output);
        }

        Ok(EncodedArtifact {
            path: self.output_path,
            codec: EncoderCodec::H264,
            backend: EncoderBackend::Passthrough,
            artifact_format: EncoderArtifactFormat::Mp4,
            width: self.width,
            height: self.height,
            metrics: self.metrics,
        })
    }

    pub(crate) fn metrics_mut(&mut self) -> &mut EncodeMetrics {
        &mut self.metrics
    }

    fn write_header(&mut self) -> Result<()> {
        let sps = self
            .sps
            .as_ref()
            .ok_or_else(|| EncoderError::message("write_header: SPS missing"))?;
        let pps = self
            .pps
            .as_ref()
            .ok_or_else(|| EncoderError::message("write_header: PPS missing"))?;
        let extradata = build_avcc_extradata(sps, pps)?;

        let output = self
            .output
            .as_mut()
            .ok_or_else(|| EncoderError::message("output context missing"))?;

        // The MP4 muxer needs an output stream. Construct it by adding a
        // stream backed by an h264 codec descriptor; we don't open an
        // encoder — we only need the output stream parameters populated.
        let codec = ffmpeg::encoder::find(ffmpeg::codec::Id::H264)
            .ok_or_else(|| EncoderError::message("ffmpeg H264 codec descriptor not available"))?;
        let mut stream = output
            .add_stream(codec)
            .map_err(|e| EncoderError::message(format!("output.add_stream failed: {e}")))?;
        self.stream_index = stream.index();
        stream.set_time_base(ffmpeg::Rational::new(1, 1_000_000));

        // Populate the codec parameters via raw FFI: codec id, type,
        // dimensions, extradata. ffmpeg-next doesn't expose extradata
        // setters on AVCodecParameters directly.
        unsafe {
            let params = stream.parameters().as_mut_ptr();
            (*params).codec_type = ffmpeg::ffi::AVMediaType::AVMEDIA_TYPE_VIDEO;
            (*params).codec_id = ffmpeg::ffi::AVCodecID::AV_CODEC_ID_H264;
            (*params).width = self.width as i32;
            (*params).height = self.height as i32;
            (*params).format = ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_YUV420P as i32;
            // codec_tag = 0 lets the muxer pick the right tag (avc1).
            (*params).codec_tag = 0;
            // extradata must be allocated with av_malloc + AV_INPUT_BUFFER_PADDING_SIZE.
            let ext_size = extradata.len() as i32;
            let alloc_size = ext_size as usize + ffmpeg::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize;
            let extradata_ptr = ffmpeg::ffi::av_mallocz(alloc_size) as *mut u8;
            if extradata_ptr.is_null() {
                return Err(EncoderError::message("av_mallocz for extradata failed"));
            }
            std::ptr::copy_nonoverlapping(extradata.as_ptr(), extradata_ptr, extradata.len());
            (*params).extradata = extradata_ptr;
            (*params).extradata_size = ext_size;
        }

        output
            .write_header()
            .map_err(|e| EncoderError::message(format!("output.write_header failed: {e}")))?;

        // The MP4 muxer typically rewrites time_base to 1/15360 during
        // write_header. Re-read it so packet PTS is rescaled correctly.
        self.stream_time_base = output
            .stream(self.stream_index)
            .ok_or_else(|| EncoderError::message("missing video stream after write_header"))?
            .time_base();
        self.header_written = true;
        Ok(())
    }

    fn compute_pts_us(&self, frame_timestamp_us: u64) -> Option<i64> {
        if frame_timestamp_us < self.recording_start_us {
            return None;
        }
        let delta = frame_timestamp_us - self.recording_start_us;
        if delta > i64::MAX as u64 {
            return None;
        }
        Some(delta as i64)
    }

    fn bump_if_nonmonotonic(&mut self, mut pts_us: i64) -> i64 {
        if let Some(last) = self.last_pts_us {
            if pts_us <= last {
                if !self.nonmonotonic_warning_logged {
                    eprintln!(
                        "rollio-encoder: passthrough non-monotonic PTS bumped (file={}, last={}, got={})",
                        self.output_path.display(),
                        last,
                        pts_us
                    );
                    self.nonmonotonic_warning_logged = true;
                }
                pts_us = last + 1;
            }
        }
        self.last_pts_us = Some(pts_us);
        pts_us
    }
}

// ---------------------------------------------------------------------------
// Annex-B parser
// ---------------------------------------------------------------------------

/// Split an Annex-B byte stream into individual NAL unit byte slices,
/// stripping start codes (`00 00 00 01` or `00 00 01`) and the trailing
/// (optional) start code. Returns slices that borrow from `input`.
fn parse_annex_b(input: &[u8]) -> Vec<&[u8]> {
    let mut nals = Vec::new();
    let starts = find_start_codes(input);
    for window in starts.windows(2) {
        let begin = window[0].1; // byte after start code
        let end = window[1].0; // byte at next start code
        if end > begin {
            nals.push(&input[begin..end]);
        }
    }
    if let Some(last) = starts.last() {
        let begin = last.1;
        if begin < input.len() {
            nals.push(&input[begin..]);
        }
    }
    nals
}

/// Find every Annex-B start code in the input. Returns a list of
/// `(start_code_offset, payload_offset)` pairs where `start_code_offset`
/// is the index of the first `0x00` and `payload_offset` is the index of
/// the byte immediately after the `0x01`.
fn find_start_codes(input: &[u8]) -> Vec<(usize, usize)> {
    let mut hits = Vec::new();
    let mut i = 0usize;
    while i + 2 < input.len() {
        if input[i] == 0 && input[i + 1] == 0 {
            if input[i + 2] == 1 {
                hits.push((i, i + 3));
                i += 3;
                continue;
            }
            if i + 3 < input.len() && input[i + 2] == 0 && input[i + 3] == 1 {
                hits.push((i, i + 4));
                i += 4;
                continue;
            }
        }
        i += 1;
    }
    hits
}

fn nal_unit_type(nal: &[u8]) -> u8 {
    if nal.is_empty() {
        return 0;
    }
    nal[0] & 0x1F
}

/// Build the AVCC `avcC` extradata blob from raw SPS/PPS NAL payloads.
/// Layout (ISO/IEC 14496-15 5.2.4.1.1):
///   configurationVersion = 1                   (1 byte)
///   AVCProfileIndication = sps[1]              (1 byte)
///   profile_compatibility = sps[2]             (1 byte)
///   AVCLevelIndication = sps[3]                (1 byte)
///   reserved (6 bits, 1's) | lengthSizeMinusOne (2 bits, 3) (1 byte)
///   reserved (3 bits, 1's) | numOfSPS (5 bits) (1 byte)
///     for each SPS:
///       sps_length (2 bytes BE)
///       sps_bytes
///   numOfPPS (1 byte)
///     for each PPS:
///       pps_length (2 bytes BE)
///       pps_bytes
fn build_avcc_extradata(sps: &[u8], pps: &[u8]) -> Result<Vec<u8>> {
    if sps.len() < 4 {
        return Err(EncoderError::message(
            "SPS NAL too short to extract profile/level",
        ));
    }
    let mut out = Vec::with_capacity(11 + sps.len() + pps.len());
    out.push(1); // configurationVersion
    out.push(sps[1]); // profile
    out.push(sps[2]); // profile_compatibility
    out.push(sps[3]); // level
    out.push(0xFF); // reserved (6 bits 1) | lengthSizeMinusOne (2 bits = 3, i.e. 4 bytes)
    out.push(0xE1); // reserved (3 bits 1) | numOfSPS (5 bits = 1)
    out.extend_from_slice(&(sps.len() as u16).to_be_bytes());
    out.extend_from_slice(sps);
    out.push(1); // numOfPPS
    out.extend_from_slice(&(pps.len() as u16).to_be_bytes());
    out.extend_from_slice(pps);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_annex_b_with_three_byte_start_codes() {
        let stream = [
            0x00, 0x00, 0x01, 0x67, 0x42, 0x00, 0x1E, 0x00, 0x00, 0x01, 0x68, 0xCE, 0x06, 0xE2,
            0x00, 0x00, 0x01, 0x65, 0xB8, 0x00, 0x00,
        ];
        let nals = parse_annex_b(&stream);
        assert_eq!(nals.len(), 3);
        assert_eq!(nal_unit_type(nals[0]), NAL_TYPE_SPS);
        assert_eq!(nal_unit_type(nals[1]), NAL_TYPE_PPS);
        assert_eq!(nal_unit_type(nals[2]), NAL_TYPE_SLICE_IDR);
    }

    #[test]
    fn parse_annex_b_with_four_byte_start_codes() {
        let stream = [
            0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x00, 0x1E, 0x00, 0x00, 0x00, 0x01, 0x68, 0xCE,
            0x06, 0xE2, 0x00, 0x00, 0x00, 0x01, 0x65, 0xB8, 0x00, 0x00,
        ];
        let nals = parse_annex_b(&stream);
        assert_eq!(nals.len(), 3);
        assert_eq!(nal_unit_type(nals[0]), NAL_TYPE_SPS);
        assert_eq!(nal_unit_type(nals[1]), NAL_TYPE_PPS);
        assert_eq!(nal_unit_type(nals[2]), NAL_TYPE_SLICE_IDR);
    }

    #[test]
    fn build_avcc_extradata_layout() {
        // SPS: NAL type 7 (0x67), profile=0x42, compat=0x00, level=0x1E
        let sps = [0x67, 0x42, 0x00, 0x1E, 0xAB];
        let pps = [0x68, 0xCE, 0x06, 0xE2];
        let extradata = build_avcc_extradata(&sps, &pps).unwrap();
        // 11 bytes header + sps + pps.
        assert_eq!(extradata.len(), 11 + sps.len() + pps.len());
        assert_eq!(extradata[0], 1);
        assert_eq!(extradata[1], sps[1]); // profile
        assert_eq!(extradata[2], sps[2]); // compat
        assert_eq!(extradata[3], sps[3]); // level
        assert_eq!(extradata[4], 0xFF);
        assert_eq!(extradata[5], 0xE1);
        let sps_len = u16::from_be_bytes([extradata[6], extradata[7]]) as usize;
        assert_eq!(sps_len, sps.len());
        assert_eq!(&extradata[8..8 + sps_len], &sps);
        let pps_count_off = 8 + sps_len;
        assert_eq!(extradata[pps_count_off], 1);
        let pps_len =
            u16::from_be_bytes([extradata[pps_count_off + 1], extradata[pps_count_off + 2]])
                as usize;
        assert_eq!(pps_len, pps.len());
        assert_eq!(
            &extradata[pps_count_off + 3..pps_count_off + 3 + pps_len],
            &pps
        );
    }
}
