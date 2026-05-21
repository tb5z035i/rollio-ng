//! Helpers built on top of the vendored flatc-generated code in `src/fbs/`.
//! The raw generated modules live at crate root (declared with `#[path]` in
//! `lib.rs`); this file exposes the small surface the validator actually uses.

use crate::fbs_foxglove::foxglove::FrameTransform;

/// Decode a flatbuffer message body as `foxglove.FrameTransform` and return
/// `(parent_frame_id, child_frame_id)`. Returns `None` if the buffer fails to
/// verify (any flatbuffer-level error). Empty / missing string fields are
/// returned as empty strings, matching the Python validator's behaviour.
pub fn read_frame_transform_pair(buf: &[u8]) -> Option<(String, String)> {
    let ft = flatbuffers::root::<FrameTransform>(buf).ok()?;
    let parent = ft.parent_frame_id().unwrap_or("").to_string();
    let child = ft.child_frame_id().unwrap_or("").to_string();
    Some((parent, child))
}
