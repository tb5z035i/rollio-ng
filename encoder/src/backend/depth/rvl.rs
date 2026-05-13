//! RVL depth encoder backend.
//!
//! Pure-Rust, CPU-only implementation. Wraps the existing
//! `RvlCodecSession` from `crate::codec`. No HW alternative exists
//! today; if a future depth codec (e.g. Zdepth, lossless WebP-as-depth)
//! lands, it joins this module's siblings under
//! `crate::backend::depth`.

use super::{DepthBackendId, DepthCodec, DepthEncoderBackend};
use crate::codec::{CodecSession, CodecSessionParams, OwnedFrame, RvlCodecSession};
use crate::error::Result;

pub struct RvlBackend;

impl DepthEncoderBackend for RvlBackend {
    fn id(&self) -> DepthBackendId {
        DepthBackendId::Rvl
    }

    fn supports(&self, codec: DepthCodec) -> bool {
        matches!(codec, DepthCodec::Rvl)
    }

    fn open_session(
        &self,
        params: &CodecSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Box<dyn CodecSession>> {
        let session = RvlCodecSession::new(params, first_frame)?;
        Ok(Box::new(session))
    }
}
