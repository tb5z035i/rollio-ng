mod frames;

use clap::Parser;
use iceoryx2::prelude::*;
use rollio_types::messages::{CameraFrameHeader, PixelFormat, RobotState};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let payload_len = args.width as usize * args.height as usize * 3;
    let frame_duration = Duration::from_secs_f64(1.0 / args.fps as f64);

    eprintln!(
        "test-publisher: cameras={}, robots={}, fps={}, {}x{}, payload={}B",
        args.cameras, args.robots, args.fps, args.width, args.height, payload_len
    );

    // Create iceoryx2 node
    let node = NodeBuilder::new().create::<ipc::Service>()?;

    // --- Camera publishers ---
    let mut cam_publishers = Vec::new();
    for i in 0..args.cameras {
        let name = format!("camera_{i}");
        let service_name_str = format!("camera/{name}/frames");
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
        let service_name_str = format!("robot/{name}/state");
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

    eprintln!("publishing at {} fps...", args.fps);

    let start_time = Instant::now();
    let mut frame_index: u64 = 0;
    let mut next_frame_time = Instant::now();
    let mut last_status_time = Instant::now();
    let mut frames_since_status: u64 = 0;

    loop {
        // Generate frame into temp buffer (needed for counter burn-in)
        frames::generate_color_bars(&mut frame_buf, args.width, args.height, frame_index);

        let timestamp_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        // Publish camera frames
        for (_name, publisher) in &cam_publishers {
            let mut sample = publisher.loan_slice_uninit(payload_len)?;
            // Set user header before writing payload (write_from_slice consumes the uninit sample)
            *sample.user_header_mut() = CameraFrameHeader {
                timestamp_ns,
                width: args.width,
                height: args.height,
                pixel_format: PixelFormat::Rgb24,
                frame_index,
            };
            let sample = sample.write_from_slice(&frame_buf);
            sample.send()?;
        }

        // Publish robot states with sine-wave positions
        let elapsed_secs = start_time.elapsed().as_secs_f64();
        for (_name, publisher) in &robot_publishers {
            let mut state = RobotState::default();
            state.timestamp_ns = timestamp_ns;
            state.num_joints = 6;
            for j in 0..6 {
                state.positions[j] = (elapsed_secs + j as f64 * 0.5).sin();
            }
            // velocities and efforts stay at 0.0
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
        let sleep_until = next_frame_time;
        if sleep_until > Instant::now() {
            std::thread::sleep(sleep_until - Instant::now());
        }
    }
}
