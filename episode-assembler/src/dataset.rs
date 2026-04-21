//! Shared episode-assembler data structures and the top-level
//! `stage_episode` orchestrator.
//!
//! The two output flavours — the LeRobot v2.1 row-aligned view and the
//! per-channel raw Parquet dump — are owned by their dedicated sibling
//! modules:
//!
//! * `crate::lerobot` materialises the canonical row table (lossy; LeRobot
//!   convention).
//! * `crate::raw` writes the full high-frequency samples losslessly.
//!
//! `stage_episode` invokes both and moves any encoder-produced video files
//! into the staging directory tree. Storage's `merge_tree_if_present`
//! recursion picks up `data/`, `videos/`, and `raw/` together when the
//! episode is finalised.

use crate::lerobot;
use crate::raw;
use rollio_types::config::{AssemblerRuntimeConfigV2, RobotCommandKind, RobotStateKind};
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
    pub action_samples: BTreeMap<String, Vec<ActionSample>>,
    pub video_paths: BTreeMap<String, PathBuf>,
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

    // 2. Move encoder-produced VFR videos into `videos/...`.
    for camera in &config.cameras {
        let source = episode
            .video_paths
            .get(&camera.channel_id)
            .ok_or_else(|| format!("missing video for camera {}", camera.channel_id))?;
        let extension = source
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| camera.artifact_format.extension().to_string());
        let target = lerobot::staged_video_path(
            &staging_dir,
            episode.episode_index,
            config.chunk_size,
            &camera.channel_id,
            &extension,
        );
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        move_or_copy_file(source, &target)?;
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

pub(crate) fn action_key(channel_id: &str, command_kind: RobotCommandKind) -> String {
    format!("{channel_id}/{}", command_kind.topic_suffix())
}

pub(crate) fn sanitize_component(value: &str) -> String {
    value.replace('/', "__")
}

fn move_or_copy_file(source: &Path, target: &Path) -> Result<(), Box<dyn Error>> {
    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(_) => {
            fs::copy(source, target)?;
            fs::remove_file(source)?;
            Ok(())
        }
    }
}

pub(crate) fn remove_episode_artifacts(episode: &EpisodeAssemblyInput) {
    for path in episode.video_paths.values() {
        let _ = fs::remove_file(path);
    }
}
