//! LeRobot v2.1 row-aligned view of an assembled episode.
//!
//! This module owns the *lossy* conversion from the raw bus samples into a
//! row-per-frame Parquet table that LeRobot tooling expects:
//!
//! * Row times are the **canonical** timeline `controller_start_us + i * 1e6 / fps`
//!   for `i in 0..duration_us * fps / 1e6`. The Parquet `timestamp` column
//!   holds seconds **relative to `controller_start_us`** starting at 0.0
//!   (matching the LeRobot v2.1 convention).
//! * Robot states / actions are sampled nearest-prior-or-equal at each row
//!   instant.
//!
//! The downsampling from 250 Hz state samples to 30 fps row entries is the
//! known "loss point B" — by design for LeRobot v2.1. The full
//! high-frequency samples are persisted losslessly by `crate::raw` next to
//! the LeRobot artifacts, so the raw data is always recoverable.

use crate::dataset::{
    action_key, observation_key, sanitize_component, ActionSample, EpisodeAssemblyInput,
    ObservationSample,
};
use arrow_array::builder::{Float64Builder, ListBuilder};
use arrow_array::{ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use rollio_types::config::{
    AssemblerActionRuntimeConfigV2, AssemblerObservationRuntimeConfigV2, AssemblerRuntimeConfigV2,
    RobotStateKind,
};
use rollio_types::messages::PixelFormat;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Canonical row data for one episode.
#[derive(Debug, Clone)]
pub(crate) struct LeRobotRows {
    pub timestamps_s: Vec<f64>,
    pub frame_indices: Vec<i64>,
    pub episode_indices: Vec<i64>,
    pub global_indices: Vec<i64>,
    pub task_indices: Vec<i64>,
    pub done_flags: Vec<bool>,
    pub observation_columns: BTreeMap<String, Vec<Vec<f64>>>,
    pub action_rows: Vec<Vec<f64>>,
}

impl LeRobotRows {
    pub fn row_count(&self) -> usize {
        self.timestamps_s.len()
    }
}

/// Materialize the LeRobot v2.1 row table for `episode` and write it to
/// `parquet_path`.
///
/// Returns the row count so callers can populate `info.json::total_frames`.
pub(crate) fn write_episode_parquet(
    parquet_path: &Path,
    config: &AssemblerRuntimeConfigV2,
    episode: &EpisodeAssemblyInput,
) -> Result<usize, Box<dyn Error>> {
    let rows = build_episode_rows(config, episode);
    if let Some(parent) = parquet_path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_parquet(parquet_path, config, &rows)?;
    Ok(rows.row_count())
}

pub(crate) fn build_episode_rows(
    config: &AssemblerRuntimeConfigV2,
    episode: &EpisodeAssemblyInput,
) -> LeRobotRows {
    let frame_timestamps_us =
        canonical_frame_timestamps(episode.start_time_us, episode.stop_time_us, config.fps);
    let row_count = frame_timestamps_us.len();

    // Parquet `timestamp` column is seconds RELATIVE to the controller's
    // recording-start anchor (LeRobot v2.1 convention). Computing it from
    // the index keeps every episode's first row at exactly 0.0 regardless
    // of NTP slew on the wall clock.
    let timestamps_s = (0..row_count)
        .map(|i| i as f64 / config.fps as f64)
        .collect::<Vec<_>>();
    let frame_indices = (0..row_count as i64).collect::<Vec<_>>();
    let episode_indices = vec![episode.episode_index as i64; row_count];
    let global_indices = frame_indices
        .iter()
        .map(|index| episode.episode_index as i64 * 1_000_000 + index)
        .collect::<Vec<_>>();
    let task_indices = vec![0_i64; row_count];
    let done_flags = (0..row_count)
        .map(|index| index + 1 == row_count)
        .collect::<Vec<_>>();

    let mut observation_columns = BTreeMap::new();
    for observation in &config.observations {
        let key = observation_key(&observation.channel_id, observation.state_kind);
        let samples = episode
            .observation_samples
            .get(&key)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut rows = Vec::with_capacity(row_count);
        for timestamp_us in &frame_timestamps_us {
            rows.push(observation_values_at(
                samples,
                *timestamp_us,
                observation.value_len as usize,
            ));
        }
        observation_columns.insert(key, rows);
    }

    let mut action_rows = Vec::with_capacity(row_count);
    for timestamp_us in &frame_timestamps_us {
        let mut row = Vec::new();
        for action in &config.actions {
            let key = action_key(&action.channel_id, action.command_kind);
            let samples = episode
                .action_samples
                .get(&key)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            row.extend(action_values_at(
                samples,
                *timestamp_us,
                action.value_len as usize,
            ));
        }
        action_rows.push(row);
    }

    LeRobotRows {
        timestamps_s,
        frame_indices,
        episode_indices,
        global_indices,
        task_indices,
        done_flags,
        observation_columns,
        action_rows,
    }
}

/// Build the canonical episode timeline.
///
/// Row times are evenly spaced at `1 / fps` intervals starting at
/// `controller_start_us`, with row count rounded from the duration:
///
/// ```text
/// row_count = round((stop - start) / 1e6 * fps)
/// t_i = start + i * 1e6 / fps   (microseconds)
/// ```
///
/// If the controller-supplied duration is zero (e.g. the user pressed
/// record and then stop on the same loop tick), the function still emits
/// at least one row so the assembler always produces a non-empty Parquet
/// file.
pub(crate) fn canonical_frame_timestamps(
    controller_start_us: u64,
    controller_stop_us: u64,
    fps: u32,
) -> Vec<u64> {
    let fps = fps.max(1);
    let duration_us = controller_stop_us.saturating_sub(controller_start_us);
    let mut row_count = ((duration_us as f64 / 1_000_000.0) * fps as f64).round() as usize;
    if row_count == 0 {
        row_count = 1;
    }
    let step_us = 1_000_000.0 / fps as f64;
    (0..row_count)
        .map(|index| controller_start_us + (index as f64 * step_us).round() as u64)
        .collect()
}

fn observation_values_at(
    samples: &[ObservationSample],
    timestamp_us: u64,
    width: usize,
) -> Vec<f64> {
    if samples.is_empty() {
        return vec![0.0; width];
    }
    let mut selected = &samples[0];
    for sample in samples {
        if sample.timestamp_us <= timestamp_us {
            selected = sample;
        } else {
            break;
        }
    }
    resize_values(&selected.values, width)
}

fn action_values_at(samples: &[ActionSample], timestamp_us: u64, width: usize) -> Vec<f64> {
    if samples.is_empty() {
        return vec![0.0; width];
    }
    let mut selected = &samples[0];
    for sample in samples {
        if sample.timestamp_us <= timestamp_us {
            selected = sample;
        } else {
            break;
        }
    }
    resize_values(&selected.values, width)
}

fn resize_values(values: &[f64], width: usize) -> Vec<f64> {
    let mut resized = vec![0.0; width];
    for (index, value) in values.iter().copied().enumerate().take(width) {
        resized[index] = value;
    }
    resized
}

fn write_parquet(
    path: &Path,
    config: &AssemblerRuntimeConfigV2,
    rows: &LeRobotRows,
) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(parquet_schema(config));
    let batch = RecordBatch::try_new(schema.clone(), parquet_columns(config, rows))?;
    let file = File::create(path)?;
    let mut writer = ArrowWriter::try_new(file, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

/// Shared inner field for every `List<Float64>` column.
///
/// LeRobot feature vectors never contain null elements, so the inner field
/// is declared non-nullable. Both the schema and the `ListBuilder` must
/// reference the same field definition; otherwise `RecordBatch::try_new`
/// rejects the batch with `column types must match schema types, expected
/// List(non-null Float64) but found List(Float64)`.
fn feature_list_inner_field() -> Arc<Field> {
    Arc::new(Field::new("item", DataType::Float64, false))
}

fn feature_list_data_type() -> DataType {
    DataType::List(feature_list_inner_field())
}

fn parquet_schema(config: &AssemblerRuntimeConfigV2) -> Schema {
    let mut fields = vec![
        Field::new("timestamp", DataType::Float64, false),
        Field::new("frame_index", DataType::Int64, false),
        Field::new("episode_index", DataType::Int64, false),
        Field::new("global_index", DataType::Int64, false),
        Field::new("task_index", DataType::Int64, false),
        Field::new("done", DataType::Boolean, false),
        Field::new("action", feature_list_data_type(), false),
    ];
    for observation in &config.observations {
        fields.push(Field::new(
            observation_feature_key(observation),
            feature_list_data_type(),
            false,
        ));
    }
    Schema::new(fields)
}

fn parquet_columns(config: &AssemblerRuntimeConfigV2, rows: &LeRobotRows) -> Vec<ArrayRef> {
    let mut arrays: Vec<ArrayRef> = vec![
        Arc::new(Float64Array::from(rows.timestamps_s.clone())),
        Arc::new(Int64Array::from(rows.frame_indices.clone())),
        Arc::new(Int64Array::from(rows.episode_indices.clone())),
        Arc::new(Int64Array::from(rows.global_indices.clone())),
        Arc::new(Int64Array::from(rows.task_indices.clone())),
        Arc::new(BooleanArray::from(rows.done_flags.clone())),
        Arc::new(build_list_array(&rows.action_rows)),
    ];

    for observation in &config.observations {
        let key = observation_key(&observation.channel_id, observation.state_kind);
        let values = rows
            .observation_columns
            .get(&key)
            .expect("observation columns should exist for every configured observation");
        arrays.push(Arc::new(build_list_array(values)));
    }

    arrays
}

fn build_list_array(rows: &[Vec<f64>]) -> arrow_array::ListArray {
    let values = Float64Builder::new();
    let mut builder = ListBuilder::new(values).with_field(feature_list_inner_field());
    for row in rows {
        for value in row {
            builder.values().append_value(*value);
        }
        builder.append(true);
    }
    builder.finish()
}

// ---------------------------------------------------------------------------
// info.json
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DatasetInfo {
    pub codebase_version: String,
    pub robot_type: Option<String>,
    pub total_episodes: u32,
    pub total_frames: u64,
    pub total_tasks: u32,
    pub chunks_size: u32,
    pub fps: u32,
    pub splits: BTreeMap<String, String>,
    pub data_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_path: Option<String>,
    /// Path template for the per-channel raw Parquet dump (mirrors
    /// `data_path` / `video_path` formatting). Always present after the
    /// raw-dump phase landed; older datasets predating that phase omit it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_path: Option<String>,
    pub features: BTreeMap<String, FeatureSpec>,
    pub embedded_config_toml: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FeatureSpec {
    pub dtype: String,
    pub shape: Vec<usize>,
    pub names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_info: Option<VideoInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct VideoInfo {
    pub codec: String,
    pub artifact_format: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

pub(crate) fn write_stage_metadata(
    staging_dir: &Path,
    config: &AssemblerRuntimeConfigV2,
    episode: &EpisodeAssemblyInput,
    frame_count: usize,
) -> Result<(), Box<dyn Error>> {
    let meta_dir = staging_dir.join("meta");
    fs::create_dir_all(&meta_dir)?;

    // info.json — dataset-wide schema and counts.
    let info_path = meta_dir.join("info.json");
    let info = build_dataset_info(config, episode, frame_count);
    let mut info_file = File::create(&info_path)?;
    info_file.write_all(serde_json::to_string_pretty(&info)?.as_bytes())?;
    info_file.write_all(b"\n")?;

    // episodes.jsonl — one record per episode with the absolute UNIX-epoch
    // anchor (controller_start_us / controller_stop_us) so downstream
    // consumers can correlate the LeRobot "relative seconds" timeline
    // with external logs (controller, ROS bag, screen captures, etc.).
    // Storage merges this file across episodes (append mode), matching the
    // LeRobot v2.1 convention of one JSONL row per episode.
    let episodes_path = meta_dir.join("episodes.jsonl");
    let record = EpisodeRecord {
        episode_index: episode.episode_index,
        controller_start_us: episode.start_time_us,
        controller_stop_us: episode.stop_time_us,
        length: frame_count as u64,
        tasks: vec!["collect".into()],
    };
    let mut episodes_file = File::create(&episodes_path)?;
    episodes_file.write_all(serde_json::to_string(&record)?.as_bytes())?;
    episodes_file.write_all(b"\n")?;

    Ok(())
}

/// One record per episode in `meta/episodes.jsonl`. Field names match the
/// LeRobot v2.1 convention (`episode_index`, `length`, `tasks`) so the
/// LeRobot loader doesn't need to special-case Rollio output. The two
/// `controller_*_us` fields are Rollio-specific extensions: they carry
/// the absolute UNIX-epoch anchor for the relative `timestamp` column in
/// the data parquet, which lets analysts align the recording against
/// external timelines (other camera systems, ROS bags, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EpisodeRecord {
    episode_index: u32,
    controller_start_us: u64,
    controller_stop_us: u64,
    length: u64,
    tasks: Vec<String>,
}

fn build_dataset_info(
    config: &AssemblerRuntimeConfigV2,
    episode: &EpisodeAssemblyInput,
    frame_count: usize,
) -> DatasetInfo {
    let data_path = format!(
        "data/chunk-{}/episode_{:06}.parquet",
        chunk_index(episode.episode_index, config.chunk_size),
        episode.episode_index
    );
    let video_path = config.cameras.first().map(|camera| {
        format!(
            "videos/chunk-{}/{}/episode_{:06}.{}",
            chunk_index(episode.episode_index, config.chunk_size),
            sanitize_component(&camera.channel_id),
            episode.episode_index,
            camera.artifact_format.extension()
        )
    });
    let raw_path = if config.observations.is_empty() && config.actions.is_empty() {
        None
    } else {
        Some(crate::raw::raw_path_template(
            episode.episode_index,
            config.chunk_size,
        ))
    };
    DatasetInfo {
        codebase_version: env!("CARGO_PKG_VERSION").to_string(),
        robot_type: None,
        total_episodes: 1,
        total_frames: frame_count as u64,
        total_tasks: 1,
        chunks_size: config.chunk_size,
        fps: config.fps,
        splits: BTreeMap::from([("train".into(), "0:1".into())]),
        data_path,
        video_path,
        raw_path,
        features: build_feature_map(config),
        embedded_config_toml: config.embedded_config_toml.clone(),
    }
}

fn build_feature_map(config: &AssemblerRuntimeConfigV2) -> BTreeMap<String, FeatureSpec> {
    let mut features = BTreeMap::new();
    for name in [
        "timestamp",
        "frame_index",
        "episode_index",
        "global_index",
        "task_index",
    ] {
        features.insert(
            name.into(),
            FeatureSpec {
                dtype: if name == "timestamp" {
                    "float64"
                } else {
                    "int64"
                }
                .into(),
                shape: vec![],
                names: None,
                video_info: None,
            },
        );
    }
    features.insert(
        "done".into(),
        FeatureSpec {
            dtype: "bool".into(),
            shape: vec![],
            names: None,
            video_info: None,
        },
    );

    let action_names = action_feature_names(&config.actions);
    features.insert(
        "action".into(),
        FeatureSpec {
            dtype: "float64".into(),
            shape: vec![action_names.len()],
            names: Some(action_names),
            video_info: None,
        },
    );

    for observation in &config.observations {
        features.insert(
            observation_feature_key(observation),
            FeatureSpec {
                dtype: "float64".into(),
                shape: vec![observation.value_len as usize],
                names: Some(observation_value_names(observation)),
                video_info: None,
            },
        );
    }

    for camera in &config.cameras {
        features.insert(
            format!(
                "observation.images.{}",
                sanitize_component(&camera.channel_id)
            ),
            FeatureSpec {
                dtype: "video".into(),
                shape: vec![
                    camera.height as usize,
                    camera.width as usize,
                    channels_for_pixel_format(camera.pixel_format),
                ],
                names: None,
                video_info: Some(VideoInfo {
                    codec: camera.codec.as_str().into(),
                    artifact_format: camera.artifact_format.extension().into(),
                    width: camera.width,
                    height: camera.height,
                    fps: camera.fps,
                }),
            },
        );
    }

    features
}

fn observation_feature_key(observation: &AssemblerObservationRuntimeConfigV2) -> String {
    format!(
        "observation.state.{}.{}",
        sanitize_component(&observation.channel_id),
        observation.state_kind.topic_suffix()
    )
}

fn action_feature_names(actions: &[AssemblerActionRuntimeConfigV2]) -> Vec<String> {
    let mut names = Vec::new();
    for action in actions {
        for index in 0..action.value_len {
            names.push(format!(
                "{}.{}.{}",
                sanitize_component(&action.channel_id),
                action.command_kind.topic_suffix(),
                index
            ));
        }
    }
    names
}

fn observation_value_names(observation: &AssemblerObservationRuntimeConfigV2) -> Vec<String> {
    let prefix = match observation.state_kind {
        RobotStateKind::EndEffectorPose => "pose",
        RobotStateKind::EndEffectorTwist => "twist",
        RobotStateKind::EndEffectorWrench => "wrench",
        RobotStateKind::ParallelPosition
        | RobotStateKind::ParallelVelocity
        | RobotStateKind::ParallelEffort => "parallel",
        _ => "joint",
    };
    (0..observation.value_len)
        .map(|index| format!("{prefix}_{index}"))
        .collect()
}

fn channels_for_pixel_format(pixel_format: PixelFormat) -> usize {
    match pixel_format {
        PixelFormat::Rgb24 | PixelFormat::Bgr24 => 3,
        PixelFormat::Depth16 => 1,
        PixelFormat::Gray8 => 1,
        PixelFormat::Yuyv => 2,
        PixelFormat::Mjpeg => 3,
    }
}

pub(crate) fn staged_parquet_path(
    staging_dir: &Path,
    episode_index: u32,
    chunk_size: u32,
) -> PathBuf {
    staging_dir.join(format!(
        "data/chunk-{}/episode_{:06}.parquet",
        chunk_index(episode_index, chunk_size),
        episode_index
    ))
}

pub(crate) fn staged_video_path(
    staging_dir: &Path,
    episode_index: u32,
    chunk_size: u32,
    channel_id: &str,
    extension: &str,
) -> PathBuf {
    staging_dir.join(format!(
        "videos/chunk-{}/{}/episode_{:06}.{}",
        chunk_index(episode_index, chunk_size),
        sanitize_component(channel_id),
        episode_index,
        extension
    ))
}

fn chunk_index(episode_index: u32, chunk_size: u32) -> String {
    format!("{:03}", episode_index / chunk_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollio_types::config::{
        AssemblerObservationRuntimeConfigV2, EncodedHandoffMode, EpisodeFormat,
    };

    fn config_with_one_observation() -> AssemblerRuntimeConfigV2 {
        AssemblerRuntimeConfigV2 {
            process_id: "lerobot-test".into(),
            format: EpisodeFormat::LeRobotV2_1,
            fps: 30,
            chunk_size: 1000,
            missing_video_timeout_ms: 5000,
            staging_dir: "/tmp/rollio-lerobot-test".into(),
            encoded_handoff: EncodedHandoffMode::default(),
            cameras: Vec::new(),
            observations: vec![AssemblerObservationRuntimeConfigV2 {
                channel_id: "robot_a/arm".into(),
                state_kind: RobotStateKind::JointPosition,
                state_topic: "robot_a/arm/states/joint_position".into(),
                value_len: 6,
            }],
            actions: Vec::new(),
            embedded_config_toml: String::new(),
        }
    }

    #[test]
    fn canonical_frame_timestamps_one_second_at_thirty_fps_yields_thirty_rows() {
        let anchor: u64 = 1_700_000_000_000_000;
        let stamps = canonical_frame_timestamps(anchor, anchor + 1_000_000, 30);
        assert_eq!(stamps.len(), 30);
        assert_eq!(stamps[0], anchor);
        let expected_offset: u64 = (29.0_f64 * 1_000_000.0 / 30.0).round() as u64;
        assert_eq!(stamps[29], anchor + expected_offset);
    }

    #[test]
    fn canonical_frame_timestamps_handles_zero_duration() {
        let anchor = 12_345_678;
        let stamps = canonical_frame_timestamps(anchor, anchor, 30);
        // At least one row even on a zero-duration episode.
        assert_eq!(stamps.len(), 1);
        assert_eq!(stamps[0], anchor);
    }

    #[test]
    fn build_episode_rows_produces_rows_starting_at_zero_seconds() {
        let config = config_with_one_observation();
        let key = observation_key("robot_a/arm", RobotStateKind::JointPosition);
        let mut observation_samples = BTreeMap::new();
        observation_samples.insert(
            key,
            vec![ObservationSample {
                timestamp_us: 1_000_000,
                values: vec![0.42; 6],
            }],
        );

        let episode = EpisodeAssemblyInput {
            episode_index: 0,
            start_time_us: 1_000_000,
            stop_time_us: 1_000_000 + 1_000_000,
            observation_samples,
            action_samples: BTreeMap::new(),
            video_paths: BTreeMap::new(),
        };

        let rows = build_episode_rows(&config, &episode);
        assert_eq!(rows.row_count(), 30);
        assert_eq!(rows.timestamps_s[0], 0.0);
        assert!((rows.timestamps_s[29] - 29.0 / 30.0).abs() < 1e-12);
        // Every observation row sees the single sample we provided.
        let observation_rows = &rows.observation_columns
            [&observation_key("robot_a/arm", RobotStateKind::JointPosition)];
        assert_eq!(observation_rows.len(), 30);
        for row in observation_rows {
            assert_eq!(row, &vec![0.42; 6]);
        }
    }

    #[test]
    fn observation_values_at_picks_nearest_prior_sample() {
        let samples = vec![
            ObservationSample {
                timestamp_us: 100,
                values: vec![1.0, 2.0],
            },
            ObservationSample {
                timestamp_us: 200,
                values: vec![3.0, 4.0],
            },
            ObservationSample {
                timestamp_us: 300,
                values: vec![5.0, 6.0],
            },
        ];
        // Before any sample arrives, fall back to samples[0].
        assert_eq!(observation_values_at(&samples, 50, 2), vec![1.0, 2.0]);
        // Exactly at timestamp 200 should pick samples[1].
        assert_eq!(observation_values_at(&samples, 200, 2), vec![3.0, 4.0]);
        // Between 200 and 300 should also pick samples[1].
        assert_eq!(observation_values_at(&samples, 250, 2), vec![3.0, 4.0]);
        // At or after 300 picks samples[2].
        assert_eq!(observation_values_at(&samples, 1_000, 2), vec![5.0, 6.0]);
    }

    #[test]
    fn record_batch_columns_match_schema_for_action_and_observations() {
        let config = config_with_one_observation();
        let key = observation_key("robot_a/arm", RobotStateKind::JointPosition);
        let mut observation_columns = BTreeMap::new();
        observation_columns.insert(key, vec![vec![0.0_f64; 6], vec![1.0_f64; 6]]);
        let rows = LeRobotRows {
            timestamps_s: vec![0.0, 1.0 / 30.0],
            frame_indices: vec![0, 1],
            episode_indices: vec![0, 0],
            global_indices: vec![0, 1],
            task_indices: vec![0, 0],
            done_flags: vec![false, true],
            observation_columns,
            action_rows: vec![Vec::new(), Vec::new()],
        };

        let schema = Arc::new(parquet_schema(&config));
        let columns = parquet_columns(&config, &rows);
        let batch = RecordBatch::try_new(schema, columns)
            .expect("schema and columns must align for List<Float64> features");
        assert_eq!(batch.num_rows(), 2);
    }
}
