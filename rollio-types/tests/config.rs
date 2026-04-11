use rollio_types::config::*;
use rollio_types::messages::PixelFormat;
use std::str::FromStr;

#[test]
fn parse_example_config() {
    let toml_text = include_str!("../../config/config.example.toml");
    let config = Config::from_str(toml_text).expect("config.example.toml should parse");
    assert_eq!(config.devices.len(), 4);
    assert_eq!(config.pairing.len(), 1);
    assert_eq!(config.episode.fps, 30);
    assert_eq!(config.episode.format, EpisodeFormat::LeRobotV2_1);
    assert_eq!(config.encoder.codec, EncoderCodec::H264);
    assert_eq!(config.encoder.backend, EncoderBackend::Auto);
    assert_eq!(
        config.encoder.resolved_artifact_format(),
        EncoderArtifactFormat::Mp4
    );
    assert_eq!(config.storage.backend, StorageBackend::Local);
    assert_eq!(config.visualizer.port, 9090);
    assert_eq!(config.controller.shutdown_timeout_ms, 3000);
    assert_eq!(
        config.device_named("camera_top").unwrap().pixel_format,
        Some(PixelFormat::Rgb24)
    );
    assert_eq!(
        config.ui_runtime_config().websocket_url.as_deref(),
        Some("ws://127.0.0.1:9090")
    );
    assert_eq!(config.ui.start_key, "s");
    assert_eq!(config.ui.stop_key, "e");
    assert_eq!(config.ui.keep_key, "k");
    assert_eq!(config.ui.discard_key, "x");
}

#[test]
fn parse_hardware_example_config() {
    let toml_text = include_str!("../../config/config.hardware.example.toml");
    let config = Config::from_str(toml_text).expect("hardware example should parse");
    assert_eq!(config.devices.len(), 4);

    let color_camera = config.device_named("camera_front_color").unwrap();
    assert_eq!(color_camera.driver, "realsense");
    assert_eq!(color_camera.stream.as_deref(), Some("color"));
    assert_eq!(color_camera.pixel_format, Some(PixelFormat::Rgb24));
    assert_eq!(color_camera.transport.as_deref(), Some("usb"));

    let webcam = config.device_named("camera_webcam_front").unwrap();
    assert_eq!(webcam.driver, "v4l2");
    assert_eq!(webcam.id, "/dev/video0");
    assert_eq!(webcam.stream.as_deref(), None);
    assert_eq!(webcam.pixel_format, Some(PixelFormat::Rgb24));
    assert_eq!(webcam.transport.as_deref(), Some("usb"));

    let robot = config.device_named("airbot_leader").unwrap();
    assert_eq!(robot.driver, "airbot-play");
    assert_eq!(robot.id, "PZ25C02402000244");
    assert_eq!(robot.mode, Some(RobotMode::FreeDrive));
    assert_eq!(robot.transport.as_deref(), Some("can"));
    assert_eq!(robot.interface.as_deref(), Some("can0"));
    assert_eq!(robot.product_variant.as_deref(), Some("play-e2"));
    assert_eq!(robot.model_path.as_deref(), Some("./models/play_e2.urdf"));
    assert_eq!(
        robot.gravity_comp_torque_scales.as_ref().map(Vec::len),
        Some(6)
    );
}

#[test]
fn parse_v4l2_pseudo_config() {
    let toml_text = include_str!("../../config/config.v4l2-pseudo.toml");
    let config = Config::from_str(toml_text).expect("v4l2 pseudo config should parse");
    assert_eq!(config.devices.len(), 3);
    assert_eq!(config.pairing.len(), 1);

    let webcam = config.device_named("camera_webcam").unwrap();
    assert_eq!(webcam.driver, "v4l2");
    assert_eq!(webcam.id, "/dev/video0");
    assert_eq!(webcam.stream.as_deref(), Some("color"));
    assert_eq!(webcam.pixel_format, Some(PixelFormat::Rgb24));

    let leader = config.device_named("leader_arm").unwrap();
    let follower = config.device_named("follower_arm").unwrap();
    assert_eq!(leader.driver, "pseudo");
    assert_eq!(leader.mode, Some(RobotMode::FreeDrive));
    assert_eq!(follower.driver, "pseudo");
    assert_eq!(follower.mode, Some(RobotMode::CommandFollowing));
}

#[test]
fn parse_v4l2_realsense_rgb_config() {
    let toml_text = include_str!("../../config/config.v4l2-realsense-rgb.toml");
    let config = Config::from_str(toml_text).expect("v4l2 realsense rgb config should parse");
    assert_eq!(config.devices.len(), 2);
    assert!(config.robot_names().is_empty());

    let webcam = config.device_named("camera_webcam_front").unwrap();
    assert_eq!(webcam.driver, "v4l2");
    assert_eq!(webcam.id, "/dev/video0");
    assert_eq!(webcam.pixel_format, Some(PixelFormat::Rgb24));

    let realsense = config.device_named("camera_d435i_rgb").unwrap();
    assert_eq!(realsense.driver, "realsense");
    assert_eq!(realsense.id, "332322071743");
    assert_eq!(realsense.stream.as_deref(), Some("color"));
    assert_eq!(realsense.pixel_format, Some(PixelFormat::Rgb24));
}

#[test]
fn missing_devices_rejected() {
    let toml_text = r#"
[episode]
format = "lerobot-v2.1"
fps = 30

[encoder]
codec = "libx264"

[storage]
backend = "local"
output_path = "./out"

[monitor]
metrics_frequency_hz = 1.0
"#;
    let err = Config::from_str(toml_text).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("devices"),
        "error should mention 'devices': {msg}"
    );
}

#[test]
fn invalid_fps_rejected() {
    let toml_text = r#"
[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "cam"
type = "camera"
driver = "pseudo"
id = "c0"
fps = 0
width = 640
height = 480
pixel_format = "rgb24"

[encoder]
codec = "libx264"

[storage]
backend = "local"
output_path = "./out"

[monitor]
metrics_frequency_hz = 1.0
"#;
    let err = Config::from_str(toml_text).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("fps"), "error should mention fps: {msg}");
}

#[test]
fn duplicate_device_names_rejected() {
    let toml_text = r#"
[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "cam"
type = "camera"
driver = "pseudo"
id = "c0"
width = 640
height = 480
fps = 30
pixel_format = "rgb24"

[[devices]]
name = "cam"
type = "camera"
driver = "pseudo"
id = "c1"
width = 640
height = 480
fps = 30
pixel_format = "rgb24"

[encoder]
codec = "libx264"

[storage]
backend = "local"
output_path = "./out"

[monitor]
metrics_frequency_hz = 1.0
"#;
    let err = Config::from_str(toml_text).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("duplicate"),
        "error should mention duplicate: {msg}"
    );
}

#[test]
fn unknown_codec_rejected() {
    let toml_text = r#"
[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "cam"
type = "camera"
driver = "pseudo"
id = "c0"
width = 640
height = 480
fps = 30
pixel_format = "rgb24"

[encoder]
codec = "nonexistent"

[storage]
backend = "local"
output_path = "./out"

[monitor]
metrics_frequency_hz = 1.0
"#;
    let err = Config::from_str(toml_text).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("nonexistent"),
        "error should name the bad codec: {msg}"
    );
}

#[test]
fn encoder_runtime_config_accepts_camera_name_only() {
    let toml_text = r#"
process_id = "encoder.camera_top"
camera_name = "camera_top"
output_dir = "./out"
codec = "rvl"
fps = 30
"#;
    let config = EncoderRuntimeConfig::from_str(toml_text).expect("runtime config should parse");
    assert_eq!(config.codec, EncoderCodec::Rvl);
    assert_eq!(config.backend, EncoderBackend::Auto);
    assert_eq!(
        config.resolved_artifact_format(),
        EncoderArtifactFormat::Rvl
    );
    assert_eq!(
        config.output_file_name(7),
        "encoder_camera_top_episode_000007.rvl"
    );
}

#[test]
fn encoder_runtime_config_requires_camera_or_topic() {
    let toml_text = r#"
process_id = "encoder.camera_top"
output_dir = "./out"
codec = "h264"
fps = 30
"#;
    let err = EncoderRuntimeConfig::from_str(toml_text).expect_err("config should be rejected");
    assert!(
        err.to_string().contains("camera_name or frame_topic"),
        "unexpected error: {err}"
    );
}

#[test]
fn encoder_config_rejects_rvl_with_mp4_artifact_format() {
    let toml_text = r#"
[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "cam"
type = "camera"
driver = "pseudo"
id = "c0"
width = 640
height = 480
fps = 30
pixel_format = "depth16"

[encoder]
codec = "rvl"
artifact_format = "mp4"

[storage]
backend = "local"
output_path = "./out"
"#;
    let err = Config::from_str(toml_text).expect_err("rvl+mp4 should be rejected");
    assert!(
        err.to_string().contains("rvl requires artifact_format=rvl"),
        "unexpected error: {err}"
    );
}

#[test]
fn pairing_references_unknown_device() {
    let toml_text = r#"
[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "arm_a"
type = "robot"
driver = "pseudo"
id = "r0"
dof = 6
mode = "free-drive"

[[pairing]]
leader = "arm_a"
follower = "arm_b_doesnt_exist"

[encoder]
codec = "libx264"

[storage]
backend = "local"
output_path = "./out"
"#;
    let err = Config::from_str(toml_text).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("arm_b_doesnt_exist"),
        "error should name the bad device: {msg}"
    );
}

#[test]
fn direct_joint_pairing_accepts_remap_and_scales() {
    let toml_text = r#"
[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "leader_arm"
type = "robot"
driver = "pseudo"
id = "leader"
dof = 6
mode = "free-drive"

[[devices]]
name = "follower_arm"
type = "robot"
driver = "pseudo"
id = "follower"
dof = 6
mode = "command-following"

[[pairing]]
leader = "leader_arm"
follower = "follower_arm"
mapping = "direct-joint"
joint_index_map = [5, 4, 3, 2, 1, 0]
joint_scales = [2.0, 1.0, 1.0, 1.0, 1.0, 0.5]

[encoder]
codec = "libx264"

[storage]
backend = "local"
output_path = "./out"
"#;
    let config = Config::from_str(toml_text).expect("pairing config should parse");
    assert_eq!(config.pairing[0].joint_index_map, vec![5, 4, 3, 2, 1, 0]);
    assert_eq!(
        config.pairing[0].joint_scales,
        vec![2.0, 1.0, 1.0, 1.0, 1.0, 0.5]
    );
}

#[test]
fn direct_joint_pairing_rejects_bad_joint_index_map_len() {
    let toml_text = r#"
[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "leader_arm"
type = "robot"
driver = "pseudo"
id = "leader"
dof = 6
mode = "free-drive"

[[devices]]
name = "follower_arm"
type = "robot"
driver = "pseudo"
id = "follower"
dof = 6
mode = "command-following"

[[pairing]]
leader = "leader_arm"
follower = "follower_arm"
mapping = "direct-joint"
joint_index_map = [0, 1, 2]

[encoder]
codec = "libx264"

[storage]
backend = "local"
output_path = "./out"
"#;
    let err = Config::from_str(toml_text).expect_err("pairing should be rejected");
    assert!(
        err.to_string().contains("joint_index_map length"),
        "unexpected error: {err}"
    );
}

#[test]
fn ui_reserved_shortcuts_are_rejected() {
    let toml_text = r#"
websocket_url = "ws://127.0.0.1:9090"
start_key = "d"
stop_key = "e"
keep_key = "k"
discard_key = "x"
"#;
    let err = UiRuntimeConfig::from_str(toml_text).expect_err("reserved UI key should fail");
    assert!(
        err.to_string().contains("reserved UI shortcut"),
        "unexpected error: {err}"
    );
}

#[test]
fn teleop_runtime_config_parses_direct_joint_mapping() {
    let toml_text = r#"
process_id = "teleop.leader_arm.to.follower_arm"
leader_name = "leader_arm"
follower_name = "follower_arm"
leader_state_topic = "robot/leader_arm/state"
follower_state_topic = "robot/follower_arm/state"
follower_command_topic = "robot/follower_arm/command"
mapping = "direct-joint"
joint_index_map = [5, 4, 3, 2, 1, 0]
joint_scales = [2.0, 1.0, 1.0, 1.0, 1.0, 0.5]
"#;
    let config =
        TeleopRuntimeConfig::from_str(toml_text).expect("teleop runtime config should parse");
    assert_eq!(config.process_id, "teleop.leader_arm.to.follower_arm");
    assert_eq!(config.joint_index_map.len(), 6);
    assert_eq!(config.joint_scales[0], 2.0);
}

#[test]
fn monitor_thresholds_parsed() {
    let toml_text = include_str!("../../config/config.example.toml");
    let config = Config::from_str(toml_text).unwrap();
    assert!(!config.monitor.thresholds.is_empty());
    let cam_thresholds = config
        .monitor
        .thresholds
        .get("camera_top")
        .expect("should have camera_top thresholds");
    let fps_threshold = cam_thresholds
        .get("actual_fps")
        .expect("should have actual_fps threshold");
    assert_eq!(fps_threshold.lt, Some(28.0));
    assert_eq!(
        fps_threshold.explanation,
        "Top camera FPS dropped below target"
    );
}

#[test]
fn device_extra_fields_round_trip_through_shared_config() {
    let toml_text = r#"
[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "custom_cam"
type = "camera"
driver = "vendor-x"
id = "123"
width = 640
height = 480
fps = 30
pixel_format = "rgb24"
custom_gain = 12
vendor_profile = "wide"

[encoder]
codec = "libx264"

[storage]
backend = "local"
output_path = "./out"

[monitor]
metrics_frequency_hz = 1.0
"#;
    let config = Config::from_str(toml_text).expect("config should preserve custom device keys");
    let device = config
        .device_named("custom_cam")
        .expect("device should exist");
    assert_eq!(
        device
            .extra
            .get("custom_gain")
            .and_then(|value| value.as_integer()),
        Some(12)
    );
    assert_eq!(
        device
            .extra
            .get("vendor_profile")
            .and_then(|value| value.as_str()),
        Some("wide")
    );

    let inline = toml::to_string(device).expect("device should serialize");
    assert!(inline.contains("custom_gain = 12"));
    assert!(inline.contains("vendor_profile = \"wide\""));
}

#[test]
fn v4l2_camera_device_accepts_bgr24_output() {
    let toml_text = r#"
[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "webcam"
type = "camera"
driver = "v4l2"
id = "/dev/video2"
width = 800
height = 600
fps = 25
pixel_format = "bgr24"

[encoder]
codec = "libx264"

[storage]
backend = "local"
output_path = "./out"

[monitor]
metrics_frequency_hz = 1.0
"#;

    let config = Config::from_str(toml_text).expect("v4l2 config should parse");
    let device = config.device_named("webcam").expect("device should exist");
    assert_eq!(device.driver, "v4l2");
    assert_eq!(device.pixel_format, Some(PixelFormat::Bgr24));
    assert_eq!(device.id, "/dev/video2");
}

#[test]
fn airbot_joint_arrays_must_match_dof() {
    let toml_text = r#"
[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "airbot"
type = "robot"
driver = "airbot-play"
id = "arm0"
dof = 6
mode = "free-drive"
interface = "can0"
product_variant = "play-e2"
model_path = "./robot.urdf"
gravity_comp_torque_scales = [1.0, 1.0, 1.0]

[encoder]
codec = "libx264"

[storage]
backend = "local"
output_path = "./out"
"#;
    let err = Config::from_str(toml_text).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("gravity_comp_torque_scales"),
        "error should mention the bad AIRBOT tuning array: {msg}"
    );
}
