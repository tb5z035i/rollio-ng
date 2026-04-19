use std::error::Error;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub fn resolve_program(local_candidate: PathBuf, fallback_name: &str) -> OsString {
    if local_candidate.exists() {
        local_candidate.into_os_string()
    } else {
        OsString::from(fallback_name)
    }
}

pub fn resolve_existing_path(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|candidate| candidate.exists())
}

pub fn workspace_root() -> Result<PathBuf, Box<dyn Error>> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "failed to resolve workspace root".into())
}

pub fn current_executable_dir() -> Result<PathBuf, Box<dyn Error>> {
    let current_executable = std::env::current_exe()?;
    current_executable
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "failed to resolve controller executable directory".into())
}

/// Default executable basename for a driver. Every device executable now
/// follows the unified `rollio-device-{driver}` convention; the previous
/// `rollio-camera-*` / `rollio-robot-*` split is gone (devices may expose
/// mixed camera + robot channels under a single binary). Underscores in
/// the driver string are converted to dashes for shell-friendly names.
pub fn default_device_executable_name(driver: &str) -> String {
    format!("rollio-device-{}", driver.replace('_', "-"))
}

/// Resolve a registered executable name to a concrete program path.
/// Preference order: workspace `target/debug`, the controller's own
/// directory (e.g. an installed Cargo workspace), the C++ camera build
/// directories, then PATH lookup. Returning the bare basename when
/// nothing matches lets `Command::new` fall back to a normal `$PATH`
/// resolution at spawn time.
pub fn resolve_registered_program(
    executable_name: &str,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> OsString {
    let target_debug = workspace_root.join("target/debug").join(executable_name);
    let mut camera_candidates = Vec::new();
    let camera_build_root = workspace_root.join("cameras/build");
    if let Ok(entries) = std::fs::read_dir(&camera_build_root) {
        for entry in entries.flatten() {
            camera_candidates.push(entry.path().join(executable_name));
        }
    }
    let local_binary = resolve_existing_path(
        [current_exe_dir.join(executable_name), target_debug]
            .into_iter()
            .chain(camera_candidates),
    );
    local_binary
        .map(PathBuf::into_os_string)
        .unwrap_or_else(|| OsString::from(executable_name))
}
