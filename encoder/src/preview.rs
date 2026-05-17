//! Preview-side helpers used by the preview-role encoder runtime.
//!
//! - [`PreviewBuilder`] decodes/copies a camera frame, downscales it
//!   to the configured preview dims, and converts it to RGB24 (the
//!   common surface used by both the JPEG-mode preview tap and any
//!   future encoded preview pipeline).
//! - [`JpegCompressor`] turbojpeg-encodes an RGB24 buffer into a
//!   JPEG payload ready to ship on the per-camera `…/preview-jpeg`
//!   topic.
//!
//! These two pieces are deliberately decoupled from the codec
//! sessions in [`crate::codec`]: encoded-mode previews drive a
//! `CodecSession` directly with the camera-native frame, while
//! JPEG-mode previews go through `PreviewBuilder` + `JpegCompressor`.

use crate::codec::OwnedFrame;
use crate::error::{EncoderError, Result};
use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{PixelType, ResizeAlg, ResizeOptions, Resizer};
use ffmpeg_next as ffmpeg;
use rollio_types::messages::{CameraFrameHeader, PixelFormat};
use std::borrow::Cow;

// ---------------------------------------------------------------------------
// PreviewBuilder
// ---------------------------------------------------------------------------

/// Builds downsized RGB24 preview frames from camera-native input.
///
/// The preview-role encoder owns one of these per camera and feeds
/// every received frame through it (throttled to the configured fps).
/// Output is RGB24 at the configured preview dimensions; depth16 is
/// mapped to grayscale RGB24 with a fixed 1m reference (matches the
/// legacy preview policy so the live preview doesn't flicker).
pub struct PreviewBuilder {
    output_width: u32,
    output_height: u32,
    /// Minimum interval between published preview frames, in microseconds.
    /// Calculated from the configured `preview_fps` upper bound.
    min_interval_us: u64,
    /// Last frame timestamp we actually published, in microseconds.
    last_emit_us: Option<u64>,
    /// Per-session MJPEG decoder; lazily initialized when the first
    /// MJPG frame arrives.
    mjpeg_decoder: Option<ffmpeg::decoder::Video>,
    /// swscale context: source pixel format -> RGB24 at preview dims.
    /// Rebuilt if the source pixel format changes mid-session.
    scaler: Option<ffmpeg::software::scaling::context::Context>,
    /// Source pixel format the current scaler expects on its input.
    scaler_input_pixel: Option<ffmpeg::util::format::pixel::Pixel>,
    /// Source dims the current scaler expects.
    scaler_input_dims: Option<(u32, u32)>,
}

/// One preview frame ready to publish.
pub struct BuiltPreview {
    pub width: u32,
    pub height: u32,
    pub timestamp_us: u64,
    pub frame_index: u64,
    pub rgb: Vec<u8>,
}

impl PreviewBuilder {
    pub fn new(output_width: u32, output_height: u32, preview_fps: u32) -> Self {
        let min_interval_us = if preview_fps == 0 {
            0
        } else {
            1_000_000 / u64::from(preview_fps)
        };
        Self {
            output_width,
            output_height,
            min_interval_us,
            last_emit_us: None,
            mjpeg_decoder: None,
            scaler: None,
            scaler_input_pixel: None,
            scaler_input_dims: None,
        }
    }

    pub fn output_width(&self) -> u32 {
        self.output_width
    }

    pub fn output_height(&self) -> u32 {
        self.output_height
    }

    /// Replace the configured output dims (used by `set_preview_size`
    /// flow). Drops the cached scaler so it is rebuilt at the new
    /// dims on the next frame.
    pub fn set_output_dims(&mut self, width: u32, height: u32) {
        self.output_width = width;
        self.output_height = height;
        self.scaler = None;
        self.scaler_input_pixel = None;
        self.scaler_input_dims = None;
    }

    /// Returns Ok(Some(...)) when the throttle says it's time to publish,
    /// Ok(None) when the frame is dropped to honour `preview_fps`, and an
    /// Err for unrecoverable decode/conversion failures.
    pub fn build(&mut self, frame: &OwnedFrame) -> Result<Option<BuiltPreview>> {
        let timestamp_us = frame.header.timestamp_us;
        if !self.is_emit_due(timestamp_us) {
            return Ok(None);
        }

        let source = if frame.header.pixel_format == PixelFormat::Depth16 {
            depth16_to_gray8_av_frame(frame)?
        } else {
            decode_or_copy_frame_to_av(frame, &mut self.mjpeg_decoder)?
        };
        let source_pixel = source.format();
        self.ensure_scaler(source_pixel, source.width(), source.height())?;

        let scaler = self
            .scaler
            .as_mut()
            .expect("scaler should be initialized after ensure_scaler");
        let mut rgb_frame = ffmpeg::frame::Video::empty();
        scaler.run(&source, &mut rgb_frame)?;

        let rgb = compact_rgb_frame(&rgb_frame);
        self.last_emit_us = Some(timestamp_us);
        Ok(Some(BuiltPreview {
            width: self.output_width,
            height: self.output_height,
            timestamp_us,
            frame_index: frame.header.frame_index,
            rgb,
        }))
    }

    fn is_emit_due(&self, timestamp_us: u64) -> bool {
        match self.last_emit_us {
            None => true,
            Some(last) if self.min_interval_us == 0 => timestamp_us >= last,
            Some(last) => {
                timestamp_us == 0
                    || timestamp_us < last
                    || timestamp_us - last >= self.min_interval_us
            }
        }
    }

    fn ensure_scaler(
        &mut self,
        source_pixel: ffmpeg::util::format::pixel::Pixel,
        source_width: u32,
        source_height: u32,
    ) -> Result<()> {
        let dims = (source_width, source_height);
        if self.scaler.is_some()
            && self.scaler_input_pixel == Some(source_pixel)
            && self.scaler_input_dims == Some(dims)
        {
            return Ok(());
        }
        let mut scaler = ffmpeg::software::scaling::context::Context::get(
            source_pixel,
            source_width,
            source_height,
            ffmpeg::util::format::pixel::Pixel::RGB24,
            self.output_width,
            self.output_height,
            ffmpeg::software::scaling::flag::Flags::FAST_BILINEAR,
        )?;
        crate::media::set_swscale_color_range_to_mpeg(
            &mut scaler,
            source_pixel,
            ffmpeg::util::format::pixel::Pixel::RGB24,
        )?;
        self.scaler = Some(scaler);
        self.scaler_input_pixel = Some(source_pixel);
        self.scaler_input_dims = Some(dims);
        Ok(())
    }
}

/// RealSense D435 Z16 depth uses a 0.001 m scale by default, so a raw
/// value of 1000 corresponds to ~1.0 m. Keeping the preview reference
/// fixed avoids frame-to-frame flicker from per-frame normalization.
const DEPTH16_PREVIEW_REFERENCE_RAW: u32 = 1000;

fn depth16_to_gray8_av_frame(frame: &OwnedFrame) -> Result<ffmpeg::frame::Video> {
    let width = frame.header.width;
    let height = frame.header.height;
    let expected_len = (width as usize) * (height as usize) * 2;
    if expected_len == 0 {
        return Err(EncoderError::message(
            "depth16 preview requires non-empty frame dimensions",
        ));
    }
    if frame.payload.len() < expected_len {
        return Err(EncoderError::message(format!(
            "depth16 payload too short: expected at least {} bytes for {}x{}, got {}",
            expected_len,
            width,
            height,
            frame.payload.len()
        )));
    }
    let mut gray =
        ffmpeg::frame::Video::new(ffmpeg::util::format::pixel::Pixel::GRAY8, width, height);
    let stride = gray.stride(0);
    let dst = gray.data_mut(0);
    let depth_bytes = &frame.payload[..expected_len];
    for row in 0..height as usize {
        let src_row_offset = row * (width as usize) * 2;
        let dst_row_offset = row * stride;
        for col in 0..width as usize {
            let chunk_offset = src_row_offset + col * 2;
            let depth =
                u16::from_le_bytes([depth_bytes[chunk_offset], depth_bytes[chunk_offset + 1]])
                    as u32;
            let intensity = if depth == 0 {
                0u8
            } else {
                let clamped = depth.min(DEPTH16_PREVIEW_REFERENCE_RAW);
                (((DEPTH16_PREVIEW_REFERENCE_RAW - clamped) * 255) / DEPTH16_PREVIEW_REFERENCE_RAW)
                    as u8
            };
            dst[dst_row_offset + col] = intensity;
        }
    }
    Ok(gray)
}

/// Decode an MJPG bus frame into an `AVFrame`, or copy a raw frame
/// into one. Shared by the libav codec sessions and the preview
/// builder; the MJPEG decoder is borrowed from the caller so each
/// consumer keeps its own per-session state without crossing thread
/// boundaries.
pub(crate) fn decode_or_copy_frame_to_av(
    frame: &OwnedFrame,
    mjpeg_decoder: &mut Option<ffmpeg::decoder::Video>,
) -> Result<ffmpeg::frame::Video> {
    match frame.header.pixel_format {
        PixelFormat::Mjpeg => {
            if mjpeg_decoder.is_none() {
                let codec = ffmpeg::decoder::find(ffmpeg::codec::Id::MJPEG)
                    .ok_or_else(|| EncoderError::message("MJPEG decoder not available in libav"))?;
                let context = ffmpeg::codec::context::Context::new_with_codec(codec);
                *mjpeg_decoder = Some(context.decoder().video()?);
            }
            let decoder = mjpeg_decoder
                .as_mut()
                .expect("MJPEG decoder is initialized");
            let packet = ffmpeg::Packet::copy(&frame.payload);
            decoder.send_packet(&packet)?;
            let mut decoded = ffmpeg::frame::Video::empty();
            if decoder.receive_frame(&mut decoded).is_err() {
                return Err(EncoderError::message(
                    "MJPEG decoder did not produce a frame for one bus payload",
                ));
            }
            Ok(decoded)
        }
        other => {
            let source_pixel = crate::media::pixel_format_for_libav(other)?;
            let mut source =
                ffmpeg::frame::Video::new(source_pixel, frame.header.width, frame.header.height);
            crate::media::copy_frame_payload(&mut source, &frame.header, &frame.payload)?;
            Ok(source)
        }
    }
}

fn compact_rgb_frame(frame: &ffmpeg::frame::Video) -> Vec<u8> {
    let row_bytes = frame.width() as usize * 3;
    let stride = frame.stride(0);
    let mut output = vec![0u8; row_bytes * frame.height() as usize];
    for row in 0..frame.height() as usize {
        let src_offset = row * stride;
        let dst_offset = row * row_bytes;
        output[dst_offset..dst_offset + row_bytes]
            .copy_from_slice(&frame.data(0)[src_offset..src_offset + row_bytes]);
    }
    output
}

// ---------------------------------------------------------------------------
// JpegCompressor
//
// Ported verbatim from `visualizer/src/jpeg.rs` (deleted as part of the
// preview rewrite). Compresses a finished RGB24 preview buffer to JPEG
// via libjpeg-turbo and exposes a borrowed view of the reusable output
// buffer so we avoid per-frame allocation churn.
// ---------------------------------------------------------------------------

pub struct JpegCompressor {
    compressor: turbojpeg::Compressor,
    /// Reused JPEG output buffer to avoid per-frame allocation/copy churn.
    jpeg_dst: turbojpeg::OutputBuf<'static>,
    /// Resizer + reusable destination image for camera-frame scaling
    /// when the camera's native dims exceed the requested preview
    /// dims. `PreviewBuilder` already downsizes to preview dims, but
    /// we keep the resizer here for the case where JpegCompressor is
    /// used directly on a camera-native RGB24 buffer (not the typical
    /// path; the new encoder pipeline always goes through
    /// PreviewBuilder first).
    resizer: Resizer,
    resize_dst: Option<Image<'static>>,
}

pub struct CompressedPreview<'a> {
    pub jpeg_data: &'a [u8],
    pub width: u32,
    pub height: u32,
}

impl JpegCompressor {
    pub fn new(quality: i32) -> Result<Self> {
        let mut compressor = turbojpeg::Compressor::new()
            .map_err(|e| EncoderError::message(format!("turbojpeg init failed: {e}")))?;
        compressor
            .set_quality(quality)
            .map_err(|e| EncoderError::message(format!("turbojpeg set_quality failed: {e}")))?;
        compressor
            .set_subsamp(turbojpeg::Subsamp::Sub2x2)
            .map_err(|e| EncoderError::message(format!("turbojpeg set_subsamp failed: {e}")))?;
        compressor
            .set_optimize(false)
            .map_err(|e| EncoderError::message(format!("turbojpeg set_optimize failed: {e}")))?;
        Ok(Self {
            compressor,
            jpeg_dst: turbojpeg::OutputBuf::new_owned(),
            resizer: Resizer::new(),
            resize_dst: None,
        })
    }

    /// Convert a camera frame to RGB preview pixels and JPEG-compress.
    /// Useful in code paths that skip `PreviewBuilder` (e.g. tests).
    pub fn compress_frame<'a>(
        &'a mut self,
        header: &CameraFrameHeader,
        frame_data: &[u8],
        max_width: u32,
        max_height: u32,
    ) -> Result<CompressedPreview<'a>> {
        let preview_pixels = preview_rgb_bytes(header, frame_data)?;
        self.compress(
            preview_pixels.as_ref(),
            header.width,
            header.height,
            max_width,
            max_height,
        )
    }

    /// JPEG-compress a finished RGB24 buffer. Caller is responsible
    /// for sizing it to the target dims; if the source is larger than
    /// `max_width`/`max_height`, a fallback resize is applied.
    pub fn compress<'a>(
        &'a mut self,
        rgb_data: &[u8],
        width: u32,
        height: u32,
        max_width: u32,
        max_height: u32,
    ) -> Result<CompressedPreview<'a>> {
        let compressor = &mut self.compressor;
        let resizer = &mut self.resizer;
        let resize_dst = &mut self.resize_dst;
        let jpeg_dst = &mut self.jpeg_dst;

        if width <= max_width && height <= max_height {
            return Ok(CompressedPreview {
                jpeg_data: compress_raw(compressor, jpeg_dst, rgb_data, width, height)?,
                width,
                height,
            });
        }

        let scale_w = max_width as f64 / width as f64;
        let scale_h = max_height as f64 / height as f64;
        let scale = scale_w.min(scale_h).min(1.0);
        let new_width = ((width as f64 * scale).round() as u32).max(1);
        let new_height = ((height as f64 * scale).round() as u32).max(1);

        let mut dst_image = resize_dst
            .take()
            .filter(|img| img.width() == new_width && img.height() == new_height)
            .unwrap_or_else(|| Image::new(new_width, new_height, PixelType::U8x3));

        let src_image = ImageRef::new(width, height, rgb_data, PixelType::U8x3)
            .map_err(|e| EncoderError::message(format!("resizer src image failed: {e}")))?;

        let resize_opts = ResizeOptions::new().resize_alg(ResizeAlg::Nearest);
        resizer
            .resize(&src_image, &mut dst_image, Some(&resize_opts))
            .map_err(|e| EncoderError::message(format!("preview resize failed: {e}")))?;

        match compress_raw(
            compressor,
            jpeg_dst,
            dst_image.buffer(),
            new_width,
            new_height,
        ) {
            Ok(jpeg_data) => {
                *resize_dst = Some(dst_image);
                Ok(CompressedPreview {
                    jpeg_data,
                    width: new_width,
                    height: new_height,
                })
            }
            Err(e) => {
                *resize_dst = Some(dst_image);
                Err(e)
            }
        }
    }
}

fn preview_rgb_bytes<'a>(
    header: &CameraFrameHeader,
    frame_data: &'a [u8],
) -> Result<Cow<'a, [u8]>> {
    match header.pixel_format {
        PixelFormat::Rgb24 => {
            let expected_len = expected_frame_len(header, 3)?;
            require_frame_len(frame_data, expected_len, header.pixel_format)?;
            Ok(Cow::Borrowed(&frame_data[..expected_len]))
        }
        PixelFormat::Bgr24 => {
            let expected_len = expected_frame_len(header, 3)?;
            require_frame_len(frame_data, expected_len, header.pixel_format)?;
            let mut rgb = Vec::with_capacity(expected_len);
            for chunk in frame_data[..expected_len].chunks_exact(3) {
                rgb.extend_from_slice(&[chunk[2], chunk[1], chunk[0]]);
            }
            Ok(Cow::Owned(rgb))
        }
        PixelFormat::Gray8 => {
            let expected_len = expected_frame_len(header, 1)?;
            require_frame_len(frame_data, expected_len, header.pixel_format)?;
            let mut rgb = Vec::with_capacity(expected_len * 3);
            for &value in &frame_data[..expected_len] {
                rgb.extend_from_slice(&[value, value, value]);
            }
            Ok(Cow::Owned(rgb))
        }
        PixelFormat::Depth16 => Ok(Cow::Owned(depth16_to_rgb(header, frame_data)?)),
        PixelFormat::Yuyv | PixelFormat::Mjpeg | PixelFormat::H264AnnexB | PixelFormat::Nv12 => {
            Err(EncoderError::message(format!(
                "preview JPEG compression does not support {:?} frames; \
                 go through PreviewBuilder (raw) or the passthrough backend (H264AnnexB)",
                header.pixel_format
            )))
        }
    }
}

fn expected_frame_len(header: &CameraFrameHeader, bytes_per_pixel: usize) -> Result<usize> {
    let expected_len = header.width as usize * header.height as usize * bytes_per_pixel;
    if expected_len == 0 {
        return Err(EncoderError::message(
            "preview compression requires non-empty frame dimensions",
        ));
    }
    Ok(expected_len)
}

fn require_frame_len(
    frame_data: &[u8],
    expected_len: usize,
    pixel_format: PixelFormat,
) -> Result<()> {
    if frame_data.len() < expected_len {
        return Err(EncoderError::message(format!(
            "{pixel_format:?} frame payload too short: expected at least {expected_len} bytes, got {}",
            frame_data.len()
        )));
    }
    Ok(())
}

fn depth16_to_rgb(header: &CameraFrameHeader, frame_data: &[u8]) -> Result<Vec<u8>> {
    let expected_len = header.width as usize * header.height as usize * 2;
    if expected_len == 0 {
        return Err(EncoderError::message(
            "preview compression requires non-empty frame dimensions",
        ));
    }
    require_frame_len(frame_data, expected_len, header.pixel_format)?;
    let depth_bytes = &frame_data[..expected_len];
    let mut rgb = vec![0u8; header.width as usize * header.height as usize * 3];
    for (pixel_idx, chunk) in depth_bytes.chunks_exact(2).enumerate() {
        let depth = u16::from_le_bytes([chunk[0], chunk[1]]) as u32;
        let intensity = if depth == 0 {
            0
        } else {
            let clamped_depth = depth.min(DEPTH16_PREVIEW_REFERENCE_RAW);
            (((DEPTH16_PREVIEW_REFERENCE_RAW - clamped_depth) * 255)
                / DEPTH16_PREVIEW_REFERENCE_RAW) as u8
        };
        let offset = pixel_idx * 3;
        rgb[offset] = intensity;
        rgb[offset + 1] = intensity;
        rgb[offset + 2] = intensity;
    }
    Ok(rgb)
}

fn compress_raw<'a>(
    compressor: &mut turbojpeg::Compressor,
    jpeg_dst: &'a mut turbojpeg::OutputBuf<'static>,
    rgb_data: &[u8],
    width: u32,
    height: u32,
) -> Result<&'a [u8]> {
    compressor
        .set_subsamp(turbojpeg::Subsamp::Sub2x2)
        .map_err(|e| EncoderError::message(format!("turbojpeg set_subsamp failed: {e}")))?;
    let image = turbojpeg::Image {
        pixels: rgb_data,
        width: width as usize,
        pitch: width as usize * 3,
        height: height as usize,
        format: turbojpeg::PixelFormat::RGB,
    };
    compressor
        .compress(image, jpeg_dst)
        .map_err(|e| EncoderError::message(format!("turbojpeg compress failed: {e}")))?;
    let jpeg_bytes: &'a [u8] = &jpeg_dst[..];
    Ok(jpeg_bytes)
}
