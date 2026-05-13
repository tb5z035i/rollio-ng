//! CPU (software) color encoder backend.
//!
//! Wraps the existing libav-based `LibavCodecSession` with the backend
//! pinned to `EncoderBackend::Cpu`. The session itself decodes MJPEG
//! via libav's CPU MJPEG decoder, runs swscale, and hands NV12 / YUV
//! frames to `libx264` / `libx265` / `libsvtav1` / `mjpeg`.
//!
//! Phase 1: this is the literal previous CPU path with no behavior
//! change — `LibavCodecSession::new` is reused verbatim. Future
//! phases will not modify this backend; the hardware-accelerated work
//! lives in the NVIDIA and VAAPI sibling modules.

use rollio_types::config::{EncoderBackend, EncoderCodec};
use rollio_types::messages::PixelFormat;

use super::{ColorBackendId, ColorCodec, ColorEncoderBackend};
use crate::codec::{CodecSession, CodecSessionParams, LibavCodecSession, OwnedFrame};
use crate::error::Result;

pub struct LibavCpuBackend;

impl ColorEncoderBackend for LibavCpuBackend {
    fn id(&self) -> ColorBackendId {
        ColorBackendId::Cpu
    }

    fn priority(&self) -> u32 {
        // Last resort under `Auto` — every host has a CPU.
        10
    }

    fn available(&self) -> bool {
        // libav is linked statically; CPU is always available.
        true
    }

    fn supports(&self, codec: ColorCodec, input: PixelFormat) -> bool {
        // Every color codec is reachable via libav's software encoders;
        // MJPG / YUYV / RGB / BGR / Gray8 are all handled by the
        // existing decode-or-copy + swscale path in `LibavCodecSession`.
        matches!(
            codec,
            ColorCodec::H264 | ColorCodec::H265 | ColorCodec::Av1 | ColorCodec::Mjpg
        ) && matches!(
            input,
            PixelFormat::Rgb24
                | PixelFormat::Bgr24
                | PixelFormat::Yuyv
                | PixelFormat::Mjpeg
                | PixelFormat::Gray8
        )
    }

    fn open_session(
        &self,
        params: &CodecSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Box<dyn CodecSession>> {
        // Pin the backend so `media::resolve_backend` inside
        // `LibavCodecSession::new` is a no-op rather than another
        // round of Auto resolution.
        let pinned = with_backend(params, EncoderBackend::Cpu);
        let session = LibavCodecSession::new(&pinned, first_frame)?;
        Ok(Box::new(session))
    }
}

/// Build a copy of `params` with `backend` overridden. Used by every
/// backend impl to fix `Auto` to its concrete identifier before
/// handing control to `LibavCodecSession::new`.
pub(crate) fn with_backend<'a>(
    params: &CodecSessionParams<'a>,
    backend: EncoderBackend,
) -> CodecSessionParams<'a> {
    CodecSessionParams {
        codec: params.codec,
        backend,
        fps: params.fps,
        crf: params.crf,
        preset: params.preset,
        tune: params.tune,
        bit_depth: params.bit_depth,
        chroma_subsampling: params.chroma_subsampling,
        color_space: params.color_space,
        process_id: params.process_id,
        episode_index: params.episode_index,
        recording_start_us: params.recording_start_us,
        output_width: params.output_width,
        output_height: params.output_height,
        allow_rescale: params.allow_rescale,
    }
}

/// Whether `codec` has an available libav encoder on this host for the
/// given libav-side backend. Shared by the three Libav color backends
/// so each one can implement `available()` without duplicating the
/// codec-name lookup.
pub(crate) fn libav_codec_available(codec: ColorCodec, backend: EncoderBackend) -> bool {
    let codec: EncoderCodec = codec.into();
    crate::media::select_encoder_name(codec, backend).is_some()
}
