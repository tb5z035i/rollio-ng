use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

pub struct FfmpegRgbSource {
    source_label: String,
    child: Child,
    mode: SourceMode,
}

enum SourceMode {
    Sequential {
        stdout: ChildStdout,
    },
    LatestFrame {
        latest_frame: LatestFrameBuffer,
        reader_thread: Option<JoinHandle<()>>,
    },
}

#[derive(Clone)]
struct LatestFrameBuffer {
    frame_len: usize,
    inner: Arc<(Mutex<LatestFrameState>, Condvar)>,
}

#[derive(Default)]
struct LatestFrameState {
    frame: Option<Vec<u8>>,
    error: Option<String>,
    closed: bool,
}

impl LatestFrameBuffer {
    fn new(frame_len: usize) -> Self {
        Self {
            frame_len,
            inner: Arc::new((Mutex::new(LatestFrameState::default()), Condvar::new())),
        }
    }

    fn publish_frame(&self, capture_buf: &mut Vec<u8>) {
        let (lock, condvar) = &*self.inner;
        let mut state = lock.lock().unwrap();
        let slot = state.frame.get_or_insert_with(|| vec![0u8; self.frame_len]);
        if slot.len() != self.frame_len {
            *slot = vec![0u8; self.frame_len];
        }
        std::mem::swap(slot, capture_buf);
        state.error = None;
        state.closed = false;
        condvar.notify_all();
    }

    fn publish_terminal_state(&self, error: Option<String>) {
        let (lock, condvar) = &*self.inner;
        let mut state = lock.lock().unwrap();
        state.error = error;
        state.closed = true;
        condvar.notify_all();
    }

    fn fill_next_frame(&self, dst: &mut [u8], source_label: &str) -> io::Result<()> {
        if dst.len() != self.frame_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "destination frame size {} does not match latest-frame buffer size {} for {}",
                    dst.len(),
                    self.frame_len,
                    source_label
                ),
            ));
        }

        let (lock, condvar) = &*self.inner;
        let mut state = lock.lock().unwrap();
        while state.frame.is_none() && state.error.is_none() && !state.closed {
            state = condvar.wait(state).unwrap();
        }

        if let Some(err) = state.error.as_ref() {
            return Err(io::Error::other(format!(
                "failed to capture latest frame from {}: {}",
                source_label, err
            )));
        }

        if let Some(frame) = state.frame.as_ref() {
            dst.copy_from_slice(frame);
            return Ok(());
        }

        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!(
                "camera source {} closed before producing a frame",
                source_label
            ),
        ))
    }
}

impl FfmpegRgbSource {
    pub fn from_file(path: PathBuf, width: u32, height: u32, fps: u32) -> io::Result<Self> {
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

        let source_label = format!("file {}", path.display());
        Self::spawn_sequential(command, source_label, "camera-file")
    }

    pub fn from_v4l2_device(
        device: PathBuf,
        width: u32,
        height: u32,
        fps: u32,
        input_format: Option<&str>,
    ) -> io::Result<Self> {
        if !device.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("camera device does not exist: {}", device.display()),
            ));
        }

        let frame_len = frame_len(width, height)?;
        let video_size = format!("{width}x{height}");
        let filter = format!(
            "scale=w={width}:h={height}:force_original_aspect_ratio=decrease:flags=fast_bilinear,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:black"
        );

        let mut command = Command::new("ffmpeg");
        command
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-nostdin")
            .arg("-fflags")
            .arg("nobuffer")
            .arg("-flags")
            .arg("low_delay")
            .arg("-analyzeduration")
            .arg("0")
            .arg("-probesize")
            .arg("32")
            .arg("-thread_queue_size")
            .arg("1")
            .arg("-use_wallclock_as_timestamps")
            .arg("1")
            .arg("-f")
            .arg("v4l2")
            .arg("-video_size")
            .arg(&video_size)
            .arg("-framerate")
            .arg(fps.to_string())
            .arg("-an")
            .arg("-sn")
            .arg("-dn")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        if let Some(input_format) = input_format {
            command.arg("-input_format").arg(input_format);
        }

        command
            .arg("-i")
            .arg(&device)
            .arg("-vf")
            .arg(filter)
            .arg("-pix_fmt")
            .arg("rgb24")
            .arg("-f")
            .arg("rawvideo")
            .arg("pipe:1");

        let source_label = format!("device {}", device.display());
        Self::spawn_latest_frame(command, source_label, "camera-device", frame_len)
    }

    fn spawn_sequential(
        mut command: Command,
        source_label: String,
        option_name: &str,
    ) -> io::Result<Self> {
        let mut child = command.spawn().map_err(|err| {
            if err.kind() == io::ErrorKind::NotFound {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("ffmpeg not found on PATH; install ffmpeg to use --{option_name}"),
                )
            } else {
                io::Error::new(
                    err.kind(),
                    format!("failed to start ffmpeg for {source_label}: {err}"),
                )
            }
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            io::Error::other(format!(
                "failed to capture ffmpeg stdout for {source_label}"
            ))
        })?;

        Ok(Self {
            source_label,
            child,
            mode: SourceMode::Sequential { stdout },
        })
    }

    fn spawn_latest_frame(
        mut command: Command,
        source_label: String,
        option_name: &str,
        frame_len: usize,
    ) -> io::Result<Self> {
        let mut child = command.spawn().map_err(|err| {
            if err.kind() == io::ErrorKind::NotFound {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("ffmpeg not found on PATH; install ffmpeg to use --{option_name}"),
                )
            } else {
                io::Error::new(
                    err.kind(),
                    format!("failed to start ffmpeg for {source_label}: {err}"),
                )
            }
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            io::Error::other(format!(
                "failed to capture ffmpeg stdout for {source_label}"
            ))
        })?;

        let latest_frame = LatestFrameBuffer::new(frame_len);
        let latest_frame_reader = latest_frame.clone();
        let reader_source_label = source_label.clone();
        let reader_thread = thread::Builder::new()
            .name("rollio-camera-capture".to_string())
            .spawn(move || {
                let mut stdout = stdout;
                let mut capture_buf = vec![0u8; frame_len];
                loop {
                    match stdout.read_exact(&mut capture_buf) {
                        Ok(()) => latest_frame_reader.publish_frame(&mut capture_buf),
                        Err(err) => {
                            latest_frame_reader.publish_terminal_state(Some(format!(
                                "failed to read from {}: {}",
                                reader_source_label, err
                            )));
                            break;
                        }
                    }
                }
            })
            .map_err(|err| {
                io::Error::new(
                    err.kind(),
                    format!("failed to start reader thread for {source_label}: {err}"),
                )
            })?;

        Ok(Self {
            source_label,
            child,
            mode: SourceMode::LatestFrame {
                latest_frame,
                reader_thread: Some(reader_thread),
            },
        })
    }

    pub fn fill_next_frame(&mut self, dst: &mut [u8]) -> io::Result<()> {
        match &mut self.mode {
            SourceMode::Sequential { stdout } => stdout.read_exact(dst).map_err(|err| {
                let status_hint = match self.child.try_wait() {
                    Ok(Some(status)) => format!("ffmpeg exited with status {status}"),
                    Ok(None) => "ffmpeg is still running".to_string(),
                    Err(wait_err) => format!("failed to inspect ffmpeg status: {wait_err}"),
                };

                io::Error::new(
                    err.kind(),
                    format!(
                        "failed to read decoded frame from {}: {err} ({status_hint})",
                        self.source_label
                    ),
                )
            }),
            SourceMode::LatestFrame { latest_frame, .. } => {
                latest_frame.fill_next_frame(dst, &self.source_label)
            }
        }
    }
}

impl Drop for FfmpegRgbSource {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
        }
        if let SourceMode::LatestFrame { reader_thread, .. } = &mut self.mode {
            if let Some(reader_thread) = reader_thread.take() {
                let _ = reader_thread.join();
            }
        }
        let _ = self.child.wait();
    }
}

fn frame_len(width: u32, height: u32) -> io::Result<usize> {
    let pixels = usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().map(|h| w * h))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid frame dimensions"))?;
    pixels
        .checked_mul(3)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "frame size overflow"))
}
