//! Depth-channel encoder backends.
//!
//! The depth side is intentionally separate from the color side: it
//! carries its own typed codec enum (`DepthCodec`), its own registry,
//! and its own trait. No color backend ever sees `DepthCodec::Rvl`,
//! and no depth backend has to care about H.264/HEVC/AV1.
//!
//! Today the only registered backend is `RvlBackend` (pure-Rust,
//! CPU-only). Future depth codecs (Zstd16, lossless WebP-as-depth,
//! Zdepth) plug in by adding a sibling module + one registry entry —
//! the color backends are untouched.

pub mod rvl;

use std::sync::{Arc, OnceLock};

use rollio_types::config::EncoderCodec;

use crate::codec::{CodecSession, CodecSessionParams, OwnedFrame};
use crate::error::{EncoderError, Result};

/// Stable identifier for each registered depth backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DepthBackendId {
    Rvl,
}

impl DepthBackendId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rvl => "rvl",
        }
    }
}

/// Runtime-only refinement of `EncoderCodec` containing only depth
/// codecs. Conversion from `EncoderCodec` errors on non-depth codecs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthCodec {
    Rvl,
}

impl TryFrom<EncoderCodec> for DepthCodec {
    type Error = EncoderError;

    fn try_from(value: EncoderCodec) -> Result<Self> {
        match value {
            EncoderCodec::Rvl => Ok(Self::Rvl),
            other => Err(EncoderError::message(format!(
                "{} is a color codec; not routable to a depth backend",
                other.as_str()
            ))),
        }
    }
}

impl From<DepthCodec> for EncoderCodec {
    fn from(value: DepthCodec) -> Self {
        match value {
            DepthCodec::Rvl => Self::Rvl,
        }
    }
}

/// Implementation contract for one depth-side encoder backend.
///
/// Depth backends always receive `PixelFormat::Depth16` frames; the
/// input format is therefore not part of `supports()`.
pub trait DepthEncoderBackend: Send + Sync {
    fn id(&self) -> DepthBackendId;

    /// Whether this backend can encode `codec`.
    fn supports(&self, codec: DepthCodec) -> bool;

    fn open_session(
        &self,
        params: &CodecSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Box<dyn CodecSession>>;
}

/// Holder of every registered depth backend.
pub struct DepthBackendRegistry {
    backends: Vec<Arc<dyn DepthEncoderBackend>>,
}

static REGISTRY: OnceLock<DepthBackendRegistry> = OnceLock::new();

impl DepthBackendRegistry {
    pub fn global() -> &'static DepthBackendRegistry {
        REGISTRY.get_or_init(Self::default_set)
    }

    pub fn default_set() -> Self {
        let backends: Vec<Arc<dyn DepthEncoderBackend>> = vec![Arc::new(rvl::RvlBackend)];
        Self { backends }
    }

    pub fn backends(&self) -> &[Arc<dyn DepthEncoderBackend>] {
        &self.backends
    }

    /// Open a depth session. Depth has no `Auto` resolution today
    /// because only one backend exists; once that changes we'll wire
    /// priority logic mirroring the color side.
    pub fn open(
        &self,
        codec: DepthCodec,
        params: &CodecSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Box<dyn CodecSession>> {
        let backend = self
            .backends
            .iter()
            .find(|b| b.supports(codec))
            .ok_or_else(|| {
                EncoderError::message(format!("no depth backend supports codec={:?}", codec))
            })?;
        backend.open_session(params, first_frame)
    }
}
