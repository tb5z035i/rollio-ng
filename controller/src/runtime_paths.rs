use rollio_types::config::DeviceType;
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

pub fn driver_dir_name(driver: &str) -> String {
    driver.replace('-', "_")
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

pub fn device_executable_name(device_type: DeviceType, driver: &str) -> String {
    let driver_name = driver.replace('_', "-");
    match device_type {
        DeviceType::Camera => format!("rollio-camera-{driver_name}"),
        DeviceType::Robot => format!("rollio-robot-{driver_name}"),
    }
}

pub fn resolve_device_program(
    device_type: DeviceType,
    driver: &str,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> OsString {
    let executable_name = device_executable_name(device_type, driver);
    let local_binary = match device_type {
        DeviceType::Camera => resolve_existing_path([
            workspace_root
                .join("cameras/build")
                .join(driver_dir_name(driver))
                .join(&executable_name),
            current_exe_dir.join(&executable_name),
            workspace_root.join("target/debug").join(&executable_name),
        ]),
        DeviceType::Robot => resolve_existing_path([
            current_exe_dir.join(&executable_name),
            workspace_root.join("target/debug").join(&executable_name),
        ]),
    };

    local_binary
        .map(PathBuf::into_os_string)
        .unwrap_or_else(|| OsString::from(executable_name))
}
