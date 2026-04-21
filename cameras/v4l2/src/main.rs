use clap::{Parser, Subcommand};
use iceoryx2::prelude::*;
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
use std::io;
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

/// Converts (or passes through) V4L2 mmap buffers into the configured bus
/// pixel format. The driver no longer JPEG-decodes or YUV-converts on the
/// camera side: those formats are now published verbatim and decoded by
/// the encoder, which uses libavcodec (orders of magnitude faster than the
/// previous pure-Rust paths). Only the cheap RGB byte-swap and grayscale
/// expansion remain, used when an RGB-native camera is configured to
/// publish a different RGB ordering or when an IR sensor publishes
/// `gray8` over `rgb24`.
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
            PixelFormat::Rgb24 | PixelFormat::Bgr24 | PixelFormat::Yuyv | PixelFormat::Mjpeg => {}
            other => {
                return Err(format!(
                    "v4l2 driver requires pixel_format to be one of \
                     rgb24, bgr24, yuyv, mjpeg; got {}",
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

/// One frame from V4L2, ready to publish on the bus. The slice may either
/// reference the V4L2 mmap buffer directly (passthrough fast path) or the
/// converter's internal `scratch` buffer (cheap byte swap / GRAY expansion).
#[derive(Debug)]
enum ConvertedFrame<'a> {
    /// The V4L2 native format already matches the configured bus format —
    /// publish the mmap slice verbatim. Used for YUYV→YUYV, MJPG→MJPG,
    /// RGB→RGB, BGR→BGR and GRAY→GRAY (although GRAY is not currently a
    /// supported bus format).
    Passthrough(&'a [u8]),
    /// One of the cheap RGB-byte-swap or GRAY8→RGB24 expansion paths
    /// produced this slice.
    Converted(&'a [u8]),
}

impl ConvertedFrame<'_> {
    fn as_slice(&self) -> &[u8] {
        match self {
            ConvertedFrame::Passthrough(slice) | ConvertedFrame::Converted(slice) => slice,
        }
    }
}

impl FrameConverter {
    fn new(width: u32, height: u32, output_pixel_format: PixelFormat) -> Result<Self, DynError> {
        // The scratch buffer only holds RGB-like output (RGB24 or BGR24).
        // YUYV / MJPG paths use passthrough and never touch this buffer.
        let scratch_len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(3))
            .ok_or("frame payload size overflow")?;
        Ok(Self {
            width,
            height,
            output_pixel_format,
            scratch: vec![0; scratch_len],
        })
    }

    /// Map a V4L2 native fourcc to its corresponding bus `PixelFormat`, if
    /// any. Used to detect when the bus format matches the V4L2 native
    /// format so we can take the passthrough fast path.
    fn fourcc_to_bus_format(fourcc: FourCC) -> Option<PixelFormat> {
        match fourcc.repr {
            RGB3_BYTES => Some(PixelFormat::Rgb24),
            BGR3_BYTES => Some(PixelFormat::Bgr24),
            YUYV_BYTES | YUY2_BYTES => Some(PixelFormat::Yuyv),
            MJPG_BYTES | JPEG_BYTES => Some(PixelFormat::Mjpeg),
            // V4L2 GREY (Y8) maps to bus `Gray8`, but no current driver
            // configuration publishes Gray8 over the V4L2 bus topic — IR
            // streams come from RealSense.
            _ => None,
        }
    }

    /// Returns true when the per-frame converter can produce `bus_format`
    /// from a V4L2 buffer in `native_fourcc`. Either the formats match
    /// (passthrough fast path) or we have one of the cheap conversions
    /// (RGB<->BGR memcpy, GREY->RGB expansion). Used by `configure_capture`
    /// to fail at startup instead of dropping every frame at runtime.
    fn can_publish_native_as(native_fourcc: FourCC, bus_format: PixelFormat) -> bool {
        if Self::fourcc_to_bus_format(native_fourcc) == Some(bus_format) {
            return true;
        }
        matches!(
            (native_fourcc.repr, bus_format),
            (RGB3_BYTES, PixelFormat::Bgr24)
                | (BGR3_BYTES, PixelFormat::Rgb24)
                | (GREY_BYTES, PixelFormat::Rgb24)
                | (GREY_BYTES, PixelFormat::Bgr24)
        )
    }

    fn convert<'a>(
        &'a mut self,
        frame_data: &'a [u8],
        native_fourcc: FourCC,
    ) -> Result<ConvertedFrame<'a>, DynError> {
        // Fast path: V4L2 native already matches the bus format; publish
        // the mmap buffer verbatim. The encoder (or any other subscriber)
        // is responsible for decoding YUYV/MJPG when needed.
        if Self::fourcc_to_bus_format(native_fourcc) == Some(self.output_pixel_format) {
            return Ok(ConvertedFrame::Passthrough(frame_data));
        }

        // Cheap converters: only RGB↔BGR swaps and GREY→RGB expansion are
        // kept on the camera side. Everything else (YUYV→RGB, JPEG decode)
        // moved to the encoder so we don't burn CPU on the hot capture
        // thread.
        match (native_fourcc.repr, self.output_pixel_format) {
            (RGB3_BYTES, PixelFormat::Bgr24) => self.copy_rgb_swapped(frame_data),
            (BGR3_BYTES, PixelFormat::Rgb24) => self.copy_rgb_swapped(frame_data),
            (GREY_BYTES, PixelFormat::Rgb24) | (GREY_BYTES, PixelFormat::Bgr24) => {
                self.expand_gray(frame_data)
            }
            (native, _) => Err(format!(
                "v4l2 native pixel format {} cannot be converted to bus format {} on the camera side. \
                 Set `pixel_format = \"yuyv\"` or `pixel_format = \"mjpeg\"` so the raw frames \
                 are published verbatim and decoded downstream by the encoder.",
                fourcc_to_string(FourCC::new(&native)),
                self.output_pixel_format.as_ref()
            )
            .into()),
        }
    }

    fn copy_rgb_swapped<'a>(
        &'a mut self,
        frame_data: &[u8],
    ) -> Result<ConvertedFrame<'a>, DynError> {
        let expected_len = self.scratch.len();
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
        for (dst, chunk) in self
            .scratch
            .chunks_exact_mut(3)
            .zip(frame_data[..expected_len].chunks_exact(3))
        {
            dst[0] = chunk[2];
            dst[1] = chunk[1];
            dst[2] = chunk[0];
        }
        Ok(ConvertedFrame::Converted(&self.scratch))
    }

    fn expand_gray<'a>(&'a mut self, frame_data: &[u8]) -> Result<ConvertedFrame<'a>, DynError> {
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

        Ok(ConvertedFrame::Converted(&self.scratch))
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
            // Each native V4L2 format must be advertised with the bus
            // pixel_format the driver can actually publish for it. The
            // driver only does cheap conversions (RGB<->BGR, GREY->RGB);
            // MJPG/YUYV must be published verbatim and decoded by the
            // encoder. Defaulting non-BGR3 natives to rgb24 (the
            // pre-plan behaviour) silently wired discovery to a config
            // that crashes the runtime per-frame.
            pixel_format: bus_pixel_format_for_native(&profile.native_pixel_format),
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
    // Slot sizing: the iceoryx2 publisher needs an `initial_max_slice_len`
    // that's >= every payload it will ever publish. Sizing strategy:
    //   * RGB24/BGR24 fill the slot exactly (`width*height*3`)
    //   * YUYV uses `width*height*2`
    //   * MJPG is usually well under YUYV (<= ~10% of YUV422 at q=85)
    //     but a pathological JPEG can briefly exceed it; we let
    //     `AllocationStrategy::PowerOfTwo` grow the slot if needed.
    // The `width*height*3` upper bound covers RGB24 *and* leaves headroom
    // for any JPEG smaller than 3 bytes per pixel — which is essentially
    // every reasonable JPEG.
    let initial_payload_len =
        initial_payload_len(configured.width, configured.height, config.pixel_format)?;

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
        .initial_max_slice_len(initial_payload_len)
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
        let payload = converted.as_slice();
        let mut sample = publisher.loan_slice_uninit(payload.len())?;
        *sample.user_header_mut() = CameraFrameHeader {
            timestamp_us,
            width: configured.width,
            height: configured.height,
            pixel_format: config.pixel_format,
            frame_index,
        };
        let sample = sample.write_from_slice(payload);
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
        // Reject native formats the per-frame converter can't actually
        // produce for this bus format. Without this guard, a webcam that
        // only supports MJPG with `pixel_format = "rgb24"` configured
        // would pass `configure_capture` and then drop every single
        // frame in the converter. Failing here gives the operator an
        // immediate, actionable error.
        if !FrameConverter::can_publish_native_as(active.fourcc, config.pixel_format) {
            last_mismatch = Some(format!(
                "device negotiated native format {} which cannot be published as bus \
                 pixel_format = \"{}\". The camera-side converter only handles RGB<->BGR \
                 and GREY->RGB. Either set pixel_format to \"yuyv\" / \"mjpeg\" \
                 (matching the V4L2 native), or pin native_pixel_format to one of {}.",
                fourcc_to_string(active.fourcc),
                config.pixel_format.as_ref(),
                cheap_native_formats_for(config.pixel_format)
                    .iter()
                    .map(|fc| fourcc_to_string(*fc))
                    .collect::<Vec<_>>()
                    .join(", ")
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
    // Only return native formats the per-frame converter can actually
    // produce as the requested bus format. Listing YUYV/MJPG here for
    // an RGB24 bus would cause the driver to negotiate one of them,
    // pass `is_supported_native_format`, and then drop every frame in
    // the converter (no slow YUYV->RGB or JPEG decode is supported on
    // the camera side anymore — the encoder owns those decodes).
    cheap_native_formats_for(output_pixel_format)
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

/// Iceoryx2 publisher slot size for the configured bus pixel format.
///
/// The slot must be >= the largest payload we'll ever publish:
///   * RGB24 / BGR24 → `width*height*3`
///   * YUYV → `width*height*2`
///   * MJPG → varies, capped via `AllocationStrategy::PowerOfTwo` (the
///     publisher grows its slot if a JPEG happens to exceed the initial
///     hint). We pick the YUV422 worst case as a starting point because
///     real-world MJPGs are typically much smaller.
fn initial_payload_len(
    width: u32,
    height: u32,
    pixel_format: PixelFormat,
) -> Result<usize, DynError> {
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .ok_or("frame payload size overflow")?;
    let bytes = match pixel_format {
        PixelFormat::Rgb24 | PixelFormat::Bgr24 => pixels.checked_mul(3),
        PixelFormat::Yuyv | PixelFormat::Mjpeg => pixels.checked_mul(2),
        // The driver rejects other formats earlier; fall back to RGB24
        // sizing so an accidental new variant doesn't underflow the slot.
        _ => pixels.checked_mul(3),
    };
    bytes.ok_or_else(|| "frame payload size overflow".into())
}

/// Map the V4L2 native pixel format string (as it appears in the
/// driver's `query --json` output, e.g. "MJPG", "YUYV", "RGB3", "BGR3",
/// "GREY") to the bus pixel format the driver can actually publish for
/// it without per-frame conversion (MJPG/YUYV) or with a cheap
/// converter step (BGR3 -> bgr24, GREY -> rgb24-via-expand). Used by
/// `query_device` so the setup wizard's "discover and pick a default
/// profile" path produces a working config out of the box, rather than
/// defaulting every non-BGR3 native to rgb24 (which made MJPG-only
/// webcams like the Logitech C270 spawn a runtime that immediately
/// drops every frame).
fn bus_pixel_format_for_native(native: &str) -> PixelFormat {
    match native {
        "BGR3" => PixelFormat::Bgr24,
        "RGB3" => PixelFormat::Rgb24,
        "MJPG" | "JPEG" => PixelFormat::Mjpeg,
        "YUYV" | "YUY2" => PixelFormat::Yuyv,
        // GREY and any other native format we don't have a direct bus
        // mapping for falls back to rgb24 with the cheap GREY->RGB
        // expansion (or a clear startup error for unsupported natives).
        _ => PixelFormat::Rgb24,
    }
}

/// Native V4L2 fourccs that the per-frame converter can produce as a
/// given bus format (used for the actionable startup error message).
fn cheap_native_formats_for(bus_format: PixelFormat) -> Vec<FourCC> {
    match bus_format {
        PixelFormat::Rgb24 => vec![
            FourCC::new(&RGB3_BYTES),
            FourCC::new(&BGR3_BYTES),
            FourCC::new(&GREY_BYTES),
        ],
        PixelFormat::Bgr24 => vec![
            FourCC::new(&BGR3_BYTES),
            FourCC::new(&RGB3_BYTES),
            FourCC::new(&GREY_BYTES),
        ],
        PixelFormat::Yuyv => vec![FourCC::new(&YUYV_BYTES), FourCC::new(&YUY2_BYTES)],
        PixelFormat::Mjpeg => vec![FourCC::new(&MJPG_BYTES), FourCC::new(&JPEG_BYTES)],
        _ => Vec::new(),
    }
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
    fn run_config_accepts_mjpeg_output() {
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

        let parsed = parse_run_config(config).expect("mjpeg should be accepted as bus format");
        assert_eq!(parsed.pixel_format, PixelFormat::Mjpeg);
    }

    #[test]
    fn run_config_accepts_yuyv_output() {
        let config = r#"
name = "cam"
driver = "v4l2"
id = "/dev/video0"
bus_root = "cam"

[[channels]]
channel_type = "color"
kind = "camera"
profile = { width = 640, height = 480, fps = 30, pixel_format = "yuyv" }
"#;

        let parsed = parse_run_config(config).expect("yuyv should be accepted as bus format");
        assert_eq!(parsed.pixel_format, PixelFormat::Yuyv);
    }

    #[test]
    fn run_config_rejects_depth16_output() {
        let config = r#"
name = "cam"
driver = "v4l2"
id = "/dev/video0"
bus_root = "cam"

[[channels]]
channel_type = "color"
kind = "camera"
profile = { width = 640, height = 480, fps = 30, pixel_format = "depth16" }
"#;

        let error = parse_run_config(config).expect_err("depth16 should be rejected");
        assert!(
            error.to_string().contains("rgb24, bgr24, yuyv, mjpeg"),
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
    fn yuyv_native_passes_through_when_bus_format_matches() {
        let mut converter =
            FrameConverter::new(2, 1, PixelFormat::Yuyv).expect("converter should initialize");
        let payload = [16u8, 128, 235, 128];
        let converted = converter
            .convert(&payload, FourCC::new(&YUYV_BYTES))
            .expect("YUYV passthrough should succeed");
        assert_eq!(converted.as_slice(), &payload);
        assert!(matches!(converted, ConvertedFrame::Passthrough(_)));
    }

    #[test]
    fn mjpeg_native_passes_through_when_bus_format_matches() {
        // A two-byte stub stands in for a JPEG payload; the converter
        // doesn't decode anything in passthrough mode.
        let mut converter =
            FrameConverter::new(2, 1, PixelFormat::Mjpeg).expect("converter should initialize");
        let payload = [0xFFu8, 0xD8];
        let converted = converter
            .convert(&payload, FourCC::new(&MJPG_BYTES))
            .expect("MJPG passthrough should succeed");
        assert_eq!(converted.as_slice(), &payload);
        assert!(matches!(converted, ConvertedFrame::Passthrough(_)));
    }

    #[test]
    fn yuyv_to_rgb_request_is_rejected_with_actionable_error() {
        let mut converter =
            FrameConverter::new(2, 1, PixelFormat::Rgb24).expect("converter should initialize");
        let error = converter
            .convert(&[0u8; 4], FourCC::new(&YUYV_BYTES))
            .expect_err("YUYV->RGB on the camera side is no longer supported");
        let msg = error.to_string();
        assert!(msg.contains("yuyv"), "missing actionable hint: {msg}");
    }

    #[test]
    fn mjpeg_to_rgb_request_is_rejected_with_actionable_error() {
        let mut converter =
            FrameConverter::new(2, 1, PixelFormat::Rgb24).expect("converter should initialize");
        let error = converter
            .convert(&[0xFFu8, 0xD8], FourCC::new(&MJPG_BYTES))
            .expect_err("MJPG->RGB on the camera side is no longer supported");
        let msg = error.to_string();
        assert!(msg.contains("mjpeg"), "missing actionable hint: {msg}");
    }

    #[test]
    fn bgr_input_converts_to_rgb_output() {
        let mut converter =
            FrameConverter::new(1, 1, PixelFormat::Rgb24).expect("converter should initialize");
        let converted = converter
            .convert(&[10, 20, 30], FourCC::new(&BGR3_BYTES))
            .expect("BGR frame should convert");

        assert_eq!(converted.as_slice(), &[30, 20, 10]);
        assert!(matches!(converted, ConvertedFrame::Converted(_)));
    }

    #[test]
    fn rgb_input_passes_through_for_rgb_bus_format() {
        let mut converter =
            FrameConverter::new(1, 1, PixelFormat::Rgb24).expect("converter should initialize");
        let payload = [10u8, 20, 30];
        let converted = converter
            .convert(&payload, FourCC::new(&RGB3_BYTES))
            .expect("RGB passthrough should succeed");
        assert_eq!(converted.as_slice(), &payload);
        assert!(matches!(converted, ConvertedFrame::Passthrough(_)));
    }

    #[test]
    fn gray_input_expands_to_rgb_output() {
        let mut converter =
            FrameConverter::new(2, 1, PixelFormat::Rgb24).expect("converter should initialize");
        let converted = converter
            .convert(&[10, 200], FourCC::new(&GREY_BYTES))
            .expect("GREY -> RGB expansion should succeed");
        assert_eq!(converted.as_slice(), &[10, 10, 10, 200, 200, 200]);
    }

    #[test]
    fn initial_payload_len_for_yuyv_uses_yuv422_worst_case() {
        let len = initial_payload_len(640, 480, PixelFormat::Yuyv).expect("yuyv sizing");
        assert_eq!(len, 640 * 480 * 2);
    }

    #[test]
    fn initial_payload_len_for_mjpeg_uses_yuv422_worst_case() {
        // Sanity check: MJPG is variable-length, but the initial slot is
        // sized to YUYV's worst case.
        let len = initial_payload_len(640, 480, PixelFormat::Mjpeg).expect("mjpeg sizing");
        assert_eq!(len, 640 * 480 * 2);
    }

    #[test]
    fn initial_payload_len_for_rgb24_matches_pixel_count_times_three() {
        let len = initial_payload_len(640, 480, PixelFormat::Rgb24).expect("rgb24 sizing");
        assert_eq!(len, 640 * 480 * 3);
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
