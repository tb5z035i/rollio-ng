use clap::Args;
use iceoryx2::prelude::*;
use rollio_bus::{
    BACKPRESSURE_SERVICE, CONTROL_EVENTS_SERVICE, EPISODE_READY_SERVICE, EPISODE_STORED_SERVICE,
};
use rollio_types::config::{StorageBackend, StorageRuntimeConfig};
use rollio_types::messages::{
    BackpressureEvent, ControlEvent, EpisodeReady, EpisodeStored, FixedString256, FixedString64,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(long, value_name = "PATH", conflicts_with = "config_inline")]
    pub config: Option<PathBuf>,
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    pub config_inline: Option<String>,
}

#[derive(Debug, Clone)]
struct EpisodeReadyRequest {
    episode_index: u32,
    staging_dir: PathBuf,
}

enum WorkerCommand {
    Store(EpisodeReadyRequest),
    Shutdown,
}

enum WorkerEvent {
    Stored {
        episode_index: u32,
        output_path: PathBuf,
    },
    Error(String),
    ShutdownComplete,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    video_path: Option<String>,
    /// Mirror of the assembler's `info.json::raw_path` so the merged
    /// dataset advertises the per-channel raw Parquet dump alongside
    /// `data_path` and `video_path`. Older datasets predating Phase 6c
    /// omit this field — `Option<...>` keeps deserialization tolerant.
    #[serde(skip_serializing_if = "Option::is_none")]
    raw_path: Option<String>,
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

pub fn run(args: RunArgs) -> Result<(), Box<dyn Error>> {
    let config = load_runtime_config(&args)?;
    run_with_config(config)
}

fn load_runtime_config(args: &RunArgs) -> Result<StorageRuntimeConfig, Box<dyn Error>> {
    match (&args.config, &args.config_inline) {
        (Some(path), None) => Ok(StorageRuntimeConfig::from_file(path)?),
        (None, Some(inline)) => Ok(inline.parse::<StorageRuntimeConfig>()?),
        (None, None) => Err("storage requires --config or --config-inline".into()),
        (Some(_), Some(_)) => Err("storage config flags are mutually exclusive".into()),
    }
}

pub fn run_with_config(config: StorageRuntimeConfig) -> Result<(), Box<dyn Error>> {
    if config.backend != StorageBackend::Local {
        return Err("storage currently supports backend=local only".into());
    }

    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;

    let control_subscriber = create_control_subscriber(&node)?;
    let episode_ready_subscriber = create_episode_ready_subscriber(&node)?;
    let stored_publisher = create_episode_stored_publisher(&node)?;
    let backpressure_publisher = create_backpressure_publisher(&node)?;

    let (command_tx, command_rx) = mpsc::sync_channel(config.queue_size as usize);
    let (event_tx, event_rx) = mpsc::channel();
    let worker_config = config.clone();
    let worker = thread::Builder::new()
        .name("rollio-storage-worker".into())
        .spawn(move || {
            let _ = worker_main(worker_config, command_rx, event_tx);
        })
        .map_err(|error| io::Error::other(format!("failed to spawn storage worker: {error}")))?;

    let mut shutdown_sent = false;
    let mut shutdown_complete = false;
    while !shutdown_complete {
        while let Ok(event) = event_rx.try_recv() {
            match event {
                WorkerEvent::Stored {
                    episode_index,
                    output_path,
                } => {
                    stored_publisher.send_copy(EpisodeStored {
                        episode_index,
                        output_path: FixedString256::new(&output_path.to_string_lossy()),
                    })?;
                }
                WorkerEvent::Error(message) => {
                    let _ = command_tx.send(WorkerCommand::Shutdown);
                    let _ = worker.join();
                    return Err(message.into());
                }
                WorkerEvent::ShutdownComplete => shutdown_complete = true,
            }
        }

        while let Some(sample) = control_subscriber.receive()? {
            if matches!(*sample.payload(), ControlEvent::Shutdown) && !shutdown_sent {
                command_tx.send(WorkerCommand::Shutdown)?;
                shutdown_sent = true;
            }
        }

        while let Some(sample) = episode_ready_subscriber.receive()? {
            let request = EpisodeReadyRequest {
                episode_index: sample.payload().episode_index,
                staging_dir: PathBuf::from(sample.payload().staging_dir.as_str()),
            };
            if !try_enqueue_request(&command_tx, request)? {
                backpressure_publisher.send_copy(BackpressureEvent {
                    process_id: FixedString64::new(&config.process_id),
                    queue_name: FixedString64::new("episode_queue"),
                })?;
            }
        }

        thread::sleep(Duration::from_millis(2));
    }

    worker
        .join()
        .map_err(|_| io::Error::other("storage worker panicked"))?;
    Ok(())
}

fn create_control_subscriber(
    node: &Node<ipc::Service>,
) -> Result<iceoryx2::port::subscriber::Subscriber<ipc::Service, ControlEvent, ()>, Box<dyn Error>>
{
    let service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<ControlEvent>()
        .open_or_create()?;
    Ok(service.subscriber_builder().create()?)
}

fn create_episode_ready_subscriber(
    node: &Node<ipc::Service>,
) -> Result<iceoryx2::port::subscriber::Subscriber<ipc::Service, EpisodeReady, ()>, Box<dyn Error>>
{
    let service_name: ServiceName = EPISODE_READY_SERVICE.try_into()?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<EpisodeReady>()
        .open_or_create()?;
    Ok(service.subscriber_builder().create()?)
}

fn create_episode_stored_publisher(
    node: &Node<ipc::Service>,
) -> Result<iceoryx2::port::publisher::Publisher<ipc::Service, EpisodeStored, ()>, Box<dyn Error>> {
    let service_name: ServiceName = EPISODE_STORED_SERVICE.try_into()?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<EpisodeStored>()
        .open_or_create()?;
    Ok(service.publisher_builder().create()?)
}

fn create_backpressure_publisher(
    node: &Node<ipc::Service>,
) -> Result<iceoryx2::port::publisher::Publisher<ipc::Service, BackpressureEvent, ()>, Box<dyn Error>>
{
    let service_name: ServiceName = BACKPRESSURE_SERVICE.try_into()?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<BackpressureEvent>()
        .open_or_create()?;
    Ok(service.publisher_builder().create()?)
}

fn try_enqueue_request(
    sender: &mpsc::SyncSender<WorkerCommand>,
    request: EpisodeReadyRequest,
) -> Result<bool, Box<dyn Error>> {
    match sender.try_send(WorkerCommand::Store(request)) {
        Ok(()) => Ok(true),
        Err(mpsc::TrySendError::Full(_)) => Ok(false),
        Err(mpsc::TrySendError::Disconnected(_)) => {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "storage worker disconnected").into())
        }
    }
}

fn worker_main(
    config: StorageRuntimeConfig,
    receiver: mpsc::Receiver<WorkerCommand>,
    events: mpsc::Sender<WorkerEvent>,
) -> Result<(), Box<dyn Error>> {
    loop {
        match receiver.recv() {
            Ok(WorkerCommand::Store(request)) => match store_episode(&config, &request) {
                Ok(output_path) => {
                    let _ = events.send(WorkerEvent::Stored {
                        episode_index: request.episode_index,
                        output_path,
                    });
                }
                Err(error) => {
                    let _ = events.send(WorkerEvent::Error(error.to_string()));
                    return Err(error);
                }
            },
            Ok(WorkerCommand::Shutdown) | Err(_) => {
                let _ = events.send(WorkerEvent::ShutdownComplete);
                return Ok(());
            }
        }
    }
}

fn store_episode(
    config: &StorageRuntimeConfig,
    request: &EpisodeReadyRequest,
) -> Result<PathBuf, Box<dyn Error>> {
    match config.backend {
        StorageBackend::Local => store_episode_local(config, request),
        StorageBackend::Http => Err("storage backend=http is reserved for future work".into()),
    }
}

fn store_episode_local(
    config: &StorageRuntimeConfig,
    request: &EpisodeReadyRequest,
) -> Result<PathBuf, Box<dyn Error>> {
    let output_root = PathBuf::from(
        config
            .output_path
            .as_deref()
            .ok_or("local storage requires output_path")?,
    );
    fs::create_dir_all(&output_root)?;

    let staged_info = read_dataset_info(&request.staging_dir.join("meta/info.json"))?;
    merge_tree_if_present(&request.staging_dir.join("data"), &output_root.join("data"))?;
    merge_tree_if_present(
        &request.staging_dir.join("videos"),
        &output_root.join("videos"),
    )?;
    // Per-channel raw Parquet dump (Phase 6c). Lives alongside `data/` and
    // `videos/`. Without this merge the assembler's `raw::write_episode_raw_dump`
    // would write into the staging dir but never reach the dataset, since
    // `fs::remove_dir_all(&request.staging_dir)` below wipes it after the
    // normal data/videos move.
    merge_tree_if_present(&request.staging_dir.join("raw"), &output_root.join("raw"))?;
    merge_meta_jsonl_if_present(
        &request.staging_dir.join("meta/tasks.jsonl"),
        &output_root.join("meta/tasks.jsonl"),
        true,
    )?;
    merge_meta_jsonl_if_present(
        &request.staging_dir.join("meta/episodes.jsonl"),
        &output_root.join("meta/episodes.jsonl"),
        false,
    )?;
    merge_meta_jsonl_if_present(
        &request.staging_dir.join("meta/episodes_stats.jsonl"),
        &output_root.join("meta/episodes_stats.jsonl"),
        false,
    )?;
    update_root_info(&output_root.join("meta/info.json"), staged_info)?;
    fs::remove_dir_all(&request.staging_dir)?;
    Ok(output_root)
}

fn read_dataset_info(path: &Path) -> Result<DatasetInfo, Box<dyn Error>> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn update_root_info(path: &Path, staged: DatasetInfo) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut info = if path.exists() {
        read_dataset_info(path)?
    } else {
        DatasetInfo {
            total_episodes: 0,
            total_frames: 0,
            total_tasks: staged.total_tasks,
            codebase_version: staged.codebase_version.clone(),
            robot_type: staged.robot_type.clone(),
            chunks_size: staged.chunks_size,
            fps: staged.fps,
            splits: staged.splits.clone(),
            data_path: staged.data_path.clone(),
            video_path: staged.video_path.clone(),
            raw_path: staged.raw_path.clone(),
            features: staged.features.clone(),
            embedded_config_toml: staged.embedded_config_toml.clone(),
        }
    };

    info.total_episodes = info.total_episodes.saturating_add(staged.total_episodes);
    info.total_frames = info.total_frames.saturating_add(staged.total_frames);
    info.total_tasks = info.total_tasks.max(staged.total_tasks);
    if info.robot_type.is_none() {
        info.robot_type = staged.robot_type;
    }
    // Union the staged feature set into the merged feature set so the
    // dataset's `info.json` always advertises every column ever produced.
    // Without this an episode with a sparse schema (e.g. recorded with
    // the robot disconnected, so observations are missing) would either
    // overwrite the richer schema from an earlier episode or — under
    // the previous `is_empty()` guard — silently drop a richer schema
    // from a later episode. Tools (LeRobot training scripts, custom
    // analysis notebooks) typically consult `info.features` to know
    // which columns to expect across the whole dataset.
    for (key, spec) in staged.features {
        info.features.entry(key).or_insert(spec);
    }
    if info.video_path.is_none() {
        info.video_path = staged.video_path;
    }
    if info.raw_path.is_none() {
        info.raw_path = staged.raw_path;
    }
    if info.embedded_config_toml.trim().is_empty() {
        info.embedded_config_toml = staged.embedded_config_toml;
    }

    fs::write(path, serde_json::to_vec_pretty(&info)?)?;
    Ok(())
}

fn merge_tree_if_present(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    if !source.exists() {
        return Ok(());
    }
    fs::create_dir_all(destination)?;
    merge_directory_contents(source, destination)
}

fn merge_directory_contents(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let entry_path = entry.path();
        let target_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&target_path)?;
            merge_directory_contents(&entry_path, &target_path)?;
        } else if entry.file_type()?.is_file() {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            move_or_copy_file(&entry_path, &target_path)?;
        }
    }
    Ok(())
}

fn merge_meta_jsonl_if_present(
    source: &Path,
    destination: &Path,
    keep_existing_only: bool,
) -> Result<(), Box<dyn Error>> {
    if !source.exists() {
        return Ok(());
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    if keep_existing_only && destination.exists() {
        return Ok(());
    }
    let contents = fs::read(source)?;
    if keep_existing_only && !destination.exists() {
        fs::write(destination, contents)?;
        return Ok(());
    }
    let mut output = File::options()
        .create(true)
        .append(true)
        .open(destination)?;
    output.write_all(&contents)?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn store_episode_local_moves_files_and_updates_info() {
        let root = temp_dir("storage-root");
        let staging = write_staged_episode(temp_dir("storage-stage"), 0, 10);
        let config = test_config(root.clone());
        let request = EpisodeReadyRequest {
            episode_index: 0,
            staging_dir: staging.clone(),
        };

        let output = store_episode_local(&config, &request).expect("episode should store");
        assert_eq!(output, root);
        assert!(root.join("data/chunk-000/episode_000000.parquet").exists());
        assert!(root
            .join("videos/chunk-000/camera_top/episode_000000.mp4")
            .exists());
        // Phase 6c raw dump must reach the merged dataset alongside
        // `data/` and `videos/`. Without the explicit `raw/` merge in
        // `store_episode_local` this file would be wiped with the
        // staging dir and silently disappear.
        assert!(
            root.join("raw/chunk-000/episode_000000/robot_a__arm__joint_position.parquet")
                .exists(),
            "raw/ tree should be merged into the dataset"
        );
        assert!(root.join("meta/info.json").exists());
        assert!(
            !staging.exists(),
            "staging dir should be removed after store"
        );

        let info = read_dataset_info(&root.join("meta/info.json")).expect("info should parse");
        assert_eq!(info.total_episodes, 1);
        assert_eq!(info.total_frames, 10);
        assert_eq!(
            info.raw_path.as_deref(),
            Some("raw/chunk-{chunk_index:03d}/episode_{episode_index:06d}"),
            "info.json should advertise the raw_path template"
        );
    }

    #[test]
    fn store_episode_local_unions_features_across_episodes() {
        // Episode 0 stages with a populated feature set (e.g. observation
        // present); episode 1 stages with no features (e.g. robot was
        // disconnected during recording). After both land, the merged
        // info.json should keep the richer schema from episode 0 instead
        // of being clobbered by episode 1's empty set.
        let root = temp_dir("storage-root-union");
        let config = test_config(root.clone());

        let stage_full = temp_dir("storage-stage-full");
        let mut full_features = BTreeMap::new();
        full_features.insert(
            "observation.state.airbot_play__arm.joint_position".into(),
            FeatureSpec {
                dtype: "float64".into(),
                shape: vec![6],
                names: Some((0..6).map(|i| format!("joint_{i}")).collect()),
                video_info: None,
            },
        );
        let stage_full_root =
            write_staged_episode_with_features(stage_full, 0, 10, full_features.clone());
        store_episode_local(
            &config,
            &EpisodeReadyRequest {
                episode_index: 0,
                staging_dir: stage_full_root,
            },
        )
        .expect("first episode should store");

        let stage_empty = write_staged_episode_with_features(
            temp_dir("storage-stage-empty"),
            1,
            20,
            BTreeMap::new(),
        );
        store_episode_local(
            &config,
            &EpisodeReadyRequest {
                episode_index: 1,
                staging_dir: stage_empty,
            },
        )
        .expect("second episode should store");

        let info = read_dataset_info(&root.join("meta/info.json")).expect("info should parse");
        assert!(
            info.features
                .contains_key("observation.state.airbot_play__arm.joint_position"),
            "richer feature set from earlier episode should survive a later \
             episode with an empty feature set, got features: {:?}",
            info.features.keys().collect::<Vec<_>>()
        );
        assert_eq!(info.total_episodes, 2);
        assert_eq!(info.total_frames, 30);
    }

    #[test]
    fn store_episode_local_accumulates_episode_counts() {
        let root = temp_dir("storage-root-accum");
        let config = test_config(root.clone());
        let first = EpisodeReadyRequest {
            episode_index: 0,
            staging_dir: write_staged_episode(temp_dir("storage-stage-one"), 0, 10),
        };
        let second = EpisodeReadyRequest {
            episode_index: 1,
            staging_dir: write_staged_episode(temp_dir("storage-stage-two"), 1, 20),
        };

        store_episode_local(&config, &first).expect("first episode should store");
        store_episode_local(&config, &second).expect("second episode should store");

        let info = read_dataset_info(&root.join("meta/info.json")).expect("info should parse");
        assert_eq!(info.total_episodes, 2);
        assert_eq!(info.total_frames, 30);
    }

    #[test]
    fn try_enqueue_request_reports_backpressure_when_queue_is_full() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let first = EpisodeReadyRequest {
            episode_index: 0,
            staging_dir: PathBuf::from("/tmp/episode_000000"),
        };
        let second = EpisodeReadyRequest {
            episode_index: 1,
            staging_dir: PathBuf::from("/tmp/episode_000001"),
        };

        assert!(try_enqueue_request(&sender, first).expect("first request should queue"));
        assert!(
            !try_enqueue_request(&sender, second).expect("second request should hit backpressure")
        );
        drop(receiver);
    }

    fn test_config(output_path: PathBuf) -> StorageRuntimeConfig {
        StorageRuntimeConfig {
            process_id: "storage".into(),
            backend: StorageBackend::Local,
            output_path: Some(output_path.to_string_lossy().into_owned()),
            endpoint: None,
            queue_size: 2,
        }
    }

    fn write_staged_episode_with_features(
        root: PathBuf,
        episode_index: u32,
        frame_count: u64,
        features: BTreeMap<String, FeatureSpec>,
    ) -> PathBuf {
        let staged = write_staged_episode(root, episode_index, frame_count);
        // Overwrite the meta/info.json the helper just wrote with one that
        // carries the requested feature set. Everything else (paths, sizes,
        // etc.) stays the same.
        let info_path = staged.join("meta/info.json");
        let mut info = read_dataset_info(&info_path).expect("staged info should parse");
        info.features = features;
        fs::write(
            &info_path,
            serde_json::to_vec_pretty(&info).expect("info should serialize"),
        )
        .expect("info should be rewritten");
        staged
    }

    fn write_staged_episode(root: PathBuf, episode_index: u32, frame_count: u64) -> PathBuf {
        let data_path = root.join(format!("data/chunk-000/episode_{episode_index:06}.parquet"));
        let video_path = root.join(format!(
            "videos/chunk-000/camera_top/episode_{episode_index:06}.mp4"
        ));
        // Per-channel raw Parquet (Phase 6c). Storage now merges this tree
        // alongside `data/` and `videos/`, so the test fixture mirrors a
        // realistic staging dir that includes one raw observation file.
        let raw_path = root.join(format!(
            "raw/chunk-000/episode_{episode_index:06}/robot_a__arm__joint_position.parquet"
        ));
        let meta_path = root.join("meta");
        fs::create_dir_all(data_path.parent().unwrap()).expect("data dir should exist");
        fs::create_dir_all(video_path.parent().unwrap()).expect("video dir should exist");
        fs::create_dir_all(raw_path.parent().unwrap()).expect("raw dir should exist");
        fs::create_dir_all(&meta_path).expect("meta dir should exist");
        fs::write(&data_path, b"parquet").expect("parquet should be written");
        fs::write(&video_path, b"video").expect("video should be written");
        fs::write(&raw_path, b"raw_parquet").expect("raw parquet should be written");
        let info = DatasetInfo {
            codebase_version: "v2.1".into(),
            robot_type: Some("leader_arm".into()),
            total_episodes: 1,
            total_frames: frame_count,
            total_tasks: 1,
            chunks_size: 1000,
            fps: 30,
            splits: BTreeMap::new(),
            data_path: "data/chunk-{chunk_index:03d}/episode_{episode_index:06d}.parquet".into(),
            video_path: Some(
                "videos/chunk-{chunk_index:03d}/{video_key}/episode_{episode_index:06d}.mp4".into(),
            ),
            raw_path: Some("raw/chunk-{chunk_index:03d}/episode_{episode_index:06d}".into()),
            features: BTreeMap::new(),
            embedded_config_toml: "fps = 30".into(),
        };
        fs::write(
            meta_path.join("info.json"),
            serde_json::to_vec_pretty(&info).expect("info should serialize"),
        )
        .expect("info should be written");
        fs::write(
            meta_path.join("tasks.jsonl"),
            "{\"task_index\":0,\"task\":\"collect\"}\n",
        )
        .expect("tasks should be written");
        fs::write(
            meta_path.join("episodes.jsonl"),
            format!("{{\"episode_index\":{episode_index},\"length\":{frame_count}}}\n"),
        )
        .expect("episodes should be written");
        fs::write(
            meta_path.join("episodes_stats.jsonl"),
            format!("{{\"episode_index\":{episode_index},\"frame_count\":{frame_count}}}\n"),
        )
        .expect("stats should be written");
        root
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{suffix}"));
        fs::create_dir_all(&path).expect("temp dir should exist");
        path
    }
}
