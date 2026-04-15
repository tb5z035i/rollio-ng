use iceoryx2::prelude::*;
use rollio_bus::EPISODE_COMMAND_SERVICE;
use rollio_types::config::Config;
use rollio_types::messages::EpisodeCommand;
use std::error::Error;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct EpisodeCommandPublisher {
    _node: Node<ipc::Service>,
    publisher: iceoryx2::port::publisher::Publisher<ipc::Service, EpisodeCommand, ()>,
}

#[cfg(unix)]
#[test]
#[ignore = "requires built workspace binaries, cameras/build pseudo driver, and ui/web/dist"]
fn collect_pseudo_pipeline_smoke() -> Result<(), Box<dyn Error>> {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root should resolve")
        .to_path_buf();
    let target_dir = workspace_root.join("target/debug");
    ensure_prerequisites(&workspace_root, &target_dir)?;

    let temp_root = unique_temp_dir("rollio-collect-smoke");
    let output_root = temp_root.join("output");
    let staging_root = temp_root.join("staging");

    let mut config = include_str!("../../config/config.pseudo-teleop.toml")
        .parse::<Config>()
        .expect("pseudo config should parse");
    config.storage.output_path = Some(output_root.to_string_lossy().into_owned());
    config.assembler.staging_dir = staging_root.to_string_lossy().into_owned();
    config.visualizer.port = reserve_port()?;
    config.ui.http_host = "127.0.0.1".into();
    config.ui.http_port = reserve_port()?;
    let ui_runtime = config.ui_runtime_config();
    let config_inline = toml::to_string(&config)?;

    let mut child = Command::new(env!("CARGO_BIN_EXE_rollio"))
        .arg("collect")
        .arg("--config-inline")
        .arg(config_inline)
        .current_dir(&workspace_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    wait_for_ui_runtime_config(ui_runtime.http_port, Duration::from_secs(10))?;

    let publisher = create_episode_command_publisher()?;
    thread::sleep(Duration::from_secs(2));
    publisher.publisher.send_copy(EpisodeCommand::Start)?;
    thread::sleep(Duration::from_secs(2));
    publisher.publisher.send_copy(EpisodeCommand::Stop)?;
    thread::sleep(Duration::from_secs(1));
    publisher.publisher.send_copy(EpisodeCommand::Keep)?;

    let info_path = output_root.join("meta/info.json");
    let parquet_path = output_root.join("data/chunk-000/episode_000000.parquet");
    let top_video = output_root.join("videos/chunk-000/camera_top/episode_000000.mp4");
    let side_video = output_root.join("videos/chunk-000/camera_side/episode_000000.mp4");
    wait_for_paths(
        &mut child,
        &[&info_path, &parquet_path, &top_video, &side_video],
        Duration::from_secs(30),
    )?;

    let info: serde_json::Value = serde_json::from_slice(&fs::read(&info_path)?)?;
    assert_eq!(info["total_episodes"], 1);
    assert!(info["total_frames"].as_u64().unwrap_or_default() > 0);

    send_sigint(child.id());
    let status = child.wait()?;
    assert!(
        status.success(),
        "controller should exit cleanly, got {status}"
    );

    let _ = fs::remove_dir_all(&temp_root);
    Ok(())
}

fn ensure_prerequisites(workspace_root: &Path, target_dir: &Path) -> Result<(), Box<dyn Error>> {
    for binary in [
        "rollio",
        "rollio-visualizer",
        "rollio-teleop-router",
        "rollio-encoder",
        "rollio-episode-assembler",
        "rollio-storage",
        "rollio-robot-pseudo",
        "rollio-ui-server",
    ] {
        let path = target_dir.join(binary);
        if !path.exists() {
            return Err(format!(
                "missing built binary {} at {}. Run `cargo build --workspace` first.",
                binary,
                path.display()
            )
            .into());
        }
    }

    let pseudo_camera = workspace_root.join("cameras/build/pseudo/rollio-camera-pseudo");
    if !pseudo_camera.exists() {
        return Err(format!(
            "missing pseudo camera binary at {}. Run `cmake -B cameras/build -S cameras -DCMAKE_CXX_COMPILER=g++ && cmake --build cameras/build --target rollio-camera-pseudo` first.",
            pseudo_camera.display()
        )
        .into());
    }

    let ui_bundle = workspace_root.join("ui/web/dist/index.html");
    if !ui_bundle.exists() {
        return Err(format!(
            "missing UI bundle at {}. Run `cd ui/web && npm install && npm run build` first.",
            ui_bundle.display()
        )
        .into());
    }

    Ok(())
}

fn wait_for_ui_runtime_config(port: u16, timeout: Duration) -> Result<(), Box<dyn Error>> {
    let started = Instant::now();
    loop {
        match fetch_http_body(port, "/api/runtime-config") {
            Ok(body) => {
                let payload: serde_json::Value = serde_json::from_str(&body)?;
                assert_eq!(payload["websocketUrl"], "/ws");
                assert_eq!(payload["episodeKeyBindings"]["startKey"], "s");
                return Ok(());
            }
            Err(error) if started.elapsed() <= timeout => {
                let _ = error;
                thread::sleep(Duration::from_millis(250));
            }
            Err(error) => {
                return Err(format!("timed out waiting for UI server: {error}").into());
            }
        }
    }
}

fn create_episode_command_publisher() -> Result<EpisodeCommandPublisher, Box<dyn Error>> {
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;
    let service_name: ServiceName = EPISODE_COMMAND_SERVICE.try_into()?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<EpisodeCommand>()
        .max_publishers(4)
        .max_subscribers(8)
        .max_nodes(8)
        .open_or_create()?;
    Ok(EpisodeCommandPublisher {
        _node: node,
        publisher: service.publisher_builder().create()?,
    })
}

fn wait_for_paths(
    child: &mut std::process::Child,
    paths: &[&Path],
    timeout: Duration,
) -> Result<(), Box<dyn Error>> {
    let started = Instant::now();
    loop {
        if paths.iter().all(|path| path.exists()) {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            let stdout = child
                .stdout
                .take()
                .map(read_stream)
                .transpose()?
                .unwrap_or_default();
            let stderr = child
                .stderr
                .take()
                .map(read_stream)
                .transpose()?
                .unwrap_or_default();
            return Err(format!(
                "controller exited early with status {status}\nstdout:\n{stdout}\nstderr:\n{stderr}"
            )
            .into());
        }
        if started.elapsed() > timeout {
            return Err(format!(
                "timed out waiting for expected output files: {}",
                paths
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .into());
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn read_stream(mut stream: impl std::io::Read) -> Result<String, Box<dyn Error>> {
    let mut output = String::new();
    stream.read_to_string(&mut output)?;
    Ok(output)
}

fn reserve_port() -> Result<u16, Box<dyn Error>> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn fetch_http_body(port: u16, path: &str) -> Result<String, Box<dyn Error>> {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes())?;

    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    if !response.starts_with("HTTP/1.1 200") && !response.starts_with("HTTP/1.0 200") {
        return Err(format!("unexpected response: {response}").into());
    }

    Ok(response
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .to_string())
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{suffix}"));
    fs::create_dir_all(&path).expect("temp dir should be created");
    path
}

#[cfg(unix)]
fn send_sigint(pid: u32) {
    unsafe {
        libc::kill(pid as i32, libc::SIGINT);
    }
}
