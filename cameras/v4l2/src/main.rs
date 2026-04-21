use clap::{Parser, Subcommand};
use iceoryx2::prelude::*;
use jpeg_decoder::{Decoder as JpegDecoder, PixelFormat as JpegPixelFormat};
use rollio_bus::{channel_frames_service_name, CONTROL_EVENTS_SERVICE};
use rollio_types::config::{
    BinaryDeviceConfig, CameraChannelProfile, DeviceQueryChannel, DeviceQueryDevice,
    DeviceQueryResponse, DeviceType,
};
use rollio_types::messages::{CameraFrameHeader, ControlEvent, PixelFormat};
use serde::Serialize;
use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::io::{self, Cursor};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use v4l::buffer::Type;
use v4l::capability::Flags as CapabilityFlags;
use v4l::format::{Format, FourCC};
use v4l::frameinterval::FrameIntervalEnum;
use v4l::framesize::FrameSizeEnum;
use v4l::io::mmap::Stream as MmapStream;
use v4l::io::traits::CaptureStream;
use v4l::prelude::Device;
use v4l::video::capture::Parameters as CaptureParameters;
use v4l::video::Capture;

const RGB3_BYTES: [u8; 4] = *b"RGB3";
const BGR3_BYTES: [u8; 4] = *b"BGR3";
const YUYV_BYTES: [u8; 4] = *b"YUYV";
const YUY2_BYTES: [u8; 4] = *b"YUY2";
const MJPG_BYTES: [u8; 4] = *b"MJPG";
const JPEG_BYTES: [u8; 4] = *b"JPEG";
const GREY_BYTES: [u8; 4] = *b"GREY";

type DynError = Box<dyn Error>;

#[derive(Parser)]
#[command(name = "rollio-device-v4l2")]
#[command(about = "V4L2 webcam driver for Rollio")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Probe {
        #[arg(long)]
        json: bool,
    },
    Validate {
        id: String,
        #[arg(long = "channel-type")]
        channel_types: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    Capabilities {
        path: String,
    },
    Query {
        id: String,
        #[arg(long)]
        json: bool,
    },
    Run {
        #[arg(long, value_name = "PATH", conflicts_with = "config_inline")]
        config: Option<PathBuf>,
        #[arg(long = "config-inline", value_name = "TOML", conflicts_with = "config")]
        config_inline: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Clone)]
struct RunConfig {
    bus_root: String,
    channel_type: String,
    id: String,
    width: u32,
    height: u32,
    fps: u32,
    pixel_format: PixelFormat,
    native_pixel_format: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct ConfiguredCapture {
    native_fourcc: FourCC,
    width: u32,
    height: u32,
    actual_fps: Option<f64>,
}

#[derive(Debug, Serialize)]
struct ProbeDevice {
    id: String,
    name: String,
    driver: &'static str,
    #[serde(rename = "type")]
    device_type: &'static str,
    bus: String,
    native_driver: String,
}

#[derive(Debug, Serialize)]
struct ValidateOutput {
    valid: bool,
    id: String,
    name: String,
    driver: &'static str,
    bus: String,
}

#[derive(Debug, Serialize)]
struct CapabilitiesOutput {
    id: String,
    name: String,
    driver: &'static str,
    bus: String,
    native_driver: String,
    profiles: Vec<CapabilityProfile>,
}

#[derive(Debug, Serialize)]
struct CapabilityProfile {
    native_pixel_format: String,
    width: u32,
    height: u32,
    fps: f64,
}

#[derive(Debug)]
struct FrameConverter {
    width: u32,
    height: u32,
    output_pixel_format: PixelFormat,
    scratch: Vec<u8>,
}

impl TryFrom<BinaryDeviceConfig> for RunConfig {
    type Error = DynError;

    fn try_from(device: BinaryDeviceConfig) -> Result<Self, Self::Error> {
        if device.driver != "v4l2" {
            return Err("v4l2 driver requires driver = \"v4l2\"".into());
        }
        let enabled_channels = device
            .channels
            .iter()
            .filter(|channel| channel.enabled)
            .collect::<Vec<_>>();
        if enabled_channels.len() != 1 {
            return Err("v4l2 driver requires exactly one enabled channel".into());
        }
        let channel = enabled_channels[0];
        if channel.kind != DeviceType::Camera {
            return Err("v4l2 driver requires a camera channel".into());
        }
        if channel.channel_type != "color" {
            return Err("v4l2 driver supports only channel_type=\"color\"".into());
        }
        let profile = channel
            .profile
            .as_ref()
            .ok_or("v4l2 driver requires a camera profile")?;
        let width = profile.width;
        let height = profile.height;
        let fps = profile.fps;
        let pixel_format = profile.pixel_format;

        match pixel_format {
            PixelFormat::Rgb24 | PixelFormat::Bgr24 => {}
            other => {
                return Err(format!(
                    "v4l2 driver requires pixel_format to be rgb24 or bgr24, got {}",
                    other.as_ref()
                )
                .into())
            }
        }

        Ok(Self {
            bus_root: device.bus_root,
            channel_type: channel.channel_type.clone(),
            id: device.id,
            width,
            height,
            fps,
            pixel_format,
            native_pixel_format: profile.native_pixel_format.clone(),
        })
    }
}

impl FrameConverter {
    fn new(width: u32, height: u32, output_pixel_format: PixelFormat) -> Result<Self, DynError> {
        let payload_len = payload_len(width, height)?;
        Ok(Self {
            width,
            height,
            output_pixel_format,
            scratch: vec![0; payload_len],
        })
    }

    fn convert(&mut self, frame_data: &[u8], native_fourcc: FourCC) -> Result<&[u8], DynError> {
        if native_fourcc.repr == RGB3_BYTES {
            self.copy_rgb_like(frame_data, false)
        } else if native_fourcc.repr == BGR3_BYTES {
            self.copy_rgb_like(frame_data, true)
        } else if native_fourcc.repr == YUYV_BYTES || native_fourcc.repr == YUY2_BYTES {
            self.convert_yuyv(frame_data)
        } else if native_fourcc.repr == MJPG_BYTES || native_fourcc.repr == JPEG_BYTES {
            self.decode_mjpeg(frame_data)
        } else if native_fourcc.repr == GREY_BYTES {
            self.expand_gray(frame_data)
        } else {
            Err(format!(
                "unsupported negotiated V4L2 pixel format {}",
                fourcc_to_string(native_fourcc)
            )
            .into())
        }
    }

    fn copy_rgb_like(&mut self, frame_data: &[u8], input_is_bgr: bool) -> Result<&[u8], DynError> {
        let expected_len = payload_len(self.width, self.height)?;
        if frame_data.len() < expected_len {
            return Err(format!(
                "frame payload too short for {}x{} RGB frame: expected at least {} bytes, got {}",
                self.width,
                self.height,
                expected_len,
                frame_data.len()
            )
            .into());
        }

        if (input_is_bgr && self.output_pixel_format == PixelFormat::Bgr24)
            || (!input_is_bgr && self.output_pixel_format == PixelFormat::Rgb24)
        {
            self.scratch.copy_from_slice(&frame_data[..expected_len]);
            return Ok(&self.scratch);
        }

        for (dst, chunk) in self
            .scratch
            .chunks_exact_mut(3)
            .zip(frame_data[..expected_len].chunks_exact(3))
        {
            dst[0] = chunk[2];
            dst[1] = chunk[1];
            dst[2] = chunk[0];
        }
        Ok(&self.scratch)
    }

    fn convert_yuyv(&mut self, frame_data: &[u8]) -> Result<&[u8], DynError> {
        let expected_len = self.width as usize * self.height as usize * 2;
        if frame_data.len() < expected_len {
            return Err(format!(
                "frame payload too short for {}x{} YUYV frame: expected at least {} bytes, got {}",
                self.width,
                self.height,
                expected_len,
                frame_data.len()
            )
            .into());
        }

        for (dst, chunk) in self
            .scratch
            .chunks_exact_mut(6)
            .zip(frame_data[..expected_len].chunks_exact(4))
        {
            let y0 = chunk[0];
            let u = chunk[1];
            let y1 = chunk[2];
            let v = chunk[3];

            let [r0, g0, b0] = yuv_to_rgb(y0, u, v);
            let [r1, g1, b1] = yuv_to_rgb(y1, u, v);

            if self.output_pixel_format == PixelFormat::Rgb24 {
                dst.copy_from_slice(&[r0, g0, b0, r1, g1, b1]);
            } else {
                dst.copy_from_slice(&[b0, g0, r0, b1, g1, r1]);
            }
        }

        Ok(&self.scratch)
    }

    fn decode_mjpeg(&mut self, frame_data: &[u8]) -> Result<&[u8], DynError> {
        let mut decoder = JpegDecoder::new(Cursor::new(frame_data));
        let pixels = decoder.decode()?;
        let info = decoder
            .info()
            .ok_or("mjpeg decoder did not report image metadata")?;

        if info.width as u32 != self.width || info.height as u32 != self.height {
            return Err(format!(
                "decoded MJPEG frame dimensions {}x{} do not match configured {}x{}",
                info.width, info.height, self.width, self.height
            )
            .into());
        }

        match info.pixel_format {
            JpegPixelFormat::RGB24 => {
                if pixels.len() != self.scratch.len() {
                    return Err("decoded RGB MJPEG frame length mismatch".into());
                }
                if self.output_pixel_format == PixelFormat::Rgb24 {
                    self.scratch.copy_from_slice(&pixels);
                } else {
                    for (dst, chunk) in self.scratch.chunks_exact_mut(3).zip(pixels.chunks_exact(3))
                    {
                        dst[0] = chunk[2];
                        dst[1] = chunk[1];
                        dst[2] = chunk[0];
                    }
                }
                Ok(&self.scratch)
            }
            JpegPixelFormat::L8 => {
                if pixels.len() != self.width as usize * self.height as usize {
                    return Err("decoded grayscale MJPEG frame length mismatch".into());
                }
                for (dst, value) in self.scratch.chunks_exact_mut(3).zip(pixels.iter().copied()) {
                    dst[0] = value;
                    dst[1] = value;
                    dst[2] = value;
                }
                Ok(&self.scratch)
            }
            other => Err(format!("unsupported MJPEG decoder pixel format: {other:?}").into()),
        }
    }

    fn expand_gray(&mut self, frame_data: &[u8]) -> Result<&[u8], DynError> {
        let expected_len = self.width as usize * self.height as usize;
        if frame_data.len() < expected_len {
            return Err(format!(
                "frame payload too short for {}x{} grayscale frame: expected at least {} bytes, got {}",
                self.width,
                self.height,
                expected_len,
                frame_data.len()
            )
            .into());
        }

        for (dst, value) in self
            .scratch
            .chunks_exact_mut(3)
            .zip(frame_data[..expected_len].iter().copied())
        {
            dst[0] = value;
            dst[1] = value;
            dst[2] = value;
        }

        Ok(&self.scratch)
    }
}

fn main() {
    if let Err(error) = run_cli() {
        eprintln!("rollio-device-v4l2: {error}");
        std::process::exit(1);
    }
}

fn run_cli() -> Result<(), DynError> {
    let cli = Cli::parse();
    match cli.command {
        Command::Probe { json } => {
            let devices = probe_devices()?;
            if json {
                let ids = devices
                    .iter()
                    .map(|device| device.id.clone())
                    .collect::<Vec<_>>();
                println!("{}", serde_json::to_string_pretty(&ids)?);
            } else if devices.is_empty() {
                println!("no v4l2 devices discovered");
            } else {
                for device in &devices {
                    println!("{} ({})", device.name, device.id);
                }
            }
        }
        Command::Validate {
            id,
            channel_types,
            json,
        } => {
            let output = validate_device(&id, &channel_types)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else if output.valid {
                println!("{} is valid", output.id);
            } else {
                println!("{} is invalid", output.id);
            }
        }
        Command::Capabilities { path } => {
            let output = capabilities_for_device(&path)?;
            println!("{}", serde_json::to_string(&output)?);
        }
        Command::Query { id, json } => {
            let output = query_device(&id)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                print_query_human(&output);
            }
        }
        Command::Run {
            config,
            config_inline,
            dry_run,
        } => {
            let config = load_run_config(config, config_inline)?;
            if !dry_run {
                run_camera(config)?;
            }
        }
    }

    Ok(())
}

fn load_run_config(
    config_path: Option<PathBuf>,
    config_inline: Option<String>,
) -> Result<RunConfig, DynError> {
    let text = match (config_path, config_inline) {
        (Some(path), None) => fs::read_to_string(path)?,
        (None, Some(text)) => text,
        _ => return Err("run requires exactly one of --config or --config-inline".into()),
    };

    let device: BinaryDeviceConfig = text.parse()?;
    RunConfig::try_from(device)
}

fn probe_devices() -> Result<Vec<ProbeDevice>, DynError> {
    let mut devices = Vec::new();
    for path in list_video_devices()? {
        let Ok(device) = Device::with_path(&path) else {
            continue;
        };
        let Ok(caps) = device.query_caps() else {
            continue;
        };
        if !is_capture_capable(caps.capabilities) {
            continue;
        }
        if is_likely_realsense(&caps) {
            continue;
        }

        devices.push(ProbeDevice {
            id: path.clone(),
            name: caps.card,
            driver: "v4l2",
            device_type: "camera",
            bus: caps.bus,
            native_driver: caps.driver,
        });
    }

    Ok(devices)
}

fn validate_device(path: &str, channel_types: &[String]) -> Result<ValidateOutput, DynError> {
    let (_device, caps) = open_capture_device(path)?;
    if channel_types
        .iter()
        .any(|channel_type| channel_type != "color")
    {
        return Ok(ValidateOutput {
            valid: false,
            id: path.to_string(),
            name: caps.card,
            driver: "v4l2",
            bus: caps.bus,
        });
    }
    Ok(ValidateOutput {
        valid: true,
        id: path.to_string(),
        name: caps.card,
        driver: "v4l2",
        bus: caps.bus,
    })
}

fn query_device(path: &str) -> Result<DeviceQueryResponse, DynError> {
    let capabilities = capabilities_for_device(path)?;
    let profiles = capabilities
        .profiles
        .into_iter()
        .map(|profile| CameraChannelProfile {
            width: profile.width,
            height: profile.height,
            fps: profile.fps.round() as u32,
            pixel_format: match profile.native_pixel_format.as_str() {
                "BGR3" => PixelFormat::Bgr24,
                _ => PixelFormat::Rgb24,
            },
            native_pixel_format: Some(profile.native_pixel_format),
        })
        .collect::<Vec<_>>();
    Ok(DeviceQueryResponse {
        driver: "v4l2".into(),
        devices: vec![DeviceQueryDevice {
            id: path.to_string(),
            device_class: "v4l2".into(),
            device_label: capabilities.name,
            default_device_name: Some("camera".into()),
            optional_info: Default::default(),
            channels: vec![DeviceQueryChannel {
                channel_type: "color".into(),
                kind: DeviceType::Camera,
                available: true,
                channel_label: Some("V4L2 Camera".into()),
                default_name: Some("camera".into()),
                modes: vec!["enabled".into(), "disabled".into()],
                profiles,
                supported_states: Vec::new(),
                supported_commands: Vec::new(),
                supports_fk: false,
                supports_ik: false,
                dof: None,
                default_control_frequency_hz: None,
                direct_joint_compatibility: Default::default(),
                defaults: Default::default(),
                value_limits: Vec::new(),
                optional_info: Default::default(),
            }],
        }],
    })
}

fn print_query_human(output: &DeviceQueryResponse) {
    for device in &output.devices {
        println!("{} ({})", device.device_label, device.id);
        for channel in &device.channels {
            println!("  - {} [camera]", channel.channel_type);
        }
    }
}

fn capabilities_for_device(path: &str) -> Result<CapabilitiesOutput, DynError> {
    let (device, caps) = open_capture_device(path)?;
    let mut profiles = BTreeSet::new();

    if let Ok(formats) = device.enum_formats() {
        for format in formats {
            let Ok(frame_sizes) = device.enum_framesizes(format.fourcc) else {
                continue;
            };
            for (width, height) in candidate_sizes(&frame_sizes) {
                for fps_milli in candidate_fps(&device, format.fourcc, width, height) {
                    profiles.insert((fourcc_to_string(format.fourcc), width, height, fps_milli));
                }
            }
        }
    }

    if profiles.is_empty() {
        let current_format = device.format()?;
        if let Some(fps_milli) = device.params().ok().and_then(|params| {
            fraction_to_fps_milli(params.interval.numerator, params.interval.denominator)
        }) {
            profiles.insert((
                fourcc_to_string(current_format.fourcc),
                current_format.width,
                current_format.height,
                fps_milli,
            ));
        }
    }

    Ok(CapabilitiesOutput {
        id: path.to_string(),
        name: caps.card,
        driver: "v4l2",
        bus: caps.bus,
        native_driver: caps.driver,
        profiles: profiles
            .into_iter()
            .map(
                |(native_pixel_format, width, height, fps_milli)| CapabilityProfile {
                    native_pixel_format,
                    width,
                    height,
                    fps: fps_milli as f64 / 1000.0,
                },
            )
            .collect(),
    })
}

fn run_camera(config: RunConfig) -> Result<(), DynError> {
    let (device, caps) = open_capture_device(&config.id)?;
    let configured = configure_capture(&device, &config)?;
    let payload_len = payload_len(configured.width, configured.height)?;

    let node = NodeBuilder::new().create::<ipc::Service>()?;
    let frame_service_name = channel_frames_service_name(&config.bus_root, &config.channel_type);
    let frame_service_name: ServiceName = frame_service_name.as_str().try_into()?;
    let frame_service = node
        .service_builder(&frame_service_name)
        .publish_subscribe::<[u8]>()
        .user_header::<CameraFrameHeader>()
        .open_or_create()?;
    let publisher = frame_service
        .publisher_builder()
        .initial_max_slice_len(payload_len)
        .allocation_strategy(AllocationStrategy::PowerOfTwo)
        .create()?;

    let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
    let control_service = node
        .service_builder(&control_service_name)
        .publish_subscribe::<ControlEvent>()
        .open_or_create()?;
    let control_subscriber = control_service.subscriber_builder().create()?;

    let mut stream = MmapStream::with_buffers(&device, Type::VideoCapture, 4)?;
    stream.set_timeout(Duration::from_millis(1000));

    let mut converter =
        FrameConverter::new(configured.width, configured.height, config.pixel_format)?;
    let mut frame_index = 0u64;
    let mut last_status_log = Instant::now();
    let mut last_timeout_log = Instant::now() - Duration::from_secs(5);
    // V4L2 timestamps frames against `CLOCK_MONOTONIC` (the kernel default
    // for capture buffers). Convert each frame's monotonic timestamp into
    // UNIX-epoch microseconds via a periodically re-sampled boot offset so
    // the published values are comparable to every other Rollio publisher.
    let mut clock_offset = MonotonicToUnixOffset::new();
    let mut last_offset_refresh = Instant::now();
    // Some USB webcams (e.g. Logitech C270) occasionally deliver short or
    // otherwise malformed frames after USB hiccups. Drop those frames
    // instead of aborting the whole driver, but rate-limit the log line
    // so the recording session is not killed by a transient hardware
    // glitch.
    let mut dropped_malformed_frames: u64 = 0;
    let mut last_malformed_log = Instant::now() - Duration::from_secs(5);

    eprintln!(
        "rollio-device-v4l2: device={} card={} native_format={} output_format={} size={}x{} fps_request={} fps_actual={}",
        config.id,
        caps.card,
        fourcc_to_string(configured.native_fourcc),
        config.pixel_format.as_ref(),
        configured.width,
        configured.height,
        config.fps,
        configured
            .actual_fps
            .map(|fps| format!("{fps:.3}"))
            .unwrap_or_else(|| "unknown".to_string())
    );

    loop {
        if drain_control_events(&control_subscriber)? {
            eprintln!(
                "rollio-device-v4l2: shutdown received for {}",
                config.bus_root
            );
            return Ok(());
        }

        let (buffer, metadata) = match stream.next() {
            Ok(frame) => frame,
            Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                if last_timeout_log.elapsed() >= Duration::from_secs(1) {
                    eprintln!(
                        "rollio-device-v4l2: device={} waiting for next frame after timeout",
                        config.id
                    );
                    last_timeout_log = Instant::now();
                }
                continue;
            }
            Err(error) => return Err(error.into()),
        };

        let bytes_used = metadata.bytesused as usize;
        if bytes_used == 0 {
            continue;
        }
        let frame_data = match buffer.get(..bytes_used) {
            Some(slice) => slice,
            None => {
                dropped_malformed_frames += 1;
                if last_malformed_log.elapsed() >= Duration::from_secs(1) {
                    eprintln!(
                        "rollio-device-v4l2: device={} dropping malformed frame: \
                         driver reported bytesused={} larger than mapped buffer={} \
                         (total_dropped={})",
                        config.id,
                        bytes_used,
                        buffer.len(),
                        dropped_malformed_frames,
                    );
                    last_malformed_log = Instant::now();
                }
                continue;
            }
        };
        let converted = match converter.convert(frame_data, configured.native_fourcc) {
            Ok(buf) => buf,
            Err(error) => {
                dropped_malformed_frames += 1;
                if last_malformed_log.elapsed() >= Duration::from_secs(1) {
                    eprintln!(
                        "rollio-device-v4l2: device={} dropping malformed frame: {} \
                         (total_dropped={})",
                        config.id, error, dropped_malformed_frames,
                    );
                    last_malformed_log = Instant::now();
                }
                continue;
            }
        };

        // Refresh the boot offset roughly once per second so NTP slew
        // does not silently drift the per-frame UNIX timestamps.
        if last_offset_refresh.elapsed() >= Duration::from_secs(1) {
            clock_offset.refresh();
            last_offset_refresh = Instant::now();
        }
        let timestamp_us = clock_offset
            .timeval_to_unix_us(metadata.timestamp.sec, metadata.timestamp.usec)
            .unwrap_or_else(|| wallclock_timestamp_us().unwrap_or(0));
        let mut sample = publisher.loan_slice_uninit(payload_len)?;
        *sample.user_header_mut() = CameraFrameHeader {
            timestamp_us,
            width: configured.width,
            height: configured.height,
            pixel_format: config.pixel_format,
            frame_index,
        };
        let sample = sample.write_from_slice(converted);
        sample.send()?;

        frame_index += 1;
        if last_status_log.elapsed() >= Duration::from_secs(1) {
            eprintln!(
                "rollio-device-v4l2: device={} frame_index={} latest_timestamp_us={} \
                 dropped_malformed={} active=true",
                config.id, frame_index, timestamp_us, dropped_malformed_frames,
            );
            last_status_log = Instant::now();
        }
    }
}

fn configure_capture(device: &Device, config: &RunConfig) -> Result<ConfiguredCapture, DynError> {
    let formats = device.enum_formats()?;
    let supported_formats: Vec<FourCC> = formats.iter().map(|format| format.fourcc).collect();
    let requested_formats = preferred_native_formats_for_config(config);
    let mut last_mismatch = None;

    for candidate in requested_formats.iter().copied() {
        if !supported_formats.contains(&candidate) {
            continue;
        }

        let requested = Format::new(config.width, config.height, candidate);
        let active = device.set_format(&requested)?;
        if active.width != config.width || active.height != config.height {
            last_mismatch = Some(format!(
                "device negotiated {}x{} for native format {} instead of requested {}x{}",
                active.width,
                active.height,
                fourcc_to_string(active.fourcc),
                config.width,
                config.height
            ));
            continue;
        }

        if !is_supported_native_format(active.fourcc) {
            last_mismatch = Some(format!(
                "device negotiated unsupported native format {}",
                fourcc_to_string(active.fourcc)
            ));
            continue;
        }

        let actual_params = match device.set_params(&CaptureParameters::with_fps(config.fps)) {
            Ok(params) => Some(params),
            Err(_) => device.params().ok(),
        };
        let actual_fps = actual_params.and_then(|params| {
            fraction_to_f64(params.interval.numerator, params.interval.denominator)
        });

        return Ok(ConfiguredCapture {
            native_fourcc: active.fourcc,
            width: active.width,
            height: active.height,
            actual_fps,
        });
    }

    Err(last_mismatch
        .unwrap_or_else(|| {
            format!(
                "device does not advertise a supported native V4L2 capture format for RGB conversion; available formats: {}",
                supported_formats
                    .iter()
                    .copied()
                    .map(fourcc_to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
        .into())
}

fn open_capture_device(path: &str) -> Result<(Device, v4l::Capabilities), DynError> {
    let device = Device::with_path(path)?;
    let caps = device.query_caps()?;
    if !is_capture_capable(caps.capabilities) {
        return Err(format!("{path} is not a V4L2 capture device").into());
    }
    if is_likely_realsense(&caps) {
        return Err(format!(
            "{path} appears to belong to a RealSense camera; use the dedicated realsense driver instead"
        )
        .into());
    }
    Ok((device, caps))
}

fn list_video_devices() -> Result<Vec<String>, DynError> {
    let mut devices = fs::read_dir("/dev")?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if !file_name.starts_with("video") {
                return None;
            }
            Some(entry.path().display().to_string())
        })
        .collect::<Vec<_>>();
    devices.sort();
    Ok(devices)
}

fn is_capture_capable(flags: CapabilityFlags) -> bool {
    flags.intersects(CapabilityFlags::VIDEO_CAPTURE | CapabilityFlags::VIDEO_CAPTURE_MPLANE)
}

fn is_likely_realsense(caps: &v4l::Capabilities) -> bool {
    caps.card.to_ascii_lowercase().contains("realsense")
        || caps.driver.to_ascii_lowercase().contains("realsense")
}

fn preferred_native_formats(output_pixel_format: PixelFormat) -> Vec<FourCC> {
    match output_pixel_format {
        PixelFormat::Rgb24 => vec![
            FourCC::new(&RGB3_BYTES),
            FourCC::new(&BGR3_BYTES),
            FourCC::new(&YUYV_BYTES),
            FourCC::new(&YUY2_BYTES),
            FourCC::new(&MJPG_BYTES),
            FourCC::new(&JPEG_BYTES),
            FourCC::new(&GREY_BYTES),
        ],
        PixelFormat::Bgr24 => vec![
            FourCC::new(&BGR3_BYTES),
            FourCC::new(&RGB3_BYTES),
            FourCC::new(&YUYV_BYTES),
            FourCC::new(&YUY2_BYTES),
            FourCC::new(&MJPG_BYTES),
            FourCC::new(&JPEG_BYTES),
            FourCC::new(&GREY_BYTES),
        ],
        _ => Vec::new(),
    }
}

fn preferred_native_formats_for_config(config: &RunConfig) -> Vec<FourCC> {
    if let Some(native_pixel_format) = config.native_pixel_format.as_deref() {
        if let Some(fourcc) = native_fourcc_from_name(native_pixel_format) {
            let mut formats = vec![fourcc];
            formats.extend(
                preferred_native_formats(config.pixel_format)
                    .into_iter()
                    .filter(|candidate| *candidate != fourcc),
            );
            return formats;
        }
    }
    preferred_native_formats(config.pixel_format)
}

fn native_fourcc_from_name(name: &str) -> Option<FourCC> {
    match name.trim().to_ascii_uppercase().as_str() {
        "RGB3" => Some(FourCC::new(&RGB3_BYTES)),
        "BGR3" => Some(FourCC::new(&BGR3_BYTES)),
        "YUYV" => Some(FourCC::new(&YUYV_BYTES)),
        "YUY2" => Some(FourCC::new(&YUY2_BYTES)),
        "MJPG" => Some(FourCC::new(&MJPG_BYTES)),
        "JPEG" => Some(FourCC::new(&JPEG_BYTES)),
        "GREY" => Some(FourCC::new(&GREY_BYTES)),
        _ => None,
    }
}

fn is_supported_native_format(fourcc: FourCC) -> bool {
    matches!(
        fourcc.repr,
        RGB3_BYTES | BGR3_BYTES | YUYV_BYTES | YUY2_BYTES | MJPG_BYTES | JPEG_BYTES | GREY_BYTES
    )
}

fn payload_len(width: u32, height: u32) -> Result<usize, DynError> {
    let pixels = width as usize * height as usize;
    pixels
        .checked_mul(3)
        .ok_or_else(|| "frame payload size overflow".into())
}

fn wallclock_timestamp_us() -> Result<u64, DynError> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_micros()
        .try_into()?)
}

/// Tracks a `CLOCK_MONOTONIC` -> UNIX-epoch offset (microseconds).
///
/// V4L2 stamps capture buffers using `CLOCK_MONOTONIC` (the kernel default
/// for `V4L2_BUF_FLAG_TIMESTAMP_MONOTONIC`). To convert to a UNIX-epoch
/// microsecond value we sample `(SystemTime::now(), CLOCK_MONOTONIC now)`
/// and apply the difference. The offset is re-sampled periodically by the
/// caller to absorb NTP slew.
#[derive(Debug)]
struct MonotonicToUnixOffset {
    offset_us: i128,
}

impl MonotonicToUnixOffset {
    fn new() -> Self {
        let mut this = Self { offset_us: 0 };
        this.refresh();
        this
    }

    fn refresh(&mut self) {
        let unix_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as i128)
            .unwrap_or(0);
        let monotonic_us = read_monotonic_us();
        self.offset_us = unix_us - monotonic_us;
    }

    fn timeval_to_unix_us(&self, sec: i64, usec: i64) -> Option<u64> {
        if sec == 0 && usec == 0 {
            return None;
        }
        let monotonic_us = (sec as i128) * 1_000_000 + (usec as i128);
        let unix_us = monotonic_us + self.offset_us;
        u64::try_from(unix_us.max(0)).ok()
    }
}

fn read_monotonic_us() -> i128 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime is async-signal-safe and writes only into the
    // provided timespec.
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    if rc != 0 {
        return 0;
    }
    (ts.tv_sec as i128) * 1_000_000 + (ts.tv_nsec as i128) / 1_000
}

fn fourcc_to_string(fourcc: FourCC) -> String {
    fourcc
        .str()
        .map(str::to_string)
        .unwrap_or_else(|_| format!("{:?}", fourcc.repr))
}

fn fraction_to_f64(numerator: u32, denominator: u32) -> Option<f64> {
    if numerator == 0 || denominator == 0 {
        return None;
    }
    Some(denominator as f64 / numerator as f64)
}

fn fraction_to_fps_milli(numerator: u32, denominator: u32) -> Option<u32> {
    fraction_to_f64(numerator, denominator).map(|fps| (fps * 1000.0).round() as u32)
}

fn candidate_fps(device: &Device, fourcc: FourCC, width: u32, height: u32) -> Vec<u32> {
    let Ok(intervals) = device.enum_frameintervals(fourcc, width, height) else {
        return Vec::new();
    };

    let mut fps = BTreeSet::new();
    for interval in intervals {
        match interval.interval {
            FrameIntervalEnum::Discrete(discrete) => {
                if let Some(fps_milli) =
                    fraction_to_fps_milli(discrete.numerator, discrete.denominator)
                {
                    fps.insert(fps_milli);
                }
            }
            FrameIntervalEnum::Stepwise(stepwise) => {
                for fraction in [stepwise.min, stepwise.max] {
                    if let Some(fps_milli) =
                        fraction_to_fps_milli(fraction.numerator, fraction.denominator)
                    {
                        fps.insert(fps_milli);
                    }
                }
            }
        }
    }

    fps.into_iter().collect()
}

fn candidate_sizes(frame_sizes: &[v4l::FrameSize]) -> Vec<(u32, u32)> {
    let mut sizes = Vec::new();
    for frame_size in frame_sizes {
        match &frame_size.size {
            FrameSizeEnum::Discrete(discrete) => {
                push_size(&mut sizes, discrete.width, discrete.height)
            }
            FrameSizeEnum::Stepwise(stepwise) => {
                let width_step = stepwise.step_width.max(1);
                let height_step = stepwise.step_height.max(1);
                let width_count =
                    ((stepwise.max_width - stepwise.min_width) / width_step + 1) as u64;
                let height_count =
                    ((stepwise.max_height - stepwise.min_height) / height_step + 1) as u64;
                if width_count.saturating_mul(height_count) <= 64 {
                    let mut width = stepwise.min_width;
                    while width <= stepwise.max_width {
                        let mut height = stepwise.min_height;
                        while height <= stepwise.max_height {
                            push_size(&mut sizes, width, height);
                            match height.checked_add(height_step) {
                                Some(next) if next > height => height = next,
                                _ => break,
                            }
                        }
                        match width.checked_add(width_step) {
                            Some(next) if next > width => width = next,
                            _ => break,
                        }
                    }
                } else {
                    push_size(&mut sizes, stepwise.min_width, stepwise.min_height);
                    push_size(&mut sizes, stepwise.max_width, stepwise.max_height);
                }
            }
        }
    }
    sizes
}

fn push_size(sizes: &mut Vec<(u32, u32)>, width: u32, height: u32) {
    if !sizes.iter().any(|(w, h)| *w == width && *h == height) {
        sizes.push((width, height));
    }
}

fn drain_control_events(
    subscriber: &iceoryx2::port::subscriber::Subscriber<ipc::Service, ControlEvent, ()>,
) -> Result<bool, DynError> {
    loop {
        match subscriber.receive()? {
            Some(sample) if matches!(*sample.payload(), ControlEvent::Shutdown) => return Ok(true),
            Some(_) => {}
            None => return Ok(false),
        }
    }
}

fn yuv_to_rgb(y: u8, u: u8, v: u8) -> [u8; 3] {
    let c = y as i32 - 16;
    let d = u as i32 - 128;
    let e = v as i32 - 128;

    let r = clamp_to_u8((298 * c + 409 * e + 128) >> 8);
    let g = clamp_to_u8((298 * c - 100 * d - 208 * e + 128) >> 8);
    let b = clamp_to_u8((298 * c + 516 * d + 128) >> 8);
    [r, g, b]
}

fn clamp_to_u8(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

trait PixelFormatExt {
    fn as_ref(&self) -> &'static str;
}

impl PixelFormatExt for PixelFormat {
    fn as_ref(&self) -> &'static str {
        match self {
            PixelFormat::Rgb24 => "rgb24",
            PixelFormat::Bgr24 => "bgr24",
            PixelFormat::Yuyv => "yuyv",
            PixelFormat::Mjpeg => "mjpeg",
            PixelFormat::Depth16 => "depth16",
            PixelFormat::Gray8 => "gray8",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_run_config(text: &str) -> Result<RunConfig, DynError> {
        let device: BinaryDeviceConfig = text.parse()?;
        RunConfig::try_from(device)
    }

    #[test]
    fn run_config_requires_rgb_or_bgr_output() {
        let config = r#"
name = "cam"
driver = "v4l2"
id = "/dev/video0"
bus_root = "cam"

[[channels]]
channel_type = "color"
kind = "camera"
profile = { width = 640, height = 480, fps = 30, pixel_format = "mjpeg" }
"#;

        let error = parse_run_config(config).expect_err("mjpeg output should be rejected");
        assert!(
            error.to_string().contains("rgb24 or bgr24"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn run_config_allows_bgr24_output() {
        let config = r#"
name = "cam"
driver = "v4l2"
id = "/dev/video0"
bus_root = "cam"

[[channels]]
channel_type = "color"
kind = "camera"
profile = { width = 640, height = 480, fps = 30, pixel_format = "bgr24" }
"#;

        let parsed = parse_run_config(config).expect("bgr24 config should parse");
        assert_eq!(parsed.pixel_format, PixelFormat::Bgr24);
    }

    #[test]
    fn yuyv_conversion_matches_expected_grayscale_extremes() {
        let mut converter =
            FrameConverter::new(2, 1, PixelFormat::Rgb24).expect("converter should initialize");
        let converted = converter
            .convert(&[16, 128, 235, 128], FourCC::new(&YUYV_BYTES))
            .expect("YUYV should convert");

        assert_eq!(converted, &[0, 0, 0, 255, 255, 255]);
    }

    #[test]
    fn bgr_input_converts_to_rgb_output() {
        let mut converter =
            FrameConverter::new(1, 1, PixelFormat::Rgb24).expect("converter should initialize");
        let converted = converter
            .convert(&[10, 20, 30], FourCC::new(&BGR3_BYTES))
            .expect("BGR frame should convert");

        assert_eq!(converted, &[30, 20, 10]);
    }

    #[test]
    fn monotonic_to_unix_offset_returns_none_for_zero_timeval() {
        let offset = MonotonicToUnixOffset { offset_us: 1_000 };
        assert_eq!(offset.timeval_to_unix_us(0, 0), None);
    }

    #[test]
    fn monotonic_to_unix_offset_adds_offset_to_monotonic() {
        // Synthetic offset so the math is checkable without sampling clocks.
        let offset = MonotonicToUnixOffset {
            offset_us: 1_700_000_000_000_000, // ~2023-11-15 UTC, in us
        };
        // monotonic = 5.5 seconds since boot
        let unix_us = offset
            .timeval_to_unix_us(5, 500_000)
            .expect("non-zero timeval should produce a value");
        assert_eq!(unix_us, 1_700_000_000_000_000 + 5_500_000);
    }

    #[test]
    fn read_monotonic_us_is_monotonic_nondecreasing() {
        let a = read_monotonic_us();
        let b = read_monotonic_us();
        assert!(b >= a);
    }

    #[test]
    fn realsense_devices_are_detected_for_filtering() {
        let caps = v4l::Capabilities {
            driver: "uvcvideo".into(),
            card: "Intel(R) RealSense(TM) Depth Camera".into(),
            bus: "usb-1".into(),
            version: (1, 0, 0),
            capabilities: CapabilityFlags::VIDEO_CAPTURE,
        };

        assert!(is_likely_realsense(&caps));
    }
}
