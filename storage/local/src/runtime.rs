use clap::Args;
use iceoryx2::prelude::*;
use rollio_bus::{CONTROL_EVENTS_SERVICE, EPISODE_READY_SERVICE, EPISODE_STORED_SERVICE};
use rollio_types::config::{StorageBackend, StorageRuntimeConfig};
use rollio_types::messages::{ControlEvent, EpisodeReady, EpisodeStored, FixedString256};
use std::error::Error;
use std::fs;
use std::io;
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

pub fn run(args: RunArgs) -> Result<(), Box<dyn Error>> {
    let config = load_runtime_config(&args)?;
    run_with_config(config)
}

fn load_runtime_config(args: &RunArgs) -> Result<StorageRuntimeConfig, Box<dyn Error>> {
    match (&args.config, &args.config_inline) {
        (Some(path), None) => Ok(StorageRuntimeConfig::from_file(path)?),
        (None, Some(inline)) => Ok(inline.parse::<StorageRuntimeConfig>()?),
        (None, None) => Err("rollio-storage-local requires --config or --config-inline".into()),
        (Some(_), Some(_)) => Err("rollio-storage-local config flags are mutually exclusive".into()),
    }
}

pub fn run_with_config(config: StorageRuntimeConfig) -> Result<(), Box<dyn Error>> {
    if config.backend != StorageBackend::Local {
        return Err("rollio-storage-local currently supports backend=local only".into());
    }

    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;

    let control_subscriber = create_control_subscriber(&node)?;
    let episode_ready_subscriber = create_episode_ready_subscriber(&node)?;
    let stored_publisher = create_episode_stored_publisher(&node)?;

    let (command_tx, command_rx) = mpsc::channel::<WorkerCommand>();
    let (event_tx, event_rx) = mpsc::channel::<WorkerEvent>();
    let worker_config = config.clone();
    let worker = thread::Builder::new()
        .name("rollio-storage-local-worker".into())
        .spawn(move || {
            let _ = worker_main(worker_config, command_rx, event_tx);
        })
        .map_err(|error| {
            io::Error::other(format!("failed to spawn rollio-storage-local worker: {error}"))
        })?;

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
            command_tx.send(WorkerCommand::Store(request))?;
        }

        thread::sleep(Duration::from_millis(2));
    }

    worker
        .join()
        .map_err(|_| io::Error::other("rollio-storage-local worker panicked"))?;
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

fn worker_main(
    config: StorageRuntimeConfig,
    receiver: mpsc::Receiver<WorkerCommand>,
    events: mpsc::Sender<WorkerEvent>,
) -> Result<(), Box<dyn Error>> {
    loop {
        match receiver.recv() {
            Ok(WorkerCommand::Store(request)) => match store_episode_local(&config, &request) {
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

/// Move every entry under `request.staging_dir` into
/// `output_path/episode_{idx:06}/` and remove the now-empty staging
/// dir. Same-filesystem moves use `fs::rename` (atomic); cross-device
/// moves fall back to copy + delete.
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
    let episode_dir = output_root.join(format!("episode_{:06}", request.episode_index));
    if episode_dir.exists() {
        fs::remove_dir_all(&episode_dir)?;
    }
    fs::create_dir_all(&episode_dir)?;

    for entry in fs::read_dir(&request.staging_dir)? {
        let entry = entry?;
        let src = entry.path();
        let file_name = entry.file_name();
        let dst = episode_dir.join(&file_name);
        move_path(&src, &dst)?;
    }

    fs::remove_dir_all(&request.staging_dir).ok();
    Ok(episode_dir)
}

fn move_path(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        // EXDEV — staging dir on a different filesystem (e.g. tmpfs)
        // than the output root. Fall back to recursive copy + delete.
        Err(error) if error.raw_os_error() == Some(libc_exdev()) => {
            copy_recursive(src, dst)?;
            if src.is_dir() {
                fs::remove_dir_all(src)?;
            } else {
                fs::remove_file(src)?;
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn libc_exdev() -> i32 {
    18 // EXDEV on Linux and BSDs
}

#[cfg(not(unix))]
fn libc_exdev() -> i32 {
    i32::MIN
}

fn copy_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    if src.is_dir() {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(src, dst).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn test_config(output: &Path) -> StorageRuntimeConfig {
        StorageRuntimeConfig {
            process_id: "storage-local".into(),
            backend: StorageBackend::Local,
            output_path: Some(output.to_string_lossy().into_owned()),
            endpoint: None,
            queue_size: 2,
        }
    }

    #[test]
    fn store_episode_moves_mcap_file_into_episode_subdir() {
        let staging_root = TempDir::new().expect("staging tempdir");
        let output_root = TempDir::new().expect("output tempdir");
        let staging_dir = staging_root.path().join("episode_000007");
        fs::create_dir_all(&staging_dir).unwrap();
        let mcap_path = staging_dir.join("episode.mcap");
        let mut file = fs::File::create(&mcap_path).unwrap();
        file.write_all(b"fake-mcap-bytes").unwrap();
        drop(file);

        let config = test_config(output_root.path());
        let request = EpisodeReadyRequest {
            episode_index: 7,
            staging_dir: staging_dir.clone(),
        };

        let output = store_episode_local(&config, &request).expect("store should succeed");
        assert_eq!(output, output_root.path().join("episode_000007"));
        let moved = output.join("episode.mcap");
        assert!(moved.exists(), "moved file should exist at {moved:?}");
        assert_eq!(fs::read(&moved).unwrap(), b"fake-mcap-bytes");
        assert!(!staging_dir.exists(), "staging dir should be removed");
    }

    #[test]
    fn store_episode_overwrites_existing_episode_dir() {
        let staging_root = TempDir::new().expect("staging tempdir");
        let output_root = TempDir::new().expect("output tempdir");
        let existing = output_root.path().join("episode_000003");
        fs::create_dir_all(&existing).unwrap();
        fs::write(existing.join("stale.txt"), "old").unwrap();

        let staging_dir = staging_root.path().join("episode_000003");
        fs::create_dir_all(&staging_dir).unwrap();
        fs::write(staging_dir.join("episode.mcap"), "new").unwrap();

        let config = test_config(output_root.path());
        let request = EpisodeReadyRequest {
            episode_index: 3,
            staging_dir,
        };
        let output = store_episode_local(&config, &request).expect("store should succeed");
        assert!(!output.join("stale.txt").exists(), "stale file removed");
        assert_eq!(fs::read(output.join("episode.mcap")).unwrap(), b"new");
    }
}
