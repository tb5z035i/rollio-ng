use crate::runtime_paths::resolve_registered_program;
use serde_json::Value;
use std::collections::BTreeSet;
use std::error::Error;
use std::ffi::OsString;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Always-probed device executables. Each entry is the basename of an
/// executable expected to live either in the workspace's `target/debug` (for
/// in-tree drivers built via `cargo`) or somewhere on `$PATH`. Adding a new
/// in-tree driver only requires adding its executable name here. Third-party
/// drivers don't need to register: they're picked up automatically by the
/// PATH scan in `enumerate_path_device_executables`.
///
/// `rollio-device-pseudo` is intentionally absent so installing the binary
/// system-wide doesn't silently inject simulated devices into every
/// `rollio setup` run; pseudo is opt-in via the `--sim-pseudo` CLI flag.
pub(crate) fn known_device_executables() -> &'static [&'static str] {
    &[
        "rollio-device-airbot-play",
        "rollio-device-realsense",
        "rollio-device-v4l2",
        "rollio-device-agx-nero",
        // UMI bridge: subscribes to cora's FastDDS topics and republishes
        // onto rollio's iceoryx2 bus. Always probed (probe is fast and
        // requires no DDS contact); when no operator config is provided
        // `query --json` returns an empty channel list and the controller
        // skips it during setup-wizard rendering.
        "rollio-device-umi",
    ]
}

/// Discovery options that aren't expressible as `rollio-device-*` registry
/// entries. Currently just controls how many synthetic pseudo devices to
/// inject (the `pseudo` driver is excluded from the always-on registry and
/// PATH scan; this is the only path that surfaces it).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DiscoveryOptions {
    pub(crate) simulated_pseudo: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ProbeEntry {
    /// Executable basename (e.g. `"rollio-device-airbot-play"`). Used for
    /// error messages and to route subsequent `query --json` invocations
    /// back to the same driver process.
    pub(crate) executable: String,
    /// Resolved program path, preferring `target/debug` over `$PATH`.
    pub(crate) program: OsString,
    /// One element from the driver's `probe --json` output array.
    pub(crate) probe_entry: Value,
}

#[derive(Debug)]
pub(crate) enum DriverCommandError {
    NotFound {
        program: String,
    },
    Io {
        program: String,
        source: std::io::Error,
    },
    Timeout {
        program: String,
        args: String,
    },
    Failed {
        program: String,
        args: String,
        details: String,
    },
    InvalidJson {
        program: String,
        source: serde_json::Error,
        stdout: String,
    },
}

impl std::fmt::Display for DriverCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { program } => write!(f, "driver executable not found: {program}"),
            Self::Io { program, source } => write!(f, "failed to run {program}: {source}"),
            Self::Timeout { program, args } => {
                write!(f, "driver command timed out: {program} {args}")
            }
            Self::Failed {
                program,
                args,
                details,
            } => write!(f, "driver command failed: {program} {args}: {details}"),
            Self::InvalidJson {
                program,
                source,
                stdout,
            } => write!(
                f,
                "driver command returned invalid JSON: {program}: {source}; stdout={stdout}"
            ),
        }
    }
}

impl Error for DriverCommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::InvalidJson { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Build the set of executables to probe by combining the explicit registry
/// with anything on `$PATH` matching `rollio-device-*`. The two lists are
/// deduped by basename; registry entries take priority because the local
/// `target/debug` build is preferred over PATH lookups via
/// `resolve_registered_program`. `rollio-device-pseudo` is excluded from
/// the PATH scan to keep it opt-in.
fn collect_device_executables(simulated_pseudo: usize) -> Vec<String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<String> = Vec::new();
    for executable in known_device_executables() {
        let name = (*executable).to_owned();
        if seen.insert(name.clone()) {
            out.push(name);
        }
    }
    for executable in enumerate_path_device_executables() {
        if seen.insert(executable.clone()) {
            out.push(executable);
        }
    }
    if simulated_pseudo > 0 {
        let name = "rollio-device-pseudo".to_owned();
        if seen.insert(name.clone()) {
            out.push(name);
        }
    }
    out
}

/// Scan every directory on `$PATH` for executables whose filename starts with
/// `rollio-device-`. Returns the deduplicated basenames in first-PATH-entry
/// order. `rollio-device-pseudo` is filtered out so installing the pseudo
/// driver system-wide doesn't auto-inject simulated devices into every
/// `rollio setup` run.
pub(crate) fn enumerate_path_device_executables() -> Vec<String> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<String> = Vec::new();
    for dir in std::env::split_paths(&path) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name_str) = name.to_str() else {
                continue;
            };
            if !name_str.starts_with("rollio-device-") {
                continue;
            }
            if name_str == "rollio-device-pseudo" {
                continue;
            }
            if !is_executable_entry(&entry) {
                continue;
            }
            if seen.insert(name_str.to_owned()) {
                out.push(name_str.to_owned());
            }
        }
    }
    out
}

#[cfg(unix)]
fn is_executable_entry(entry: &std::fs::DirEntry) -> bool {
    use std::os::unix::fs::PermissionsExt;
    entry
        .metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_entry(entry: &std::fs::DirEntry) -> bool {
    let path = entry.path();
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    matches!(extension.as_deref(), Some("exe" | "cmd" | "bat" | "com"))
}

pub(crate) fn discover_probe_entries(
    workspace_root: &Path,
    process_working_dir: &Path,
    current_exe_dir: &Path,
    options: DiscoveryOptions,
    discovery_timeout: Duration,
) -> Result<(Vec<ProbeEntry>, Vec<String>), Box<dyn Error>> {
    let mut entries = Vec::new();
    let mut probe_errors = Vec::new();

    for executable in collect_device_executables(options.simulated_pseudo) {
        let extra_args = if executable == "rollio-device-pseudo" && options.simulated_pseudo > 0 {
            vec![
                OsString::from("--count"),
                OsString::from(options.simulated_pseudo.to_string()),
            ]
        } else {
            Vec::new()
        };
        extend_probe_entries(
            executable,
            &extra_args,
            workspace_root,
            process_working_dir,
            current_exe_dir,
            discovery_timeout,
            &mut entries,
            &mut probe_errors,
        );
    }

    if entries.is_empty() && !probe_errors.is_empty() {
        return Err(probe_errors.join("; ").into());
    }

    Ok((entries, probe_errors))
}

pub(crate) fn run_driver_json(
    program: &OsString,
    args: &[OsString],
    working_directory: &Path,
    timeout: Duration,
) -> Result<Value, DriverCommandError> {
    let program_name = os_string_lossy(program);
    let args_display = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    let mut child = Command::new(program)
        .args(args)
        .current_dir(working_directory)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                DriverCommandError::NotFound {
                    program: program_name.clone(),
                }
            } else {
                DriverCommandError::Io {
                    program: program_name.clone(),
                    source,
                }
            }
        })?;

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(DriverCommandError::Timeout {
                        program: program_name,
                        args: args_display,
                    });
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(source) => {
                return Err(DriverCommandError::Io {
                    program: program_name,
                    source,
                });
            }
        }
    };

    let stdout = read_child_pipe(child.stdout.take()).map_err(|source| DriverCommandError::Io {
        program: program_name.clone(),
        source,
    })?;
    let stderr = read_child_pipe(child.stderr.take()).map_err(|source| DriverCommandError::Io {
        program: program_name.clone(),
        source,
    })?;

    if !status.success() {
        let details = if stderr.trim().is_empty() {
            stdout.trim().to_owned()
        } else {
            stderr.trim().to_owned()
        };
        return Err(DriverCommandError::Failed {
            program: program_name,
            args: args_display,
            details,
        });
    }

    serde_json::from_str(stdout.trim()).map_err(|source| DriverCommandError::InvalidJson {
        program: program_name,
        source,
        stdout,
    })
}

#[allow(clippy::too_many_arguments)]
fn extend_probe_entries(
    executable: String,
    extra_probe_args: &[OsString],
    workspace_root: &Path,
    process_working_dir: &Path,
    current_exe_dir: &Path,
    discovery_timeout: Duration,
    entries: &mut Vec<ProbeEntry>,
    probe_errors: &mut Vec<String>,
) {
    let program = resolve_registered_program(&executable, workspace_root, current_exe_dir);
    let mut probe_args = vec![OsString::from("probe"), OsString::from("--json")];
    probe_args.extend(extra_probe_args.iter().cloned());

    let probe_output = match run_driver_json(
        &program,
        &probe_args,
        process_working_dir,
        discovery_timeout,
    ) {
        Ok(value) => value,
        Err(DriverCommandError::NotFound { .. }) => return,
        Err(error) => {
            probe_errors.push(format!("{}: {error}", executable));
            return;
        }
    };

    let Some(probe_entries) = probe_output.as_array() else {
        probe_errors.push(format!(
            "{}: probe output must be a JSON array, got {}",
            executable, probe_output
        ));
        return;
    };

    for probe_entry in probe_entries {
        entries.push(ProbeEntry {
            executable: executable.clone(),
            program: program.clone(),
            probe_entry: probe_entry.clone(),
        });
    }
}

fn read_child_pipe(mut pipe: Option<impl Read>) -> Result<String, std::io::Error> {
    let mut output = String::new();
    if let Some(pipe) = pipe.as_mut() {
        pipe.read_to_string(&mut output)?;
    }
    Ok(output)
}

fn os_string_lossy(value: &OsString) -> String {
    value.to_string_lossy().into_owned()
}
