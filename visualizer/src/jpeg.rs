/// SIMD-accelerated preview JPEG compression using libjpeg-turbo (via
/// turbojpeg crate) and fast_image_resize for image downsampling.
///
/// The `JpegCompressor` struct reuses its internal compressor and resize buffer
/// across frames to avoid per-frame allocation overhead.
use std::borrow::Cow;

use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{PixelType, ResizeAlg, ResizeOptions, Resizer};
use rollio_types::messages::{CameraFrameHeader, PixelFormat};

/// Wraps a turbojpeg Compressor with a reusable resize buffer for high-throughput
/// RGB preview JPEG compression with optional downsampling.
pub struct JpegCompressor {
    compressor: turbojpeg::Compressor,
    resizer: Resizer,
    /// Reused RGB destination buffer when downsampling; reallocated only when dimensions change.
    resize_dst: Option<Image<'static>>,
    /// Reused JPEG output buffer to avoid per-frame allocation and copy churn.
    jpeg_dst: turbojpeg::OutputBuf<'static>,
}

pub struct CompressedPreview<'a> {
    pub jpeg_data: &'a [u8],
    pub width: u32,
    pub height: u32,
}

impl JpegCompressor {
    /// Create a new compressor with the given JPEG quality (1-100).
    /// Uses 4:2:0 chroma subsampling for optimal compression ratio.
    pub fn new(quality: i32) -> Result<Self, Box<dyn std::error::Error>> {
        let mut compressor = turbojpeg::Compressor::new()?;
        compressor.set_quality(quality)?;
        compressor.set_subsamp(turbojpeg::Subsamp::Sub2x2)?;
        compressor.set_optimize(false)?;

        let resizer = Resizer::new();

        Ok(Self {
            compressor,
            resizer,
            resize_dst: None,
            jpeg_dst: turbojpeg::OutputBuf::new_owned(),
        })
    }

    /// Convert a camera frame to RGB preview pixels and compress it to JPEG.
    pub fn compress_frame<'a>(
        &'a mut self,
        header: &CameraFrameHeader,
        frame_data: &[u8],
        max_width: u32,
        max_height: u32,
    ) -> Result<CompressedPreview<'a>, Box<dyn std::error::Error>> {
        let preview_pixels = preview_rgb_bytes(header, frame_data)?;
        self.compress(
            preview_pixels.as_ref(),
            header.width,
            header.height,
            max_width,
            max_height,
        )
    }

    /// Compress an RGB preview pixel buffer to JPEG and return a borrowed view of the
    /// reusable output buffer.
    ///
    /// If the source frame exceeds the preview bounds, it is first downsampled
    /// (maintaining aspect ratio) using a fast nearest-neighbor path, then
    /// compressed.
    fn compress<'a>(
        &'a mut self,
        rgb_data: &[u8],
        width: u32,
        height: u32,
        max_width: u32,
        max_height: u32,
    ) -> Result<CompressedPreview<'a>, Box<dyn std::error::Error>> {
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

        let src_image = ImageRef::new(width, height, rgb_data, PixelType::U8x3)?;

        let resize_opts = ResizeOptions::new().resize_alg(ResizeAlg::Nearest);
        resizer.resize(&src_image, &mut dst_image, Some(&resize_opts))?;

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
) -> Result<Cow<'a, [u8]>, Box<dyn std::error::Error>> {
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
        PixelFormat::Yuyv | PixelFormat::Mjpeg => Err(format!(
            "preview compression does not support {:?} frames yet",
            header.pixel_format
        )
        .into()),
    }
}

fn expected_frame_len(
    header: &CameraFrameHeader,
    bytes_per_pixel: usize,
) -> Result<usize, Box<dyn std::error::Error>> {
    let expected_len = header.width as usize * header.height as usize * bytes_per_pixel;
    if expected_len == 0 {
        return Err("preview compression requires non-empty frame dimensions".into());
    }
    Ok(expected_len)
}

fn require_frame_len(
    frame_data: &[u8],
    expected_len: usize,
    pixel_format: PixelFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    if frame_data.len() < expected_len {
        return Err(format!(
            "{pixel_format:?} frame payload too short: expected at least {expected_len} bytes, got {}",
            frame_data.len()
        )
        .into());
    }
    Ok(())
}

// RealSense D435 Z16 depth uses a 0.001 m scale by default, so a raw value of
// 1000 corresponds to ~1.0 m. Keeping the preview reference fixed avoids
// frame-to-frame flicker from per-frame normalization.
const DEPTH16_PREVIEW_REFERENCE_RAW: u32 = 1000;

fn depth16_to_rgb(
    header: &CameraFrameHeader,
    frame_data: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let expected_len = header.width as usize * header.height as usize * 2;
    if expected_len == 0 {
        return Err("preview compression requires non-empty frame dimensions".into());
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

/// Compress raw RGB24 preview data to JPEG without resizing.
fn compress_raw<'a>(
    compressor: &mut turbojpeg::Compressor,
    jpeg_dst: &'a mut turbojpeg::OutputBuf<'static>,
    rgb_data: &[u8],
    width: u32,
    height: u32,
) -> Result<&'a [u8], Box<dyn std::error::Error>> {
    compressor.set_subsamp(turbojpeg::Subsamp::Sub2x2)?;
    let image = turbojpeg::Image {
        pixels: rgb_data,
        width: width as usize,
        pitch: width as usize * 3,
        height: height as usize,
        format: turbojpeg::PixelFormat::RGB,
    };

    compressor.compress(image, jpeg_dst)?;
    let jpeg_bytes: &'a [u8] = &jpeg_dst[..];
    Ok(jpeg_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gray8_frames_expand_to_rgb() {
        let header = CameraFrameHeader {
            width: 2,
            height: 1,
            pixel_format: PixelFormat::Gray8,
            ..CameraFrameHeader::default()
        };
        let preview = preview_rgb_bytes(&header, &[16, 32]).expect("gray8 preview should convert");
        assert_eq!(preview.as_ref(), &[16, 16, 16, 32, 32, 32]);
    }

    #[test]
    fn depth16_frames_use_fixed_one_meter_reference() {
        let header = CameraFrameHeader {
            width: 2,
            height: 2,
            pixel_format: PixelFormat::Depth16,
            ..CameraFrameHeader::default()
        };
        let frame = [
            0u16.to_le_bytes(),
            250u16.to_le_bytes(),
            500u16.to_le_bytes(),
            1000u16.to_le_bytes(),
        ]
        .concat();

        let preview = preview_rgb_bytes(&header, &frame).expect("depth16 preview should convert");
        let pixels = preview.as_ref();

        assert_eq!(&pixels[0..3], &[0, 0, 0]);
        assert_eq!(&pixels[3..6], &[191, 191, 191]);
        assert_eq!(&pixels[6..9], &[127, 127, 127]);
        assert_eq!(&pixels[9..12], &[0, 0, 0]);
    }

    #[test]
    fn depth16_frames_compress_after_grayscale_rgb_mapping() {
        let header = CameraFrameHeader {
            width: 640,
            height: 480,
            pixel_format: PixelFormat::Depth16,
            ..CameraFrameHeader::default()
        };
        let mut frame = vec![0u8; header.width as usize * header.height as usize * 2];
        for (index, chunk) in frame.chunks_exact_mut(2).enumerate() {
            let depth = 500u16 + (index % 2048) as u16;
            chunk.copy_from_slice(&depth.to_le_bytes());
        }

        let mut compressor = JpegCompressor::new(30).expect("compressor should initialize");
        let preview = compressor
            .compress_frame(&header, &frame, 320, 240)
            .expect("depth16 frame should compress");

        assert_eq!(preview.width, 320);
        assert_eq!(preview.height, 240);
        assert!(!preview.jpeg_data.is_empty());
    }
}
