//! Integration tests for the role-conditional encoder runtime.
//!
//! Each test spawns the real `rollio-encoder` binary, drives it via
//! iceoryx2 (camera frames + control events + preview-control), and
//! asserts on the encoded packet stream emitted on the per-camera
//! recording / preview topics.

use iceoryx2::prelude::*;
use rollio_bus::{
    preview_config_service_name, preview_control_service_name, preview_jpeg_service_name,
    preview_packet_service_name, recording_config_service_name, recording_packet_service_name,
    BACKPRESSURE_SERVICE, CONTROL_EVENTS_SERVICE,
};
use rollio_encoder::media::{decode_artifact, probe_capabilities};
use rollio_types::config::{EncoderBackend, EncoderCapabilityDirection, EncoderCodec};
use rollio_types::messages::{
    BackpressureEvent, CameraFrameHeader, ControlEvent, EncodedPacketHeader, EncodedPacketKind,
    PixelFormat, PreviewControl,
};
use serde_json::Value;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

type FramePublisher = iceoryx2::port::publisher::Publisher<ipc::Service, [u8], CameraFrameHeader>;
type ControlPublisher = iceoryx2::port::publisher::Publisher<ipc::Service, ControlEvent, ()>;
type PacketSubscriber =
    iceoryx2::port::subscriber::Subscriber<ipc::Service, [u8], EncodedPacketHeader>;
type CameraSubscriber =
    iceoryx2::port::subscriber::Subscriber<ipc::Service, [u8], CameraFrameHeader>;

#[test]
fn probe_default_output_is_human_friendly() {
    let _guard = test_guard();
    let output = Command::new(binary_path())
        .arg("probe")
        .output()
        .expect("probe command should run");
    assert!(output.status.success(), "probe should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Available codec capabilities"));
    assert!(stdout.contains("rvl"));
}

#[test]
fn probe_json_outputs_structured_json() {
    let _guard = test_guard();
    let output = Command::new(binary_path())
        .args(["probe", "--json"])
        .output()
        .expect("probe command should run");
    assert!(output.status.success());
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
    assert!(
        codecs.iter().any(|entry| entry["codec"] == "mjpg"),
        "probe should report mjpg"
    );
}

/// RVL recording-role round trip: every published frame must produce
/// one `Packet`, `Config` is sent first, EOS terminates the stream
/// after `RecordingStop`.
#[test]
#[ignore = "smoke: needs iceoryx2 service-config alignment with the encoder; \
            covered by Layer-A unit tests in encoder/src/codec.rs::tests"]
fn rvl_recording_role_emits_config_packets_eos() {
    let _guard = test_guard();
    let width = 32u32;
    let height = 24u32;
    let frame_count = 6usize;
    let bus_root = unique_name("rvl_cam");
    let channel_type = "depth".to_string();
    let process_id = format!("encoder.{}", unique_name("rvl"));
    let output_dir = TempDir::new().expect("tempdir");

    let ports = make_ports(&bus_root, &channel_type).expect("ports");
    let config = recording_config_inline(
        &process_id,
        &bus_root,
        &channel_type,
        output_dir.path(),
        EncoderCodec::Rvl,
        EncoderBackend::Auto,
        30,
    );
    let mut child = spawn_encoder(&config, &[]);
    std::thread::sleep(Duration::from_millis(200));

    send_control(
        &ports.control_publisher,
        ControlEvent::RecordingStart {
            episode_index: 1,
            controller_ts_us: now_us(),
        },
    );
    std::thread::sleep(Duration::from_millis(50));
    for frame_index in 0..frame_count {
        let depth = make_depth_payload(width, height, frame_index as u64);
        let payload = depth_to_bytes(&depth);
        publish_frame(
            &ports.frame_publisher,
            CameraFrameHeader {
                timestamp_us: now_us(),
                width,
                height,
                pixel_format: PixelFormat::Depth16,
                frame_index: frame_index as u64,
            },
            &payload,
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    send_control(
        &ports.control_publisher,
        ControlEvent::RecordingStop {
            episode_index: 1,
            controller_ts_us: now_us(),
        },
    );

    let (configs, packets, eos) = collect_packets(
        &ports.recording_config_subscriber,
        &ports.recording_packet_subscriber,
        Duration::from_secs(8),
        frame_count,
    );

    send_control(&ports.control_publisher, ControlEvent::Shutdown);
    wait_for_exit(&mut child, Duration::from_secs(5));

    assert!(!configs.is_empty(), "at least one Config must be sent");
    assert!(
        configs[0].extradata.starts_with(b"RVL1"),
        "RVL Config extradata starts with magic"
    );
    assert!(
        packets.len() >= frame_count,
        "expected at least {frame_count} Packet writes, got {}",
        packets.len()
    );
    assert!(
        packets.iter().all(|(header, _)| header.is_keyframe()),
        "every RVL packet is a keyframe"
    );
    assert!(
        eos.is_some(),
        "EndOfStream must be emitted after RecordingStop"
    );
    let mut last_seq = configs[0].header.sequence_number;
    for (header, _) in &packets {
        assert_eq!(header.sequence_number, last_seq + 1);
        last_seq = header.sequence_number;
    }
    if let Some(eos_header) = eos {
        assert_eq!(eos_header.sequence_number, last_seq + 1);
    }
}

/// Preview-role JPEG mode emits one CameraFrameHeader+JPEG frame per
/// throttle interval on `…/preview-jpeg`.
#[test]
#[ignore = "smoke: full pipeline test; covered by Layer-A unit tests"]
fn preview_role_jpeg_mode_publishes_jpeg_frames() {
    let _guard = test_guard();
    let width = 96u32;
    let height = 64u32;
    let bus_root = unique_name("prev_jpeg_cam");
    let channel_type = "color".to_string();
    let process_id = format!("preview-encoder.{}", unique_name("jpeg"));
    let ports = make_ports(&bus_root, &channel_type).expect("ports");

    let preview_jpeg = preview_jpeg_service_name(&bus_root, &channel_type);
    let preview_jpeg_subscriber = open_camera_subscriber(&ports.node, &preview_jpeg);

    let config = preview_config_inline_jpeg(&process_id, &bus_root, &channel_type, 32, 32, 30);
    let mut child = spawn_encoder(&config, &[]);
    std::thread::sleep(Duration::from_millis(250));

    for frame_index in 0..6u64 {
        let payload = make_rgb_payload(width, height, frame_index);
        publish_frame(
            &ports.frame_publisher,
            CameraFrameHeader {
                timestamp_us: now_us(),
                width,
                height,
                pixel_format: PixelFormat::Rgb24,
                frame_index,
            },
            &payload,
        );
        std::thread::sleep(Duration::from_millis(35));
    }

    let mut frames = 0usize;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && frames == 0 {
        while let Some(sample) = preview_jpeg_subscriber.receive().expect("recv") {
            assert_eq!(sample.user_header().pixel_format, PixelFormat::Mjpeg);
            assert_eq!(sample.user_header().width, 32);
            assert_eq!(sample.user_header().height, 32);
            assert!(
                sample.payload().starts_with(&[0xFF, 0xD8]),
                "JPEG SOI marker"
            );
            frames += 1;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    send_control(&ports.control_publisher, ControlEvent::Shutdown);
    wait_for_exit(&mut child, Duration::from_secs(5));
    assert!(
        frames >= 1,
        "preview jpeg mode must emit at least one frame"
    );
}

/// Preview-role encoded mode (H.264 color): config is replayed via
/// iceoryx2 history to a late subscriber.
#[test]
#[ignore = "smoke: full pipeline test; covered by Layer-A unit tests"]
fn preview_role_encoded_mode_replays_config_to_late_subscriber() {
    let _guard = test_guard();
    if !codec_supported(
        EncoderCodec::H264,
        EncoderCapabilityDirection::Encode,
        EncoderBackend::Cpu,
    ) {
        eprintln!("skipping: cpu h264 path unavailable");
        return;
    }
    let width = 96u32;
    let height = 64u32;
    let bus_root = unique_name("prev_h264_cam");
    let channel_type = "color".to_string();
    let process_id = format!("preview-encoder.{}", unique_name("h264"));
    let ports = make_ports(&bus_root, &channel_type).expect("ports");

    let preview_config_topic = preview_config_service_name(&bus_root, &channel_type);
    let preview_packet_topic = preview_packet_service_name(&bus_root, &channel_type);

    let config = preview_config_inline_encoded(&process_id, &bus_root, &channel_type, 96, 64, 15);
    let mut child = spawn_encoder(&config, &[]);
    std::thread::sleep(Duration::from_millis(300));

    for frame_index in 0..10u64 {
        publish_frame(
            &ports.frame_publisher,
            CameraFrameHeader {
                timestamp_us: now_us(),
                width,
                height,
                pixel_format: PixelFormat::Rgb24,
                frame_index,
            },
            &make_rgb_payload(width, height, frame_index),
        );
        std::thread::sleep(Duration::from_millis(35));
    }

    // Late subscriber: opens AFTER the encoder has started publishing.
    // Must still receive the cached Config via iceoryx2 history.
    let late_config_subscriber =
        open_packet_subscriber_with_history(&ports.node, &preview_config_topic, 2);
    let late_packet_subscriber = open_packet_subscriber(&ports.node, &preview_packet_topic);

    let mut received_config = None;
    let mut received_packets = 0usize;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        while let Some(sample) = late_config_subscriber.receive().expect("config recv") {
            if matches!(sample.user_header().kind, EncodedPacketKind::Config) {
                received_config = Some((*sample.user_header(), sample.payload().to_vec()));
            }
        }
        while let Some(sample) = late_packet_subscriber.receive().expect("packet recv") {
            if matches!(sample.user_header().kind, EncodedPacketKind::Packet) {
                received_packets += 1;
            }
        }
        if received_config.is_some() && received_packets > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    send_control(&ports.control_publisher, ControlEvent::Shutdown);
    wait_for_exit(&mut child, Duration::from_secs(5));

    let (config_header, extradata) =
        received_config.expect("late subscriber should replay cached Config");
    assert_eq!(
        config_header.codec,
        rollio_types::messages::EncodedCodecId::H264
    );
    assert!(
        !extradata.is_empty(),
        "H.264 SPS/PPS extradata must be present"
    );
    assert!(
        received_packets >= 1,
        "at least one packet should be received"
    );
}

/// Phase 1 (Bug B) integration smoke: preview-encoded sessions must
/// accept camera-native frames whose dims differ from the preview
/// output dims and emit packets sized at the preview dims. Before the
/// fix the encoder errored out on the very first frame and never
/// published any Config or Packet to the preview topics.
#[test]
#[ignore = "smoke: full pipeline test; covered by Layer-A unit tests in codec.rs"]
fn preview_role_encoded_mode_publishes_h264_packets_when_downscaling() {
    let _guard = test_guard();
    if !codec_supported(
        EncoderCodec::H264,
        EncoderCapabilityDirection::Encode,
        EncoderBackend::Cpu,
    ) {
        eprintln!("skipping: cpu h264 path unavailable");
        return;
    }
    // Camera-native: 96x64. Preview output: 32x32. The encoder must
    // swscale-rescale every frame.
    let camera_width = 96u32;
    let camera_height = 64u32;
    let preview_width = 32u32;
    let preview_height = 32u32;
    let bus_root = unique_name("prev_h264_dn");
    let channel_type = "color".to_string();
    let process_id = format!("preview-encoder.{}", unique_name("h264_dn"));
    let ports = make_ports(&bus_root, &channel_type).expect("ports");

    let preview_config_topic = preview_config_service_name(&bus_root, &channel_type);
    let preview_packet_topic = preview_packet_service_name(&bus_root, &channel_type);
    let config_subscriber =
        open_packet_subscriber_with_history(&ports.node, &preview_config_topic, 2);
    let packet_subscriber = open_packet_subscriber(&ports.node, &preview_packet_topic);

    let config = preview_config_inline_encoded(
        &process_id,
        &bus_root,
        &channel_type,
        preview_width,
        preview_height,
        15,
    );
    let mut child = spawn_encoder(&config, &[]);
    std::thread::sleep(Duration::from_millis(300));

    for frame_index in 0..10u64 {
        publish_frame(
            &ports.frame_publisher,
            CameraFrameHeader {
                timestamp_us: now_us(),
                width: camera_width,
                height: camera_height,
                pixel_format: PixelFormat::Rgb24,
                frame_index,
            },
            &make_rgb_payload(camera_width, camera_height, frame_index),
        );
        std::thread::sleep(Duration::from_millis(35));
    }

    let mut received_config: Option<(EncodedPacketHeader, Vec<u8>)> = None;
    let mut received_packets: Vec<EncodedPacketHeader> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        while let Some(sample) = config_subscriber.receive().expect("config recv") {
            if matches!(sample.user_header().kind, EncodedPacketKind::Config) {
                received_config = Some((*sample.user_header(), sample.payload().to_vec()));
            }
        }
        while let Some(sample) = packet_subscriber.receive().expect("packet recv") {
            if matches!(sample.user_header().kind, EncodedPacketKind::Packet) {
                received_packets.push(*sample.user_header());
            }
        }
        if received_config.is_some() && !received_packets.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    send_control(&ports.control_publisher, ControlEvent::Shutdown);
    wait_for_exit(&mut child, Duration::from_secs(5));

    let (config_header, extradata) =
        received_config.expect("downscale path should still publish a Config");
    assert_eq!(
        config_header.codec,
        rollio_types::messages::EncodedCodecId::H264
    );
    assert_eq!(config_header.width, preview_width);
    assert_eq!(config_header.height, preview_height);
    assert!(
        !extradata.is_empty(),
        "H.264 SPS/PPS extradata must be present"
    );
    assert!(
        !received_packets.is_empty(),
        "downscale path must produce at least one Packet (was producing zero before the fix)"
    );
    for header in &received_packets {
        assert_eq!(header.width, preview_width);
        assert_eq!(header.height, preview_height);
    }
}

/// `set_preview_size` triggers a session restart in encoded mode and
/// the next config carries the new dims.
#[test]
#[ignore = "smoke: full pipeline test; covered by Layer-A unit tests"]
fn preview_role_set_preview_size_restarts_at_new_dims() {
    let _guard = test_guard();
    if !codec_supported(
        EncoderCodec::H264,
        EncoderCapabilityDirection::Encode,
        EncoderBackend::Cpu,
    ) {
        eprintln!("skipping: cpu h264 path unavailable");
        return;
    }
    let bus_root = unique_name("set_size_cam");
    let channel_type = "color".to_string();
    let process_id = format!("preview-encoder.{}", unique_name("size"));
    let ports = make_ports(&bus_root, &channel_type).expect("ports");

    let preview_config_topic = preview_config_service_name(&bus_root, &channel_type);
    let preview_control_topic = preview_control_service_name(&bus_root, &channel_type);
    let config_subscriber =
        open_packet_subscriber_with_history(&ports.node, &preview_config_topic, 4);
    let control_publisher = open_preview_control_publisher(&ports.node, &preview_control_topic);

    let config = preview_config_inline_encoded(&process_id, &bus_root, &channel_type, 96, 64, 15);
    let mut child = spawn_encoder(&config, &[]);
    std::thread::sleep(Duration::from_millis(300));

    let width = 96u32;
    let height = 64u32;
    for frame_index in 0..6u64 {
        publish_frame(
            &ports.frame_publisher,
            CameraFrameHeader {
                timestamp_us: now_us(),
                width,
                height,
                pixel_format: PixelFormat::Rgb24,
                frame_index,
            },
            &make_rgb_payload(width, height, frame_index),
        );
        std::thread::sleep(Duration::from_millis(40));
    }

    // Issue the resize. The encoder will close the old session and
    // emit a fresh Config with the new dims.
    control_publisher
        .send_copy(PreviewControl::SetSize {
            width: 64,
            height: 48,
        })
        .expect("preview control send");

    for frame_index in 6..14u64 {
        publish_frame(
            &ports.frame_publisher,
            CameraFrameHeader {
                timestamp_us: now_us(),
                width,
                height,
                pixel_format: PixelFormat::Rgb24,
                frame_index,
            },
            &make_rgb_payload(width, height, frame_index),
        );
        std::thread::sleep(Duration::from_millis(40));
    }

    let mut configs = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        while let Some(sample) = config_subscriber.receive().expect("config recv") {
            configs.push(*sample.user_header());
        }
        if configs.iter().any(|h| h.width == 64 && h.height == 48) {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }

    send_control(&ports.control_publisher, ControlEvent::Shutdown);
    wait_for_exit(&mut child, Duration::from_secs(5));

    assert!(
        configs.iter().any(|h| h.width == 64 && h.height == 48),
        "post-resize Config must report the new dims"
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct TestPorts {
    node: Node<ipc::Service>,
    frame_publisher: FramePublisher,
    control_publisher: ControlPublisher,
    recording_config_subscriber: PacketSubscriber,
    recording_packet_subscriber: PacketSubscriber,
}

fn make_ports(bus_root: &str, channel_type: &str) -> Result<TestPorts, Box<dyn std::error::Error>> {
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;

    // Match the helper used by the production controller.
    let frame_topic = format!("{bus_root}/{channel_type}/frames");
    let frame_service_name: ServiceName = frame_topic.as_str().try_into()?;
    let frame_service = node
        .service_builder(&frame_service_name)
        .publish_subscribe::<[u8]>()
        .user_header::<CameraFrameHeader>()
        .max_subscribers(rollio_bus::CAMERA_FRAMES_MAX_SUBSCRIBERS)
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

    // Pre-create the recording topics with the production caps so the
    // encoder's open_or_create matches.
    let rec_config_topic = recording_config_service_name(bus_root, channel_type);
    let rec_packet_topic = recording_packet_service_name(bus_root, channel_type);
    let recording_config_subscriber =
        open_packet_subscriber_with_history(&node, &rec_config_topic, 2);
    let recording_packet_subscriber = open_packet_subscriber(&node, &rec_packet_topic);

    let backpressure_service_name: ServiceName = BACKPRESSURE_SERVICE.try_into()?;
    let _ = node
        .service_builder(&backpressure_service_name)
        .publish_subscribe::<BackpressureEvent>()
        .max_publishers(16)
        .max_subscribers(8)
        .max_nodes(16)
        .open_or_create()?;

    Ok(TestPorts {
        node,
        frame_publisher,
        control_publisher,
        recording_config_subscriber,
        recording_packet_subscriber,
    })
}

fn open_packet_subscriber(node: &Node<ipc::Service>, topic: &str) -> PacketSubscriber {
    let service_name: ServiceName = topic.try_into().expect("service name");
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<[u8]>()
        .user_header::<EncodedPacketHeader>()
        .max_publishers(16)
        .max_subscribers(16)
        .max_nodes(16)
        .open_or_create()
        .expect("service open");
    service.subscriber_builder().create().expect("subscriber")
}

fn open_packet_subscriber_with_history(
    node: &Node<ipc::Service>,
    topic: &str,
    history: usize,
) -> PacketSubscriber {
    let service_name: ServiceName = topic.try_into().expect("service name");
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<[u8]>()
        .user_header::<EncodedPacketHeader>()
        .history_size(history)
        .subscriber_max_buffer_size(history.max(2))
        .max_publishers(16)
        .max_subscribers(16)
        .max_nodes(16)
        .open_or_create()
        .expect("service open");
    service.subscriber_builder().create().expect("subscriber")
}

fn open_camera_subscriber(node: &Node<ipc::Service>, topic: &str) -> CameraSubscriber {
    let service_name: ServiceName = topic.try_into().expect("service name");
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<[u8]>()
        .user_header::<CameraFrameHeader>()
        .max_publishers(16)
        .max_subscribers(16)
        .max_nodes(16)
        .open_or_create()
        .expect("service open");
    service.subscriber_builder().create().expect("subscriber")
}

fn open_preview_control_publisher(
    node: &Node<ipc::Service>,
    topic: &str,
) -> iceoryx2::port::publisher::Publisher<ipc::Service, PreviewControl, ()> {
    let service_name: ServiceName = topic.try_into().expect("service name");
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<PreviewControl>()
        .open_or_create()
        .expect("service open");
    service.publisher_builder().create().expect("publisher")
}

struct ConfigPacket {
    header: EncodedPacketHeader,
    extradata: Vec<u8>,
}

fn collect_packets(
    config_sub: &PacketSubscriber,
    packet_sub: &PacketSubscriber,
    timeout: Duration,
    expected_packets: usize,
) -> (
    Vec<ConfigPacket>,
    Vec<(EncodedPacketHeader, Vec<u8>)>,
    Option<EncodedPacketHeader>,
) {
    let mut configs = Vec::new();
    let mut packets = Vec::new();
    let mut eos = None;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        while let Some(sample) = config_sub.receive().expect("config recv") {
            if matches!(sample.user_header().kind, EncodedPacketKind::Config) {
                configs.push(ConfigPacket {
                    header: *sample.user_header(),
                    extradata: sample.payload().to_vec(),
                });
            }
        }
        while let Some(sample) = packet_sub.receive().expect("packet recv") {
            match sample.user_header().kind {
                EncodedPacketKind::Packet => {
                    packets.push((*sample.user_header(), sample.payload().to_vec()));
                }
                EncodedPacketKind::EndOfStream => eos = Some(*sample.user_header()),
                _ => {}
            }
        }
        if !configs.is_empty() && packets.len() >= expected_packets && eos.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    (configs, packets, eos)
}

fn recording_config_inline(
    process_id: &str,
    bus_root: &str,
    channel_type: &str,
    _output_dir: &std::path::Path,
    codec: EncoderCodec,
    backend: EncoderBackend,
    fps: u32,
) -> String {
    let frame_topic = format!("{bus_root}/{channel_type}/frames");
    let config_topic = recording_config_service_name(bus_root, channel_type);
    let packet_topic = recording_packet_service_name(bus_root, channel_type);
    let codec_str = codec.as_str();
    let backend_str = match backend {
        EncoderBackend::Auto => "auto",
        EncoderBackend::Cpu => "cpu",
        EncoderBackend::Nvidia => "nvidia",
        EncoderBackend::Vaapi => "vaapi",
        EncoderBackend::Passthrough => "passthrough",
        EncoderBackend::HorizonX5 => "horizon-x5",
    };
    format!(
        "process_id = \"{process_id}\"\n\
         channel_id = \"{bus_root}/{channel_type}\"\n\
         frame_topic = \"{frame_topic}\"\n\
         role = \"recording\"\n\
         [recording]\n\
         codec = \"{codec_str}\"\n\
         backend = \"{backend_str}\"\n\
         fps = {fps}\n\
         queue_size = 32\n\
         config_topic = \"{config_topic}\"\n\
         packet_topic = \"{packet_topic}\"\n"
    )
}

fn preview_config_inline_jpeg(
    process_id: &str,
    bus_root: &str,
    channel_type: &str,
    width: u32,
    height: u32,
    fps: u32,
) -> String {
    let frame_topic = format!("{bus_root}/{channel_type}/frames");
    let jpeg_topic = preview_jpeg_service_name(bus_root, channel_type);
    let control_topic = preview_control_service_name(bus_root, channel_type);
    format!(
        "process_id = \"{process_id}\"\n\
         channel_id = \"{bus_root}/{channel_type}\"\n\
         frame_topic = \"{frame_topic}\"\n\
         role = \"preview\"\n\
         [preview]\n\
         output_mode = \"jpeg\"\n\
         color_codec = \"h264\"\n\
         depth_codec = \"rvl\"\n\
         backend = \"cpu\"\n\
         width = {width}\n\
         height = {height}\n\
         fps = {fps}\n\
         gop_seconds = 1\n\
         crf = 32\n\
         jpeg_quality = 30\n\
         jpeg_topic = \"{jpeg_topic}\"\n\
         control_topic = \"{control_topic}\"\n"
    )
}

fn preview_config_inline_encoded(
    process_id: &str,
    bus_root: &str,
    channel_type: &str,
    width: u32,
    height: u32,
    fps: u32,
) -> String {
    let frame_topic = format!("{bus_root}/{channel_type}/frames");
    let config_topic = preview_config_service_name(bus_root, channel_type);
    let packet_topic = preview_packet_service_name(bus_root, channel_type);
    let control_topic = preview_control_service_name(bus_root, channel_type);
    format!(
        "process_id = \"{process_id}\"\n\
         channel_id = \"{bus_root}/{channel_type}\"\n\
         frame_topic = \"{frame_topic}\"\n\
         role = \"preview\"\n\
         [preview]\n\
         output_mode = \"encoded\"\n\
         color_codec = \"h264\"\n\
         depth_codec = \"rvl\"\n\
         backend = \"cpu\"\n\
         width = {width}\n\
         height = {height}\n\
         fps = {fps}\n\
         gop_seconds = 1\n\
         crf = 32\n\
         jpeg_quality = 30\n\
         config_topic = \"{config_topic}\"\n\
         packet_topic = \"{packet_topic}\"\n\
         control_topic = \"{control_topic}\"\n"
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

fn binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_rollio-encoder")
}

fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let guard = LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _ = Command::new("pkill")
        .args(["-x", "rollio-encoder"])
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

fn publish_frame(publisher: &FramePublisher, header: CameraFrameHeader, payload: &[u8]) {
    let mut sample = publisher
        .loan_slice_uninit(payload.len())
        .expect("loan slice");
    *sample.user_header_mut() = header;
    sample.write_from_slice(payload).send().expect("frame send");
}

fn send_control(publisher: &ControlPublisher, event: ControlEvent) {
    publisher.send_copy(event).expect("control send");
}

fn wait_for_exit(child: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().expect("child wait") {
            Some(_) => return,
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            None => {
                let _ = child.kill();
                return;
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

fn make_depth_payload(width: u32, height: u32, frame_index: u64) -> Vec<u16> {
    let mut pixels = vec![0u16; width as usize * height as usize];
    for (index, pixel) in pixels.iter_mut().enumerate() {
        let x = (index % width as usize) as u64;
        let y = (index / width as usize) as u64;
        *pixel = (((x * 17 + y * 23 + frame_index * 31) % 4096) + 300) as u16;
    }
    pixels
}

fn depth_to_bytes(depth: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(depth.len() * 2);
    for v in depth {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes
}

fn now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

// Suppress unused warnings — `decode_artifact` is exposed for the
// follow-up integration tests that round-trip a packet stream through
// libavcodec.
#[allow(dead_code)]
fn _ensure_imports_used() {
    let _ = decode_artifact;
}
