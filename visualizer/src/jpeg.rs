/// SIMD-accelerated JPEG compression using libjpeg-turbo (via turbojpeg crate)
/// and fast_image_resize for image downsampling.
///
/// The `JpegCompressor` struct reuses its internal compressor and resize buffer
/// across frames to avoid per-frame allocation overhead.
use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{PixelType, ResizeAlg, ResizeOptions, Resizer};

/// Wraps a turbojpeg Compressor with a reusable resize buffer for high-throughput
/// RGB24 → JPEG compression with optional downsampling.
pub struct JpegCompressor {
    compressor: turbojpeg::Compressor,
    resizer: Resizer,
    /// Reused destination buffer when downsampling; reallocated only when dimensions change.
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

    /// Compress an RGB24 buffer to JPEG and return a borrowed view of the
    /// reusable output buffer.
    ///
    /// If the source frame exceeds the preview bounds, it is first downsampled
    /// (maintaining aspect ratio) using a fast nearest-neighbor path, then
    /// compressed.
    pub fn compress<'a>(
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

/// Compress raw RGB24 data to JPEG without resizing.
fn compress_raw<'a>(
    compressor: &mut turbojpeg::Compressor,
    jpeg_dst: &'a mut turbojpeg::OutputBuf<'static>,
    rgb_data: &[u8],
    width: u32,
    height: u32,
) -> Result<&'a [u8], Box<dyn std::error::Error>> {
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
