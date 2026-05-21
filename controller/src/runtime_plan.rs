use crate::process::{ChildSpec, ResolvedCommand};
use crate::runtime_paths::{
    default_device_executable_name, resolve_program, resolve_registered_program,
};
use rollio_types::config::{
    AssemblerRuntimeConfigV2, BinaryDeviceConfig, CollectionMode, EncoderRuntimeConfigV2,
    ProjectConfig, StorageRuntimeConfig, TeleopRuntimeConfigV2,
};
use std::error::Error;
use std::ffi::OsString;
use std::net::TcpListener;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// Resolve a user-facing path string from the project config against the
/// invocation cwd of the controller. Absolute paths are returned unchanged;
/// relative paths are joined onto `invocation_cwd` and lexically normalized
/// (collapsing `.` / `..`) so that downstream children — which we spawn with
/// a different cwd (`state_dir`) — see the path the user expected.
pub(crate) fn resolve_invocation_relative_path(value: &str, invocation_cwd: &Path) -> String {
    let path = Path::new(value);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        invocation_cwd.join(path)
    };
    normalize_lexical(&joined).to_string_lossy().into_owned()
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(component.as_os_str());
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

fn reserve_loopback_port() -> Result<u16, Box<dyn Error>> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

pub(crate) fn build_collect_specs(
    config: &ProjectConfig,
    workspace_root: &Path,
    share_root: &Path,
    child_working_dir: &Path,
    current_exe_dir: &Path,
    invocation_cwd: &Path,
) -> Result<Vec<ChildSpec>, Box<dyn Error>> {
    let mut specs =
        build_preview_specs(config, workspace_root, child_working_dir, current_exe_dir)?;

    // The control server hosts the long-lived control plane WebSocket and
    // forwards episode commands / status / backpressure via iceoryx2. The
    // visualizer is now read-only.
    let control_port = reserve_loopback_port()?;
    specs.push(build_control_server_spec(
        ControlServerRole::Collect,
        control_port,
        workspace_root,
        child_working_dir,
        current_exe_dir,
    )?);

    // Encoders are now spawned by `build_preview_specs` (so the preview
    // tap is also alive in setup/identifying mode). Adding them again
    // here would double-spawn each encoder process and break iceoryx2
    // service negotiation.
    let embedded_config_toml = toml::to_string(config)?;
    let assembler_config = config.assembler_runtime_config_v2(embedded_config_toml);
    let mut assembler_spec = build_assembler_spec(
        &assembler_config,
        workspace_root,
        child_working_dir,
        current_exe_dir,
    )?;
    forward_runtime_env(&mut assembler_spec, config);
    specs.push(assembler_spec);

    let storage_config = config.storage_runtime_config();
    specs.push(build_storage_spec(
        &storage_config,
        config.episode.format,
        workspace_root,
        child_working_dir,
        current_exe_dir,
        invocation_cwd,
    )?);

    let mut ui_runtime_config = config.ui_runtime_config();
    if ui_runtime_config.control_websocket_url.is_none() {
        ui_runtime_config.control_websocket_url = Some(format!("ws://127.0.0.1:{control_port}"));
    }
    let web_bundle_dir = share_root.join("ui/web/dist");
    let web_index = web_bundle_dir.join("index.html");
    if !web_index.exists() {
        return Err(format!(
            "Web UI bundle not found at {}. Run `cd ui/web && npm run build` first, \
             or set ROLLIO_SHARE_DIR.",
            web_index.display()
        )
        .into());
    }

    ui_runtime_config
        .preview_websocket_url
        .as_ref()
        .ok_or("ui runtime config did not produce an upstream preview websocket url")?;
    ui_runtime_config
        .control_websocket_url
        .as_ref()
        .ok_or("ui runtime config did not produce an upstream control websocket url")?;
    eprintln!(
        "rollio: web ui available at {}",
        ui_browser_url(&ui_runtime_config.http_host, ui_runtime_config.http_port)
    );
    specs.push(ChildSpec {
        id: "ui".into(),
        command: ResolvedCommand {
            program: resolve_program(
                current_exe_dir.join("rollio-web-gateway"),
                "rollio-web-gateway",
            ),
            args: vec![
                OsString::from("--config-inline"),
                OsString::from(toml::to_string(&ui_runtime_config)?),
                OsString::from("--asset-dir"),
                web_bundle_dir.into_os_string(),
            ],
        },
        working_directory: child_working_dir.to_path_buf(),
        inherit_stdio: false,
        env: Vec::new(),
    });

    Ok(specs)
}

pub(crate) fn build_preview_specs(
    config: &ProjectConfig,
    workspace_root: &Path,
    child_working_dir: &Path,
    current_exe_dir: &Path,
) -> Result<Vec<ChildSpec>, Box<dyn Error>> {
    let mut specs = Vec::new();

    specs.push(build_visualizer_spec(
        config,
        workspace_root,
        child_working_dir,
        current_exe_dir,
    )?);

    for device in &config.devices {
        specs.push(build_device_spec(
            device,
            workspace_root,
            child_working_dir,
            current_exe_dir,
        )?);
    }

    // The visualizer subscribes to each camera's preview tap, which is
    // published by the encoder process — not the camera driver itself.
    // The encoder is a no-op outside an active recording (no codec work),
    // but it still produces the always-on RGB24 preview tap. Spawning it
    // here means setup/identifying mode also gets live camera previews.
    for encoder_config in config.encoder_runtime_configs_v2() {
        specs.push(build_encoder_spec(
            &encoder_config,
            workspace_root,
            child_working_dir,
            current_exe_dir,
        )?);
    }

    if config.mode == CollectionMode::Teleop {
        for teleop_config in config.teleop_runtime_configs_v2() {
            specs.push(build_teleop_spec(
                &teleop_config,
                workspace_root,
                child_working_dir,
                current_exe_dir,
            )?);
        }
    }

    for spec in &mut specs {
        forward_runtime_env(spec, config);
    }

    Ok(specs)
}

/// Forward `[runtime]` flags to a child as env vars so the pipeline
/// processes don't need to re-parse the project TOML. Currently just
/// `advanced_pipeline_logs`; expand as more global toggles land.
fn forward_runtime_env(spec: &mut ChildSpec, config: &ProjectConfig) {
    if config.runtime.advanced_pipeline_logs {
        spec.env.push((
            OsString::from(rollio_types::config::RuntimeConfig::ENV_ADVANCED_PIPELINE_LOGS),
            OsString::from("1"),
        ));
    }
}

pub(crate) fn build_visualizer_spec(
    config: &ProjectConfig,
    _workspace_root: &Path,
    child_working_dir: &Path,
    current_exe_dir: &Path,
) -> Result<ChildSpec, Box<dyn Error>> {
    let visualizer_config = toml::to_string(&config.visualizer_runtime_config_v2())?;
    Ok(ChildSpec {
        id: "visualizer".into(),
        command: ResolvedCommand {
            program: resolve_program(
                current_exe_dir.join("rollio-visualizer"),
                "rollio-visualizer",
            ),
            args: vec![
                OsString::from("--config-inline"),
                OsString::from(visualizer_config),
            ],
        },
        working_directory: child_working_dir.to_path_buf(),
        inherit_stdio: false,
        env: Vec::new(),
    })
}

/// Role that the control server plays in the lifecycle.
///
/// The control server forwards UI commands and state for either the setup
/// wizard or the collect session. Both roles share the same binary, but
/// subscribe to/publish on different iceoryx2 services.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ControlServerRole {
    Setup,
    Collect,
}

impl ControlServerRole {
    fn as_toml_value(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Collect => "collect",
        }
    }
}

pub(crate) fn build_control_server_spec(
    role: ControlServerRole,
    port: u16,
    _workspace_root: &Path,
    child_working_dir: &Path,
    current_exe_dir: &Path,
) -> Result<ChildSpec, Box<dyn Error>> {
    let inline_config = format!("port = {port}\nrole = \"{}\"\n", role.as_toml_value());
    Ok(ChildSpec {
        id: "control-server".into(),
        command: ResolvedCommand {
            program: resolve_program(
                current_exe_dir.join("rollio-control-server"),
                "rollio-control-server",
            ),
            args: vec![
                OsString::from("--config-inline"),
                OsString::from(inline_config),
            ],
        },
        working_directory: child_working_dir.to_path_buf(),
        inherit_stdio: false,
        env: Vec::new(),
    })
}

pub(crate) fn build_device_spec(
    device: &BinaryDeviceConfig,
    workspace_root: &Path,
    child_working_dir: &Path,
    current_exe_dir: &Path,
) -> Result<ChildSpec, Box<dyn Error>> {
    let inline_config = toml::to_string(device)?;
    let executable_name = device
        .executable
        .clone()
        .unwrap_or_else(|| default_device_executable_name(&device.driver));
    let program = resolve_registered_program(&executable_name, workspace_root, current_exe_dir);
    let common_args = vec![
        OsString::from("run"),
        OsString::from("--config-inline"),
        OsString::from(inline_config),
    ];

    Ok(ChildSpec {
        id: format!("device-{}", device.name),
        command: ResolvedCommand {
            program,
            args: common_args,
        },
        working_directory: child_working_dir.to_path_buf(),
        inherit_stdio: false,
        env: Vec::new(),
    })
}

pub(crate) fn build_teleop_spec(
    config: &TeleopRuntimeConfigV2,
    _workspace_root: &Path,
    child_working_dir: &Path,
    current_exe_dir: &Path,
) -> Result<ChildSpec, Box<dyn Error>> {
    let inline_config = toml::to_string(config)?;

    Ok(ChildSpec {
        id: format!("teleop-{}", config.process_id),
        command: ResolvedCommand {
            program: resolve_program(
                current_exe_dir.join("rollio-teleop-router"),
                "rollio-teleop-router",
            ),
            args: vec![
                OsString::from("run"),
                OsString::from("--config-inline"),
                OsString::from(inline_config),
            ],
        },
        working_directory: child_working_dir.to_path_buf(),
        inherit_stdio: false,
        env: Vec::new(),
    })
}

pub(crate) fn build_encoder_spec(
    config: &EncoderRuntimeConfigV2,
    _workspace_root: &Path,
    child_working_dir: &Path,
    current_exe_dir: &Path,
) -> Result<ChildSpec, Box<dyn Error>> {
    let inline_config = toml::to_string(config)?;
    let binary = encoder_binary_for(config, current_exe_dir);
    Ok(ChildSpec {
        id: format!(
            "encoder-{}-{}",
            config.role.as_str(),
            config.channel_id.replace('/', "-")
        ),
        command: ResolvedCommand {
            program: resolve_program(current_exe_dir.join(binary), binary),
            args: vec![
                OsString::from("run"),
                OsString::from("--config-inline"),
                OsString::from(inline_config),
            ],
        },
        working_directory: child_working_dir.to_path_buf(),
        inherit_stdio: false,
        env: Vec::new(),
    })
}

/// Select the encoder binary based on the configured backend.
/// `HorizonX5` routes to the dedicated `rollio-encoder-x5` binary;
/// `Auto` checks whether the X5 binary is installed (it has higher
/// priority than the software backends); all other backends use the
/// default `rollio-encoder`.
fn encoder_binary_for(config: &EncoderRuntimeConfigV2, exe_dir: &Path) -> &'static str {
    use rollio_types::config::EncoderBackend;
    let backend = config
        .recording
        .as_ref()
        .map(|r| r.backend)
        .or_else(|| config.preview.as_ref().map(|p| p.backend))
        .unwrap_or_default();
    match backend {
        EncoderBackend::HorizonX5 => "rollio-encoder-x5",
        EncoderBackend::Auto => {
            let x5_path = exe_dir.join("rollio-encoder-x5");
            if x5_path.exists() && probe_encoder_has_backend(&x5_path, EncoderBackend::HorizonX5) {
                "rollio-encoder-x5"
            } else {
                "rollio-encoder"
            }
        }
        _ => "rollio-encoder",
    }
}

/// Run `<binary> probe --json` and check whether the given backend is
/// reported as available. Returns false on any failure (missing binary,
/// timeout, parse error, or backend not available).
fn probe_encoder_has_backend(binary: &Path, target: rollio_types::config::EncoderBackend) -> bool {
    use rollio_types::config::EncoderCapabilityReport;

    let output = match Command::new(binary)
        .args(["probe", "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => {
            // Give the probe 5 seconds — it should be near-instant (just a
            // dlopen check + codec enumeration).
            match child.wait_with_output() {
                Ok(o) => o,
                Err(_) => return false,
            }
        }
        Err(_) => return false,
    };

    if !output.status.success() {
        return false;
    }

    let report: EncoderCapabilityReport = match serde_json::from_slice(&output.stdout) {
        Ok(r) => r,
        Err(_) => return false,
    };

    report
        .codecs
        .iter()
        .any(|cap| cap.backend == target && cap.available)
}

pub(crate) fn build_assembler_spec(
    config: &AssemblerRuntimeConfigV2,
    _workspace_root: &Path,
    child_working_dir: &Path,
    current_exe_dir: &Path,
) -> Result<ChildSpec, Box<dyn Error>> {
    let inline_config = toml::to_string(config)?;
    let binary = assembler_binary_for(config.format)?;
    Ok(ChildSpec {
        id: "assembler".into(),
        command: ResolvedCommand {
            program: resolve_program(current_exe_dir.join(binary), binary),
            args: vec![
                OsString::from("run"),
                OsString::from("--config-inline"),
                OsString::from(inline_config),
            ],
        },
        working_directory: child_working_dir.to_path_buf(),
        inherit_stdio: false,
        env: Vec::new(),
    })
}

/// Pick the assembler binary for the project's chosen episode
/// format. Returns an error for formats with no implementation
/// (`LeRobotV3_0`).
pub(crate) fn assembler_binary_for(
    format: rollio_types::config::EpisodeFormat,
) -> Result<&'static str, Box<dyn Error>> {
    use rollio_types::config::EpisodeFormat;
    match format {
        EpisodeFormat::LeRobotV2_1 => Ok("rollio-episode-lerobot"),
        EpisodeFormat::Mcap => Ok("rollio-episode-mcap"),
        EpisodeFormat::LeRobotV3_0 => {
            Err("episode.format = lerobot-v3.0 is not implemented yet".into())
        }
    }
}

pub(crate) fn build_storage_spec(
    config: &StorageRuntimeConfig,
    format: rollio_types::config::EpisodeFormat,
    _workspace_root: &Path,
    child_working_dir: &Path,
    current_exe_dir: &Path,
    invocation_cwd: &Path,
) -> Result<ChildSpec, Box<dyn Error>> {
    let mut config = config.clone();
    if let Some(output_path) = config.output_path.as_ref() {
        config.output_path = Some(resolve_invocation_relative_path(
            output_path,
            invocation_cwd,
        ));
    }
    let binary = storage_binary_for(format, config.backend)?;
    let inline_config = toml::to_string(&config)?;
    Ok(ChildSpec {
        id: "storage".into(),
        command: ResolvedCommand {
            program: resolve_program(current_exe_dir.join(binary), binary),
            args: vec![
                OsString::from("run"),
                OsString::from("--config-inline"),
                OsString::from(inline_config),
            ],
        },
        working_directory: child_working_dir.to_path_buf(),
        inherit_stdio: false,
        env: Vec::new(),
    })
}

/// Pick the storage binary for the project's `(format, backend)` pair.
/// User-facing TOML only carries `backend`; the binary distinction
/// between the LeRobot data/tb5z035i/workspaceset-merger and the generic per-episode mover is
/// an internal concern of the controller.
pub(crate) fn storage_binary_for(
    format: rollio_types::config::EpisodeFormat,
    backend: rollio_types::config::StorageBackend,
) -> Result<&'static str, Box<dyn Error>> {
    use rollio_types::config::{EpisodeFormat, StorageBackend};
    match (format, backend) {
        (EpisodeFormat::LeRobotV2_1 | EpisodeFormat::LeRobotV3_0, StorageBackend::Local) => {
            Ok("rollio-storage-local-lerobot")
        }
        (EpisodeFormat::Mcap, StorageBackend::Local) => Ok("rollio-storage-local"),
        (_, StorageBackend::Http) => Err("storage.backend = http is not implemented yet".into()),
    }
}

pub(crate) fn ui_browser_url(host: &str, port: u16) -> String {
    let display_host = match host {
        "0.0.0.0" | "::" => "127.0.0.1",
        _ => host,
    };
    format!("http://{display_host}:{port}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollio_types::config::{EpisodeFormat, StorageBackend, StorageRuntimeConfig};

    #[test]
    fn resolve_invocation_relative_path_keeps_absolute_paths() {
        let resolved = resolve_invocation_relative_path("/tmp/output", Path::new("/home/user"));
        assert_eq!(resolved, "/tmp/output");
    }

    #[test]
    fn resolve_invocation_relative_path_joins_relative_against_cwd_and_normalizes() {
        let resolved = resolve_invocation_relative_path("./output", Path::new("/home/user/proj"));
        assert_eq!(resolved, "/home/user/proj/output");

        let nested =
            resolve_invocation_relative_path("data/runs/./latest", Path::new("/srv/rollio"));
        assert_eq!(nested, "/srv/rollio/data/runs/latest");

        let parent = resolve_invocation_relative_path("../shared", Path::new("/srv/rollio/run"));
        assert_eq!(parent, "/srv/rollio/shared");
    }

    #[test]
    fn build_storage_spec_resolves_relative_output_path_against_invocation_cwd() {
        let storage_config = StorageRuntimeConfig {
            process_id: "storage-local-lerobot".into(),
            backend: StorageBackend::Local,
            output_path: Some("./output".into()),
            endpoint: None,
            queue_size: 4,
        };
        let invocation_cwd = Path::new("/home/operator/session-2026-04-21");
        let spec = build_storage_spec(
            &storage_config,
            EpisodeFormat::LeRobotV2_1,
            Path::new("."),
            Path::new("/var/lib/rollio/state"),
            Path::new("."),
            invocation_cwd,
        )
        .expect("storage spec should build");

        let inline = spec.command.args[2].to_string_lossy();
        assert!(
            inline.contains("output_path = \"/home/operator/session-2026-04-21/output\""),
            "expected absolute output_path in inline config, got: {inline}"
        );
        assert!(
            !inline.contains("output_path = \"./output\""),
            "relative output_path should be replaced, got: {inline}"
        );
    }

    #[test]
    fn build_storage_spec_preserves_absolute_output_path() {
        let storage_config = StorageRuntimeConfig {
            process_id: "storage-local-lerobot".into(),
            backend: StorageBackend::Local,
            output_path: Some("/data/rollio/output".into()),
            endpoint: None,
            queue_size: 4,
        };
        let spec = build_storage_spec(
            &storage_config,
            EpisodeFormat::LeRobotV2_1,
            Path::new("."),
            Path::new("/var/lib/rollio/state"),
            Path::new("."),
            Path::new("/home/operator/anything"),
        )
        .expect("storage spec should build");

        let inline = spec.command.args[2].to_string_lossy();
        assert!(
            inline.contains("output_path = \"/data/rollio/output\""),
            "absolute output_path should be preserved, got: {inline}"
        );
    }

    #[test]
    fn build_storage_spec_picks_lerobot_binary_for_lerobot_format() {
        let storage_config = StorageRuntimeConfig {
            process_id: "storage-local-lerobot".into(),
            backend: StorageBackend::Local,
            output_path: Some("/tmp/out".into()),
            endpoint: None,
            queue_size: 4,
        };
        let spec = build_storage_spec(
            &storage_config,
            EpisodeFormat::LeRobotV2_1,
            Path::new("."),
            Path::new("/var/lib/rollio/state"),
            Path::new("."),
            Path::new("/home/operator"),
        )
        .expect("storage spec should build");
        let program = spec.command.program.to_string_lossy();
        assert!(
            program.contains("rollio-storage-local-lerobot"),
            "expected lerobot-specific binary, got: {program}"
        );
    }

    #[test]
    fn build_storage_spec_picks_generic_binary_for_mcap_format() {
        let storage_config = StorageRuntimeConfig {
            process_id: "storage-local".into(),
            backend: StorageBackend::Local,
            output_path: Some("/tmp/out".into()),
            endpoint: None,
            queue_size: 4,
        };
        let spec = build_storage_spec(
            &storage_config,
            EpisodeFormat::Mcap,
            Path::new("."),
            Path::new("/var/lib/rollio/state"),
            Path::new("."),
            Path::new("/home/operator"),
        )
        .expect("storage spec should build");
        let program = spec.command.program.to_string_lossy();
        assert!(
            program.ends_with("rollio-storage-local"),
            "expected generic mover binary, got: {program}",
        );
        assert!(
            !program.contains("rollio-storage-local-lerobot"),
            "MCAP must not spawn the lerobot-specific binary, got: {program}"
        );
    }

    #[test]
    fn build_storage_spec_rejects_http_backend() {
        let storage_config = StorageRuntimeConfig {
            process_id: "storage-http".into(),
            backend: StorageBackend::Http,
            output_path: None,
            endpoint: Some("https://example.com/upload".into()),
            queue_size: 4,
        };
        let err = build_storage_spec(
            &storage_config,
            EpisodeFormat::Mcap,
            Path::new("."),
            Path::new("/var/lib/rollio/state"),
            Path::new("."),
            Path::new("/home/operator"),
        )
        .expect_err("http backend should be rejected with a clear message");
        assert!(format!("{err}").contains("http"));
    }
}
