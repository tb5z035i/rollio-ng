use clap::{Args, Parser, Subcommand};
use iceoryx2::prelude::*;
use rollio_bus::{
    channel_camera_control_service_name, channel_command_service_name,
    channel_frames_service_name, channel_mode_control_service_name,
    channel_mode_info_service_name, channel_state_service_name, CONTROL_EVENTS_SERVICE,
    STATE_BUFFER, STATE_MAX_NODES, STATE_MAX_PUBLISHERS, STATE_MAX_SUBSCRIBERS,
};
use rollio_types::config::{
    BinaryDeviceConfig, CameraChannelProfile, ChannelCommandDefaults, DeviceQueryChannel,
    DeviceQueryDevice, DeviceQueryResponse, DeviceType, DirectJointCompatibility,
    DirectJointCompatibilityPeer, RobotCommandKind, RobotMode, RobotStateKind,
    StateValueLimitsEntry,
};
use rollio_types::messages::{
    CameraControl, CameraFrameHeader, ControlEvent, DeviceChannelMode, JointMitCommand15,
    JointVector15, PixelFormat, Pose7,
};
use serde_json::json;
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ffmpeg::util::format::pixel::Pixel;
use ffmpeg_next as ffmpeg;

const DRIVER_NAME: &str = "pseudo";

#[derive(Debug, Parser)]
#[command(name = "rollio-device-pseudo")]
#[command(about = "Synthetic hierarchical multi-channel device driver for Rollio")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Probe(ProbeArgs),
    Validate(ValidateArgs),
    Query(QueryArgs),
    Run(RunArgs),
}

#[derive(Debug, Clone, Args)]
struct ProbeArgs {
    #[arg(long, default_value_t = 0)]
    sim_cameras: u32,
    #[arg(long, default_value_t = 0)]
    sim_arms: u32,
    #[arg(long, default_value_t = 6)]
    dof: u32,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Args)]
struct ValidateArgs {
    id: String,
    #[arg(long = "channel-type")]
    channel_types: Vec<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Args)]
struct QueryArgs {
    id: String,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Args)]
struct RunArgs {
    #[arg(long, value_name = "PATH", conflicts_with = "config_inline")]
    config: Option<PathBuf>,
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    config_inline: Option<String>,
    #[arg(long)]
    dry_run: bool,
    /// MJPEG quality (libavcodec qscale, 1-31, lower=better). Overrides TOML.
    #[arg(long)]
    mjpeg_quality: Option<u32>,
    /// H.264 target bitrate in bits per second. Overrides TOML.
    #[arg(long)]
    h264_bitrate_bps: Option<u32>,
    /// H.264 GOP size (keyframe interval). Overrides TOML.
    #[arg(long)]
    h264_gop: Option<u32>,
    /// H.264 x264 preset. Overrides TOML.
    #[arg(long)]
    h264_preset: Option<String>,
    /// H.264 x264 tune. Overrides TOML.
    #[arg(long)]
    h264_tune: Option<String>,
    /// H.264 x264 profile. Overrides TOML.
    #[arg(long)]
    h264_profile: Option<String>,
}

/// Encoder knob defaults applied when neither TOML nor CLI specifies a value.
struct EncoderKnobs {
    mjpeg_quality: u32,
    h264_bitrate_bps: u32,
    h264_gop: u32,
    h264_preset: String,
    h264_tune: String,
    h264_profile: String,
}

impl EncoderKnobs {
    fn from_profile_cli(profile: &CameraChannelProfile, cli: &RunArgs) -> Self {
        let w = profile.width;
        let h = profile.height;
        let fps = profile.fps;
        let default_bitrate = ((w as u64 * h as u64 * fps as u64) / 10).min(u32::MAX as u64) as u32;
        Self {
            mjpeg_quality: cli.mjpeg_quality.or(profile.mjpeg_quality).unwrap_or(5),
            h264_bitrate_bps: cli
                .h264_bitrate_bps
                .or(profile.h264_bitrate_bps)
                .unwrap_or(default_bitrate),
            h264_gop: cli.h264_gop.or(profile.h264_gop).unwrap_or(fps),
            h264_preset: cli
                .h264_preset
                .clone()
                .or_else(|| profile.h264_preset.clone())
                .unwrap_or_else(|| "ultrafast".into()),
            h264_tune: cli
                .h264_tune
                .clone()
                .or_else(|| profile.h264_tune.clone())
                .unwrap_or_else(|| "zerolatency".into()),
            h264_profile: cli
                .h264_profile
                .clone()
                .or_else(|| profile.h264_profile.clone())
                .unwrap_or_else(|| "baseline".into()),
        }
    }
}

struct CameraRuntime {
    _channel_type: String,
    width: u32,
    height: u32,
    fps: u32,
    pixel_format: PixelFormat,
    publisher: iceoryx2::port::publisher::Publisher<ipc::Service, [u8], CameraFrameHeader>,
    /// RGB24 scratch buffer (always generated first, then converted).
    rgb_buf: Vec<u8>,
    /// Output buffer for the converted/published frame.
    frame: Vec<u8>,
    frame_index: u64,
    next_tick: Instant,
    knobs: EncoderKnobs,
    /// Streaming H.264 encoder held across frames (None for non-H264 formats).
    h264: Option<H264Encoder>,
}

/// H.264 Annex B streaming encoder state (libx264, no GLOBAL_HEADER, in-band SPS/PPS).
struct H264Encoder {
    encoder: ffmpeg::encoder::Video,
    scaler: ffmpeg::software::scaling::context::Context,
    yuv: ffmpeg::util::frame::Video,
    pkt: ffmpeg::packet::Packet,
    frame_pts: i64,
    force_idr: bool,
}

impl H264Encoder {
    fn new(
        width: u32,
        height: u32,
        fps: u32,
        knobs: &EncoderKnobs,
    ) -> Result<Self, Box<dyn Error>> {
        let codec = ffmpeg::encoder::find_by_name("libx264").ok_or("libx264 encoder not found")?;
        let mut encoder = ffmpeg::codec::context::Context::new_with_codec(codec)
            .encoder()
            .video()?;
        encoder.set_width(width);
        encoder.set_height(height);
        encoder.set_format(Pixel::YUV420P);
        encoder.set_frame_rate(Some(ffmpeg::Rational(fps as i32, 1)));
        encoder.set_time_base(ffmpeg::Rational(1, 1_000_000));
        encoder.set_bit_rate(knobs.h264_bitrate_bps as usize);
        encoder.set_max_b_frames(0);
        encoder.set_gop(knobs.h264_gop);
        // Deliberately NO GLOBAL_HEADER — passthrough backend needs in-band SPS/PPS.
        let mut codec_opts = ffmpeg::Dictionary::new();
        codec_opts.set("preset", &knobs.h264_preset);
        codec_opts.set("tune", &knobs.h264_tune);
        codec_opts.set("profile", &knobs.h264_profile);
        let encoder = encoder.open_as_with(codec, codec_opts)?;

        let scaler = ffmpeg::software::scaling::context::Context::get(
            Pixel::RGB24,
            width,
            height,
            Pixel::YUV420P,
            width,
            height,
            ffmpeg::software::scaling::flag::Flags::BILINEAR,
        )?;

        let yuv = ffmpeg::util::frame::Video::new(Pixel::YUV420P, width, height);

        Ok(Self {
            encoder,
            scaler,
            yuv,
            pkt: ffmpeg::packet::Packet::empty(),
            frame_pts: 0,
            force_idr: false,
        })
    }

    fn encode(
        &mut self,
        rgb: &[u8],
        width: u32,
        height: u32,
        out: &mut Vec<u8>,
    ) -> Result<(), Box<dyn Error>> {
        let mut rgb_frame = ffmpeg::util::frame::Video::new(Pixel::RGB24, width, height);
        rgb_frame.data_mut(0).copy_from_slice(rgb);
        self.scaler.run(&rgb_frame, &mut self.yuv)?;
        self.yuv.set_pts(Some(self.frame_pts));
        self.frame_pts += 1;

        if self.force_idr {
            self.yuv.set_kind(ffmpeg::picture::Type::I);
            unsafe { (*self.yuv.as_mut_ptr()).key_frame = 1; }
            self.force_idr = false;
        }

        self.encoder.send_frame(&self.yuv)?;
        self.encoder.receive_packet(&mut self.pkt)?;
        out.clear();
        out.extend_from_slice(self.pkt.data().unwrap_or(&[]));
        Ok(())
    }
}

struct RobotRuntime {
    _channel_type: String,
    dof: usize,
    mode: RobotMode,
    frequency_hz: f64,
    state_publishers: Vec<StatePublisher>,
    command_subscribers: Vec<CommandSubscriber>,
    command_defaults: ChannelCommandDefaults,
    current_positions: [f64; rollio_types::messages::MAX_DOF],
    target_positions: [f64; rollio_types::messages::MAX_DOF],
    previous_positions: [f64; rollio_types::messages::MAX_DOF],
    /// Phase 7: wall-clock timestamp captured the instant the simulated
    /// state values were updated (i.e. the pseudo equivalent of
    /// "sensor-feedback receipt time"). Read by `publish_robot_states` so
    /// the published `timestamp_us` reflects when the values were
    /// produced, not when the iceoryx2 publish call happened.
    current_state_timestamp_us: u64,
    next_tick: Instant,
    started_at: Instant,
}

enum StatePublisher {
    JointPosition(iceoryx2::port::publisher::Publisher<ipc::Service, JointVector15, ()>),
    JointVelocity(iceoryx2::port::publisher::Publisher<ipc::Service, JointVector15, ()>),
    JointEffort(iceoryx2::port::publisher::Publisher<ipc::Service, JointVector15, ()>),
    EndEffectorPose(iceoryx2::port::publisher::Publisher<ipc::Service, Pose7, ()>),
}

enum CommandSubscriber {
    JointPosition(iceoryx2::port::subscriber::Subscriber<ipc::Service, JointVector15, ()>),
    JointMit(iceoryx2::port::subscriber::Subscriber<ipc::Service, JointMitCommand15, ()>),
}

type ShutdownSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, ControlEvent, ()>;
type ChannelModeSubscriber =
    iceoryx2::port::subscriber::Subscriber<ipc::Service, DeviceChannelMode, ()>;
type ChannelModePublisher =
    iceoryx2::port::publisher::Publisher<ipc::Service, DeviceChannelMode, ()>;

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Probe(args) => run_probe(args)?,
        Command::Validate(args) => run_validate(args)?,
        Command::Query(args) => run_query(args)?,
        Command::Run(args) => run_device_command(args)?,
    }
    Ok(())
}

fn run_probe(args: ProbeArgs) -> Result<(), Box<dyn Error>> {
    let ids = pseudo_probe_ids(args.sim_cameras, args.sim_arms, args.dof);
    if args.json {
        println!("{}", serde_json::to_string_pretty(&ids)?);
    } else if ids.is_empty() {
        println!("no pseudo devices discovered");
    } else {
        for id in ids {
            println!("{id}");
        }
    }
    Ok(())
}

fn run_validate(args: ValidateArgs) -> Result<(), Box<dyn Error>> {
    let valid = query_pseudo_device(&args.id).is_some_and(|device| {
        args.channel_types.is_empty()
            || args.channel_types.iter().all(|channel_type| {
                device
                    .channels
                    .iter()
                    .any(|channel| channel.channel_type == *channel_type)
            })
    });
    let report = json!({
        "valid": valid,
        "id": args.id,
        "channel_types": args.channel_types,
    });
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if valid {
        println!("{} is valid", args.id);
    } else {
        println!("{} is invalid", args.id);
    }
    if valid {
        Ok(())
    } else {
        Err("pseudo validate failed".into())
    }
}

fn run_query(args: QueryArgs) -> Result<(), Box<dyn Error>> {
    let Some(device) = query_pseudo_device(&args.id) else {
        return Err(format!("unknown pseudo device: {}", args.id).into());
    };
    let response = DeviceQueryResponse {
        driver: DRIVER_NAME.into(),
        devices: vec![device],
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        print_query_human(&response);
    }
    Ok(())
}

fn run_device_command(args: RunArgs) -> Result<(), Box<dyn Error>> {
    let device = load_device_config(&args)?;
    if args.dry_run {
        return Ok(());
    }
    run_device(device, args)
}

fn load_device_config(args: &RunArgs) -> Result<BinaryDeviceConfig, Box<dyn Error>> {
    let device = if let Some(config_path) = &args.config {
        BinaryDeviceConfig::from_file(config_path)?
    } else if let Some(config_inline) = &args.config_inline {
        config_inline.parse::<BinaryDeviceConfig>()?
    } else {
        return Err("run requires either --config or --config-inline".into());
    };
    if device.driver != DRIVER_NAME {
        return Err(format!(
            "device \"{}\" uses driver \"{}\", expected {DRIVER_NAME}",
            device.name, device.driver
        )
        .into());
    }
    Ok(device)
}

fn run_device(device: BinaryDeviceConfig, cli: RunArgs) -> Result<(), Box<dyn Error>> {
    let stop = Arc::new(AtomicBool::new(false));
    let cli = Arc::new(cli);

    // See `robots/airbot_play_rust/src/bin/device.rs::run_device`. The same
    // SIGINT/SIGTERM rationale applies — the per-channel loops poll `stop`
    // and want a chance to run their cleanup (publish a final state
    // snapshot, transition to Disabled) before the process exits.
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop))?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop))?;

    let mut handles = Vec::new();

    for channel in device
        .channels
        .iter()
        .filter(|channel| channel.enabled)
        .cloned()
    {
        let bus_root = device.bus_root.clone();
        let stop_flag = Arc::clone(&stop);
        let cli = Arc::clone(&cli);
        let thread_name = format!("rollio-pseudo-{}", channel.channel_type);
        let handle = std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                let result = match channel.kind {
                    DeviceType::Camera => {
                        run_camera_channel(bus_root, channel, Arc::clone(&stop_flag), &cli)
                    }
                    DeviceType::Robot => {
                        run_robot_channel(bus_root, channel, Arc::clone(&stop_flag))
                    }
                };
                if result.is_err() {
                    stop_flag.store(true, Ordering::Relaxed);
                }
                result.map_err(|error| error.to_string())
            })?;
        handles.push(handle);
    }

    let mut first_error = None;
    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
            Err(_) => {
                if first_error.is_none() {
                    first_error = Some("pseudo channel thread panicked".to_owned());
                }
            }
        }
    }

    if let Some(error) = first_error {
        Err(error.into())
    } else {
        Ok(())
    }
}

fn camera_period(fps: u32) -> Duration {
    Duration::from_secs_f64(1.0 / fps.max(1) as f64)
}

fn pixel_count(width: u32, height: u32) -> usize {
    (width as usize) * (height as usize)
}

fn rgb_frame_len(width: u32, height: u32) -> usize {
    pixel_count(width, height) * 3
}

fn initial_slot_len(width: u32, height: u32, pf: PixelFormat) -> usize {
    let pixels = pixel_count(width, height);
    match pf {
        PixelFormat::Rgb24 | PixelFormat::Bgr24 => pixels * 3,
        PixelFormat::Yuyv | PixelFormat::Depth16 => pixels * 2,
        PixelFormat::Gray8 => pixels,
        PixelFormat::Mjpeg | PixelFormat::H264AnnexB => pixels * 3,
        PixelFormat::Nv12 => pixels * 3 / 2,
    }
}

fn publish_camera_frame(camera: &mut CameraRuntime) -> Result<(), Box<dyn Error>> {
    // Always generate RGB24 color bars first.
    generate_color_bars(
        &mut camera.rgb_buf,
        camera.width,
        camera.height,
        camera.frame_index,
    );

    let (payload, pixel_format) = match camera.pixel_format {
        PixelFormat::Rgb24 => {
            camera.frame[..camera.rgb_buf.len()].copy_from_slice(&camera.rgb_buf);
            (&camera.frame[..camera.rgb_buf.len()], PixelFormat::Rgb24)
        }
        PixelFormat::Bgr24 => {
            convert_rgb24_to_bgr24(
                &camera.rgb_buf,
                &mut camera.frame,
                camera.width,
                camera.height,
            );
            let len = rgb_frame_len(camera.width, camera.height);
            (&camera.frame[..len], PixelFormat::Bgr24)
        }
        PixelFormat::Yuyv => {
            convert_rgb24_to_yuyv(
                &camera.rgb_buf,
                &mut camera.frame,
                camera.width,
                camera.height,
            );
            let len = pixel_count(camera.width, camera.height) * 2;
            (&camera.frame[..len], PixelFormat::Yuyv)
        }
        PixelFormat::Gray8 => {
            convert_rgb24_to_gray8(
                &camera.rgb_buf,
                &mut camera.frame,
                camera.width,
                camera.height,
            );
            let len = pixel_count(camera.width, camera.height);
            (&camera.frame[..len], PixelFormat::Gray8)
        }
        PixelFormat::Depth16 => {
            generate_depth16(
                &mut camera.frame,
                camera.width,
                camera.height,
                camera.frame_index,
            );
            let len = pixel_count(camera.width, camera.height) * 2;
            (&camera.frame[..len], PixelFormat::Depth16)
        }
        PixelFormat::Mjpeg => {
            let len = encode_mjpeg(
                &camera.rgb_buf,
                &mut camera.frame,
                camera.width,
                camera.height,
                camera.knobs.mjpeg_quality,
            )?;
            (&camera.frame[..len], PixelFormat::Mjpeg)
        }
        PixelFormat::H264AnnexB => {
            let h264 = camera.h264.as_mut().expect("h264 encoder not initialized");
            h264.encode(
                &camera.rgb_buf,
                camera.width,
                camera.height,
                &mut camera.frame,
            )?;
            (&camera.frame[..], PixelFormat::H264AnnexB)
        }
        PixelFormat::Nv12 => {
            // Produce a synthetic NV12 frame: Y = luma from RGB, UV = 128 (gray)
            let pixels = pixel_count(camera.width, camera.height);
            let uv_size = pixels / 2;
            let total = pixels + uv_size;
            for i in 0..pixels {
                let r = camera.rgb_buf[i * 3] as u16;
                let g = camera.rgb_buf[i * 3 + 1] as u16;
                let b = camera.rgb_buf[i * 3 + 2] as u16;
                camera.frame[i] = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16).min(255) as u8;
            }
            camera.frame[pixels..total].fill(128);
            (&camera.frame[..total], PixelFormat::Nv12)
        }
    };

    let timestamp_us = unix_timestamp_us();
    let mut sample = camera.publisher.loan_slice_uninit(payload.len())?;
    *sample.user_header_mut() = CameraFrameHeader {
        timestamp_us,
        width: camera.width,
        height: camera.height,
        pixel_format,
        frame_index: camera.frame_index,
    };
    let sample = sample.write_from_slice(payload);
    sample.send()?;
    camera.frame_index += 1;
    Ok(())
}

fn run_camera_channel(
    bus_root: String,
    channel: rollio_types::config::DeviceChannelConfigV2,
    stop: Arc<AtomicBool>,
    cli: &RunArgs,
) -> Result<(), Box<dyn Error>> {
    let channel_type = channel.channel_type.clone();
    let profile = channel.profile.ok_or("pseudo camera requires profile")?;
    let pf = profile.pixel_format;
    let knobs = EncoderKnobs::from_profile_cli(&profile, cli);
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;
    let service_name: ServiceName = channel_frames_service_name(&bus_root, &channel.channel_type)
        .as_str()
        .try_into()?;
    let slot_len = initial_slot_len(profile.width, profile.height, pf);
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<[u8]>()
        .user_header::<CameraFrameHeader>()
        .open_or_create()?;
    let publisher = service
        .publisher_builder()
        .initial_max_slice_len(slot_len)
        .allocation_strategy(AllocationStrategy::PowerOfTwo)
        .create()?;
    let shutdown_subscriber = open_shutdown_subscriber(&node)?;
    let mode_info_publisher = open_channel_mode_publisher(&node, &bus_root, &channel_type)?;
    let camera_control_subscriber = {
        let topic = channel_camera_control_service_name(&bus_root, &channel_type);
        let sn: ServiceName = topic.as_str().try_into()?;
        let svc = node
            .service_builder(&sn)
            .publish_subscribe::<CameraControl>()
            .open_or_create()?;
        svc.subscriber_builder().create()?
    };

    let rgb_size = rgb_frame_len(profile.width, profile.height);
    let frame_capacity = slot_len.max(rgb_size);

    let h264 = if pf == PixelFormat::H264AnnexB {
        Some(H264Encoder::new(
            profile.width,
            profile.height,
            profile.fps,
            &knobs,
        )?)
    } else {
        None
    };

    let mut camera = CameraRuntime {
        _channel_type: channel_type,
        width: profile.width,
        height: profile.height,
        fps: profile.fps,
        pixel_format: pf,
        publisher,
        rgb_buf: vec![0; rgb_size],
        frame: vec![0; frame_capacity],
        frame_index: 0,
        next_tick: Instant::now(),
        knobs,
        h264,
    };

    loop {
        if stop.load(Ordering::Relaxed) || drain_shutdown_events(&shutdown_subscriber)? {
            return Ok(());
        }
        while let Some(sample) = camera_control_subscriber.receive()? {
            if *sample.payload() == CameraControl::RequestKeyframe {
                if let Some(h264) = camera.h264.as_mut() {
                    h264.force_idr = true;
                }
            }
        }

        let now = Instant::now();
        if now >= camera.next_tick {
            publish_camera_frame(&mut camera)?;
            camera.next_tick += camera_period(camera.fps);
            mode_info_publisher.send_copy(DeviceChannelMode::Enabled)?;
        } else {
            std::thread::sleep((camera.next_tick - now).min(Duration::from_millis(5)));
        }
    }
}

// ---------------------------------------------------------------------------
// Pixel format converters (source: RGB24 color bars)
// ---------------------------------------------------------------------------

fn convert_rgb24_to_bgr24(src: &[u8], dst: &mut [u8], w: u32, h: u32) {
    let pixels = (w as usize) * (h as usize);
    for (s_chunk, d_chunk) in src[..pixels * 3]
        .chunks_exact(3)
        .zip(dst[..pixels * 3].chunks_exact_mut(3))
    {
        d_chunk[0] = s_chunk[2];
        d_chunk[1] = s_chunk[1];
        d_chunk[2] = s_chunk[0];
    }
}

fn convert_rgb24_to_yuyv(src: &[u8], dst: &mut [u8], w: u32, h: u32) {
    let wu = w as usize;
    let hu = h as usize;
    for y in 0..hu {
        for x_pair in 0..(wu / 2) {
            let x0 = x_pair * 2;
            let x1 = x0 + 1;
            let s0 = (y * wu + x0) * 3;
            let r0 = src[s0] as f64;
            let g0 = src[s0 + 1] as f64;
            let b0 = src[s0 + 2] as f64;
            let s1 = (y * wu + x1) * 3;
            let r1 = src[s1] as f64;
            let g1 = src[s1 + 1] as f64;
            let b1 = src[s1 + 2] as f64;

            let y0 = (16.0 + 0.257 * r0 + 0.504 * g0 + 0.098 * b0).clamp(16.0, 235.0) as u8;
            let y1 = (16.0 + 0.257 * r1 + 0.504 * g1 + 0.098 * b1).clamp(16.0, 235.0) as u8;
            let rav = (r0 + r1) * 0.5;
            let gav = (g0 + g1) * 0.5;
            let bav = (b0 + b1) * 0.5;
            let u = (128.0 - 0.148 * rav - 0.291 * gav + 0.439 * bav).clamp(16.0, 240.0) as u8;
            let v = (128.0 + 0.439 * rav - 0.368 * gav - 0.071 * bav).clamp(16.0, 240.0) as u8;

            let d = (y * wu + x0) * 2;
            dst[d] = y0;
            dst[d + 1] = u;
            dst[d + 2] = y1;
            dst[d + 3] = v;
        }
    }
}

fn convert_rgb24_to_gray8(src: &[u8], dst: &mut [u8], w: u32, h: u32) {
    let pixels = (w as usize) * (h as usize);
    for (s_chunk, d_pixel) in src[..pixels * 3]
        .chunks_exact(3)
        .zip(dst[..pixels].iter_mut())
    {
        let r = s_chunk[0] as f64;
        let g = s_chunk[1] as f64;
        let b = s_chunk[2] as f64;
        *d_pixel = (0.299 * r + 0.587 * g + 0.114 * b)
            .round()
            .clamp(0.0, 255.0) as u8;
    }
}

fn generate_depth16(buf: &mut [u8], w: u32, h: u32, frame_index: u64) {
    let wu = w as usize;
    let hu = h as usize;
    let offset = (frame_index % 1000) as u16;
    for y in 0..hu {
        let row_base = y * wu * 2;
        let v_scale = (y as f64 / hu.max(1) as f64 * 65535.0) as u16;
        for x in 0..wu {
            let h_scale = (x as f64 / wu.max(1) as f64 * 65535.0) as u16;
            let val = v_scale
                .wrapping_add(h_scale)
                .wrapping_add(offset)
                .to_le_bytes();
            let p = row_base + x * 2;
            buf[p] = val[0];
            buf[p + 1] = val[1];
        }
    }
}

fn encode_mjpeg(
    rgb: &[u8],
    dst: &mut Vec<u8>,
    w: u32,
    h: u32,
    quality: u32,
) -> Result<usize, Box<dyn Error>> {
    let codec = ffmpeg::encoder::find_by_name("mjpeg").ok_or("mjpeg encoder not found")?;
    let mut ctx = ffmpeg::codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()?;
    ctx.set_width(w);
    ctx.set_height(h);
    ctx.set_format(Pixel::YUVJ420P);
    ctx.set_time_base(ffmpeg::Rational(1, 1));
    ctx.set_frame_rate(Some(ffmpeg::Rational(1, 1)));
    unsafe {
        (*ctx.as_mut_ptr()).qmin = quality.clamp(1, 31) as i32;
        (*ctx.as_mut_ptr()).qmax = quality.clamp(1, 31) as i32;
    }
    let mut encoder = ctx.open_as_with(codec, ffmpeg::Dictionary::new())?;

    let mut scaler = ffmpeg::software::scaling::context::Context::get(
        Pixel::RGB24,
        w,
        h,
        Pixel::YUVJ420P,
        w,
        h,
        ffmpeg::software::scaling::flag::Flags::BILINEAR,
    )?;
    let mut rgb_frame = ffmpeg::util::frame::Video::new(Pixel::RGB24, w, h);
    rgb_frame
        .data_mut(0)
        .copy_from_slice(&rgb[..(w as usize * h as usize * 3)]);

    let mut yuv = ffmpeg::util::frame::Video::new(Pixel::YUVJ420P, w, h);
    scaler.run(&rgb_frame, &mut yuv)?;
    yuv.set_pts(Some(0));

    encoder.send_frame(&yuv)?;
    let _ = encoder.send_eof();

    dst.clear();
    let mut pkt = ffmpeg::packet::Packet::empty();
    while encoder.receive_packet(&mut pkt).is_ok() {
        dst.extend_from_slice(pkt.data().unwrap_or(&[]));
    }
    Ok(dst.len())
}

fn publish_robot_states(robot: &mut RobotRuntime) -> Result<(), Box<dyn Error>> {
    if robot.mode == RobotMode::CommandFollowing {
        drain_commands(robot)?;
        update_command_following_state(robot);
    } else if robot.mode != RobotMode::Disabled {
        update_free_drive_state(robot);
    } else {
        // Disabled mode does not call any `update_*_state`, so the previous
        // tick's timestamp would be reused. Refresh it so each emitted
        // frame still carries a meaningful "produced now" instant.
        robot.current_state_timestamp_us = unix_timestamp_us();
    }

    // Phase 7: prefer the timestamp captured inside `update_*_state` so
    // the published value reflects when the simulated state was produced
    // instead of when iceoryx2 actually accepted the publish.
    let timestamp_us = robot.current_state_timestamp_us;
    let positions = robot.current_positions;
    let mut velocities = [0.0f64; rollio_types::messages::MAX_DOF];
    let mut efforts = [0.0f64; rollio_types::messages::MAX_DOF];
    let elapsed_secs = robot.started_at.elapsed().as_secs_f64();
    for joint_idx in 0..robot.dof {
        velocities[joint_idx] = positions[joint_idx] - robot.previous_positions[joint_idx];
        efforts[joint_idx] = match robot.mode {
            RobotMode::Disabled => 0.0,
            RobotMode::FreeDrive | RobotMode::Identifying => {
                0.1 * (elapsed_secs * 0.5 + joint_idx as f64).sin()
            }
            RobotMode::CommandFollowing => {
                let kp = robot
                    .command_defaults
                    .joint_mit_kp
                    .get(joint_idx)
                    .copied()
                    .unwrap_or(1.0);
                (robot.target_positions[joint_idx] - positions[joint_idx]) * kp
            }
        };
    }
    robot.previous_positions = positions;

    let joint_positions = JointVector15::from_slice(timestamp_us, &positions[..robot.dof]);
    let joint_velocities = JointVector15::from_slice(timestamp_us, &velocities[..robot.dof]);
    let joint_efforts = JointVector15::from_slice(timestamp_us, &efforts[..robot.dof]);
    let ee_pose = Pose7 {
        timestamp_us,
        values: [
            positions[0],
            positions.get(1).copied().unwrap_or_default(),
            positions.get(2).copied().unwrap_or_default(),
            0.0,
            0.0,
            0.0,
            1.0,
        ],
    };

    for publisher in &robot.state_publishers {
        match publisher {
            StatePublisher::JointPosition(publisher) => {
                publisher.send_copy(joint_positions)?;
            }
            StatePublisher::JointVelocity(publisher) => {
                publisher.send_copy(joint_velocities)?;
            }
            StatePublisher::JointEffort(publisher) => {
                publisher.send_copy(joint_efforts)?;
            }
            StatePublisher::EndEffectorPose(publisher) => {
                publisher.send_copy(ee_pose)?;
            }
        }
    }

    Ok(())
}

fn run_robot_channel(
    bus_root: String,
    channel: rollio_types::config::DeviceChannelConfigV2,
    stop: Arc<AtomicBool>,
) -> Result<(), Box<dyn Error>> {
    let dof = channel.dof.ok_or("pseudo robot requires dof")? as usize;
    let mode = channel.mode.ok_or("pseudo robot requires mode")?;
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;
    let shutdown_subscriber = open_shutdown_subscriber(&node)?;
    let mode_subscriber = open_channel_mode_subscriber(&node, &bus_root, &channel.channel_type)?;
    let mode_info_publisher = open_channel_mode_publisher(&node, &bus_root, &channel.channel_type)?;
    // Apply the shared state-buffer caps (see `rollio_bus::STATE_BUFFER`).
    let mut state_publishers = Vec::new();
    for state_kind in &channel.publish_states {
        let topic: ServiceName =
            channel_state_service_name(&bus_root, &channel.channel_type, state_kind.topic_suffix())
                .as_str()
                .try_into()?;
        match state_kind {
            RobotStateKind::JointPosition => {
                let service = node
                    .service_builder(&topic)
                    .publish_subscribe::<JointVector15>()
                    .subscriber_max_buffer_size(STATE_BUFFER)
                    .history_size(STATE_BUFFER)
                    .max_publishers(STATE_MAX_PUBLISHERS)
                    .max_subscribers(STATE_MAX_SUBSCRIBERS)
                    .max_nodes(STATE_MAX_NODES)
                    .open_or_create()?;
                state_publishers.push(StatePublisher::JointPosition(
                    service.publisher_builder().create()?,
                ));
            }
            RobotStateKind::JointVelocity => {
                let service = node
                    .service_builder(&topic)
                    .publish_subscribe::<JointVector15>()
                    .subscriber_max_buffer_size(STATE_BUFFER)
                    .history_size(STATE_BUFFER)
                    .max_publishers(STATE_MAX_PUBLISHERS)
                    .max_subscribers(STATE_MAX_SUBSCRIBERS)
                    .max_nodes(STATE_MAX_NODES)
                    .open_or_create()?;
                state_publishers.push(StatePublisher::JointVelocity(
                    service.publisher_builder().create()?,
                ));
            }
            RobotStateKind::JointEffort => {
                let service = node
                    .service_builder(&topic)
                    .publish_subscribe::<JointVector15>()
                    .subscriber_max_buffer_size(STATE_BUFFER)
                    .history_size(STATE_BUFFER)
                    .max_publishers(STATE_MAX_PUBLISHERS)
                    .max_subscribers(STATE_MAX_SUBSCRIBERS)
                    .max_nodes(STATE_MAX_NODES)
                    .open_or_create()?;
                state_publishers.push(StatePublisher::JointEffort(
                    service.publisher_builder().create()?,
                ));
            }
            RobotStateKind::EndEffectorPose => {
                let service = node
                    .service_builder(&topic)
                    .publish_subscribe::<Pose7>()
                    .subscriber_max_buffer_size(STATE_BUFFER)
                    .history_size(STATE_BUFFER)
                    .max_publishers(STATE_MAX_PUBLISHERS)
                    .max_subscribers(STATE_MAX_SUBSCRIBERS)
                    .max_nodes(STATE_MAX_NODES)
                    .open_or_create()?;
                state_publishers.push(StatePublisher::EndEffectorPose(
                    service.publisher_builder().create()?,
                ));
            }
            _ => {}
        }
    }

    let mut command_subscribers = Vec::new();
    for command_kind in [RobotCommandKind::JointPosition, RobotCommandKind::JointMit] {
        let topic: ServiceName = channel_command_service_name(
            &bus_root,
            &channel.channel_type,
            command_kind.topic_suffix(),
        )
        .as_str()
        .try_into()?;
        match command_kind {
            RobotCommandKind::JointPosition => {
                let service = node
                    .service_builder(&topic)
                    .publish_subscribe::<JointVector15>()
                    .subscriber_max_buffer_size(STATE_BUFFER)
                    .history_size(STATE_BUFFER)
                    .max_publishers(STATE_MAX_PUBLISHERS)
                    .max_subscribers(STATE_MAX_SUBSCRIBERS)
                    .max_nodes(STATE_MAX_NODES)
                    .open_or_create()?;
                command_subscribers.push(CommandSubscriber::JointPosition(
                    service.subscriber_builder().create()?,
                ));
            }
            RobotCommandKind::JointMit => {
                let service = node
                    .service_builder(&topic)
                    .publish_subscribe::<JointMitCommand15>()
                    .subscriber_max_buffer_size(STATE_BUFFER)
                    .history_size(STATE_BUFFER)
                    .max_publishers(STATE_MAX_PUBLISHERS)
                    .max_subscribers(STATE_MAX_SUBSCRIBERS)
                    .max_nodes(STATE_MAX_NODES)
                    .open_or_create()?;
                command_subscribers.push(CommandSubscriber::JointMit(
                    service.subscriber_builder().create()?,
                ));
            }
            _ => {}
        }
    }

    let mut robot = RobotRuntime {
        _channel_type: channel.channel_type,
        dof,
        mode,
        frequency_hz: channel.control_frequency_hz.unwrap_or(60.0),
        state_publishers,
        command_subscribers,
        command_defaults: channel.command_defaults,
        current_positions: [0.0; rollio_types::messages::MAX_DOF],
        target_positions: [0.0; rollio_types::messages::MAX_DOF],
        previous_positions: [0.0; rollio_types::messages::MAX_DOF],
        current_state_timestamp_us: 0,
        next_tick: Instant::now(),
        started_at: Instant::now(),
    };

    loop {
        if stop.load(Ordering::Relaxed) || drain_shutdown_events(&shutdown_subscriber)? {
            return Ok(());
        }

        if let Some(next_mode) = drain_robot_mode_events(&mode_subscriber)? {
            robot.mode = next_mode;
        }
        mode_info_publisher.send_copy(robot_mode_to_channel_mode(robot.mode))?;

        let now = Instant::now();
        if now >= robot.next_tick {
            publish_robot_states(&mut robot)?;
            robot.next_tick += Duration::from_secs_f64(1.0 / robot.frequency_hz.max(1.0));
        } else {
            std::thread::sleep((robot.next_tick - now).min(Duration::from_millis(5)));
        }
    }
}

fn drain_commands(robot: &mut RobotRuntime) -> Result<(), Box<dyn Error>> {
    for subscriber in &robot.command_subscribers {
        match subscriber {
            CommandSubscriber::JointPosition(subscriber) => loop {
                let Some(sample) = subscriber.receive()? else {
                    break;
                };
                let payload = sample.payload();
                let active = robot.dof.min(payload.len as usize);
                robot.target_positions[..active].copy_from_slice(&payload.values[..active]);
            },
            CommandSubscriber::JointMit(subscriber) => loop {
                let Some(sample) = subscriber.receive()? else {
                    break;
                };
                let payload = sample.payload();
                let active = robot.dof.min(payload.len as usize);
                robot.target_positions[..active].copy_from_slice(&payload.position[..active]);
            },
        }
    }
    Ok(())
}

fn update_free_drive_state(robot: &mut RobotRuntime) {
    let elapsed_secs = robot.started_at.elapsed().as_secs_f64();
    for (joint_idx, position) in robot
        .current_positions
        .iter_mut()
        .take(robot.dof)
        .enumerate()
    {
        let frequency = 0.5 + joint_idx as f64 * 0.075;
        *position =
            (elapsed_secs * frequency * std::f64::consts::TAU + joint_idx as f64 * 0.4).sin();
    }
    // Phase 7: stamp at the moment we computed the new state values, not
    // at publish time. For the pseudo driver this is the closest analogue
    // to a real robot's sensor-feedback receipt timestamp.
    robot.current_state_timestamp_us = unix_timestamp_us();
}

fn update_command_following_state(robot: &mut RobotRuntime) {
    let period = 1.0 / robot.frequency_hz.max(1.0);
    let alpha = (period / 0.05f64.max(period)).clamp(0.0, 1.0);
    for joint_idx in 0..robot.dof {
        let error = robot.target_positions[joint_idx] - robot.current_positions[joint_idx];
        robot.current_positions[joint_idx] += error * alpha;
    }
    robot.current_state_timestamp_us = unix_timestamp_us();
}

fn drain_shutdown_events(subscriber: &ShutdownSubscriber) -> Result<bool, Box<dyn Error>> {
    loop {
        match subscriber.receive()? {
            Some(sample) => {
                if matches!(*sample.payload(), ControlEvent::Shutdown) {
                    return Ok(true);
                }
            }
            None => return Ok(false),
        }
    }
}

fn open_shutdown_subscriber(
    node: &Node<ipc::Service>,
) -> Result<ShutdownSubscriber, Box<dyn Error>> {
    let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
    let control_service = node
        .service_builder(&control_service_name)
        .publish_subscribe::<ControlEvent>()
        .open_or_create()?;
    Ok(control_service.subscriber_builder().create()?)
}

fn open_channel_mode_subscriber(
    node: &Node<ipc::Service>,
    bus_root: &str,
    channel_type: &str,
) -> Result<ChannelModeSubscriber, Box<dyn Error>> {
    let mode_service_name: ServiceName = channel_mode_control_service_name(bus_root, channel_type)
        .as_str()
        .try_into()?;
    let mode_service = node
        .service_builder(&mode_service_name)
        .publish_subscribe::<DeviceChannelMode>()
        .max_publishers(16)
        .max_subscribers(16)
        .max_nodes(16)
        .open_or_create()?;
    Ok(mode_service.subscriber_builder().create()?)
}

fn open_channel_mode_publisher(
    node: &Node<ipc::Service>,
    bus_root: &str,
    channel_type: &str,
) -> Result<ChannelModePublisher, Box<dyn Error>> {
    let mode_service_name: ServiceName = channel_mode_info_service_name(bus_root, channel_type)
        .as_str()
        .try_into()?;
    let mode_service = node
        .service_builder(&mode_service_name)
        .publish_subscribe::<DeviceChannelMode>()
        .max_publishers(16)
        .max_subscribers(16)
        .max_nodes(16)
        .open_or_create()?;
    Ok(mode_service.publisher_builder().create()?)
}

fn drain_robot_mode_events(
    subscriber: &ChannelModeSubscriber,
) -> Result<Option<RobotMode>, Box<dyn Error>> {
    let mut latest = None;
    loop {
        match subscriber.receive()? {
            Some(sample) => {
                latest = match *sample.payload() {
                    DeviceChannelMode::FreeDrive => Some(RobotMode::FreeDrive),
                    DeviceChannelMode::CommandFollowing => Some(RobotMode::CommandFollowing),
                    DeviceChannelMode::Identifying => Some(RobotMode::Identifying),
                    DeviceChannelMode::Disabled => Some(RobotMode::Disabled),
                    DeviceChannelMode::Enabled => Some(RobotMode::FreeDrive),
                };
            }
            None => return Ok(latest),
        }
    }
}

fn robot_mode_to_channel_mode(mode: RobotMode) -> DeviceChannelMode {
    match mode {
        RobotMode::FreeDrive => DeviceChannelMode::FreeDrive,
        RobotMode::CommandFollowing => DeviceChannelMode::CommandFollowing,
        RobotMode::Identifying => DeviceChannelMode::Identifying,
        RobotMode::Disabled => DeviceChannelMode::Disabled,
    }
}

fn pseudo_probe_ids(sim_cameras: u32, sim_arms: u32, dof: u32) -> Vec<String> {
    let mut ids = Vec::new();
    for index in 0..sim_cameras {
        ids.push(format!("pseudo_camera_{index}"));
    }
    for index in 0..sim_arms {
        ids.push(format!("pseudo_robot_{index}_dof_{dof}"));
    }
    ids
}

/// All 7 pixel formats in display order.
const ALL_PIXEL_FORMATS: [PixelFormat; 7] = [
    PixelFormat::Rgb24,
    PixelFormat::Bgr24,
    PixelFormat::Yuyv,
    PixelFormat::Mjpeg,
    PixelFormat::Depth16,
    PixelFormat::Gray8,
    PixelFormat::H264AnnexB,
];

#[allow(clippy::manual_map)]
fn query_pseudo_device(id: &str) -> Option<DeviceQueryDevice> {
    if id.starts_with("pseudo_camera_") {
        Some(DeviceQueryDevice {
            id: id.into(),
            device_class: "pseudo-camera".into(),
            device_label: "Pseudo Camera".into(),
            default_device_name: Some("pseudo_camera".into()),
            optional_info: Default::default(),
            channels: vec![DeviceQueryChannel {
                channel_type: "color".into(),
                kind: DeviceType::Camera,
                available: true,
                channel_label: Some("Pseudo Camera".into()),
                default_name: Some("pseudo_camera".into()),
                modes: vec!["enabled".into(), "disabled".into()],
                profiles: ALL_PIXEL_FORMATS
                    .iter()
                    .flat_map(|&pf| {
                        [(640, 480), (1280, 720)]
                            .iter()
                            .map(move |&(w, h)| CameraChannelProfile {
                                width: w,
                                height: h,
                                fps: 30,
                                pixel_format: pf,
                                native_pixel_format: None,
                                mjpeg_quality: None,
                                h264_bitrate_bps: None,
                                h264_gop: None,
                                h264_preset: None,
                                h264_tune: None,
                                h264_profile: None,
                            })
                    })
                    .collect(),
                supported_states: Vec::new(),
                supported_commands: Vec::new(),
                supports_fk: false,
                supports_ik: false,
                dof: None,
                default_control_frequency_hz: None,
                direct_joint_compatibility: DirectJointCompatibility::default(),
                defaults: ChannelCommandDefaults::default(),
                value_limits: Vec::new(),
                optional_info: Default::default(),
            }],
        })
    } else if let Some(dof) = parse_robot_dof(id) {
        Some(DeviceQueryDevice {
            id: id.into(),
            device_class: "pseudo-robot".into(),
            device_label: if dof == 1 {
                "Pseudo End Effector".into()
            } else {
                "Pseudo Arm".into()
            },
            default_device_name: Some(if dof == 1 {
                "pseudo_eef".into()
            } else {
                "pseudo_arm".into()
            }),
            optional_info: Default::default(),
            channels: vec![DeviceQueryChannel {
                channel_type: "arm".into(),
                kind: DeviceType::Robot,
                available: true,
                channel_label: Some(if dof == 1 {
                    "Pseudo End Effector".into()
                } else {
                    "Pseudo Arm".into()
                }),
                default_name: Some(if dof == 1 {
                    "pseudo_eef".into()
                } else {
                    "pseudo_arm".into()
                }),
                modes: vec![
                    "free-drive".into(),
                    "command-following".into(),
                    "identifying".into(),
                    "disabled".into(),
                ],
                profiles: Vec::new(),
                supported_states: vec![
                    RobotStateKind::JointPosition,
                    RobotStateKind::JointVelocity,
                    RobotStateKind::JointEffort,
                    RobotStateKind::EndEffectorPose,
                ],
                supported_commands: vec![
                    RobotCommandKind::JointPosition,
                    RobotCommandKind::JointMit,
                ],
                supports_fk: true,
                supports_ik: false,
                dof: Some(dof),
                default_control_frequency_hz: Some(60.0),
                direct_joint_compatibility: DirectJointCompatibility {
                    can_lead: vec![DirectJointCompatibilityPeer {
                        driver: DRIVER_NAME.into(),
                        channel_type: "arm".into(),
                    }],
                    can_follow: vec![DirectJointCompatibilityPeer {
                        driver: DRIVER_NAME.into(),
                        channel_type: "arm".into(),
                    }],
                },
                defaults: ChannelCommandDefaults {
                    joint_mit_kp: vec![1.0; dof as usize],
                    joint_mit_kd: vec![0.1; dof as usize],
                    parallel_mit_kp: Vec::new(),
                    parallel_mit_kd: Vec::new(),
                },
                value_limits: pseudo_robot_value_limits(dof),
                optional_info: Default::default(),
            }],
        })
    } else {
        None
    }
}

/// Pseudo robot sweeps joints between -1 and 1 rad in `update_free_drive_state`,
/// so a ±π envelope keeps a comfortable margin while making the bars
/// meaningful. Velocity / effort use bounds matching the synthetic feedback
/// behaviour (small noise + sinusoid).
fn pseudo_robot_value_limits(dof: u32) -> Vec<StateValueLimitsEntry> {
    let dof = dof as usize;
    vec![
        StateValueLimitsEntry::symmetric(RobotStateKind::JointPosition, std::f64::consts::PI, dof),
        StateValueLimitsEntry::symmetric(RobotStateKind::JointVelocity, 1.0, dof),
        StateValueLimitsEntry::symmetric(RobotStateKind::JointEffort, 1.0, dof),
        StateValueLimitsEntry::new(
            RobotStateKind::EndEffectorPose,
            vec![-1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0],
            vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        ),
    ]
}

fn parse_robot_dof(id: &str) -> Option<u32> {
    let marker = "_dof_";
    let (_, tail) = id.rsplit_once(marker)?;
    tail.parse().ok()
}

fn print_query_human(response: &DeviceQueryResponse) {
    for device in &response.devices {
        println!("{} ({})", device.device_label, device.id);
        for channel in &device.channels {
            println!("  - {} [{}]", channel.channel_type, kind_name(channel.kind));
        }
    }
}

fn kind_name(kind: DeviceType) -> &'static str {
    match kind {
        DeviceType::Camera => "camera",
        DeviceType::Robot => "robot",
    }
}

fn unix_timestamp_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

fn generate_color_bars(buf: &mut [u8], width: u32, height: u32, frame_index: u64) {
    const COLORS: [(u8, u8, u8); 8] = [
        (255, 255, 255),
        (255, 255, 0),
        (0, 255, 255),
        (0, 255, 0),
        (255, 0, 255),
        (255, 0, 0),
        (0, 0, 255),
        (0, 0, 0),
    ];

    let w = width as usize;
    let h = height as usize;
    let bar_width = (w / COLORS.len()).max(1);
    let scroll = (frame_index as usize) % w.max(1);

    for y in 0..h {
        let row_offset = y * w * 3;
        for x in 0..w {
            let shifted = (x + scroll) % w;
            let bar_idx = (shifted / bar_width).min(COLORS.len() - 1);
            let (r, g, b) = COLORS[bar_idx];
            let px = row_offset + x * 3;
            buf[px] = r;
            buf[px + 1] = g;
            buf[px + 2] = b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yuyv_conversion_uses_rgb_scratch_and_exact_wire_payload() {
        let width = 640;
        let height = 480;
        let mut rgb = vec![0; rgb_frame_len(width, height)];
        let mut yuyv = vec![0; pixel_count(width, height) * 2];

        generate_color_bars(&mut rgb, width, height, 0);
        convert_rgb24_to_yuyv(&rgb, &mut yuyv, width, height);

        assert_eq!(rgb.len(), 640 * 480 * 3);
        assert_eq!(yuyv.len(), 640 * 480 * 2);
    }

    #[test]
    fn h264_annex_b_keeps_rgb_scratch_despite_variable_wire_payload() {
        let width = 640;
        let height = 480;
        let mut rgb = vec![0; rgb_frame_len(width, height)];

        assert_eq!(PixelFormat::H264AnnexB.bytes_per_pixel(), 0);
        assert_eq!(
            initial_slot_len(width, height, PixelFormat::H264AnnexB),
            rgb.len()
        );

        generate_color_bars(&mut rgb, width, height, 0);
        assert_eq!(rgb.len(), 640 * 480 * 3);
    }
}
