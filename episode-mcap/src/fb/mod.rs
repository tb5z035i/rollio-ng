//! FlatBuffer generated types for MCAP message encoding.
//!
//! These are merged flatc outputs (produced by `tools/merge_fbs.py` in the
//! mcap_spec repo). Two namespace modules:
//! - `foxglove` — standard Foxglove schema types (Time, Vector3, CompressedVideo, JointStates, …)
//! - `discover` — project-specific types (Imu, TactileData)

#[allow(unused_imports, dead_code, non_snake_case, clippy::all)]
pub mod fbs_foxglove;

#[allow(unused_imports, dead_code, non_snake_case, clippy::all)]
pub mod fbs_discover;

// Re-export the namespace modules at a convenient depth.
pub use fbs_discover::discover;
pub use fbs_foxglove::foxglove;
