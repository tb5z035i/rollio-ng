//! Packet -> file muxer used by the LeRobot v2.1 staging path.
//!
//! Each camera channel produces one video file under
//! `videos/chunk-XXX/<channel>/episode_YYYYYY.<ext>`. The container
//! is derived from the codec via
//! [`rollio_types::config::container_for`]:
//!
//! * H.264 / H.265 / MJPG -> MP4 (mov) — see [`ffmpeg_video`].
//! * AV1 -> MKV — see [`ffmpeg_video`].
//! * RVL -> bespoke `.rvl` byte layout — see [`rvl_frame`].
//!
//! The muxer takes ownership of a [`crate::packets::RecordingStreamBuffer`]
//! that has already passed sequence validation and reached EOS. The
//! result is a fully written file at the staged path.

pub mod ffmpeg_video;
pub mod rvl_frame;

use crate::packets::RecordingStreamBuffer;
use rollio_types::config::{container_for, ContainerKind};
use std::error::Error;
use std::path::{Path, PathBuf};

/// Mux a finished per-camera packet stream into the staged video file.
///
/// `target_dir` is the staging directory; the muxer creates the
/// subdirectory tree (`videos/chunk-XXX/<channel>`) implied by
/// `relative_path` and writes the file there.
pub fn mux_camera_stream(
    target_dir: &Path,
    relative_path: &Path,
    stream: &RecordingStreamBuffer,
) -> Result<PathBuf, Box<dyn Error>> {
    let config = stream
        .config
        .as_ref()
        .ok_or("stream is missing Config packet; cannot pick container")?;
    let target = target_dir.join(relative_path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match container_for(config.codec) {
        ContainerKind::Mp4 | ContainerKind::Mkv => {
            ffmpeg_video::write_stream(&target, stream)?;
        }
        ContainerKind::RvlNative => {
            rvl_frame::write_stream(&target, stream)?;
        }
    }
    Ok(target)
}
