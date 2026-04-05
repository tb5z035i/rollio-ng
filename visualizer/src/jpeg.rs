/// SIMD-accelerated JPEG compression using libjpeg-turbo (via turbojpeg crate)
/// and fast_image_resize for image downsampling.
///
/// The `JpegCompressor` struct reuses its internal compressor and resize buffer
/// across frames to avoid per-frame allocation overhead.

use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};

/// Wraps a turbojpeg Compressor with a reusable resize buffer for high-throughput
/// RGB24 → JPEG compression with optional downsampling.
pub struct JpegCompressor {
    compressor: turbojpeg::Compressor,
    resizer: Resizer,
    /// Reused destination buffer when downsampling; reallocated only when dimensions change.
    resize_dst: Option<Image<'static>>,
}

impl JpegCompressor {
    /// Create a new compressor with the given JPEG quality (1-100).
    /// Uses 4:2:0 chroma subsampling for optimal compression ratio.
    pub fn new(quality: i32) -> Result<Self, Box<dyn std::error::Error>> {
        let mut compressor = turbojpeg::Compressor::new()?;
        compressor.set_quality(quality)?;
        compressor.set_subsamp(turbojpeg::Subsamp::Sub2x2)?;

        let resizer = Resizer::new();

        Ok(Self {
            compressor,
            resizer,
            resize_dst: None,
        })
    }

    /// Compress an RGB24 buffer to JPEG.
    ///
    /// If `width > max_width`, the image is first downsampled (maintaining aspect ratio)
    /// using SIMD-accelerated bilinear filtering, then compressed.
    pub fn compress(
        &mut self,
        rgb_data: &[u8],
        width: u32,
        height: u32,
        max_width: u32,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        if width <= max_width {
            return self.compress_raw(rgb_data, width, height);
        }

        let scale = max_width as f64 / width as f64;
        let new_width = max_width;
        let new_height = ((height as f64 * scale) as u32).max(1);

        let mut dst_image = self
            .resize_dst
            .take()
            .filter(|img| img.width() == new_width && img.height() == new_height)
            .unwrap_or_else(|| Image::new(new_width, new_height, PixelType::U8x3));

        let src_image = ImageRef::new(width, height, rgb_data, PixelType::U8x3)?;

        let resize_opts =
            ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Bilinear));
        self.resizer
            .resize(&src_image, &mut dst_image, Some(&resize_opts))?;

        match self.compress_raw(dst_image.buffer(), new_width, new_height) {
            Ok(jpeg_data) => {
                self.resize_dst = Some(dst_image);
                Ok(jpeg_data)
            }
            Err(e) => {
                self.resize_dst = Some(dst_image);
                Err(e)
            }
        }
    }

    /// Compress raw RGB24 data to JPEG without resizing.
    fn compress_raw(
        &mut self,
        rgb_data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let image = turbojpeg::Image {
            pixels: rgb_data,
            width: width as usize,
            pitch: width as usize * 3,
            height: height as usize,
            format: turbojpeg::PixelFormat::RGB,
        };

        Ok(self.compressor.compress_to_vec(image)?)
    }
}
