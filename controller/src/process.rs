use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
#[cfg(test)]
use std::sync::atomic::AtomicBool;
#[cfg(test)]
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ResolvedCommand {
    pub program: OsString,
    pub args: Vec<OsString>,
}

#[derive(Debug, Clone)]
pub struct ChildSpec {
    pub id: String,
    pub command: ResolvedCommand,
    pub working_directory: PathBuf,
    pub inherit_stdio: bool,
}

#[derive(Debug)]
pub struct ManagedChild {
    pub id: String,
    pub child: Child,
    pub log_path: Option<PathBuf>,
}

#[derive(Debug)]
pub enum ShutdownTrigger {
    Signal,
    ChildExited { id: String, status: ExitStatus },
}

pub fn spawn_child(spec: &ChildSpec, log_dir: &Path) -> io::Result<ManagedChild> {
    fs::create_dir_all(log_dir)?;

    let mut command = Command::new(&spec.command.program);
    command.args(&spec.command.args);
    command.current_dir(&spec.working_directory);

    let log_path = if spec.inherit_stdio {
        None
    } else {
        let log_path = log_dir.join(format!("{}.log", sanitize_identifier(&spec.id)));
        let stdout = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&log_path)?;
        let stderr = stdout.try_clone()?;

        command.stdin(Stdio::null());
        command.stdout(Stdio::from(stdout));
        command.stderr(Stdio::from(stderr));
        Some(log_path)
    };

    let child = command.spawn()?;
    Ok(ManagedChild {
        id: spec.id.clone(),
        child,
        log_path,
    })
}

#[cfg(test)]
pub fn monitor_children(
    children: &mut [ManagedChild],
    shutdown_requested: &AtomicBool,
    poll_interval: Duration,
) -> io::Result<ShutdownTrigger> {
    loop {
        if shutdown_requested.load(Ordering::Relaxed) {
            return Ok(ShutdownTrigger::Signal);
        }

        if let Some(trigger) = poll_children_once(children)? {
            return Ok(trigger);
        }

        thread::sleep(poll_interval);
    }
}

pub fn poll_children_once(children: &mut [ManagedChild]) -> io::Result<Option<ShutdownTrigger>> {
    for child in children.iter_mut() {
        if let Some(status) = child.child.try_wait()? {
            return Ok(Some(ShutdownTrigger::ChildExited {
                id: child.id.clone(),
                status,
            }));
        }
    }
    Ok(None)
}

pub fn terminate_children(
    children: &mut [ManagedChild],
    timeout: Duration,
    poll_interval: Duration,
) -> io::Result<()> {
    request_graceful_shutdown(children)?;
    let deadline = Instant::now() + timeout;

    loop {
        let mut remaining_children = 0usize;
        for child in children.iter_mut() {
            if child.child.try_wait()?.is_none() {
                remaining_children += 1;
            }
        }

        if remaining_children == 0 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            break;
        }

        thread::sleep(poll_interval);
    }

    for child in children.iter_mut() {
        if child.child.try_wait()?.is_none() {
            let _ = child.child.kill();
            let _ = child.child.wait();
        }
    }

    Ok(())
}

fn request_graceful_shutdown(children: &mut [ManagedChild]) -> io::Result<()> {
    for child in children.iter_mut() {
        if child.child.try_wait()?.is_none() {
            send_terminate_signal(child.child.id());
        }
    }

    Ok(())
}

#[cfg(unix)]
fn send_terminate_signal(pid: u32) {
    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }
}

#[cfg(not(unix))]
fn send_terminate_signal(_pid: u32) {}

fn sanitize_identifier(input: &str) -> String {
    input
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '_',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn monitor_children_detects_child_exit() {
        let log_dir = unique_test_log_dir();
        let spec = ChildSpec {
            id: "crasher".into(),
            command: shell_command("exit 42"),
            working_directory: PathBuf::from("."),
            inherit_stdio: false,
        };
        let mut child = spawn_child(&spec, &log_dir).expect("child should spawn");
        let shutdown_requested = AtomicBool::new(false);

        let trigger = monitor_children(
            std::slice::from_mut(&mut child),
            &shutdown_requested,
            Duration::from_millis(10),
        )
        .expect("monitoring should succeed");

        match trigger {
            ShutdownTrigger::ChildExited { id, status } => {
                assert_eq!(id, "crasher");
                assert_eq!(status.code(), Some(42));
            }
            ShutdownTrigger::Signal => panic!("expected child exit trigger"),
        }
    }

    #[test]
    fn terminate_children_kills_lingering_processes() {
        let log_dir = unique_test_log_dir();
        let spec = ChildSpec {
            id: "sleeper".into(),
            command: shell_command("sleep 30"),
            working_directory: PathBuf::from("."),
            inherit_stdio: false,
        };
        let mut child = spawn_child(&spec, &log_dir).expect("child should spawn");

        terminate_children(
            std::slice::from_mut(&mut child),
            Duration::from_millis(50),
            Duration::from_millis(10),
        )
        .expect("termination should succeed");

        assert!(
            child
                .child
                .try_wait()
                .expect("wait should succeed")
                .is_some(),
            "child should have been terminated"
        );
    }

    #[test]
    fn terminate_children_tries_sigterm_before_sigkill() {
        let log_dir = unique_test_log_dir();
        let spec = ChildSpec {
            id: "term_handler".into(),
            command: shell_command("trap 'exit 0' TERM; while :; do sleep 1; done"),
            working_directory: PathBuf::from("."),
            inherit_stdio: false,
        };
        let mut child = spawn_child(&spec, &log_dir).expect("child should spawn");

        terminate_children(
            std::slice::from_mut(&mut child),
            Duration::from_millis(250),
            Duration::from_millis(10),
        )
        .expect("termination should succeed");

        let status = child
            .child
            .try_wait()
            .expect("wait should succeed")
            .expect("child should have exited");
        #[cfg(unix)]
        assert!(
            status.code() == Some(0) || status.signal() == Some(libc::SIGTERM),
            "expected SIGTERM-based shutdown, got {status:?}"
        );
        #[cfg(not(unix))]
        assert!(
            status.success(),
            "expected graceful shutdown, got {status:?}"
        );
    }

    #[test]
    fn monitor_children_returns_on_signal_request() {
        let log_dir = unique_test_log_dir();
        let spec = ChildSpec {
            id: "signal_wait".into(),
            command: shell_command("sleep 30"),
            working_directory: PathBuf::from("."),
            inherit_stdio: false,
        };
        let mut child = spawn_child(&spec, &log_dir).expect("child should spawn");
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let shutdown_requested_clone = Arc::clone(&shutdown_requested);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            shutdown_requested_clone.store(true, Ordering::Relaxed);
        });

        let trigger = monitor_children(
            std::slice::from_mut(&mut child),
            shutdown_requested.as_ref(),
            Duration::from_millis(10),
        )
        .expect("monitoring should succeed");
        assert!(matches!(trigger, ShutdownTrigger::Signal));

        terminate_children(
            std::slice::from_mut(&mut child),
            Duration::from_millis(50),
            Duration::from_millis(10),
        )
        .expect("termination should succeed");
    }

    fn shell_command(script: &str) -> ResolvedCommand {
        ResolvedCommand {
            program: OsString::from("sh"),
            args: vec![OsString::from("-c"), OsString::from(script)],
        }
    }

    fn unique_test_log_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("rollio-controller-tests-{nanos}"))
    }
}
