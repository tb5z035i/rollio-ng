use iceoryx2::prelude::*;
use rollio_bus::{
    camera_frames_service_name, channel_preview_service_name, BACKPRESSURE_SERVICE,
    CONTROL_EVENTS_SERVICE, VIDEO_READY_SERVICE,
};
use rollio_encoder::media::{decode_artifact, decode_artifact_with_backend, probe_capabilities};
use rollio_types::config::{EncoderBackend, EncoderCapabilityDirection, EncoderCodec};
use rollio_types::messages::{
    BackpressureEvent, CameraFrameHeader, ControlEvent, PixelFormat, VideoReady,
};
use serde_json::Value;
use std::fs;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

type FramePublisher = iceoryx2::port::publisher::Publisher<ipc::Service, [u8], CameraFrameHeader>;
type ControlPublisher = iceoryx2::port::publisher::Publisher<ipc::Service, ControlEvent, ()>;
type VideoReadySubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, VideoReady, ()>;
type BackpressureSubscriber =
    iceoryx2::port::subscriber::Subscriber<ipc::Service, BackpressureEvent, ()>;

struct TestPorts {
    _node: Node<ipc::Service>,
    frame_publisher: FramePublisher,
    control_publisher: ControlPublisher,
    ready_subscriber: VideoReadySubscriber,
    backpressure_subscriber: BackpressureSubscriber,
}

/// Regression: `VIDEO_READY_SERVICE` and `BACKPRESSURE_SERVICE` are shared
/// across every encoder process. iceoryx2 defaults `max_publishers` to 2,
/// so a project with 3+ enabled camera channels (e.g. 2 V4L2 webcams + a
/// RealSense color/depth/infrared device = 5 encoders) used to crash the
/// 3rd encoder with `PublisherCreateError::ExceedsMaxSupportedPublishers`,
/// which surfaced as `child "encoder-realsense-infrared" exited with
/// status exit status: 1`.
///
/// The fix raises the cap to 16 in both `encoder::runtime::run` and
/// `episode_assembler::runtime::create_video_ready_subscriber`. This test
/// re-creates the failure mode in-process: open the two shared services
/// with the production caps once, then attach 5 publishers in succession.
#[test]
fn five_publishers_can_share_video_ready_and_backpressure_services() {
    let _guard = test_guard();
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()
        .expect("node should build");

    let ready_service_name: ServiceName =
        VIDEO_READY_SERVICE.try_into().expect("ready service name");
    let ready_service = node
        .service_builder(&ready_service_name)
        .publish_subscribe::<VideoReady>()
        .max_publishers(16)
        .max_subscribers(8)
        .max_nodes(16)
        .open_or_create()
        .expect("video ready service should create with 16-publisher cap");

    let backpressure_service_name: ServiceName = BACKPRESSURE_SERVICE
        .try_into()
        .expect("backpressure service name");
    let backpressure_service = node
        .service_builder(&backpressure_service_name)
        .publish_subscribe::<BackpressureEvent>()
        .max_publishers(16)
        .max_subscribers(8)
        .max_nodes(16)
        .open_or_create()
        .expect("backpressure service should create with 16-publisher cap");

    let mut ready_publishers = Vec::new();
    let mut backpressure_publishers = Vec::new();
    for index in 0..5 {
        ready_publishers.push(
            ready_service
                .publisher_builder()
                .create()
                .unwrap_or_else(|error| {
                    panic!("video ready publisher #{index} should attach: {error:?}")
                }),
        );
        backpressure_publishers.push(
            backpressure_service
                .publisher_builder()
                .create()
                .unwrap_or_else(|error| {
                    panic!("backpressure publisher #{index} should attach: {error:?}")
                }),
        );
    }
    assert_eq!(ready_publishers.len(), 5);
    assert_eq!(backpressure_publishers.len(), 5);
}

#[test]
fn probe_default_output_is_human_friendly() {
    let _guard = test_guard();
    let output = Command::new(binary_path())
        .arg("probe")
        .output()
        .expect("probe command should run");

    assert!(
        output.status.success(),
        "probe should succeed, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Available codec capabilities"),
        "expected human-friendly heading, got: {stdout}"
    );
    assert!(
        stdout.contains("rvl"),
        "expected always-available RVL capability in output: {stdout}"
    );
    assert!(
        stdout.contains("--json"),
        "expected JSON hint in output: {stdout}"
    );
    assert!(
        !stdout.trim_start().starts_with('{'),
        "default probe output should not be JSON: {stdout}"
    );
}

#[test]
fn probe_json_outputs_structured_json() {
    let _guard = test_guard();
    let output = Command::new(binary_path())
        .args(["probe", "--json"])
        .output()
        .expect("probe command should run");

    assert!(
        output.status.success(),
        "probe --json should succeed, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let parsed: Value =
        serde_json::from_slice(&output.stdout).expect("probe --json output should be JSON");
    let codecs = parsed["codecs"]
        .as_array()
        .expect("probe output should contain a codecs array");
    assert!(
        codecs.iter().any(|entry| entry["codec"] == "h264"),
        "probe should report h264"
    );
    assert!(
        codecs.iter().any(|entry| entry["codec"] == "rvl"),
        "probe should report rvl"
    );
}

#[test]
fn cpu_video_codecs_round_trip_when_available() {
    let _guard = test_guard();
    for codec in [EncoderCodec::H264, EncoderCodec::H265, EncoderCodec::Av1] {
        if !codec_supported(
            codec,
            EncoderCapabilityDirection::Encode,
            EncoderBackend::Cpu,
        ) || !codec_supported(
            codec,
            EncoderCapabilityDirection::Decode,
            EncoderBackend::Cpu,
        ) {
            eprintln!("skipping {:?} because the CPU path is unavailable", codec);
            continue;
        }
        run_video_roundtrip(codec, EncoderBackend::Cpu)
            .expect("video codec round-trip should succeed");
    }
}

#[test]
#[ignore = "requires NVIDIA hardware and drivers"]
fn nvidia_video_codecs_round_trip_when_available() {
    let _guard = test_guard();
    for codec in [EncoderCodec::H264, EncoderCodec::H265, EncoderCodec::Av1] {
        if !codec_supported(
            codec,
            EncoderCapabilityDirection::Encode,
            EncoderBackend::Nvidia,
        ) || !codec_supported(
            codec,
            EncoderCapabilityDirection::Decode,
            EncoderBackend::Nvidia,
        ) {
            eprintln!(
                "skipping {:?} because the NVIDIA path is unavailable",
                codec
            );
            continue;
        }
        run_video_roundtrip(codec, EncoderBackend::Nvidia)
            .expect("NVIDIA round-trip should succeed");
    }
}

#[test]
#[ignore = "requires VAAPI-capable device and drivers"]
fn vaapi_video_codecs_round_trip_when_available() {
    let _guard = test_guard();
    for codec in [EncoderCodec::H264, EncoderCodec::H265, EncoderCodec::Av1] {
        if !codec_supported(
            codec,
            EncoderCapabilityDirection::Encode,
            EncoderBackend::Vaapi,
        ) || !codec_supported(
            codec,
            EncoderCapabilityDirection::Decode,
            EncoderBackend::Vaapi,
        ) {
            eprintln!("skipping {:?} because the VAAPI path is unavailable", codec);
            continue;
        }
        run_video_roundtrip(codec, EncoderBackend::Vaapi).expect("VAAPI round-trip should succeed");
    }
}

#[test]
fn rvl_round_trip_is_lossless_and_reports_efficiency() {
    let _guard = test_guard();
    let width = 64;
    let height = 48;
    let frame_count = 10;
    let camera_name = unique_name("depth_cam");
    let process_id = format!("encoder.{}", unique_name("rvl"));
    let output_dir = TempDir::new().expect("tempdir");
    let ports = create_test_ports(&camera_name).expect("ports should be created");
    let config = runtime_config(
        &process_id,
        &camera_name,
        output_dir.path(),
        "rvl",
        "auto",
        32,
        30,
    );

    let mut child = spawn_encoder(&config, &[]);
    std::thread::sleep(Duration::from_millis(150));
    send_control_event(
        &ports.control_publisher,
        ControlEvent::RecordingStart {
            episode_index: 1,
            controller_ts_us: unix_timestamp_us(),
        },
    );
    std::thread::sleep(Duration::from_millis(50));

    let mut first = None;
    let mut last = None;
    let started = Instant::now();
    for frame_index in 0..frame_count {
        let depth = make_depth_payload(width, height, frame_index as u64);
        let depth_bytes = depth_to_bytes(&depth);
        if first.is_none() {
            first = Some(depth.clone());
        }
        last = Some(depth.clone());
        publish_frame(
            &ports.frame_publisher,
            CameraFrameHeader {
                timestamp_us: unix_timestamp_us(),
                width,
                height,
                pixel_format: PixelFormat::Depth16,
                frame_index: frame_index as u64,
            },
            &depth_bytes,
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    send_control_event(
        &ports.control_publisher,
        ControlEvent::RecordingStop {
            episode_index: 1,
            controller_ts_us: unix_timestamp_us(),
        },
    );

    let ready = wait_for_video_ready(
        &ports.ready_subscriber,
        &process_id,
        Duration::from_secs(10),
    )
    .expect("expected a video_ready event");
    send_control_event(&ports.control_publisher, ControlEvent::Shutdown);
    wait_for_exit(&mut child, Duration::from_secs(5));

    let artifact_path = std::path::PathBuf::from(ready.file_path.as_str());
    let decoded = decode_artifact(&artifact_path, EncoderCodec::Rvl).expect("decode should work");
    assert_eq!(decoded.width, width);
    assert_eq!(decoded.height, height);
    assert_eq!(decoded.frame_count, frame_count);
    assert_eq!(
        decoded.first_depth_frame.as_deref(),
        first.as_ref().map(Vec::as_slice)
    );
    assert_eq!(
        decoded.last_depth_frame.as_deref(),
        last.as_ref().map(Vec::as_slice)
    );

    let encoded_bytes = fs::metadata(&artifact_path).expect("artifact exists").len();
    let raw_bytes = (width as u64 * height as u64 * 2) * frame_count as u64;
    eprintln!(
        "benchmark codec=rvl frames={} elapsed_ms={:.3} raw_bytes={} encoded_bytes={} compression_ratio={:.3} rss_kb={:?}",
        frame_count,
        started.elapsed().as_secs_f64() * 1_000.0,
        raw_bytes,
        encoded_bytes,
        raw_bytes as f64 / encoded_bytes.max(1) as f64,
        current_rss_kb(),
    );
}

#[test]
fn backpressure_publishes_event_and_encoder_keeps_working() {
    let _guard = test_guard();
    let width = 64;
    let height = 48;
    let frame_count = 32;
    let camera_name = unique_name("pressure_cam");
    let process_id = format!("encoder.{}", unique_name("pressure"));
    let output_dir = TempDir::new().expect("tempdir");
    let ports = create_test_ports(&camera_name).expect("ports should be created");
    let config = runtime_config(
        &process_id,
        &camera_name,
        output_dir.path(),
        "rvl",
        "auto",
        1,
        30,
    );

    let mut child = spawn_encoder(&config, &[("ROLLIO_ENCODER_TEST_ENCODE_DELAY_MS", "20")]);
    std::thread::sleep(Duration::from_millis(150));
    send_control_event(
        &ports.control_publisher,
        ControlEvent::RecordingStart {
            episode_index: 2,
            controller_ts_us: unix_timestamp_us(),
        },
    );
    std::thread::sleep(Duration::from_millis(50));

    for frame_index in 0..frame_count {
        let depth = make_depth_payload(width, height, frame_index as u64);
        let depth_bytes = depth_to_bytes(&depth);
        publish_frame(
            &ports.frame_publisher,
            CameraFrameHeader {
                timestamp_us: unix_timestamp_us(),
                width,
                height,
                pixel_format: PixelFormat::Depth16,
                frame_index: frame_index as u64,
            },
            &depth_bytes,
        );
    }

    let backpressure = wait_for_backpressure(
        &ports.backpressure_subscriber,
        &process_id,
        Duration::from_secs(5),
    )
    .expect("expected backpressure event");
    assert_eq!(backpressure.queue_name.as_str(), "frame_queue");

    send_control_event(
        &ports.control_publisher,
        ControlEvent::RecordingStop {
            episode_index: 2,
            controller_ts_us: unix_timestamp_us(),
        },
    );
    let ready = wait_for_video_ready(
        &ports.ready_subscriber,
        &process_id,
        Duration::from_secs(10),
    )
    .expect("expected a video_ready event");
    send_control_event(&ports.control_publisher, ControlEvent::Shutdown);
    wait_for_exit(&mut child, Duration::from_secs(5));

    let decoded = decode_artifact(
        &std::path::PathBuf::from(ready.file_path.as_str()),
        EncoderCodec::Rvl,
    )
    .expect("artifact should remain decodable after backpressure");
    assert!(
        decoded.frame_count < frame_count,
        "some frames should have been dropped under backpressure"
    );
    assert!(
        decoded.frame_count > 0,
        "encoder should keep making progress"
    );
}

fn run_video_roundtrip(
    codec: EncoderCodec,
    backend: EncoderBackend,
) -> Result<(), Box<dyn std::error::Error>> {
    let width = 96;
    let height = 64;
    let frame_count = 8;
    let camera_name = unique_name("rgb_cam");
    let process_id = format!("encoder.{}", unique_name(codec.as_str()));
    let output_dir = TempDir::new()?;
    let ports = create_test_ports(&camera_name)?;
    let config = runtime_config(
        &process_id,
        &camera_name,
        output_dir.path(),
        codec.as_str(),
        backend_name(backend),
        32,
        30,
    );

    let mut child = spawn_encoder(&config, &[]);
    std::thread::sleep(Duration::from_millis(150));
    send_control_event(
        &ports.control_publisher,
        ControlEvent::RecordingStart {
            episode_index: 1,
            controller_ts_us: unix_timestamp_us(),
        },
    );
    std::thread::sleep(Duration::from_millis(50));

    let mut original_frames = Vec::new();
    let started = Instant::now();
    for frame_index in 0..frame_count {
        let frame = make_rgb_payload(width, height, frame_index as u64);
        original_frames.push(frame.clone());
        publish_frame(
            &ports.frame_publisher,
            CameraFrameHeader {
                timestamp_us: unix_timestamp_us(),
                width,
                height,
                pixel_format: PixelFormat::Rgb24,
                frame_index: frame_index as u64,
            },
            &frame,
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    send_control_event(
        &ports.control_publisher,
        ControlEvent::RecordingStop {
            episode_index: 1,
            controller_ts_us: unix_timestamp_us(),
        },
    );

    let ready = wait_for_video_ready(
        &ports.ready_subscriber,
        &process_id,
        Duration::from_secs(20),
    )
    .expect("expected video_ready");
    send_control_event(&ports.control_publisher, ControlEvent::Shutdown);
    wait_for_exit(&mut child, Duration::from_secs(5));

    let artifact_path = std::path::PathBuf::from(ready.file_path.as_str());
    let decoded = match backend {
        EncoderBackend::Cpu | EncoderBackend::Auto => decode_artifact(&artifact_path, codec)?,
        other => decode_artifact_with_backend(&artifact_path, codec, other)?,
    };
    assert_eq!(decoded.width, width);
    assert_eq!(decoded.height, height);
    assert_eq!(decoded.frame_count, frame_count);

    let first_mae = mean_absolute_error_rgb(
        decoded
            .first_rgb_frame
            .as_ref()
            .expect("decoded first frame should be present"),
        &original_frames[0],
    );
    let last_mae = mean_absolute_error_rgb(
        decoded
            .last_rgb_frame
            .as_ref()
            .expect("decoded last frame should be present"),
        original_frames.last().expect("last frame should exist"),
    );
    assert!(
        first_mae < 65.0,
        "first frame MAE too high for {:?}: {first_mae}",
        codec
    );
    assert!(
        last_mae < 65.0,
        "last frame MAE too high for {:?}: {last_mae}",
        codec
    );

    let encoded_bytes = fs::metadata(&artifact_path)?.len();
    let raw_bytes = (width as u64 * height as u64 * 3) * frame_count as u64;
    eprintln!(
        "benchmark codec={} frames={} elapsed_ms={:.3} raw_bytes={} encoded_bytes={} compression_ratio={:.3} first_mae={:.3} last_mae={:.3} rss_kb={:?}",
        codec.as_str(),
        frame_count,
        started.elapsed().as_secs_f64() * 1_000.0,
        raw_bytes,
        encoded_bytes,
        raw_bytes as f64 / encoded_bytes.max(1) as f64,
        first_mae,
        last_mae,
        current_rss_kb(),
    );
    Ok(())
}

/// YUYV is one of the new "native" bus pixel formats. The encoder must
/// accept raw YUV422 frames, swscale them to YUV420P, and produce a
/// playable video. Uses a synthetic gradient payload so we don't need a
/// real camera.
#[test]
fn yuyv_frames_round_trip_through_cpu_h264() {
    let _guard = test_guard();
    if !codec_supported(
        EncoderCodec::H264,
        EncoderCapabilityDirection::Encode,
        EncoderBackend::Cpu,
    ) || !codec_supported(
        EncoderCodec::H264,
        EncoderCapabilityDirection::Decode,
        EncoderBackend::Cpu,
    ) {
        eprintln!("skipping yuyv round-trip: cpu h264 path unavailable");
        return;
    }

    let width = 96u32;
    let height = 64u32;
    let frame_count = 8usize;
    let camera_name = unique_name("yuyv_cam");
    let process_id = format!("encoder.{}", unique_name("yuyv_h264"));
    let output_dir = TempDir::new().expect("tempdir");
    let ports = create_test_ports(&camera_name).expect("ports");
    let config = runtime_config(
        &process_id,
        &camera_name,
        output_dir.path(),
        EncoderCodec::H264.as_str(),
        backend_name(EncoderBackend::Cpu),
        32,
        30,
    );
    let mut child = spawn_encoder(&config, &[]);
    std::thread::sleep(Duration::from_millis(150));
    send_control_event(
        &ports.control_publisher,
        ControlEvent::RecordingStart {
            episode_index: 1,
            controller_ts_us: unix_timestamp_us(),
        },
    );
    std::thread::sleep(Duration::from_millis(50));

    for frame_index in 0..frame_count {
        let payload = make_yuyv_payload(width, height, frame_index as u64);
        publish_frame(
            &ports.frame_publisher,
            CameraFrameHeader {
                timestamp_us: unix_timestamp_us(),
                width,
                height,
                pixel_format: PixelFormat::Yuyv,
                frame_index: frame_index as u64,
            },
            &payload,
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    send_control_event(
        &ports.control_publisher,
        ControlEvent::RecordingStop {
            episode_index: 1,
            controller_ts_us: unix_timestamp_us(),
        },
    );

    let ready = wait_for_video_ready(
        &ports.ready_subscriber,
        &process_id,
        Duration::from_secs(20),
    )
    .expect("expected video_ready for yuyv encode");
    send_control_event(&ports.control_publisher, ControlEvent::Shutdown);
    wait_for_exit(&mut child, Duration::from_secs(5));

    let artifact_path = std::path::PathBuf::from(ready.file_path.as_str());
    let decoded =
        decode_artifact(&artifact_path, EncoderCodec::H264).expect("decode should succeed");
    assert_eq!(decoded.width, width, "decoded width should match");
    assert_eq!(decoded.height, height, "decoded height should match");
    assert_eq!(
        decoded.frame_count, frame_count,
        "decoded frame count should match published frame count"
    );
}

/// MJPG is the second new "native" format. The encoder must run libav's
/// MJPEG decoder per frame, then re-encode through the configured codec.
/// Uses a synthetic JPEG produced by libav itself so the test stays
/// deterministic and doesn't depend on a real camera dump.
#[test]
fn mjpeg_frames_round_trip_through_cpu_h264() {
    let _guard = test_guard();
    if !codec_supported(
        EncoderCodec::H264,
        EncoderCapabilityDirection::Encode,
        EncoderBackend::Cpu,
    ) || !codec_supported(
        EncoderCodec::H264,
        EncoderCapabilityDirection::Decode,
        EncoderBackend::Cpu,
    ) {
        eprintln!("skipping mjpeg round-trip: cpu h264 path unavailable");
        return;
    }

    let width = 96u32;
    let height = 64u32;
    let frame_count = 6usize;
    let camera_name = unique_name("mjpeg_cam");
    let process_id = format!("encoder.{}", unique_name("mjpeg_h264"));
    let output_dir = TempDir::new().expect("tempdir");
    let ports = create_test_ports(&camera_name).expect("ports");
    let config = runtime_config(
        &process_id,
        &camera_name,
        output_dir.path(),
        EncoderCodec::H264.as_str(),
        backend_name(EncoderBackend::Cpu),
        32,
        30,
    );
    let mut child = spawn_encoder(&config, &[]);
    std::thread::sleep(Duration::from_millis(150));
    send_control_event(
        &ports.control_publisher,
        ControlEvent::RecordingStart {
            episode_index: 1,
            controller_ts_us: unix_timestamp_us(),
        },
    );
    std::thread::sleep(Duration::from_millis(50));

    let jpeg_payloads: Vec<Vec<u8>> = (0..frame_count)
        .map(|frame_index| make_synthetic_jpeg(width, height, frame_index as u64))
        .collect();
    for (frame_index, payload) in jpeg_payloads.iter().enumerate() {
        publish_frame(
            &ports.frame_publisher,
            CameraFrameHeader {
                timestamp_us: unix_timestamp_us(),
                width,
                height,
                pixel_format: PixelFormat::Mjpeg,
                frame_index: frame_index as u64,
            },
            payload,
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    send_control_event(
        &ports.control_publisher,
        ControlEvent::RecordingStop {
            episode_index: 1,
            controller_ts_us: unix_timestamp_us(),
        },
    );

    let ready = wait_for_video_ready(
        &ports.ready_subscriber,
        &process_id,
        Duration::from_secs(20),
    )
    .expect("expected video_ready for mjpeg encode");
    send_control_event(&ports.control_publisher, ControlEvent::Shutdown);
    wait_for_exit(&mut child, Duration::from_secs(5));

    let artifact_path = std::path::PathBuf::from(ready.file_path.as_str());
    let decoded =
        decode_artifact(&artifact_path, EncoderCodec::H264).expect("decode should succeed");
    assert_eq!(decoded.width, width, "decoded width should match");
    assert_eq!(decoded.height, height, "decoded height should match");
    assert_eq!(
        decoded.frame_count, frame_count,
        "decoded frame count should match published frame count"
    );
}

/// The encoder must publish RGB24 preview frames on the
/// `channel_preview_service_name` topic even when no episode is being
/// recorded. The visualizer relies on this for its always-on live
/// preview.
#[test]
fn preview_tap_publishes_without_active_recording() {
    let _guard = test_guard();
    let width = 96u32;
    let height = 64u32;
    let camera_name = unique_name("preview_cam");
    let process_id = format!("encoder.{}", unique_name("preview"));
    let output_dir = TempDir::new().expect("tempdir");
    // Pre-create the camera frame service ports, then attach a
    // subscriber to the preview tap topic before the encoder spawns so
    // we don't miss any frames.
    let ports = create_test_ports(&camera_name).expect("ports");
    let preview_topic = channel_preview_service_name(&camera_name, "color");
    let preview_node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()
        .expect("preview test node");
    let preview_service_name: ServiceName = preview_topic
        .as_str()
        .try_into()
        .expect("preview topic should be a valid service name");
    let preview_service = preview_node
        .service_builder(&preview_service_name)
        .publish_subscribe::<[u8]>()
        .user_header::<CameraFrameHeader>()
        .open_or_create()
        .expect("preview service");
    let preview_subscriber = preview_service
        .subscriber_builder()
        .create()
        .expect("preview subscriber");

    let config = runtime_config(
        &process_id,
        &camera_name,
        output_dir.path(),
        EncoderCodec::H264.as_str(),
        backend_name(EncoderBackend::Cpu),
        32,
        30,
    );
    let mut child = spawn_encoder(&config, &[]);
    // Give the worker thread enough time to open the preview publisher.
    std::thread::sleep(Duration::from_millis(250));

    // Publish a few raw RGB24 frames *without* sending RecordingStart.
    // The preview tap should still see them.
    for frame_index in 0..6u64 {
        let payload = make_rgb_payload(width, height, frame_index);
        publish_frame(
            &ports.frame_publisher,
            CameraFrameHeader {
                timestamp_us: unix_timestamp_us(),
                width,
                height,
                pixel_format: PixelFormat::Rgb24,
                frame_index,
            },
            &payload,
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    // Drain the preview subscriber and confirm at least one frame was
    // published.
    let mut preview_frames: usize = 0;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && preview_frames == 0 {
        while let Some(sample) = preview_subscriber
            .receive()
            .expect("preview receive should succeed")
        {
            assert_eq!(
                sample.user_header().pixel_format,
                PixelFormat::Rgb24,
                "preview tap must always publish RGB24"
            );
            // Preview dimensions come from the runtime config (320x240).
            assert_eq!(sample.user_header().width, 320);
            assert_eq!(sample.user_header().height, 240);
            assert_eq!(sample.payload().len(), 320 * 240 * 3);
            preview_frames += 1;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    send_control_event(&ports.control_publisher, ControlEvent::Shutdown);
    wait_for_exit(&mut child, Duration::from_secs(5));

    assert!(
        preview_frames >= 1,
        "preview tap should publish at least one frame even with no active recording \
         (received {preview_frames})"
    );
}

fn binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_rollio-encoder")
}

fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    // Recover from a poisoned mutex: when one test panics the next ones
    // would otherwise unwrap to `PoisonError`, masking the real failure.
    // The guard is purely a serialization tool — tests don't share mutable
    // state through it — so taking the inner guard via `into_inner()` on
    // poisoning is safe.
    let guard = LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _ = Command::new("pkill")
        .args(["-f", "rollio-encoder"])
        .status();
    guard
}

fn unique_name(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{prefix}_{nanos}")
}

fn codec_supported(
    codec: EncoderCodec,
    direction: EncoderCapabilityDirection,
    backend: EncoderBackend,
) -> bool {
    probe_capabilities()
        .expect("probe should succeed")
        .codecs
        .into_iter()
        .any(|capability| {
            capability.codec == codec
                && capability.direction == direction
                && capability.backend == backend
                && capability.available
        })
}

fn create_test_ports(camera_name: &str) -> Result<TestPorts, Box<dyn std::error::Error>> {
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;

    let frame_service_name: ServiceName = camera_frames_service_name(camera_name)
        .as_str()
        .try_into()?;
    let frame_service = node
        .service_builder(&frame_service_name)
        .publish_subscribe::<[u8]>()
        .user_header::<CameraFrameHeader>()
        .open_or_create()?;
    let frame_publisher = frame_service
        .publisher_builder()
        .initial_max_slice_len(1024 * 1024)
        .allocation_strategy(AllocationStrategy::PowerOfTwo)
        .create()?;

    let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
    let control_service = node
        .service_builder(&control_service_name)
        .publish_subscribe::<ControlEvent>()
        .open_or_create()?;
    let control_publisher = control_service.publisher_builder().create()?;

    // Match the production quotas in `encoder::runtime::run` and
    // `episode_assembler::runtime::create_video_ready_subscriber`. iceoryx2
    // uses `max_publishers = 2` by default, and `open_or_create` rejects
    // services whose existing config doesn't satisfy the requested caps —
    // so if the test fixture opens these services first with defaults, the
    // encoder under test then fails with
    // `PublisherCreateError::ExceedsMaxSupportedPublishers` on its third
    // publisher (or even the first, if the spec mismatches).
    let ready_service_name: ServiceName = VIDEO_READY_SERVICE.try_into()?;
    let ready_service = node
        .service_builder(&ready_service_name)
        .publish_subscribe::<VideoReady>()
        .max_publishers(16)
        .max_subscribers(8)
        .max_nodes(16)
        .open_or_create()?;
    let ready_subscriber = ready_service.subscriber_builder().create()?;

    let backpressure_service_name: ServiceName = BACKPRESSURE_SERVICE.try_into()?;
    let backpressure_service = node
        .service_builder(&backpressure_service_name)
        .publish_subscribe::<BackpressureEvent>()
        .max_publishers(16)
        .max_subscribers(8)
        .max_nodes(16)
        .open_or_create()?;
    let backpressure_subscriber = backpressure_service.subscriber_builder().create()?;

    Ok(TestPorts {
        _node: node,
        frame_publisher,
        control_publisher,
        ready_subscriber,
        backpressure_subscriber,
    })
}

fn runtime_config(
    process_id: &str,
    camera_name: &str,
    output_dir: &std::path::Path,
    codec: &str,
    backend: &str,
    queue_size: u32,
    fps: u32,
) -> String {
    let frame_topic = camera_frames_service_name(camera_name);
    let preview_topic = rollio_bus::channel_preview_service_name(camera_name, "color");
    format!(
        "process_id = \"{process_id}\"\n\
         channel_id = \"{camera_name}/color\"\n\
         frame_topic = \"{frame_topic}\"\n\
         preview_topic = \"{preview_topic}\"\n\
         output_dir = \"{}\"\n\
         codec = \"{codec}\"\n\
         backend = \"{backend}\"\n\
         queue_size = {queue_size}\n\
         fps = {fps}\n\
         preview_width = 320\n\
         preview_height = 240\n\
         preview_fps = 10\n",
        output_dir.display()
    )
}

fn spawn_encoder(config: &str, extra_env: &[(&str, &str)]) -> Child {
    let mut command = Command::new(binary_path());
    command
        .arg("run")
        .arg("--config-inline")
        .arg(config)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command.spawn().expect("encoder should start")
}

fn publish_frame(publisher: &FramePublisher, header: CameraFrameHeader, payload: &[u8]) {
    let mut sample = publisher
        .loan_slice_uninit(payload.len())
        .expect("sample allocation should work");
    *sample.user_header_mut() = header;
    sample
        .write_from_slice(payload)
        .send()
        .expect("frame should publish");
}

fn send_control_event(publisher: &ControlPublisher, event: ControlEvent) {
    publisher
        .send_copy(event)
        .expect("control event should publish");
}

fn wait_for_video_ready(
    subscriber: &VideoReadySubscriber,
    process_id: &str,
    timeout: Duration,
) -> Option<VideoReady> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(sample) = subscriber.receive().expect("subscriber should work") {
            if sample.payload().process_id.as_str() == process_id {
                return Some(*sample.payload());
            }
        } else {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    None
}

fn wait_for_backpressure(
    subscriber: &BackpressureSubscriber,
    process_id: &str,
    timeout: Duration,
) -> Option<BackpressureEvent> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(sample) = subscriber.receive().expect("subscriber should work") {
            if sample.payload().process_id.as_str() == process_id {
                return Some(*sample.payload());
            }
        } else {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    None
}

fn wait_for_exit(child: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().expect("child wait should succeed") {
            Some(status) => {
                assert!(status.success(), "child exited unsuccessfully: {status}");
                return;
            }
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            None => {
                let _ = child.kill();
                panic!("child did not exit within {timeout:?}");
            }
        }
    }
}

fn make_rgb_payload(width: u32, height: u32, frame_index: u64) -> Vec<u8> {
    let mut payload = vec![0u8; width as usize * height as usize * 3];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let offset = (y * width as usize + x) * 3;
            payload[offset] = ((x as u64 + frame_index * 3) % 256) as u8;
            payload[offset + 1] = ((y as u64 * 2 + frame_index * 5) % 256) as u8;
            payload[offset + 2] = (((x + y) as u64 + frame_index * 7) % 256) as u8;
        }
    }
    payload
}

/// Synthetic YUYV (Y'U'V'422) payload: 2 bytes per pixel laid out as
/// `Y0 U Y1 V`. Width must be even because Y0 and Y1 share chroma.
fn make_yuyv_payload(width: u32, height: u32, frame_index: u64) -> Vec<u8> {
    assert!(width.is_multiple_of(2), "yuyv width must be even");
    let mut payload = vec![0u8; width as usize * height as usize * 2];
    for y in 0..height as usize {
        for x_pair in 0..(width as usize / 2) {
            let row_offset = y * width as usize * 2 + x_pair * 4;
            let y0 = ((x_pair as u64 * 17 + frame_index * 11) % 220 + 16) as u8;
            let y1 = ((x_pair as u64 * 17 + frame_index * 11 + 7) % 220 + 16) as u8;
            let u = ((y as u64 * 5 + frame_index * 3) % 200 + 32) as u8;
            let v = ((y as u64 * 7 + frame_index * 13) % 200 + 32) as u8;
            payload[row_offset] = y0;
            payload[row_offset + 1] = u;
            payload[row_offset + 2] = y1;
            payload[row_offset + 3] = v;
        }
    }
    payload
}

/// Build a synthetic JPEG payload using libav's MJPEG encoder so the
/// MJPG round-trip test doesn't depend on a real camera or a baked-in
/// JPEG fixture. We re-use `ffmpeg-next` (already pulled in by the
/// encoder crate) to encode a single solid-color RGB frame to JPEG.
fn make_synthetic_jpeg(width: u32, height: u32, frame_index: u64) -> Vec<u8> {
    use ffmpeg_next as ffmpeg;
    rollio_encoder::media::ensure_ffmpeg_initialized().expect("ffmpeg init");

    let codec = ffmpeg::encoder::find(ffmpeg::codec::Id::MJPEG).expect("mjpeg encoder");
    let mut encoder_ctx = ffmpeg::codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()
        .expect("video encoder");
    encoder_ctx.set_width(width);
    encoder_ctx.set_height(height);
    encoder_ctx.set_format(ffmpeg::util::format::pixel::Pixel::YUVJ420P);
    encoder_ctx.set_time_base((1, 30));
    let mut encoder = encoder_ctx.open_as(codec).expect("open mjpeg encoder");

    // Build a YUVJ420P frame from a synthetic RGB pattern.
    let rgb = make_rgb_payload(width, height, frame_index);
    let mut rgb_frame =
        ffmpeg::frame::Video::new(ffmpeg::util::format::pixel::Pixel::RGB24, width, height);
    let stride = rgb_frame.stride(0);
    let row_bytes = width as usize * 3;
    for row in 0..height as usize {
        let dst_offset = row * stride;
        let src_offset = row * row_bytes;
        rgb_frame.data_mut(0)[dst_offset..dst_offset + row_bytes]
            .copy_from_slice(&rgb[src_offset..src_offset + row_bytes]);
    }

    let mut scaler = ffmpeg::software::scaling::context::Context::get(
        ffmpeg::util::format::pixel::Pixel::RGB24,
        width,
        height,
        ffmpeg::util::format::pixel::Pixel::YUVJ420P,
        width,
        height,
        ffmpeg::software::scaling::flag::Flags::BILINEAR,
    )
    .expect("scaler");
    let mut yuv_frame = ffmpeg::frame::Video::empty();
    scaler.run(&rgb_frame, &mut yuv_frame).expect("rgb->yuv");
    yuv_frame.set_pts(Some(0));

    encoder
        .send_frame(&yuv_frame)
        .expect("send frame to mjpeg encoder");
    encoder.send_eof().expect("send eof");

    let mut packet = ffmpeg::Packet::empty();
    encoder
        .receive_packet(&mut packet)
        .expect("receive jpeg packet");
    packet.data().expect("packet should carry bytes").to_vec()
}

fn make_depth_payload(width: u32, height: u32, frame_index: u64) -> Vec<u16> {
    let mut pixels = vec![0u16; width as usize * height as usize];
    for (index, pixel) in pixels.iter_mut().enumerate() {
        let x = (index % width as usize) as u64;
        let y = (index / width as usize) as u64;
        *pixel = (((x * 17 + y * 23 + frame_index * 31) % 4096) + 300) as u16;
    }
    pixels
}

fn mean_absolute_error_rgb(decoded: &[u8], original: &[u8]) -> f64 {
    let total: u64 = decoded
        .iter()
        .zip(original.iter())
        .map(|(decoded, original)| u8::abs_diff(*decoded, *original) as u64)
        .sum();
    total as f64 / decoded.len().max(1) as f64
}

fn depth_to_bytes(depth: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(depth.len() * 2);
    for value in depth {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn backend_name(backend: EncoderBackend) -> &'static str {
    match backend {
        EncoderBackend::Auto => "auto",
        EncoderBackend::Cpu => "cpu",
        EncoderBackend::Nvidia => "nvidia",
        EncoderBackend::Vaapi => "vaapi",
    }
}

fn current_rss_kb() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find(|line| line.starts_with("VmRSS:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u64>().ok())
}

fn unix_timestamp_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}
