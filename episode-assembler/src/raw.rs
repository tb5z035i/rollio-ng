//! Per-channel raw dump of every received bus sample.
//!
//! For each `(channel_id, kind)` pair the assembler observed during an
//! episode, this module writes a single Parquet file with the schema
//!
//! ```text
//! timestamp_us_rel: Int64 (non-null)   // sample.timestamp_us - controller_start_us
//! values:           List<Float64>       // variable length per kind
//! ```
//!
//! Files land under `staging_dir/raw/chunk-XXX/episode_YYYYYY/<channel_id>__<kind>.parquet`
//! so storage's `merge_tree_if_present` recursion picks them up alongside
//! `data/` and `videos/` without any storage-side changes.
//!
//! Cameras intentionally do **not** get a separate raw dump here: their
//! VFR MP4 already carries true relative timing in PTS (see Phase 5 of the
//! plan). The `raw_path` field in `info.json` advertises this layout to
//! downstream consumers.

use crate::dataset::{
    action_key, observation_key, sanitize_component, ActionSample, EpisodeAssemblyInput,
    ObservationSample,
};
use arrow_array::builder::{Float64Builder, ListBuilder};
use arrow_array::{ArrayRef, Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use rollio_types::config::AssemblerRuntimeConfigV2;
use std::error::Error;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Writes the per-channel raw Parquet files for `episode` under
/// `staging_dir/raw/chunk-XXX/episode_YYYYYY/`.
///
/// Returns the directory the files were written under (for inclusion in
/// the dataset `info.json` and for downstream debugging).
pub(crate) fn write_episode_raw_dump(
    staging_dir: &Path,
    config: &AssemblerRuntimeConfigV2,
    episode: &EpisodeAssemblyInput,
) -> Result<PathBuf, Box<dyn Error>> {
    let raw_dir = raw_dir_path(staging_dir, episode.episode_index, config.chunk_size);
    fs::create_dir_all(&raw_dir)?;

    // Observations: one file per (channel_id, state_kind).
    for observation in &config.observations {
        let key = observation_key(&observation.channel_id, observation.state_kind);
        let samples = episode
            .observation_samples
            .get(&key)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let file_name = format!(
            "{}__{}.parquet",
            sanitize_component(&observation.channel_id),
            observation.state_kind.topic_suffix()
        );
        write_observation_parquet(&raw_dir.join(file_name), samples, episode.start_time_us)?;
    }

    // Actions: one file per (channel_id, command_kind).
    for action in &config.actions {
        let key = action_key(&action.channel_id, action.command_kind);
        let samples = episode
            .action_samples
            .get(&key)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let file_name = format!(
            "{}__{}.parquet",
            sanitize_component(&action.channel_id),
            action.command_kind.topic_suffix()
        );
        write_action_parquet(&raw_dir.join(file_name), samples, episode.start_time_us)?;
    }

    Ok(raw_dir)
}

/// `info.json`-style relative path for the raw dump directory of one
/// episode (mirrors `data_path` / `video_path` formatting so downstream
/// consumers can compose it with the dataset root).
pub(crate) fn raw_path_template(episode_index: u32, chunk_size: u32) -> String {
    format!(
        "raw/chunk-{}/episode_{:06}",
        chunk_index_string(episode_index, chunk_size),
        episode_index
    )
}

fn raw_dir_path(staging_dir: &Path, episode_index: u32, chunk_size: u32) -> PathBuf {
    staging_dir.join(raw_path_template(episode_index, chunk_size))
}

fn chunk_index_string(episode_index: u32, chunk_size: u32) -> String {
    format!("{:03}", episode_index / chunk_size)
}

fn write_observation_parquet(
    path: &Path,
    samples: &[ObservationSample],
    start_time_us: u64,
) -> Result<(), Box<dyn Error>> {
    let timestamps: Vec<i64> = samples
        .iter()
        .map(|s| relative_timestamp_us(s.timestamp_us, start_time_us))
        .collect();
    let values_rows: Vec<&[f64]> = samples.iter().map(|s| s.values.as_slice()).collect();
    write_parquet(path, &timestamps, &values_rows)
}

fn write_action_parquet(
    path: &Path,
    samples: &[ActionSample],
    start_time_us: u64,
) -> Result<(), Box<dyn Error>> {
    let timestamps: Vec<i64> = samples
        .iter()
        .map(|s| relative_timestamp_us(s.timestamp_us, start_time_us))
        .collect();
    let values_rows: Vec<&[f64]> = samples.iter().map(|s| s.values.as_slice()).collect();
    write_parquet(path, &timestamps, &values_rows)
}

/// Convert a sample's absolute `timestamp_us` (UNIX epoch) into the
/// episode-relative microsecond offset that `timestamp_us_rel` stores.
///
/// Pre-recording samples (whose `timestamp_us < start_time_us`) yield a
/// negative offset, which is meaningful in the raw log as "this sample
/// was already in flight when the controller pressed record".
fn relative_timestamp_us(sample_us: u64, start_time_us: u64) -> i64 {
    sample_us as i64 - start_time_us as i64
}

fn write_parquet(
    path: &Path,
    timestamps: &[i64],
    values_rows: &[&[f64]],
) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let schema = Arc::new(parquet_schema());
    let batch = RecordBatch::try_new(schema.clone(), parquet_columns(timestamps, values_rows))?;
    let file = File::create(path)?;
    let mut writer = ArrowWriter::try_new(file, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

fn parquet_schema() -> Schema {
    Schema::new(vec![
        Field::new("timestamp_us_rel", DataType::Int64, false),
        Field::new("values", feature_list_data_type(), false),
    ])
}

fn parquet_columns(timestamps: &[i64], values_rows: &[&[f64]]) -> Vec<ArrayRef> {
    vec![
        Arc::new(Int64Array::from(timestamps.to_vec())),
        Arc::new(build_list_array(values_rows)),
    ]
}

fn feature_list_inner_field() -> Arc<Field> {
    Arc::new(Field::new("item", DataType::Float64, false))
}

fn feature_list_data_type() -> DataType {
    DataType::List(feature_list_inner_field())
}

fn build_list_array(rows: &[&[f64]]) -> arrow_array::ListArray {
    let values = Float64Builder::new();
    let mut builder = ListBuilder::new(values).with_field(feature_list_inner_field());
    for row in rows {
        for value in *row {
            builder.values().append_value(*value);
        }
        builder.append(true);
    }
    builder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::cast::AsArray;
    use arrow_array::Array;
    use rollio_types::config::{
        AssemblerObservationRuntimeConfigV2, EncodedHandoffMode, EpisodeFormat, RobotStateKind,
    };
    use std::collections::BTreeMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{suffix}"));
        fs::create_dir_all(&path).expect("temp dir should exist");
        path
    }

    fn config_with_one_observation() -> AssemblerRuntimeConfigV2 {
        AssemblerRuntimeConfigV2 {
            process_id: "raw-test".into(),
            format: EpisodeFormat::LeRobotV2_1,
            fps: 30,
            chunk_size: 1000,
            missing_video_timeout_ms: 5000,
            staging_dir: "/tmp/rollio-raw-test".into(),
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
    fn write_episode_raw_dump_persists_one_file_per_channel_kind() {
        let staging = temp_dir("raw-stage");
        let config = config_with_one_observation();
        let key = observation_key("robot_a/arm", RobotStateKind::JointPosition);
        let mut observation_samples = BTreeMap::new();
        observation_samples.insert(
            key.clone(),
            vec![
                ObservationSample {
                    timestamp_us: 1_000_000,
                    values: vec![0.1; 6],
                },
                ObservationSample {
                    timestamp_us: 1_004_000,
                    values: vec![0.2; 6],
                },
                ObservationSample {
                    timestamp_us: 1_008_000,
                    values: vec![0.3; 6],
                },
            ],
        );

        let episode = EpisodeAssemblyInput {
            episode_index: 0,
            start_time_us: 1_000_000,
            stop_time_us: 1_010_000,
            observation_samples,
            action_samples: BTreeMap::new(),
            video_paths: BTreeMap::new(),
        };

        let raw_dir =
            write_episode_raw_dump(&staging, &config, &episode).expect("raw dump should succeed");
        let parquet_path = raw_dir.join("robot_a__arm__joint_position.parquet");
        assert!(parquet_path.is_file());

        let file = std::fs::File::open(&parquet_path).expect("parquet file should open");
        let reader = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(file)
            .expect("parquet reader should build")
            .build()
            .expect("parquet batch iterator should build");
        let batches: Vec<RecordBatch> = reader
            .collect::<Result<Vec<_>, _>>()
            .expect("read all batches");
        let total_rows: usize = batches.iter().map(RecordBatch::num_rows).sum();
        assert_eq!(total_rows, 3);

        let batch = batches.first().expect("at least one batch");
        let timestamps = batch
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("timestamp column should be Int64");
        // Relative to start_time_us = 1_000_000.
        assert_eq!(timestamps.value(0), 0);
        assert_eq!(timestamps.value(1), 4_000);
        assert_eq!(timestamps.value(2), 8_000);

        // Values: each row is a 6-element list of length 6.
        let values_list = batch.column(1).as_list::<i32>();
        assert_eq!(values_list.len(), 3);
        for row_idx in 0..3 {
            assert_eq!(values_list.value_length(row_idx) as usize, 6);
        }
    }

    #[test]
    fn relative_timestamp_handles_pre_record_samples() {
        // Sample arrived 100 us before the controller pressed record.
        assert_eq!(relative_timestamp_us(999_900, 1_000_000), -100);
        assert_eq!(relative_timestamp_us(1_000_000, 1_000_000), 0);
        assert_eq!(relative_timestamp_us(1_002_500, 1_000_000), 2_500);
    }
}
