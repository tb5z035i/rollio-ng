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

use crate::ffi::DataloopClient;

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
        output_path: String,
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
        (None, None) => {
            Err("rollio-storage-dataloop requires --config or --config-inline".into())
        }
        (Some(_), Some(_)) => {
            Err("rollio-storage-dataloop config flags are mutually exclusive".into())
        }
    }
}

fn resolve_base_url(config: &StorageRuntimeConfig) -> Result<String, Box<dyn Error>> {
    if let Some(url) = config.endpoint.as_deref() {
        if !url.trim().is_empty() {
            return Ok(url.to_owned());
        }
    }
    std::env::var("DATALOOP_BASE_URL")
        .map_err(|_| "dataloop base_url not found in config.endpoint or DATALOOP_BASE_URL env var".into())
}

fn resolve_token(config: &StorageRuntimeConfig) -> Result<String, Box<dyn Error>> {
    if let Some(token) = config.dataloop_token.as_deref() {
        if !token.trim().is_empty() {
            return Ok(token.to_owned());
        }
    }
    std::env::var("DATALOOP_TOKEN")
        .map_err(|_| "dataloop token not found in config or DATALOOP_TOKEN env var".into())
}

fn resolve_project_id(config: &StorageRuntimeConfig) -> Result<String, Box<dyn Error>> {
    if let Some(id) = config.dataloop_project_id.as_deref() {
        if !id.trim().is_empty() {
            return Ok(id.to_owned());
        }
    }
    std::env::var("DATALOOP_PROJECT_ID")
        .or_else(|_| std::env::var("PROJECT_ID"))
        .map_err(|_| "dataloop project_id not found in config or DATALOOP_PROJECT_ID env var".into())
}

pub fn run_with_config(config: StorageRuntimeConfig) -> Result<(), Box<dyn Error>> {
    if config.backend != StorageBackend::Dataloop {
        return Err("rollio-storage-dataloop only supports backend=dataloop".into());
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
        .name("rollio-storage-dataloop-worker".into())
        .spawn(move || {
            if let Err(e) = worker_main(worker_config, command_rx, &event_tx) {
                let _ = event_tx.send(WorkerEvent::Error(e.to_string()));
            }
        })
        .map_err(|e| io::Error::other(format!("failed to spawn worker: {e}")))?;

// PLACEHOLDER_RUNTIME_MAIN_LOOP

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
                        output_path: FixedString256::new(&output_path),
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
        .map_err(|_| io::Error::other("rollio-storage-dataloop worker panicked"))?;
    Ok(())
}

// PLACEHOLDER_RUNTIME_IPC

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

// PLACEHOLDER_RUNTIME_WORKER

fn worker_main(
    config: StorageRuntimeConfig,
    receiver: mpsc::Receiver<WorkerCommand>,
    events: &mpsc::Sender<WorkerEvent>,
) -> Result<(), Box<dyn Error>> {
    let base_url = resolve_base_url(&config)?;
    let token = resolve_token(&config)?;
    let project_id = resolve_project_id(&config)?;

    let client = DataloopClient::new(&base_url, &token)
        .map_err(|e| io::Error::other(format!("failed to create dataloop client: {e}")))?;

    loop {
        match receiver.recv() {
            Ok(WorkerCommand::Store(request)) => {
                match upload_episode(&client, &project_id, &request) {
                    Ok(episode_id) => {
                        let _ = events.send(WorkerEvent::Stored {
                            episode_index: request.episode_index,
                            output_path: episode_id,
                        });
                    }
                    Err(error) => {
                        let _ = events.send(WorkerEvent::Error(error.to_string()));
                        return Err(error);
                    }
                }
            }
            Ok(WorkerCommand::Shutdown) | Err(_) => {
                let _ = events.send(WorkerEvent::ShutdownComplete);
                return Ok(());
            }
        }
    }
}

// PLACEHOLDER_RUNTIME_UPLOAD

fn upload_episode(
    client: &DataloopClient,
    project_id: &str,
    request: &EpisodeReadyRequest,
) -> Result<String, Box<dyn Error>> {
    let files = collect_files(&request.staging_dir)?;
    if files.is_empty() {
        return Err(format!(
            "staging dir is empty: {}",
            request.staging_dir.display()
        )
        .into());
    }

    let tags_json = format!(
        r#"{{"episode_index":"{}","source":"rollio"}}"#,
        request.episode_index
    );

    let anchor = &files[0];
    let result = client
        .create_episode(anchor, project_id, &tags_json)
        .map_err(|e| io::Error::other(format!("create_episode failed: {e}")))?;

    if files.len() > 1 {
        let metadata = client
            .get_episode(&result.episode_id)
            .map_err(|e| io::Error::other(format!("get_episode failed: {e}")))?;

        let remaining: Vec<&Path> = files[1..].iter().map(|p| p.as_path()).collect();
        let upload_result = client
            .upload_to_episode(&remaining, &metadata.episode_path, &metadata.bucket)
            .map_err(|e| io::Error::other(format!("upload_to_episode failed: {e}")))?;

        if upload_result.failed > 0 {
            return Err(format!(
                "upload_to_episode: {} files failed",
                upload_result.failed
            )
            .into());
        }
    }

    fs::remove_dir_all(&request.staging_dir).ok();
    Ok(result.episode_id)
}

fn collect_files(dir: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = Vec::new();
    collect_files_recursive(dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}
