use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs;
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

/// Compile-time Cargo workspace root (parent of the `controller` crate).
/// Used to locate `target/{debug,release}`, `cameras/build`, and dev UI trees.
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

fn web_bundle_index(share_root: &Path) -> PathBuf {
    share_root.join("ui/web/dist/index.html")
}

/// Read-only UI and packaged assets: directory that contains `ui/web/dist/`.
///
/// Resolution order:
/// 1. `ROLLIO_SHARE_DIR` if set and `ui/web/dist/index.html` exists under it.
/// 2. Dev checkout: compile-time workspace root if that path contains the bundle.
/// 3. FHS install: `/usr/share/rollio` if the bundle exists there.
pub fn resolve_share_root() -> Result<PathBuf, Box<dyn Error>> {
    if let Ok(dir) = env::var("ROLLIO_SHARE_DIR") {
        let root = PathBuf::from(dir);
        if web_bundle_index(&root).exists() {
            return Ok(root);
        }
        return Err(format!(
            "ROLLIO_SHARE_DIR={} does not contain ui/web/dist/index.html",
            root.display()
        )
        .into());
    }

    let dev_root = workspace_root()?;
    if web_bundle_index(&dev_root).exists() {
        return Ok(dev_root);
    }

    let usr = PathBuf::from("/usr/share/rollio");
    if web_bundle_index(&usr).exists() {
        return Ok(usr);
    }

    Err(format!(
        "Web UI bundle not found. Set ROLLIO_SHARE_DIR to a prefix containing ui/web/dist, \
         or build the UI (cd ui/web && npm run build) under {}, \
         or install packaged assets under {}.",
        dev_root.display(),
        usr.display()
    )
    .into())
}

/// Writable directory for logs and child process current working directory.
///
/// Order:
/// 1. `ROLLIO_STATE_DIR` (created if missing).
/// 2. `$XDG_STATE_HOME/rollio`.
/// 3. `$HOME/.local/state/rollio`.
/// 4. `<workspace>/target/rollio-state` (dev fallback).
pub fn resolve_state_dir() -> Result<PathBuf, Box<dyn Error>> {
    if let Ok(dir) = env::var("ROLLIO_STATE_DIR") {
        let p = PathBuf::from(dir);
        fs::create_dir_all(&p)?;
        return Ok(p);
    }

    if let Ok(xdg) = env::var("XDG_STATE_HOME") {
        let p = Path::new(&xdg).join("rollio");
        fs::create_dir_all(&p)?;
        return Ok(p);
    }

    if let Ok(home) = env::var("HOME") {
        let p = Path::new(&home).join(".local/state/rollio");
        fs::create_dir_all(&p)?;
        return Ok(p);
    }

    let w = workspace_root()?;
    let p = w.join("target/rollio-state");
    fs::create_dir_all(&p)?;
    Ok(p)
}

/// Directory used to capture child-process log files (`device-*.log`,
/// `encoder-*.log`, etc.) for a `rollio collect`/`teleop` run.
///
/// Order:
/// 1. `ROLLIO_LOG_DIR` (created if missing).
/// 2. `<invocation_cwd>/rollio-logs` — keeps logs alongside the user's
///    project / config so they can be inspected without hunting through
///    XDG state directories.
pub fn resolve_log_dir(invocation_cwd: &Path) -> Result<PathBuf, Box<dyn Error>> {
    if let Ok(dir) = env::var("ROLLIO_LOG_DIR") {
        let p = PathBuf::from(dir);
        fs::create_dir_all(&p)?;
        return Ok(p);
    }
    let p = invocation_cwd.join("rollio-logs");
    fs::create_dir_all(&p)?;
    Ok(p)
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
/// Preference order: workspace `target/release`, `target/debug`, the controller's own
/// directory (e.g. an installed Cargo workspace), the C++ camera build
/// directories, then PATH lookup. Returning the bare basename when
/// nothing matches lets `Command::new` fall back to a normal `$PATH`
/// resolution at spawn time.
pub fn resolve_registered_program(
    executable_name: &str,
    workspace_root: &Path,
    current_exe_dir: &Path,
) -> OsString {
    let target_release = workspace_root.join("target/release").join(executable_name);
    let target_debug = workspace_root.join("target/debug").join(executable_name);
    let mut camera_candidates = Vec::new();
    let camera_build_root = workspace_root.join("cameras/build");
    if let Ok(entries) = fs::read_dir(&camera_build_root) {
        for entry in entries.flatten() {
            camera_candidates.push(entry.path().join(executable_name));
        }
    }
    let local_binary = resolve_existing_path(
        [
            current_exe_dir.join(executable_name),
            target_release,
            target_debug,
        ]
        .into_iter()
        .chain(camera_candidates),
    );
    local_binary
        .map(PathBuf::into_os_string)
        .unwrap_or_else(|| OsString::from(executable_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock")
    }

    #[test]
    fn resolve_share_root_honors_rollio_share_dir() {
        let _guard = env_lock();
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        fs::create_dir_all(root.join("ui/web/dist")).expect("mkdir");
        fs::write(root.join("ui/web/dist/index.html"), "<!doctype html>").expect("write");
        std::env::set_var("ROLLIO_SHARE_DIR", root);
        let got = resolve_share_root().expect("share root");
        assert_eq!(got, root);
        std::env::remove_var("ROLLIO_SHARE_DIR");
    }

    #[test]
    fn resolve_share_dir_rejects_missing_bundle() {
        let _guard = env_lock();
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("ROLLIO_SHARE_DIR", tmp.path());
        let err = resolve_share_root().expect_err("expected err");
        assert!(err.to_string().contains("ui/web/dist/index.html"), "{err}");
        std::env::remove_var("ROLLIO_SHARE_DIR");
    }

    #[test]
    fn resolve_state_dir_honors_rollio_state_dir() {
        let _guard = env_lock();
        let tmp = tempfile::tempdir().expect("tempdir");
        let st = tmp.path().join("state-here");
        std::env::set_var("ROLLIO_STATE_DIR", &st);
        let got = resolve_state_dir().expect("state dir");
        assert_eq!(got, st);
        assert!(got.is_dir());
        std::env::remove_var("ROLLIO_STATE_DIR");
    }

    #[test]
    fn resolve_log_dir_defaults_to_invocation_cwd() {
        let _guard = env_lock();
        std::env::remove_var("ROLLIO_LOG_DIR");
        let tmp = tempfile::tempdir().expect("tempdir");
        let got = resolve_log_dir(tmp.path()).expect("log dir");
        assert_eq!(got, tmp.path().join("rollio-logs"));
        assert!(got.is_dir());
    }

    #[test]
    fn resolve_log_dir_honors_rollio_log_dir() {
        let _guard = env_lock();
        let tmp = tempfile::tempdir().expect("tempdir");
        let custom = tmp.path().join("logs-here");
        std::env::set_var("ROLLIO_LOG_DIR", &custom);
        let got = resolve_log_dir(tmp.path()).expect("log dir");
        assert_eq!(got, custom);
        assert!(got.is_dir());
        std::env::remove_var("ROLLIO_LOG_DIR");
    }
}
