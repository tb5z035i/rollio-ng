use arrow_array::builder::{Float64Builder, ListBuilder};
use arrow_array::{ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use rollio_types::config::{
    AssemblerActionRuntimeConfigV2, AssemblerObservationRuntimeConfigV2, AssemblerRuntimeConfigV2,
    RobotCommandKind, RobotStateKind,
};
use rollio_types::messages::PixelFormat;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct ObservationSample {
    pub timestamp_ns: u64,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone)]
pub(crate) struct ActionSample {
    pub timestamp_ns: u64,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone)]
pub(crate) struct EpisodeAssemblyInput {
    pub episode_index: u32,
    pub start_time_ns: u64,
    pub stop_time_ns: u64,
    pub observation_samples: BTreeMap<String, Vec<ObservationSample>>,
    pub action_samples: BTreeMap<String, Vec<ActionSample>>,
    pub video_paths: BTreeMap<String, PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct StagedEpisode {
    pub episode_index: u32,
    pub staging_dir: PathBuf,
}

#[derive(Debug, Clone)]
struct EpisodeRows {
    timestamps_s: Vec<f64>,
    frame_indices: Vec<i64>,
    episode_indices: Vec<i64>,
    global_indices: Vec<i64>,
    task_indices: Vec<i64>,
    done_flags: Vec<bool>,
    observation_columns: BTreeMap<String, Vec<Vec<f64>>>,
    action_rows: Vec<Vec<f64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DatasetInfo {
    codebase_version: String,
    robot_type: Option<String>,
    total_episodes: u32,
    total_frames: u64,
    total_tasks: u32,
    chunks_size: u32,
    fps: u32,
    splits: BTreeMap<String, String>,
    data_path: String,
    video_path: Option<String>,
    features: BTreeMap<String, FeatureSpec>,
    embedded_config_toml: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FeatureSpec {
    dtype: String,
    shape: Vec<usize>,
    names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    video_info: Option<VideoInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VideoInfo {
    codec: String,
    artifact_format: String,
    width: u32,
    height: u32,
    fps: u32,
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

    let rows = build_episode_rows(config, episode);
    let parquet_path = staged_parquet_path(&staging_dir, episode.episode_index, config.chunk_size);
    if let Some(parent) = parquet_path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_parquet(&parquet_path, config, &rows)?;

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
        let target = staged_video_path(
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

    write_stage_metadata(&staging_dir, config, episode, rows.timestamps_s.len())?;

    Ok(StagedEpisode {
        episode_index: episode.episode_index,
        staging_dir,
    })
}

fn build_episode_rows(
    config: &AssemblerRuntimeConfigV2,
    episode: &EpisodeAssemblyInput,
) -> EpisodeRows {
    let frame_timestamps_ns = build_frame_timestamps(config.fps, episode);
    let row_count = frame_timestamps_ns.len();

    let timestamps_s = frame_timestamps_ns
        .iter()
        .map(|timestamp| (*timestamp as f64) / 1_000_000_000.0)
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
        for timestamp_ns in &frame_timestamps_ns {
            rows.push(observation_values_at(
                samples,
                *timestamp_ns,
                observation.value_len as usize,
            ));
        }
        observation_columns.insert(key, rows);
    }

    let mut action_rows = Vec::with_capacity(row_count);
    for timestamp_ns in &frame_timestamps_ns {
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
                *timestamp_ns,
                action.value_len as usize,
            ));
        }
        action_rows.push(row);
    }

    EpisodeRows {
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

fn build_frame_timestamps(fps: u32, episode: &EpisodeAssemblyInput) -> Vec<u64> {
    let explicit_duration_ns = episode.stop_time_ns.saturating_sub(episode.start_time_ns);
    let inferred_duration_ns = infer_sample_duration_ns(episode);
    let duration_ns = explicit_duration_ns.max(inferred_duration_ns);
    let mut row_count = ((duration_ns as f64 / 1_000_000_000.0) * fps as f64).round() as usize;
    if row_count == 0 {
        row_count = max_sample_count(episode).max(1);
    }
    let step_ns = 1_000_000_000.0 / fps as f64;
    (0..row_count)
        .map(|index| episode.start_time_ns + (index as f64 * step_ns).round() as u64)
        .collect()
}

fn infer_sample_duration_ns(episode: &EpisodeAssemblyInput) -> u64 {
    let mut min_timestamp = None;
    let mut max_timestamp = None;
    for samples in episode.observation_samples.values() {
        for sample in samples {
            min_timestamp = Some(min_timestamp.map_or(sample.timestamp_ns, |value: u64| {
                value.min(sample.timestamp_ns)
            }));
            max_timestamp = Some(max_timestamp.map_or(sample.timestamp_ns, |value: u64| {
                value.max(sample.timestamp_ns)
            }));
        }
    }
    for samples in episode.action_samples.values() {
        for sample in samples {
            min_timestamp = Some(min_timestamp.map_or(sample.timestamp_ns, |value: u64| {
                value.min(sample.timestamp_ns)
            }));
            max_timestamp = Some(max_timestamp.map_or(sample.timestamp_ns, |value: u64| {
                value.max(sample.timestamp_ns)
            }));
        }
    }
    match (min_timestamp, max_timestamp) {
        (Some(start), Some(stop)) if stop > start => stop - start,
        _ => 0,
    }
}

fn max_sample_count(episode: &EpisodeAssemblyInput) -> usize {
    let obs_max = episode
        .observation_samples
        .values()
        .map(Vec::len)
        .max()
        .unwrap_or_default();
    let action_max = episode
        .action_samples
        .values()
        .map(Vec::len)
        .max()
        .unwrap_or_default();
    obs_max.max(action_max)
}

fn observation_values_at(
    samples: &[ObservationSample],
    timestamp_ns: u64,
    width: usize,
) -> Vec<f64> {
    if samples.is_empty() {
        return vec![0.0; width];
    }
    let mut selected = &samples[0];
    for sample in samples {
        if sample.timestamp_ns <= timestamp_ns {
            selected = sample;
        } else {
            break;
        }
    }
    resize_values(&selected.values, width)
}

fn action_values_at(samples: &[ActionSample], timestamp_ns: u64, width: usize) -> Vec<f64> {
    if samples.is_empty() {
        return vec![0.0; width];
    }
    let mut selected = &samples[0];
    for sample in samples {
        if sample.timestamp_ns <= timestamp_ns {
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
    rows: &EpisodeRows,
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
/// LeRobot feature vectors never contain null elements, so the inner field is
/// declared non-nullable. Both the schema and the `ListBuilder` must reference
/// the same field definition; otherwise `RecordBatch::try_new` rejects the
/// batch with `column types must match schema types, expected
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

fn parquet_columns(config: &AssemblerRuntimeConfigV2, rows: &EpisodeRows) -> Vec<ArrayRef> {
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

fn write_stage_metadata(
    staging_dir: &Path,
    config: &AssemblerRuntimeConfigV2,
    episode: &EpisodeAssemblyInput,
    frame_count: usize,
) -> Result<(), Box<dyn Error>> {
    let info_path = staging_dir.join("meta/info.json");
    if let Some(parent) = info_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let info = build_dataset_info(config, episode, frame_count);
    let mut file = File::create(&info_path)?;
    file.write_all(serde_json::to_string_pretty(&info)?.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
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

pub(crate) fn observation_key(channel_id: &str, state_kind: RobotStateKind) -> String {
    format!("{channel_id}/{}", state_kind.topic_suffix())
}

pub(crate) fn action_key(channel_id: &str, command_kind: RobotCommandKind) -> String {
    format!("{channel_id}/{}", command_kind.topic_suffix())
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

fn sanitize_component(value: &str) -> String {
    value.replace('/', "__")
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

fn staged_parquet_path(staging_dir: &Path, episode_index: u32, chunk_size: u32) -> PathBuf {
    staging_dir.join(format!(
        "data/chunk-{}/episode_{:06}.parquet",
        chunk_index(episode_index, chunk_size),
        episode_index
    ))
}

fn staged_video_path(
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

#[cfg(test)]
mod tests {
    use super::*;
    use rollio_types::config::{EncodedHandoffMode, EpisodeFormat};

    fn sample_config_with_observation() -> AssemblerRuntimeConfigV2 {
        AssemblerRuntimeConfigV2 {
            process_id: "test-assembler".into(),
            format: EpisodeFormat::LeRobotV2_1,
            fps: 30,
            chunk_size: 1000,
            missing_video_timeout_ms: 5000,
            staging_dir: "/tmp/rollio-assembler-test".into(),
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

    fn sample_rows(observation_key_str: &str) -> EpisodeRows {
        let mut observation_columns = BTreeMap::new();
        observation_columns.insert(
            observation_key_str.to_string(),
            vec![vec![0.0_f64; 6], vec![1.0_f64; 6]],
        );
        EpisodeRows {
            timestamps_s: vec![0.0, 1.0 / 30.0],
            frame_indices: vec![0, 1],
            episode_indices: vec![0, 0],
            global_indices: vec![0, 1],
            task_indices: vec![0, 0],
            done_flags: vec![false, true],
            observation_columns,
            action_rows: vec![vec![0.1_f64; 6], vec![0.2_f64; 6]],
        }
    }

    /// Regression: schema declares `List(non-null Float64)` but `ListBuilder`
    /// defaults to `List(nullable Float64)`. Without `with_field`, the
    /// `RecordBatch::try_new` call below fails with
    /// `column types must match schema types, expected
    /// List(non-null Float64) but found List(Float64)`,
    /// which crashes the assembler when the user keeps an episode.
    #[test]
    fn record_batch_columns_match_schema_for_action_and_observations() {
        let config = sample_config_with_observation();
        let key = observation_key("robot_a/arm", RobotStateKind::JointPosition);
        let rows = sample_rows(&key);

        let schema = Arc::new(parquet_schema(&config));
        let columns = parquet_columns(&config, &rows);
        let batch = RecordBatch::try_new(schema, columns)
            .expect("schema and columns must align for List<Float64> features");
        assert_eq!(batch.num_rows(), 2);
    }
}
