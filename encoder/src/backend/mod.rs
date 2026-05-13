//! Pluggable encoder-backend traits.
//!
//! Cameras emit one of several `PixelFormat`s. The encoder routes each
//! frame through either a *color* backend (anything that isn't depth)
//! or a *depth* backend (Depth16). Each side has its own trait so the
//! color backends never carry RVL knowledge and depth backends never
//! carry H.264/HEVC/AV1 knowledge.
//!
//! Both traits produce the same downstream `Box<dyn CodecSession>`, so
//! the runtime layer (`preview_runtime` / `recording_runtime`) is
//! oblivious to which trait opened the session.
//!
//! Adding a new vendor (e.g. Horizon X5) means writing one backend
//! module in the appropriate `color/` or `depth/` subtree and adding
//! one entry to the registry's `default_set()` — no central match arms
//! to update.

pub mod bsf;
pub mod color;
pub mod depth;
pub mod filter_graph;
