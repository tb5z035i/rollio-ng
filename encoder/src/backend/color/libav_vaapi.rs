//! VAAPI color encoder backend.
//!
//! Phase 1 wraps the existing `LibavCodecSession` with the backend
//! pinned to `EncoderBackend::Vaapi`. The session today already wires
//! a VAAPI hw_device + hw_frames context and uploads NV12 to the GPU
//! before encode, but swscale still runs on the CPU. Phase 4 replaces
//! that with a `scale_vaapi` filter graph for the full hardware
//! pipeline.

use rollio_types::config::EncoderBackend;
use rollio_types::messages::PixelFormat;

use super::libav_cpu::{libav_codec_available, with_backend};
use super::{ColorBackendId, ColorCodec, ColorEncoderBackend};
use crate::codec::{CodecSession, CodecSessionParams, LibavCodecSession, OwnedFrame};
use crate::error::Result;

pub struct LibavVaapiBackend;

impl ColorEncoderBackend for LibavVaapiBackend {
    fn id(&self) -> ColorBackendId {
        ColorBackendId::Vaapi
    }

    fn priority(&self) -> u32 {
        // Tried after Nvidia but before Cpu under `Auto`.
        50
    }

    fn available(&self) -> bool {
        libav_codec_available(ColorCodec::H264, EncoderBackend::Vaapi)
    }

    fn supports(&self, codec: ColorCodec, input: PixelFormat) -> bool {
        if !matches!(
            codec,
            ColorCodec::H264 | ColorCodec::H265 | ColorCodec::Av1 | ColorCodec::Mjpg
        ) {
            return false;
        }
        if !matches!(
            input,
            PixelFormat::Rgb24
                | PixelFormat::Bgr24
                | PixelFormat::Yuyv
                | PixelFormat::Mjpeg
                | PixelFormat::Gray8
        ) {
            return false;
        }
        libav_codec_available(codec, EncoderBackend::Vaapi)
    }

    fn open_session(
        &self,
        params: &CodecSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Box<dyn CodecSession>> {
        let pinned = with_backend(params, EncoderBackend::Vaapi);
        let session = LibavCodecSession::new(&pinned, first_frame)?;
        Ok(Box::new(session))
    }
}
