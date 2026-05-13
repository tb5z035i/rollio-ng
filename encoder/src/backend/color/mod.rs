//! Color-channel encoder backends.
//!
//! A `ColorEncoderBackend` knows how to turn raw camera frames
//! (`Rgb24` / `Bgr24` / `Yuyv` / `Mjpeg` / `Gray8`) — and, in a future
//! phase, pre-encoded `H264AnnexB` — into encoded packets via one of
//! the supported color codecs (H.264 / H.265 / AV1 / MJPG).
//!
//! Backends are registered at process start in
//! [`ColorBackendRegistry::default_set`]. Resolution at session-open
//! time is driven by the user's `[encoder.preview] backend` (or
//! `[encoder] backend` for the recording role):
//!
//! - `EncoderBackend::Auto` walks the registry in priority order
//!   (Nvidia > Vaapi > Cpu) and picks the first that reports
//!   `available() && supports(codec, input)`.
//! - An explicit backend name routes directly to the matching impl and
//!   errors if it's not present or doesn't support the requested combo
//!   — fail loudly so config typos surface immediately instead of
//!   silently falling back to the wrong path.
//!
//! Phase 1 (this commit) hosts three thin backend wrappers around the
//! existing `LibavCodecSession`. Phases 3 and 4 swap in real
//! hardware-accelerated pipelines (NVDEC + scale_cuda + NVENC for
//! NVIDIA; corresponding VAAPI filter graph for Intel/AMD) inside the
//! same trait surface.

pub mod libav_cpu;
pub mod libav_nvidia;
pub mod libav_vaapi;
pub mod passthrough;

use std::sync::{Arc, OnceLock};

use rollio_types::config::{EncoderBackend, EncoderCodec};
use rollio_types::messages::PixelFormat;

use crate::codec::{CodecSession, CodecSessionParams, OwnedFrame};
use crate::error::{EncoderError, Result};

/// Stable identifier for each registered color backend. Maps onto
/// `EncoderBackend` from the project config; only color-eligible
/// variants appear here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorBackendId {
    Cpu,
    Nvidia,
    Vaapi,
    Passthrough,
}

impl ColorBackendId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Nvidia => "nvidia",
            Self::Vaapi => "vaapi",
            Self::Passthrough => "passthrough",
        }
    }
}

/// Runtime-only refinement of `EncoderCodec` that excludes depth
/// codecs. Color backends never see `EncoderCodec::Rvl` because the
/// depth registry handles depth dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorCodec {
    H264,
    H265,
    Av1,
    Mjpg,
}

impl TryFrom<EncoderCodec> for ColorCodec {
    type Error = EncoderError;

    fn try_from(value: EncoderCodec) -> Result<Self> {
        match value {
            EncoderCodec::H264 => Ok(Self::H264),
            EncoderCodec::H265 => Ok(Self::H265),
            EncoderCodec::Av1 => Ok(Self::Av1),
            EncoderCodec::Mjpg => Ok(Self::Mjpg),
            EncoderCodec::Rvl => Err(EncoderError::message(
                "RVL is a depth codec; not routable to a color backend",
            )),
        }
    }
}

impl From<ColorCodec> for EncoderCodec {
    fn from(value: ColorCodec) -> Self {
        match value {
            ColorCodec::H264 => Self::H264,
            ColorCodec::H265 => Self::H265,
            ColorCodec::Av1 => Self::Av1,
            ColorCodec::Mjpg => Self::Mjpg,
        }
    }
}

/// Implementation contract for one color-side encoder backend.
///
/// Implementations are stateless singletons held in an `Arc` inside
/// the registry. The per-session state (encoder context, scaler,
/// MJPEG decoder, hardware-frames context, …) lives entirely inside
/// the `Box<dyn CodecSession>` returned by [`open_session`].
pub trait ColorEncoderBackend: Send + Sync {
    /// Stable identifier for logging and `EncoderBackend` mapping.
    fn id(&self) -> ColorBackendId;

    /// Higher value = tried first under `EncoderBackend::Auto`.
    /// Convention: 100 = NVIDIA, 50 = VAAPI, 10 = CPU. Future Horizon
    /// X5 picks its own number to slot wherever appropriate for the
    /// target deployment.
    fn priority(&self) -> u32;

    /// Cheap runtime probe — does this host actually have the
    /// hardware/libraries to run this backend? Called once per Auto
    /// resolution; should not perform expensive ffmpeg lookups.
    fn available(&self) -> bool;

    /// Whether this backend can encode `codec` from `input` frames.
    fn supports(&self, codec: ColorCodec, input: PixelFormat) -> bool;

    /// Construct a session that will accept frames matching
    /// `first_frame.header.pixel_format` (any subsequent format change
    /// is rejected by the session's frame-compatibility check).
    fn open_session(
        &self,
        params: &CodecSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Box<dyn CodecSession>>;
}

/// Holder of every registered color backend. Looked up via
/// [`ColorBackendRegistry::global`] (or constructed standalone in
/// tests).
pub struct ColorBackendRegistry {
    backends: Vec<Arc<dyn ColorEncoderBackend>>,
}

static REGISTRY: OnceLock<ColorBackendRegistry> = OnceLock::new();

impl ColorBackendRegistry {
    /// Process-wide singleton, initialized on first access with the
    /// default backend set.
    pub fn global() -> &'static ColorBackendRegistry {
        REGISTRY.get_or_init(Self::default_set)
    }

    /// The bundled backend set. Passthrough sits at the top of the
    /// priority list so under `Auto`, an H264AnnexB-in / H264-out
    /// stream gets relayed verbatim instead of bouncing through an
    /// unnecessary transcode.
    pub fn default_set() -> Self {
        let mut backends: Vec<Arc<dyn ColorEncoderBackend>> = vec![
            Arc::new(passthrough::PassthroughBackend),
            Arc::new(libav_nvidia::LibavNvidiaBackend),
            Arc::new(libav_vaapi::LibavVaapiBackend),
            Arc::new(libav_cpu::LibavCpuBackend),
        ];
        backends.sort_by_key(|b| std::cmp::Reverse(b.priority()));
        Self { backends }
    }

    pub fn backends(&self) -> &[Arc<dyn ColorEncoderBackend>] {
        &self.backends
    }

    /// Open a session for a color frame. `backend_hint` honours the
    /// project config: `Auto` walks the priority list; anything else
    /// routes directly and errors if unavailable.
    pub fn open(
        &self,
        codec: ColorCodec,
        backend_hint: EncoderBackend,
        params: &CodecSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Box<dyn CodecSession>> {
        let input = first_frame.header.pixel_format;
        match backend_hint {
            EncoderBackend::Auto => {
                for backend in &self.backends {
                    if backend.available() && backend.supports(codec, input) {
                        return backend.open_session(params, first_frame);
                    }
                }
                Err(EncoderError::message(format!(
                    "no color backend available for codec={:?} input={:?}",
                    codec, input
                )))
            }
            specific => {
                let target = color_backend_id_from_config(specific)?;
                let backend = self
                    .backends
                    .iter()
                    .find(|b| b.id() == target)
                    .ok_or_else(|| {
                        EncoderError::message(format!(
                            "color backend {:?} not registered",
                            specific
                        ))
                    })?;
                if !backend.available() {
                    return Err(EncoderError::message(format!(
                        "color backend {} is not available on this host",
                        target.as_str()
                    )));
                }
                if !backend.supports(codec, input) {
                    return Err(EncoderError::message(format!(
                        "color backend {} does not support codec={:?} input={:?}",
                        target.as_str(),
                        codec,
                        input
                    )));
                }
                backend.open_session(params, first_frame)
            }
        }
    }
}

fn color_backend_id_from_config(value: EncoderBackend) -> Result<ColorBackendId> {
    match value {
        EncoderBackend::Cpu => Ok(ColorBackendId::Cpu),
        EncoderBackend::Nvidia => Ok(ColorBackendId::Nvidia),
        EncoderBackend::Vaapi => Ok(ColorBackendId::Vaapi),
        EncoderBackend::Passthrough => Ok(ColorBackendId::Passthrough),
        EncoderBackend::Auto => Err(EncoderError::message(
            "color_backend_id_from_config: Auto is not a concrete backend",
        )),
    }
}
