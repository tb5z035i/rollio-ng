//! Horizon Robotics X5 SoC hardware VPU encoder backend.
//!
//! Pipeline:
//!
//! ```text
//! Raw frame (RGB24/BGR24/YUYV/Gray8/MJPEG) ─► swscale (CPU) ─► NV12
//!                                                                  │
//!                                                                  ▼
//!                                              libmultimedia VPU encoder
//!                                              (hb_mm_mc_* via C shim)
//!                                                                  │
//!                                                                  ▼
//!                                              H.264 / MJPEG encoded packets
//! ```
//!
//! The VPU only accepts NV12 input, so we use ffmpeg's swscale to
//! convert from whatever pixel format the camera delivers. The
//! conversion runs on the CPU (the X5's Cortex-A55 cores) but the
//! actual encoding is offloaded to the dedicated VPU hardware.
//!
//! Sessions are `!Send` — the VPU context handle is thread-local.
//! This matches the existing single-threaded worker pattern used by
//! all other encoder backends.

use std::path::Path;
use std::time::Instant;

use ffmpeg_next as ffmpeg;
use rollio_types::messages::{
    EncodedCodecId, EncodedPacketHeader, EncodedPacketKind, PixelFormat,
    ENCODED_PACKET_FLAG_KEYFRAME,
};

use rollio_encoder::backend::color::ColorCodec;
use rollio_encoder::codec::{encoded_codec_id, CodecSession, CodecSessionParams, EncodedPacketSink, OwnedFrame};
use rollio_encoder::error::{EncoderError, Result};
use rollio_encoder::media::EncodeMetrics;

// ─── FFI declarations (from horizon_x5_ffi.c) ──────────────────────────────

/// Opaque encoder handle returned by the C shim.
#[repr(C)]
struct X5Encoder {
    _opaque: [u8; 0],
}

extern "C" {
    fn x5_encoder_create(
        codec_id: i32,
        width: u32,
        height: u32,
        frame_rate: u32,
        bit_rate: u32,
        gop_size: u32,
        quality: i32,
    ) -> *mut X5Encoder;

    fn x5_encoder_encode(
        enc: *mut X5Encoder,
        y_plane: *const u8,
        uv_plane: *const u8,
        y_stride: u32,
        uv_stride: u32,
        width: u32,
        height: u32,
        pts: u64,
        out_buf: *mut u8,
        out_cap: u32,
        out_is_key: *mut i32,
    ) -> i32;

    fn x5_encoder_destroy(enc: *mut X5Encoder);
}

// ─── Codec ID mapping ───────────────────────────────────────────────────────

/// Maps our ColorCodec to the libmultimedia media_codec_id_t values.
const MEDIA_CODEC_ID_H264: i32 = 1;
const MEDIA_CODEC_ID_MJPEG: i32 = 3;

fn color_codec_to_x5(codec: ColorCodec) -> Option<i32> {
    match codec {
        ColorCodec::H264 => Some(MEDIA_CODEC_ID_H264),
        ColorCodec::Mjpg => Some(MEDIA_CODEC_ID_MJPEG),
        // H265 could be added trivially (MEDIA_CODEC_ID_H265 = 2)
        // but is not validated on hardware yet.
        _ => None,
    }
}

// ─── NV12 conversion via ffmpeg swscale ─────────────────────────────────────

/// Holds the swscale context and NV12 output buffer for pixel format
/// conversion. Reused across frames for the lifetime of the session.
struct Nv12Converter {
    sws_ctx: *mut ffmpeg::ffi::SwsContext,
    /// Contiguous NV12 buffer: Y plane (width*height) followed by
    /// UV plane (width * height/2).
    nv12_buf: Vec<u8>,
    width: u32,
    height: u32,
}

impl Nv12Converter {
    fn new(src_format: PixelFormat, width: u32, height: u32) -> Result<Self> {
        let src_pix_fmt = pixel_format_to_av(src_format)?;
        let dst_pix_fmt = ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_NV12;

        let sws_ctx = unsafe {
            ffmpeg::ffi::sws_getContext(
                width as i32,
                height as i32,
                src_pix_fmt,
                width as i32,
                height as i32,
                dst_pix_fmt,
                ffmpeg::ffi::SWS_BILINEAR as i32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        if sws_ctx.is_null() {
            return Err(EncoderError::message(format!(
                "horizon-x5: failed to create swscale context for {:?} -> NV12",
                src_format
            )));
        }

        // NV12: Y = width*height, UV = width*(height/2)
        let y_size = (width * height) as usize;
        let uv_size = (width * (height / 2)) as usize;
        let nv12_buf = vec![0u8; y_size + uv_size];

        Ok(Self {
            sws_ctx,
            nv12_buf,
            width,
            height,
        })
    }

    /// Convert a raw frame to NV12 in-place. Returns (y_ptr, uv_ptr, y_stride, uv_stride).
    fn convert(&mut self, payload: &[u8], src_format: PixelFormat) -> Result<(&[u8], &[u8], u32, u32)> {
        let w = self.width as i32;
        let h = self.height as i32;
        let y_size = (self.width * self.height) as usize;

        // Set up source slice pointers and strides based on pixel format
        let (src_data, src_linesize) = source_planes(payload, src_format, self.width, self.height)?;

        // Destination: NV12 — Y plane at offset 0, UV plane at offset y_size
        let dst_data: [*mut u8; 4] = [
            self.nv12_buf.as_mut_ptr(),
            self.nv12_buf.as_mut_ptr().wrapping_add(y_size),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ];
        let dst_linesize: [i32; 4] = [w, w, 0, 0];

        let ret = unsafe {
            ffmpeg::ffi::sws_scale(
                self.sws_ctx,
                src_data.as_ptr() as *const *const u8,
                src_linesize.as_ptr(),
                0,
                h,
                dst_data.as_ptr(),
                dst_linesize.as_ptr(),
            )
        };
        if ret <= 0 {
            return Err(EncoderError::message(
                "horizon-x5: swscale conversion failed",
            ));
        }

        let y_stride = self.width;
        let uv_stride = self.width;
        Ok((
            &self.nv12_buf[..y_size],
            &self.nv12_buf[y_size..],
            y_stride,
            uv_stride,
        ))
    }
}

impl Drop for Nv12Converter {
    fn drop(&mut self) {
        if !self.sws_ctx.is_null() {
            unsafe { ffmpeg::ffi::sws_freeContext(self.sws_ctx) };
        }
    }
}

/// Map PixelFormat to ffmpeg AVPixelFormat.
fn pixel_format_to_av(pf: PixelFormat) -> Result<ffmpeg::ffi::AVPixelFormat> {
    use ffmpeg::ffi::AVPixelFormat;
    match pf {
        PixelFormat::Rgb24 => Ok(AVPixelFormat::AV_PIX_FMT_RGB24),
        PixelFormat::Bgr24 => Ok(AVPixelFormat::AV_PIX_FMT_BGR24),
        PixelFormat::Yuyv => Ok(AVPixelFormat::AV_PIX_FMT_YUYV422),
        PixelFormat::Gray8 => Ok(AVPixelFormat::AV_PIX_FMT_GRAY8),
        PixelFormat::Nv12 => Ok(AVPixelFormat::AV_PIX_FMT_NV12),
        _ => Err(EncoderError::message(format!(
            "horizon-x5: unsupported input pixel format {:?}",
            pf
        ))),
    }
}

/// Build source plane pointers and linesizes for swscale.
fn source_planes(
    payload: &[u8],
    format: PixelFormat,
    width: u32,
    height: u32,
) -> Result<([*const u8; 4], [i32; 4])> {
    let ptr = payload.as_ptr();
    match format {
        PixelFormat::Rgb24 | PixelFormat::Bgr24 => {
            let stride = width as i32 * 3;
            Ok(([ptr, std::ptr::null(), std::ptr::null(), std::ptr::null()], [stride, 0, 0, 0]))
        }
        PixelFormat::Yuyv => {
            let stride = width as i32 * 2;
            Ok(([ptr, std::ptr::null(), std::ptr::null(), std::ptr::null()], [stride, 0, 0, 0]))
        }
        PixelFormat::Gray8 => {
            let stride = width as i32;
            Ok(([ptr, std::ptr::null(), std::ptr::null(), std::ptr::null()], [stride, 0, 0, 0]))
        }
        PixelFormat::Nv12 => {
            let y_size = (width * height) as usize;
            let uv_ptr = unsafe { ptr.add(y_size) };
            Ok((
                [ptr, uv_ptr, std::ptr::null(), std::ptr::null()],
                [width as i32, width as i32, 0, 0],
            ))
        }
        _ => Err(EncoderError::message(format!(
            "horizon-x5: cannot build source planes for {:?}",
            format
        ))),
    }
}

// ─── Backend (stateless singleton) ──────────────────────────────────────────

pub struct HorizonX5Backend;

impl HorizonX5Backend {
    pub fn priority(&self) -> u32 {
        80
    }

    pub fn available(&self) -> bool {
        // Runtime check: the VPU library must be present on the target
        Path::new("/usr/lib/libmultimedia.so.1").exists()
    }

    pub fn supports(&self, codec: ColorCodec, input: PixelFormat) -> bool {
        // Only H264 and MJPEG are supported by the X5 VPU
        if color_codec_to_x5(codec).is_none() {
            return false;
        }
        // We can convert any of these formats to NV12 via swscale
        matches!(
            input,
            PixelFormat::Rgb24
                | PixelFormat::Bgr24
                | PixelFormat::Yuyv
                | PixelFormat::Gray8
                | PixelFormat::Nv12
        )
    }

    pub fn open_session(
        &self,
        params: &CodecSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Box<dyn CodecSession>> {
        HorizonX5Session::new(params, first_frame)
    }
}

// ─── Session (per-stream state) ─────────────────────────────────────────────

/// Maximum encoded output buffer size. H.264 worst case is roughly
/// 1.5x the raw frame size; we allocate 2x for safety.
const MAX_ENCODED_BUF: usize = 8 * 1024 * 1024; // 8 MiB

#[allow(dead_code)]
struct HorizonX5Session {
    encoder: *mut X5Encoder,
    converter: Nv12Converter,
    codec: ColorCodec,
    encoded_codec_id: EncodedCodecId,
    width: u32,
    height: u32,
    fps: u32,
    sequence: u64,
    episode_index: u32,
    out_buf: Vec<u8>,
    metrics: EncodeMetrics,
    /// First frame produces a Config packet with extradata (SPS/PPS for H264).
    config_sent: bool,
}

impl HorizonX5Session {
    fn new(params: &CodecSessionParams<'_>, first_frame: &OwnedFrame) -> Result<Box<dyn CodecSession>> {
        let color_codec = ColorCodec::try_from(params.codec)?;
        let x5_codec_id = color_codec_to_x5(color_codec).ok_or_else(|| {
            EncoderError::message(format!(
                "horizon-x5: codec {:?} not supported by VPU",
                color_codec
            ))
        })?;

        let width = params.output_width;
        let height = params.output_height;
        let fps = params.fps;
        let input_format = first_frame.header.pixel_format;

        // Default bitrate: 4 Mbps for H264, 0 (VBR) for MJPEG
        let bit_rate = match color_codec {
            ColorCodec::H264 => 4_000_000,
            _ => 0,
        };
        let gop_size = fps; // keyframe every second
        let quality = if color_codec == ColorCodec::Mjpg { 85 } else { 0 };

        // Create the VPU encoder via C shim
        let encoder = unsafe {
            x5_encoder_create(
                x5_codec_id,
                width,
                height,
                fps,
                bit_rate,
                gop_size,
                quality,
            )
        };
        if encoder.is_null() {
            return Err(EncoderError::message(
                "horizon-x5: failed to create VPU encoder context",
            ));
        }

        // Create the NV12 converter (unless input is already NV12)
        let converter = Nv12Converter::new(input_format, width, height)?;

        let session = HorizonX5Session {
            encoder,
            converter,
            codec: color_codec,
            encoded_codec_id: encoded_codec_id(params.codec),
            width,
            height,
            fps,
            sequence: 0,
            episode_index: params.episode_index,
            out_buf: vec![0u8; MAX_ENCODED_BUF],
            metrics: EncodeMetrics::default(),
            config_sent: false,
        };

        Ok(Box::new(session))
    }

    fn make_header(&self, kind: EncodedPacketKind, pts_us: i64, is_key: bool, payload_len: u32) -> EncodedPacketHeader {
        let mut flags = 0u32;
        if is_key {
            flags |= ENCODED_PACKET_FLAG_KEYFRAME;
        }
        EncodedPacketHeader {
            kind,
            codec: self.encoded_codec_id,
            flags,
            width: self.width,
            height: self.height,
            pixel_format: PixelFormat::Nv12,
            _reserved0: 0,
            time_base_num: 1,
            time_base_den: self.fps,
            pts_us,
            dts_us: pts_us,
            duration_us: (1_000_000 / self.fps as i64),
            sequence_number: self.sequence,
            source_timestamp_us: 0,
            source_frame_index: 0,
            episode_index: self.episode_index,
            payload_len,
        }
    }
}

impl CodecSession for HorizonX5Session {
    fn encode(&mut self, frame: &OwnedFrame, sink: &mut dyn EncodedPacketSink) -> Result<()> {
        let start = Instant::now();
        let raw_bytes = frame.payload.len();
        let input_format = frame.header.pixel_format;

        // Convert to NV12
        let (y_plane, uv_plane, y_stride, uv_stride) =
            self.converter.convert(&frame.payload, input_format)?;

        // Encode via VPU
        let mut is_key: i32 = 0;
        let pts = frame.header.timestamp_us;

        let encoded_len = unsafe {
            x5_encoder_encode(
                self.encoder,
                y_plane.as_ptr(),
                uv_plane.as_ptr(),
                y_stride,
                uv_stride,
                self.width,
                self.height,
                pts,
                self.out_buf.as_mut_ptr(),
                self.out_buf.len() as u32,
                &mut is_key,
            )
        };

        if encoded_len < 0 {
            return Err(EncoderError::message(format!(
                "horizon-x5: VPU encode failed with error {}",
                encoded_len
            )));
        }

        let encoded_bytes = encoded_len as usize;
        let elapsed = start.elapsed();
        self.metrics.record_frame(raw_bytes, encoded_bytes, elapsed);

        // First frame: emit a Config packet (extradata = first keyframe for H264)
        if !self.config_sent && is_key != 0 {
            // For H264, the first keyframe contains SPS/PPS inline.
            // We send the whole first packet as both Config and Packet.
            let config_header = self.make_header(
                EncodedPacketKind::Config,
                pts as i64,
                true,
                encoded_bytes as u32,
            );
            sink.write_config(config_header, &self.out_buf[..encoded_bytes])?;
            self.config_sent = true;
        }

        // Emit the encoded packet
        let header = self.make_header(
            EncodedPacketKind::Packet,
            pts as i64,
            is_key != 0,
            encoded_bytes as u32,
        );
        sink.write_packet(header, &self.out_buf[..encoded_bytes])?;
        self.sequence += 1;

        Ok(())
    }

    fn finish(self: Box<Self>, sink: &mut dyn EncodedPacketSink) -> Result<()> {
        let header = self.make_header(EncodedPacketKind::EndOfStream, 0, false, 0);
        sink.write_eos(header)?;
        // encoder is destroyed in Drop
        Ok(())
    }

    fn metrics(&self) -> &EncodeMetrics {
        &self.metrics
    }

    fn record_dropped(&mut self) {
        self.metrics.dropped_frames += 1;
    }
}

impl Drop for HorizonX5Session {
    fn drop(&mut self) {
        if !self.encoder.is_null() {
            unsafe { x5_encoder_destroy(self.encoder) };
            self.encoder = std::ptr::null_mut();
        }
    }
}
