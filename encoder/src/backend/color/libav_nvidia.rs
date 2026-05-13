//! NVIDIA color encoder backend.
//!
//! Phase 1 wraps the existing `LibavCodecSession` with the backend
//! pinned to `EncoderBackend::Nvidia`. The current session still does
//! CPU MJPEG decode + swscale before handing NV12 to NVENC — exactly
//! what produces the ~15% CPU draw on a 1080p30 source. Phase 3 swaps
//! the body of this backend for the full hardware pipeline (NVDEC +
//! `scale_cuda` filter graph + NVENC with `hw_frames_ctx`), keeping
//! the trait surface unchanged.

use rollio_types::config::EncoderBackend;
use rollio_types::messages::PixelFormat;

use super::libav_cpu::{libav_codec_available, with_backend};
use super::{ColorBackendId, ColorCodec, ColorEncoderBackend};
use crate::codec::{CodecSession, CodecSessionParams, LibavCodecSession, OwnedFrame};
use crate::error::Result;

pub struct LibavNvidiaBackend;

impl ColorEncoderBackend for LibavNvidiaBackend {
    fn id(&self) -> ColorBackendId {
        ColorBackendId::Nvidia
    }

    fn priority(&self) -> u32 {
        // Tried first under `Auto`.
        100
    }

    fn available(&self) -> bool {
        // Probe via the existing libav encoder-name lookup. If
        // `h264_nvenc` is registered, NVENC is present on this host.
        // Cheap and deterministic; no CUDA context creation.
        libav_codec_available(ColorCodec::H264, EncoderBackend::Nvidia)
    }

    fn supports(&self, codec: ColorCodec, input: PixelFormat) -> bool {
        // NVENC has all four H.26x/AV1/MJPG encoders in modern libav
        // builds. Input format set matches the CPU backend; phase 3
        // adds NVDEC (`*_cuvid`) decode for compressed inputs that
        // changes only the open_session body, not this matrix.
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
        libav_codec_available(codec, EncoderBackend::Nvidia)
    }

    fn open_session(
        &self,
        params: &CodecSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Box<dyn CodecSession>> {
        let pinned = with_backend(params, EncoderBackend::Nvidia);
        let session = LibavCodecSession::new(&pinned, first_frame)?;
        Ok(Box::new(session))
    }
}
