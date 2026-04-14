use arrow_array::builder::{Float64Builder, ListBuilder};
use arrow_array::{ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use rollio_types::config::{AssemblerActionRuntimeConfig, AssemblerRuntimeConfig};
use rollio_types::messages::PixelFormat;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct RobotObservationSample {
    pub timestamp_ns: u64,
    pub positions: Vec<f64>,
    pub velocities: Vec<f64>,
    pub efforts: Vec<f64>,
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
    pub robot_samples: BTreeMap<String, Vec<RobotObservationSample>>,
    pub action_samples: BTreeMap<String, Vec<ActionSample>>,
    pub video_paths: BTreeMap<String, PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct StagedEpisode {
    pub episode_index: u32,
    pub staging_dir: PathBuf,
    pub frame_count: usize,
}

#[derive(Debug, Clone)]
struct EpisodeRows {
    timestamps_s: Vec<f64>,
    frame_indices: Vec<i64>,
    episode_indices: Vec<i64>,
    global_indices: Vec<i64>,
    task_indices: Vec<i64>,
    done_flags: Vec<bool>,
    robot_columns: BTreeMap<String, RobotColumnRows>,
    action_rows: Vec<Vec<f64>>,
}

#[derive(Debug, Clone, Default)]
struct RobotColumnRows {
    positions: Vec<Vec<f64>>,
    velocities: Vec<Vec<f64>>,
    efforts: Vec<Vec<f64>>,
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
    config: &AssemblerRuntimeConfig,
    episode: &EpisodeAssemblyInput,
) -> Result<StagedEpisode, Box<dyn Error>> {
    let staging_dir = Path::new(&config.staging_dir).join(format!("episode_{:06}", episode.episode_index));
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
            .get(&camera.camera_name)
            .ok_or_else(|| format!("missing video for camera {}", camera.camera_name))?;
        let extension = source
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| camera.artifact_format.extension().to_string());
        let target = staged_video_path(
            &staging_dir,
            episode.episode_index,
            config.chunk_size,
            &camera.camera_name,
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
        frame_count: rows.timestamps_s.len(),
    })
}

fn build_episode_rows(config: &AssemblerRuntimeConfig, episode: &EpisodeAssemblyInput) -> EpisodeRows {
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

    let mut robot_columns = BTreeMap::new();
    for robot in &config.robots {
        let samples = episode
            .robot_samples
            .get(&robot.robot_name)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut rows = RobotColumnRows::default();
        for timestamp_ns in &frame_timestamps_ns {
            let sample = interpolate_robot_sample(samples, *timestamp_ns, robot.dof as usize);
            rows.positions.push(sample.positions);
            rows.velocities.push(sample.velocities);
            rows.efforts.push(sample.efforts);
        }
        robot_columns.insert(robot.robot_name.clone(), rows);
    }

    let mut action_rows = Vec::with_capacity(row_count);
    for timestamp_ns in &frame_timestamps_ns {
        let mut row = Vec::new();
        for source in &config.actions {
            let samples = episode
                .action_samples
                .get(&source.source_name)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            row.extend(interpolate_action_sample(samples, *timestamp_ns, source.dof as usize));
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
        robot_columns,
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
    for samples in episode.robot_samples.values() {
        for sample in samples {
            min_timestamp = Some(min_timestamp.map_or(sample.timestamp_ns, |value: u64| value.min(sample.timestamp_ns)));
            max_timestamp = Some(max_timestamp.map_or(sample.timestamp_ns, |value: u64| value.max(sample.timestamp_ns)));
        }
    }
    for samples in episode.action_samples.values() {
        for sample in samples {
            min_timestamp = Some(min_timestamp.map_or(sample.timestamp_ns, |value: u64| value.min(sample.timestamp_ns)));
            max_timestamp = Some(max_timestamp.map_or(sample.timestamp_ns, |value: u64| value.max(sample.timestamp_ns)));
        }
    }
    match (min_timestamp, max_timestamp) {
        (Some(start), Some(stop)) if stop > start => stop - start,
        _ => 0,
    }
}

fn max_sample_count(episode: &EpisodeAssemblyInput) -> usize {
    let robot_max = episode
        .robot_samples
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
    robot_max.max(action_max)
}

fn interpolate_robot_sample(
    samples: &[RobotObservationSample],
    timestamp_ns: u64,
    width: usize,
) -> RobotObservationSample {
    if samples.is_empty() {
        return RobotObservationSample {
            timestamp_ns,
            positions: vec![0.0; width],
            velocities: vec![0.0; width],
            efforts: vec![0.0; width],
        };
    }

    match samples.binary_search_by_key(&timestamp_ns, |sample| sample.timestamp_ns) {
        Ok(index) => samples[index].clone(),
        Err(0) => samples[0].clone(),
        Err(index) if index >= samples.len() => samples[samples.len() - 1].clone(),
        Err(index) => {
            let before = &samples[index - 1];
            let after = &samples[index];
            RobotObservationSample {
                timestamp_ns,
                positions: interpolate_values(
                    &before.positions,
                    &after.positions,
                    before.timestamp_ns,
                    after.timestamp_ns,
                    timestamp_ns,
                ),
                velocities: interpolate_values(
                    &before.velocities,
                    &after.velocities,
                    before.timestamp_ns,
                    after.timestamp_ns,
                    timestamp_ns,
                ),
                efforts: interpolate_values(
                    &before.efforts,
                    &after.efforts,
                    before.timestamp_ns,
                    after.timestamp_ns,
                    timestamp_ns,
                ),
            }
        }
    }
}

fn interpolate_action_sample(samples: &[ActionSample], timestamp_ns: u64, width: usize) -> Vec<f64> {
    if samples.is_empty() {
        return vec![0.0; width];
    }

    match samples.binary_search_by_key(&timestamp_ns, |sample| sample.timestamp_ns) {
        Ok(index) => samples[index].values.clone(),
        Err(0) => samples[0].values.clone(),
        Err(index) if index >= samples.len() => samples[samples.len() - 1].values.clone(),
        Err(index) => {
            let before = &samples[index - 1];
            let after = &samples[index];
            interpolate_values(
                &before.values,
                &after.values,
                before.timestamp_ns,
                after.timestamp_ns,
                timestamp_ns,
            )
        }
    }
}

fn interpolate_values(
    before: &[f64],
    after: &[f64],
    before_ns: u64,
    after_ns: u64,
    target_ns: u64,
) -> Vec<f64> {
    if before_ns >= after_ns {
        return before.to_vec();
    }
    let ratio = (target_ns.saturating_sub(before_ns)) as f64 / (after_ns - before_ns) as f64;
    before
        .iter()
        .zip(after.iter())
        .map(|(left, right)| left + (right - left) * ratio)
        .collect()
}

fn write_parquet(
    parquet_path: &Path,
    config: &AssemblerRuntimeConfig,
    rows: &EpisodeRows,
) -> Result<(), Box<dyn Error>> {
    let mut fields = vec![
        Field::new("timestamp", DataType::Float64, false),
        Field::new("frame_index", DataType::Int64, false),
        Field::new("episode_index", DataType::Int64, false),
        Field::new("index", DataType::Int64, false),
        Field::new("task_index", DataType::Int64, false),
        Field::new("next.done", DataType::Boolean, false),
        Field::new(
            "action",
            DataType::List(Arc::new(Field::new("item", DataType::Float64, true))),
            false,
        ),
    ];

    let mut arrays: Vec<ArrayRef> = vec![
        Arc::new(Float64Array::from(rows.timestamps_s.clone())),
        Arc::new(Int64Array::from(rows.frame_indices.clone())),
        Arc::new(Int64Array::from(rows.episode_indices.clone())),
        Arc::new(Int64Array::from(rows.global_indices.clone())),
        Arc::new(Int64Array::from(rows.task_indices.clone())),
        Arc::new(BooleanArray::from(rows.done_flags.clone())),
        Arc::new(build_list_array(&rows.action_rows)),
    ];

    for robot in &config.robots {
        let Some(columns) = rows.robot_columns.get(&robot.robot_name) else {
            continue;
        };
        fields.push(Field::new(
            &format!("observation.state.{}.position", robot.robot_name),
            DataType::List(Arc::new(Field::new("item", DataType::Float64, true))),
            false,
        ));
        arrays.push(Arc::new(build_list_array(&columns.positions)));

        fields.push(Field::new(
            &format!("observation.state.{}.velocity", robot.robot_name),
            DataType::List(Arc::new(Field::new("item", DataType::Float64, true))),
            false,
        ));
        arrays.push(Arc::new(build_list_array(&columns.velocities)));

        fields.push(Field::new(
            &format!("observation.state.{}.effort", robot.robot_name),
            DataType::List(Arc::new(Field::new("item", DataType::Float64, true))),
            false,
        ));
        arrays.push(Arc::new(build_list_array(&columns.efforts)));
    }

    let schema = Arc::new(Schema::new(fields));
    let batch = RecordBatch::try_new(Arc::clone(&schema), arrays)?;
    let output = File::create(parquet_path)?;
    let mut writer = ArrowWriter::try_new(output, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

fn build_list_array(rows: &[Vec<f64>]) -> arrow_array::ListArray {
    let mut builder = ListBuilder::new(Float64Builder::new());
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
    config: &AssemblerRuntimeConfig,
    episode: &EpisodeAssemblyInput,
    frame_count: usize,
) -> Result<(), Box<dyn Error>> {
    let meta_dir = staging_dir.join("meta");
    fs::create_dir_all(&meta_dir)?;

    let info = build_dataset_info(config, frame_count);
    let info_path = meta_dir.join("info.json");
    fs::write(&info_path, serde_json::to_vec_pretty(&info)?)?;

    write_jsonl_line(
        &meta_dir.join("tasks.jsonl"),
        &serde_json::json!({
            "task_index": 0,
            "task": "collect",
        }),
    )?;
    write_jsonl_line(
        &meta_dir.join("episodes.jsonl"),
        &serde_json::json!({
            "episode_index": episode.episode_index,
            "length": frame_count,
            "tasks": [0],
        }),
    )?;
    write_jsonl_line(
        &meta_dir.join("episodes_stats.jsonl"),
        &serde_json::json!({
            "episode_index": episode.episode_index,
            "length": frame_count,
            "frame_count": frame_count,
        }),
    )?;
    Ok(())
}

fn write_jsonl_line(path: &Path, value: &serde_json::Value) -> Result<(), Box<dyn Error>> {
    let mut file = File::create(path)?;
    file.write_all(serde_json::to_string(value)?.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

fn build_dataset_info(config: &AssemblerRuntimeConfig, frame_count: usize) -> DatasetInfo {
    let mut features = BTreeMap::new();
    features.insert(
        "timestamp".into(),
        FeatureSpec {
            dtype: "float64".into(),
            shape: vec![1],
            names: None,
            video_info: None,
        },
    );
    features.insert(
        "frame_index".into(),
        FeatureSpec {
            dtype: "int64".into(),
            shape: vec![1],
            names: None,
            video_info: None,
        },
    );
    features.insert(
        "episode_index".into(),
        FeatureSpec {
            dtype: "int64".into(),
            shape: vec![1],
            names: None,
            video_info: None,
        },
    );
    features.insert(
        "index".into(),
        FeatureSpec {
            dtype: "int64".into(),
            shape: vec![1],
            names: None,
            video_info: None,
        },
    );
    features.insert(
        "task_index".into(),
        FeatureSpec {
            dtype: "int64".into(),
            shape: vec![1],
            names: None,
            video_info: None,
        },
    );
    features.insert(
        "next.done".into(),
        FeatureSpec {
            dtype: "bool".into(),
            shape: vec![1],
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

    for robot in &config.robots {
        let names = Some(joint_names(robot.dof as usize));
        features.insert(
            format!("observation.state.{}.position", robot.robot_name),
            FeatureSpec {
                dtype: "float64".into(),
                shape: vec![robot.dof as usize],
                names: names.clone(),
                video_info: None,
            },
        );
        features.insert(
            format!("observation.state.{}.velocity", robot.robot_name),
            FeatureSpec {
                dtype: "float64".into(),
                shape: vec![robot.dof as usize],
                names: names.clone(),
                video_info: None,
            },
        );
        features.insert(
            format!("observation.state.{}.effort", robot.robot_name),
            FeatureSpec {
                dtype: "float64".into(),
                shape: vec![robot.dof as usize],
                names,
                video_info: None,
            },
        );
    }

    for camera in &config.cameras {
        features.insert(
            camera.camera_name.clone(),
            FeatureSpec {
                dtype: "video".into(),
                shape: camera_shape(camera.width, camera.height, camera.pixel_format),
                names: Some(camera_axis_names(camera.pixel_format)),
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

    DatasetInfo {
        codebase_version: "v2.1".into(),
        robot_type: Some(
            config
                .robots
                .iter()
                .map(|robot| robot.robot_name.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
        total_episodes: 1,
        total_frames: frame_count as u64,
        total_tasks: 1,
        chunks_size: config.chunk_size,
        fps: config.fps,
        splits: BTreeMap::new(),
        data_path: "data/chunk-{chunk_index:03d}/episode_{episode_index:06d}.parquet".into(),
        video_path: config
            .cameras
            .first()
            .map(|camera| {
                format!(
                    "videos/chunk-{{chunk_index:03d}}/{{video_key}}/episode_{{episode_index:06d}}.{}",
                    camera.artifact_format.extension()
                )
            }),
        features,
        embedded_config_toml: config.embedded_config_toml.clone(),
    }
}

fn staged_parquet_path(staging_dir: &Path, episode_index: u32, chunk_size: u32) -> PathBuf {
    let chunk_index = episode_index / chunk_size;
    staging_dir
        .join("data")
        .join(format!("chunk-{chunk_index:03}"))
        .join(format!("episode_{episode_index:06}.parquet"))
}

fn staged_video_path(
    staging_dir: &Path,
    episode_index: u32,
    chunk_size: u32,
    camera_name: &str,
    extension: &str,
) -> PathBuf {
    let chunk_index = episode_index / chunk_size;
    staging_dir
        .join("videos")
        .join(format!("chunk-{chunk_index:03}"))
        .join(camera_name)
        .join(format!("episode_{episode_index:06}.{extension}"))
}

fn move_or_copy_file(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(_) => {
            fs::copy(source, destination)?;
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

fn joint_names(dof: usize) -> Vec<String> {
    (0..dof).map(|index| format!("joint_{index}")).collect()
}

fn action_feature_names(actions: &[AssemblerActionRuntimeConfig]) -> Vec<String> {
    let mut names = Vec::new();
    for action in actions {
        for index in 0..action.dof {
            names.push(format!("{}.{}", action.source_name, index));
        }
    }
    names
}

fn camera_shape(width: u32, height: u32, pixel_format: PixelFormat) -> Vec<usize> {
    match pixel_format {
        PixelFormat::Depth16 | PixelFormat::Gray8 => vec![height as usize, width as usize, 1],
        _ => vec![height as usize, width as usize, 3],
    }
}

fn camera_axis_names(pixel_format: PixelFormat) -> Vec<String> {
    match pixel_format {
        PixelFormat::Depth16 | PixelFormat::Gray8 => vec!["height".into(), "width".into(), "channel".into()],
        _ => vec!["height".into(), "width".into(), "channel".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollio_types::config::{
        AssemblerActionRuntimeConfig, AssemblerCameraRuntimeConfig, AssemblerRobotRuntimeConfig,
        AssemblerRuntimeConfig, EncodedHandoffMode, EpisodeFormat, EncoderArtifactFormat,
        EncoderCodec,
    };
    use rollio_types::messages::PixelFormat;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn build_episode_rows_resamples_to_nominal_fps() {
        let config = test_runtime_config(temp_dir().to_string_lossy().into_owned());
        let episode = EpisodeAssemblyInput {
            episode_index: 0,
            start_time_ns: 0,
            stop_time_ns: 3_000_000_000,
            robot_samples: BTreeMap::from([(
                "leader_arm".into(),
                (0..150)
                    .map(|index| RobotObservationSample {
                        timestamp_ns: index * 20_000_000,
                        positions: vec![index as f64; 6],
                        velocities: vec![1.0; 6],
                        efforts: vec![0.5; 6],
                    })
                    .collect(),
            )]),
            action_samples: BTreeMap::from([(
                "follower_arm".into(),
                (0..150)
                    .map(|index| ActionSample {
                        timestamp_ns: index * 20_000_000,
                        values: vec![index as f64; 6],
                    })
                    .collect(),
            )]),
            video_paths: BTreeMap::from([("camera_top".into(), temp_dir().join("episode_000000.mp4"))]),
        };

        let rows = build_episode_rows(&config, &episode);
        assert_eq!(rows.timestamps_s.len(), 90);
        assert_eq!(rows.frame_indices.len(), 90);
        assert_eq!(rows.action_rows.len(), 90);
        assert_eq!(
            rows.robot_columns["leader_arm"].positions.first().unwrap().len(),
            6
        );
    }

    #[test]
    fn stage_episode_writes_expected_layout() {
        let root = temp_dir();
        let config = test_runtime_config(root.to_string_lossy().into_owned());
        let source_video = root.join("camera_top_episode_000000.mp4");
        fs::create_dir_all(&root).expect("temp dir should exist");
        fs::write(&source_video, b"fake video").expect("source video should be written");

        let episode = EpisodeAssemblyInput {
            episode_index: 0,
            start_time_ns: 0,
            stop_time_ns: 2_000_000_000,
            robot_samples: BTreeMap::from([(
                "leader_arm".into(),
                vec![
                    RobotObservationSample {
                        timestamp_ns: 0,
                        positions: vec![0.0; 6],
                        velocities: vec![0.0; 6],
                        efforts: vec![0.0; 6],
                    },
                    RobotObservationSample {
                        timestamp_ns: 2_000_000_000,
                        positions: vec![1.0; 6],
                        velocities: vec![0.5; 6],
                        efforts: vec![0.2; 6],
                    },
                ],
            )]),
            action_samples: BTreeMap::from([(
                "follower_arm".into(),
                vec![
                    ActionSample {
                        timestamp_ns: 0,
                        values: vec![0.0; 6],
                    },
                    ActionSample {
                        timestamp_ns: 2_000_000_000,
                        values: vec![1.0; 6],
                    },
                ],
            )]),
            video_paths: BTreeMap::from([("camera_top".into(), source_video.clone())]),
        };

        let staged = stage_episode(&config, &episode).expect("episode should stage");
        assert_eq!(staged.episode_index, 0);
        assert!(staged.frame_count > 0);
        assert!(staged.staging_dir.join("meta/info.json").exists());
        assert!(
            staged
                .staging_dir
                .join("data/chunk-000/episode_000000.parquet")
                .exists()
        );
        assert!(
            staged
                .staging_dir
                .join("videos/chunk-000/camera_top/episode_000000.mp4")
                .exists()
        );
        assert!(!source_video.exists(), "source video should be moved into staging");

        let info: serde_json::Value = serde_json::from_slice(
            &fs::read(staged.staging_dir.join("meta/info.json"))
                .expect("info.json should be readable"),
        )
        .expect("info.json should be valid JSON");
        assert_eq!(info["codebase_version"], "v2.1");
        assert_eq!(info["total_episodes"], 1);
        assert!(info["total_frames"].as_u64().unwrap_or_default() > 0);
        assert_eq!(info["features"]["camera_top"]["dtype"], "video");
    }

    fn test_runtime_config(staging_dir: String) -> AssemblerRuntimeConfig {
        AssemblerRuntimeConfig {
            process_id: "episode-assembler".into(),
            format: EpisodeFormat::LeRobotV2_1,
            fps: 30,
            chunk_size: 1000,
            missing_video_timeout_ms: 5_000,
            staging_dir,
            encoded_handoff: EncodedHandoffMode::File,
            cameras: vec![AssemblerCameraRuntimeConfig {
                camera_name: "camera_top".into(),
                encoder_process_id: "encoder.camera_top".into(),
                width: 640,
                height: 480,
                fps: 30,
                pixel_format: PixelFormat::Rgb24,
                codec: EncoderCodec::H264,
                artifact_format: EncoderArtifactFormat::Mp4,
            }],
            robots: vec![AssemblerRobotRuntimeConfig {
                robot_name: "leader_arm".into(),
                state_topic: "robot/leader_arm/state".into(),
                dof: 6,
            }],
            actions: vec![AssemblerActionRuntimeConfig {
                source_name: "follower_arm".into(),
                command_topic: "robot/follower_arm/command".into(),
                dof: 6,
            }],
            embedded_config_toml: "fps = 30".into(),
        }
    }

    fn temp_dir() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rollio-assembler-tests-{suffix}"));
        fs::create_dir_all(&path).expect("temp path should exist");
        path
    }
}
