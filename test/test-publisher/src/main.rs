mod frames;
mod video_file;

use clap::{Parser, ValueHint};
use iceoryx2::prelude::*;
use rollio_bus::{camera_frames_service_name, robot_state_service_name};
use rollio_types::messages::{CameraFrameHeader, PixelFormat, RobotState};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::video_file::FfmpegRgbSource;

#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CameraDeviceInputFormat {
    #[default]
    Auto,
    Mjpeg,
    Yuyv422,
}

impl CameraDeviceInputFormat {
    fn ffmpeg_input_format(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Mjpeg => Some("mjpeg"),
            Self::Yuyv422 => Some("yuyv422"),
        }
    }
}

#[derive(Parser, Debug)]
#[command(name = "rollio-test-publisher")]
#[command(about = "Publishes synthetic camera frames and robot states via iceoryx2")]
struct Args {
    /// Number of cameras to simulate
    #[arg(long, default_value_t = 2)]
    cameras: u32,

    /// Number of robots to simulate (6 DoF each)
    #[arg(long, default_value_t = 1)]
    robots: u32,

    /// Target frames per second
    #[arg(long, default_value_t = 30)]
    fps: u32,

    /// Frame width in pixels
    #[arg(long, default_value_t = 640)]
    width: u32,

    /// Frame height in pixels
    #[arg(long, default_value_t = 480)]
    height: u32,

    /// Optional local video file to decode and publish for every camera.
    /// The file is looped indefinitely and resampled to `--fps`, `--width`,
    /// and `--height`.
    #[arg(long, value_name = "PATH", value_hint = ValueHint::FilePath, conflicts_with = "camera_device")]
    camera_file: Option<PathBuf>,

    /// Optional V4L2 camera device (for example `/dev/video0`) to capture and
    /// publish for every camera topic.
    #[arg(
        long,
        value_name = "PATH",
        value_hint = ValueHint::FilePath,
        conflicts_with = "camera_file"
    )]
    camera_device: Option<PathBuf>,

    /// Requested V4L2 input pixel format for `--camera-device`.
    #[arg(long, default_value = "auto")]
    camera_device_format: CameraDeviceInputFormat,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let payload_len = args.width as usize * args.height as usize * 3;
    let frame_duration = Duration::from_secs_f64(1.0 / args.fps as f64);
    let camera_source = args
        .camera_file
        .as_ref()
        .map(|path| format!("file={}", path.display()))
        .or_else(|| {
            args.camera_device.as_ref().map(|path| {
                let format = args
                    .camera_device_format
                    .ffmpeg_input_format()
                    .unwrap_or("auto");
                format!("device={} format={format}", path.display())
            })
        })
        .unwrap_or_else(|| "synthetic=color-bars".to_string());

    eprintln!(
        "test-publisher: cameras={}, robots={}, fps={}, {}x{}, payload={}B, camera_source={}",
        args.cameras, args.robots, args.fps, args.width, args.height, payload_len, camera_source,
    );

    // Create iceoryx2 node
    let node = NodeBuilder::new().create::<ipc::Service>()?;

    // --- Camera publishers ---
    let mut cam_publishers = Vec::new();
    for i in 0..args.cameras {
        let name = format!("camera_{i}");
        let service_name_str = camera_frames_service_name(&name);
        let service_name: ServiceName = service_name_str.as_str().try_into()?;

        let service = node
            .service_builder(&service_name)
            .publish_subscribe::<[u8]>()
            .user_header::<CameraFrameHeader>()
            .open_or_create()?;

        let publisher = service
            .publisher_builder()
            .initial_max_slice_len(payload_len)
            .allocation_strategy(AllocationStrategy::PowerOfTwo)
            .create()?;

        eprintln!("  camera publisher: {service_name_str}");
        cam_publishers.push((name, publisher));
    }

    // --- Robot publishers ---
    let mut robot_publishers = Vec::new();
    for i in 0..args.robots {
        let name = format!("robot_{i}");
        let service_name_str = robot_state_service_name(&name);
        let service_name: ServiceName = service_name_str.as_str().try_into()?;

        let service = node
            .service_builder(&service_name)
            .publish_subscribe::<RobotState>()
            .open_or_create()?;

        let publisher = service.publisher_builder().create()?;

        eprintln!("  robot publisher: {service_name_str}");
        robot_publishers.push((name, publisher));
    }

    // Pre-allocate frame buffer for burning in the counter
    // (we need a temp buffer because write_from_fn is per-byte, and burning
    //  in the counter requires reading back neighboring pixels)
    let mut frame_buf = vec![0u8; payload_len];
    let mut video_source = match (
        args.camera_file.as_ref(),
        args.camera_device.as_ref(),
        args.cameras,
    ) {
        (Some(path), None, cameras) if cameras > 0 => Some(FfmpegRgbSource::from_file(
            path.clone(),
            args.width,
            args.height,
            args.fps,
        )?),
        (None, Some(device), cameras) if cameras > 0 => Some(FfmpegRgbSource::from_v4l2_device(
            device.clone(),
            args.width,
            args.height,
            args.fps,
            args.camera_device_format.ffmpeg_input_format(),
        )?),
        _ => None,
    };

    eprintln!("publishing at {} fps...", args.fps);

    let start_time = Instant::now();
    let mut frame_index: u64 = 0;
    let mut next_frame_time = Instant::now();
    let mut last_status_time = Instant::now();
    let mut frames_since_status: u64 = 0;

    loop {
        let elapsed_secs = start_time.elapsed().as_secs_f64();
        if !cam_publishers.is_empty() {
            if let Some(source) = video_source.as_mut() {
                source.fill_next_frame(&mut frame_buf)?;
            } else {
                // Generate time-varying frame with scrolling bars + timestamp
                frames::generate_color_bars(
                    &mut frame_buf,
                    args.width,
                    args.height,
                    elapsed_secs,
                    frame_index,
                );
            }
        }

        let timestamp_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        let timestamp_ns = timestamp_us.saturating_mul(1_000);

        // Publish camera frames
        for (_name, publisher) in &cam_publishers {
            let mut sample = publisher.loan_slice_uninit(payload_len)?;
            // Set user header before writing payload (write_from_slice consumes the uninit sample)
            *sample.user_header_mut() = CameraFrameHeader {
                timestamp_us,
                width: args.width,
                height: args.height,
                pixel_format: PixelFormat::Rgb24,
                frame_index,
            };
            let sample = sample.write_from_slice(&frame_buf);
            sample.send()?;
        }

        // Publish robot states with sine-wave positions
        for (_name, publisher) in &robot_publishers {
            let mut positions = [0.0f64; 16];
            for (j, pos) in positions.iter_mut().enumerate().take(6) {
                *pos = (elapsed_secs + j as f64 * 0.5).sin();
            }
            let state = RobotState {
                timestamp_ns,
                num_joints: 6,
                positions,
                ..RobotState::default()
            };
            publisher.send_copy(state)?;
        }

        frame_index += 1;
        frames_since_status += 1;

        // Periodic status print every second
        let now = Instant::now();
        if now.duration_since(last_status_time) >= Duration::from_secs(1) {
            let actual_fps =
                frames_since_status as f64 / now.duration_since(last_status_time).as_secs_f64();
            eprintln!("  frame={frame_index}, fps={actual_fps:.1}");
            last_status_time = now;
            frames_since_status = 0;
        }

        // Precise frame pacing: sleep until next frame time
        next_frame_time += frame_duration;
        let now = Instant::now();
        if next_frame_time > now {
            std::thread::sleep(next_frame_time - now);
        } else {
            // If capture/publish work ran long, drop schedule debt instead of
            // bursting frames to "catch up" with stale timestamps.
            next_frame_time = now;
        }
    }
}
