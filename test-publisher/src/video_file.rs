use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, Stdio};

pub struct LocalVideoFileSource {
    path: PathBuf,
    child: Child,
    stdout: ChildStdout,
}

impl LocalVideoFileSource {
    pub fn new(path: PathBuf, width: u32, height: u32, fps: u32) -> io::Result<Self> {
        if !path.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("camera file does not exist: {}", path.display()),
            ));
        }

        let filter = format!(
            "fps={fps},scale=w={width}:h={height}:force_original_aspect_ratio=decrease:flags=bilinear,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:black"
        );

        let mut command = Command::new("ffmpeg");
        command
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-nostdin")
            .arg("-stream_loop")
            .arg("-1")
            .arg("-i")
            .arg(&path)
            .arg("-an")
            .arg("-sn")
            .arg("-dn")
            .arg("-vf")
            .arg(filter)
            .arg("-pix_fmt")
            .arg("rgb24")
            .arg("-f")
            .arg("rawvideo")
            .arg("pipe:1")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let mut child = command.spawn().map_err(|err| {
            if err.kind() == io::ErrorKind::NotFound {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "ffmpeg not found on PATH; install ffmpeg to use --camera-file",
                )
            } else {
                io::Error::new(
                    err.kind(),
                    format!("failed to start ffmpeg for {}: {err}", path.display()),
                )
            }
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            io::Error::other(format!(
                "failed to capture ffmpeg stdout for {}",
                path.display()
            ))
        })?;

        Ok(Self {
            path,
            child,
            stdout,
        })
    }

    pub fn fill_next_frame(&mut self, dst: &mut [u8]) -> io::Result<()> {
        self.stdout.read_exact(dst).map_err(|err| {
            let status_hint = match self.child.try_wait() {
                Ok(Some(status)) => format!("ffmpeg exited with status {status}"),
                Ok(None) => "ffmpeg is still running".to_string(),
                Err(wait_err) => format!("failed to inspect ffmpeg status: {wait_err}"),
            };

            io::Error::new(
                err.kind(),
                format!(
                    "failed to read decoded frame from {}: {err} ({status_hint})",
                    self.path.display()
                ),
            )
        })
    }
}

impl Drop for LocalVideoFileSource {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}
