//! Shared LeRobot episode staging data structures and the top-level
//! `stage_episode` orchestrator.
//!
//! This module no longer assumes the encoder produced finished video
//! files: every per-camera video is muxed in-process from the
//! recording packet stream gathered by [`crate::runtime`]. Storage's
//! `merge_tree_if_present` recursion picks up `data/`, `videos/`,
//! `raw/`, and `meta/` together when the episode is finalised.

use crate::lerobot;
use crate::muxer;
use crate::packets::RecordingStreamBuffer;
use crate::raw;
use rollio_types::config::{
    container_for, AssemblerRuntimeConfigV2, RobotCommandKind, RobotStateKind, SensorStateKind,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct ObservationSample {
    pub timestamp_us: u64,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone)]
pub(crate) struct SensorSample {
    pub timestamp_us: u64,
    pub values: Vec<f32>,
}

#[derive(Debug, Clone)]
pub(crate) struct ActionSample {
    pub timestamp_us: u64,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone)]
pub(crate) struct EpisodeAssemblyInput {
    pub episode_index: u32,
    pub start_time_us: u64,
    pub stop_time_us: u64,
    pub observation_samples: BTreeMap<String, Vec<ObservationSample>>,
    pub sensor_samples: BTreeMap<String, Vec<SensorSample>>,
    pub action_samples: BTreeMap<String, Vec<ActionSample>>,
    /// Per-channel packet stream gathered by the assembler runtime.
    /// Each buffer must have its `Config` set, all packets in
    /// strictly-monotonic order, and `eos_received = true`. The muxer
    /// rejects buffers that don't satisfy those invariants.
    pub camera_streams: BTreeMap<String, RecordingStreamBuffer>,
}

#[derive(Debug, Clone)]
pub(crate) struct StagedEpisode {
    pub episode_index: u32,
    pub staging_dir: PathBuf,
}

pub(crate) fn stage_episode(
    config: &AssemblerRuntimeConfigV2,
    episode: &EpisodeAssemblyInput,
) -> Result<StagedEpisode, Box<dyn Error>> {
    let staging_dir =
        Path::new(&config.staging_dir).join(format!("episode_{:06}", episode.episode_index));
    if staging_dir.exists() {
        fs::remove_dir_all(&staging_dir)?;
    }
    fs::create_dir_all(&staging_dir)?;

    // 1. LeRobot v2.1 row-aligned Parquet (lossy; downsampled to fps).
    let parquet_path =
        lerobot::staged_parquet_path(&staging_dir, episode.episode_index, config.chunk_size);
    let frame_count = lerobot::write_episode_parquet(&parquet_path, config, episode)?;

    // 2. Mux per-camera packets into the staged video container.
    for camera in &config.cameras {
        let stream = episode
            .camera_streams
            .get(&camera.channel_id)
            .ok_or_else(|| {
                format!(
                    "stage_episode: missing recording stream for camera {}",
                    camera.channel_id
                )
            })?;
        if !stream.is_complete() {
            return Err(format!(
                "stage_episode: camera {} stream incomplete (failed={:?}, eos={})",
                camera.channel_id, stream.failed, stream.eos_received,
            )
            .into());
        }
        let extension = container_for(camera.codec).extension();
        let relative = lerobot::staged_video_relative_path(
            episode.episode_index,
            config.chunk_size,
            &camera.channel_id,
            extension,
        );
        muxer::mux_camera_stream(&staging_dir, &relative, stream)?;
    }

    // 3. Per-channel raw Parquet (lossless; complete bus history).
    raw::write_episode_raw_dump(&staging_dir, config, episode)?;

    // 4. Dataset metadata. Written last so it can advertise the row count
    //    and the raw_path the previous steps just produced.
    lerobot::write_stage_metadata(&staging_dir, config, episode, frame_count)?;

    Ok(StagedEpisode {
        episode_index: episode.episode_index,
        staging_dir,
    })
}

pub(crate) fn observation_key(channel_id: &str, state_kind: RobotStateKind) -> String {
    format!("{channel_id}/{}", state_kind.topic_suffix())
}

pub(crate) fn sensor_observation_key(channel_id: &str, sensor_kind: SensorStateKind) -> String {
    format!("sensor/{channel_id}/{}", sensor_kind.topic_suffix())
}

pub(crate) fn action_key(channel_id: &str, command_kind: RobotCommandKind) -> String {
    format!("{channel_id}/{}", command_kind.topic_suffix())
}

pub(crate) fn sanitize_component(value: &str) -> String {
    value.replace('/', "__")
}

/// Best-effort cleanup of any partially-written staging dir for an
/// episode that the assembler has decided to drop. Returns no errors:
/// callers use this on the failure / discard path so leftover bytes
/// don't pollute the staging tree.
pub(crate) fn remove_episode_artifacts(staging_root: &str, episode_index: u32) {
    let dir = Path::new(staging_root).join(format!("episode_{episode_index:06}"));
    if dir.exists() {
        let _ = fs::remove_dir_all(dir);
    }
}
