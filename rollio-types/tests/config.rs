use rollio_types::config::*;
use rollio_types::messages::PixelFormat;
use rollio_types::schema::build_config_schema;
use std::str::FromStr;

#[test]
fn parse_example_config() {
    let toml_text = include_str!("../../config/config.example.toml");
    let config = Config::from_str(toml_text).expect("config.example.toml should parse");
    assert_eq!(config.project_name, "default");
    assert_eq!(config.mode, CollectionMode::Teleop);
    assert_eq!(config.devices.len(), 4);
    assert_eq!(config.pairing.len(), 1);
    assert_eq!(config.episode.fps, 30);
    assert_eq!(config.episode.format, EpisodeFormat::LeRobotV2_1);
    assert_eq!(config.encoder.video_codec, EncoderCodec::H264);
    assert_eq!(config.encoder.depth_codec, EncoderCodec::Rvl);
    assert_eq!(config.encoder.backend, EncoderBackend::Auto);
    assert_eq!(
        config.encoder.resolved_artifact_format(),
        EncoderArtifactFormat::Mp4
    );
    assert_eq!(
        config.encoder.resolved_depth_artifact_format(),
        EncoderArtifactFormat::Rvl
    );
    assert_eq!(config.assembler.encoded_handoff, EncodedHandoffMode::File);
    #[cfg(target_os = "linux")]
    assert_eq!(config.assembler.staging_dir, "/dev/shm/rollio");
    assert_eq!(config.storage.queue_size, 32);
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
    assert_eq!(config.ui.http_host, "127.0.0.1");
    assert_eq!(config.ui.http_port, 3000);

    let encoder_configs = config.encoder_runtime_configs();
    assert_eq!(encoder_configs.len(), 2);
    assert_eq!(encoder_configs[0].process_id, "encoder.camera_top");
    assert_eq!(
        encoder_configs[0].camera_name.as_deref(),
        Some("camera_top")
    );
    assert_eq!(
        encoder_configs[0].frame_topic.as_deref(),
        Some("camera/camera_top/frames")
    );
    assert!(
        encoder_configs[0]
            .output_dir
            .ends_with("/encoders/camera_top"),
        "unexpected output dir: {}",
        encoder_configs[0].output_dir
    );

    let assembler_runtime = config.assembler_runtime_config(toml_text.to_string());
    assert_eq!(assembler_runtime.process_id, "episode-assembler");
    assert_eq!(assembler_runtime.cameras.len(), 2);
    assert_eq!(assembler_runtime.robots.len(), 2);
    assert_eq!(assembler_runtime.actions.len(), 1);
    assert!(
        assembler_runtime.staging_dir.ends_with("/episodes"),
        "unexpected staging dir: {}",
        assembler_runtime.staging_dir
    );

    let storage_runtime = config.storage_runtime_config();
    assert_eq!(storage_runtime.process_id, "storage");
    assert_eq!(storage_runtime.queue_size, 32);
}

#[test]
fn ui_runtime_config_defaults_upstream_to_visualizer_port() {
    let mut config = include_str!("../../config/config.example.toml")
        .parse::<Config>()
        .expect("example config should parse");
    config.visualizer.port = 9910;
    config.ui.websocket_url = None;

    let ui_runtime_config = config.ui_runtime_config();
    assert_eq!(
        ui_runtime_config.websocket_url.as_deref(),
        Some("ws://127.0.0.1:9910")
    );
}

#[test]
fn draft_setup_template_starts_with_setup_friendly_defaults() {
    let config = Config::draft_setup_template();
    assert_eq!(config.project_name, "default");
    assert_eq!(config.mode, CollectionMode::Intervention);
    assert!(config.devices.is_empty());
    assert!(config.pairing.is_empty());
    assert_eq!(config.episode.format, EpisodeFormat::LeRobotV2_1);
    assert_eq!(config.episode.fps, 30);
    assert_eq!(config.encoder.video_codec, EncoderCodec::H264);
    assert_eq!(config.encoder.depth_codec, EncoderCodec::Rvl);
    assert_eq!(config.storage.backend, StorageBackend::Local);
    assert_eq!(config.storage.output_path.as_deref(), Some("./output"));
    assert_eq!(config.visualizer.port, 9090);
    assert_eq!(config.ui.start_key, "s");
}

#[test]
fn legacy_configs_infer_mode_from_pairings() {
    let teleop_toml = r#"
[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "leader_arm"
type = "robot"
driver = "pseudo"
id = "r0"
dof = 6
mode = "free-drive"

[[devices]]
name = "follower_arm"
type = "robot"
driver = "pseudo"
id = "r1"
dof = 6
mode = "command-following"

[[pairing]]
leader = "leader_arm"
follower = "follower_arm"

[encoder]
codec = "libx264"

[storage]
backend = "local"
output_path = "./out"
"#;
    let teleop = Config::from_str(teleop_toml).expect("legacy teleop config should parse");
    assert_eq!(teleop.mode, CollectionMode::Teleop);

    let intervention_toml = r#"
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
codec = "libx264"

[storage]
backend = "local"
output_path = "./out"
"#;
    let intervention =
        Config::from_str(intervention_toml).expect("legacy intervention config should parse");
    assert_eq!(intervention.mode, CollectionMode::Intervention);
}

#[test]
fn explicit_mode_rejects_pairing_mismatches() {
    let teleop_without_pairs = r#"
project_name = "demo"
mode = "teleop"

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
video_codec = "libx264"
depth_codec = "rvl"

[storage]
backend = "local"
output_path = "./out"
"#;
    let err =
        Config::from_str(teleop_without_pairs).expect_err("teleop mode should require pairing");
    assert!(
        err.to_string()
            .contains("mode=teleop requires at least one [[pairing]] entry"),
        "unexpected error: {err}"
    );

    let intervention_with_pairs = r#"
project_name = "demo"
mode = "intervention"

[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "leader_arm"
type = "robot"
driver = "pseudo"
id = "r0"
dof = 6
mode = "free-drive"

[[devices]]
name = "follower_arm"
type = "robot"
driver = "pseudo"
id = "r1"
dof = 6
mode = "command-following"

[[pairing]]
leader = "leader_arm"
follower = "follower_arm"

[encoder]
video_codec = "libx264"
depth_codec = "rvl"

[storage]
backend = "local"
output_path = "./out"
"#;
    let err = Config::from_str(intervention_with_pairs)
        .expect_err("intervention mode should reject pairings");
    assert!(
        err.to_string()
            .contains("mode=intervention does not allow [[pairing]] entries"),
        "unexpected error: {err}"
    );
}

#[test]
fn legacy_encoder_codec_populates_both_stream_classes() {
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
codec = "h264"

[storage]
backend = "local"
output_path = "./out"
"#;
    let config = Config::from_str(toml_text).expect("legacy codec alias should parse");
    assert_eq!(config.encoder.video_codec, EncoderCodec::H264);
    assert_eq!(config.encoder.depth_codec, EncoderCodec::H264);
}

#[test]
fn encoder_runtime_configs_pick_depth_codec_for_depth_streams() {
    let toml_text = r#"
project_name = "demo"
mode = "intervention"

[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "camera_rgb"
type = "camera"
driver = "pseudo"
id = "rgb0"
width = 640
height = 480
fps = 30
pixel_format = "rgb24"
stream = "color"

[[devices]]
name = "camera_depth"
type = "camera"
driver = "pseudo"
id = "depth0"
width = 640
height = 480
fps = 30
pixel_format = "depth16"
stream = "depth"

[encoder]
video_codec = "h264"
depth_codec = "rvl"

[storage]
backend = "local"
output_path = "./out"
"#;
    let config = Config::from_str(toml_text).expect("mixed camera config should parse");
    let encoder_configs = config.encoder_runtime_configs();
    assert_eq!(encoder_configs.len(), 2);
    assert_eq!(encoder_configs[0].codec, EncoderCodec::H264);
    assert_eq!(encoder_configs[1].codec, EncoderCodec::Rvl);

    let assembler_runtime = config.assembler_runtime_config(toml_text.to_string());
    assert_eq!(assembler_runtime.cameras.len(), 2);
    assert_eq!(assembler_runtime.cameras[0].codec, EncoderCodec::H264);
    assert_eq!(assembler_runtime.cameras[1].codec, EncoderCodec::Rvl);
}

#[test]
fn schema_export_includes_expected_sections_and_defaults() {
    let schema = build_config_schema();
    assert_eq!(schema.format, "rollio-config-schema");
    assert_eq!(schema.version, 1);

    let section_ids = schema
        .sections
        .iter()
        .map(|section| section.name)
        .collect::<Vec<_>>();
    assert!(section_ids.contains(&"root"));
    assert!(section_ids.contains(&"episode"));
    assert!(section_ids.contains(&"devices"));
    assert!(section_ids.contains(&"storage"));
    assert!(section_ids.contains(&"ui"));

    let root = schema
        .sections
        .iter()
        .find(|section| section.name == "root")
        .expect("root section should exist");
    let mode_field = root
        .fields
        .iter()
        .find(|field| field.name == "mode")
        .expect("mode field should exist");
    assert_eq!(
        mode_field.default.as_ref(),
        Some(&toml::Value::String("intervention".into()))
    );

    let episode = schema
        .sections
        .iter()
        .find(|section| section.name == "episode")
        .expect("episode section should exist");
    let format_field = episode
        .fields
        .iter()
        .find(|field| field.name == "format")
        .expect("episode format field should exist");
    assert_eq!(
        format_field.default.as_ref(),
        Some(&toml::Value::String("lerobot-v2.1".into()))
    );
    assert!(
        format_field
            .enum_values
            .as_ref()
            .is_some_and(|values| values.contains(&"mcap")),
        "episode format field should include mcap"
    );

    let storage = schema
        .sections
        .iter()
        .find(|section| section.name == "storage")
        .expect("storage section should exist");
    let backend_field = storage
        .fields
        .iter()
        .find(|field| field.name == "backend")
        .expect("storage backend field should exist");
    let backend_values = backend_field
        .enum_values
        .as_ref()
        .expect("storage backend field should declare enum values");
    assert!(backend_values.contains(&"local"));
    assert!(backend_values.contains(&"http"));

    let encoder = schema
        .sections
        .iter()
        .find(|section| section.name == "encoder")
        .expect("encoder section should exist");
    assert!(
        encoder
            .fields
            .iter()
            .any(|field| field.name == "video_codec"),
        "encoder section should expose video_codec"
    );
    assert!(
        encoder
            .fields
            .iter()
            .any(|field| field.name == "depth_codec"),
        "encoder section should expose depth_codec"
    );
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
fn parse_airbot_pseudo_direct_joint_config() {
    let toml_text = include_str!("../../config/config.realsense-airbot-pseudo-1.toml");
    let config = Config::from_str(toml_text).expect("airbot pseudo config should parse");
    assert_eq!(config.devices.len(), 3);
    assert_eq!(config.pairing.len(), 1);
    assert_eq!(
        config.device_named("airbot_leader").unwrap().driver,
        "airbot-play"
    );
    assert_eq!(
        config.device_named("pseudo_follower").unwrap().driver,
        "pseudo"
    );
}

#[test]
fn parse_airbot_play_eef_config() {
    let toml_text = include_str!("../../config/config.d435i-airbot-play-eef.toml");
    let config = Config::from_str(toml_text).expect("airbot play + eef config should parse");
    assert_eq!(config.devices.len(), 5);
    assert_eq!(config.pairing.len(), 2);

    let leader_eef = config.device_named("airbot_leader_eef").unwrap();
    assert_eq!(leader_eef.driver, "airbot-e2");
    assert_eq!(leader_eef.dof, Some(1));

    let follower_eef = config.device_named("airbot_follower_eef").unwrap();
    assert_eq!(follower_eef.driver, "airbot-g2");
    assert_eq!(follower_eef.dof, Some(1));
    assert_eq!(follower_eef.mit_kp.as_deref(), Some(&[10.0][..]));
    assert_eq!(follower_eef.mit_kd.as_deref(), Some(&[0.5][..]));
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
fn assembler_runtime_config_supports_explicit_iceoryx2_mode() {
    let toml_text = r#"
process_id = "episode-assembler"
format = "lerobot-v2.1"
fps = 30
chunk_size = 1000
missing_video_timeout_ms = 4000
staging_dir = "/tmp/rollio-stage"
encoded_handoff = "iceoryx2"
embedded_config_toml = "hello = \"world\""

[[cameras]]
camera_name = "camera_top"
encoder_process_id = "encoder.camera_top"
width = 640
height = 480
fps = 30
pixel_format = "rgb24"
codec = "h264"
artifact_format = "mp4"

[[robots]]
robot_name = "leader"
state_topic = "robot/leader/state"
dof = 6

[[actions]]
source_name = "follower"
command_topic = "robot/follower/command"
dof = 6
"#;
    let config = AssemblerRuntimeConfig::from_str(toml_text).expect("runtime config should parse");
    assert_eq!(config.encoded_handoff, EncodedHandoffMode::Iceoryx2);
    assert_eq!(config.cameras.len(), 1);
    assert_eq!(config.actions.len(), 1);
}

#[test]
fn storage_runtime_config_requires_queue_size() {
    let toml_text = r#"
process_id = "storage"
backend = "local"
output_path = "./output"
queue_size = 0
"#;
    let err = StorageRuntimeConfig::from_str(toml_text).expect_err("queue_size=0 should fail");
    assert!(
        err.to_string().contains("queue_size"),
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
        err.to_string()
            .contains("video_codec=rvl requires artifact_format=rvl"),
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

fn direct_joint_pairing_config_for_drivers(
    leader_driver: &str,
    leader_dof: u32,
    follower_driver: &str,
    follower_dof: u32,
) -> String {
    format!(
        r#"
[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "leader_robot"
type = "robot"
driver = "{leader_driver}"
id = "leader"
dof = {leader_dof}
mode = "free-drive"
transport = "can"
interface = "can0"

[[devices]]
name = "follower_robot"
type = "robot"
driver = "{follower_driver}"
id = "follower"
dof = {follower_dof}
mode = "command-following"
transport = "can"
interface = "can1"

[[pairing]]
leader = "leader_robot"
follower = "follower_robot"
mapping = "direct-joint"

[encoder]
codec = "libx264"

[storage]
backend = "local"
output_path = "./out"
"#
    )
}

#[test]
fn airbot_family_direct_joint_pairing_accepts_supported_matrix_entries() {
    for (leader_driver, leader_dof, follower_driver, follower_dof) in [
        ("airbot-e2", 1, "airbot-g2", 1),
        ("airbot-g2", 1, "airbot-e2", 1),
        ("airbot-g2", 1, "airbot-g2", 1),
        ("airbot-play", 6, "airbot-play", 6),
    ] {
        let toml_text = direct_joint_pairing_config_for_drivers(
            leader_driver,
            leader_dof,
            follower_driver,
            follower_dof,
        );
        Config::from_str(&toml_text).unwrap_or_else(|err| {
            panic!("expected {leader_driver} -> {follower_driver} to be accepted, got {err}")
        });
    }
}

#[test]
fn airbot_family_direct_joint_pairing_rejects_unsupported_matrix_entries() {
    for (leader_driver, leader_dof, follower_driver, follower_dof) in [
        ("airbot-e2", 1, "airbot-e2", 1),
        ("airbot-play", 6, "airbot-g2", 1),
    ] {
        let toml_text = direct_joint_pairing_config_for_drivers(
            leader_driver,
            leader_dof,
            follower_driver,
            follower_dof,
        );
        let err = Config::from_str(&toml_text).expect_err("pairing should be rejected");
        assert!(
            err.to_string()
                .contains("direct-joint mapping is not supported"),
            "unexpected error for {leader_driver} -> {follower_driver}: {err}"
        );
    }
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
fn ui_http_port_must_be_positive() {
    let toml_text = r#"
websocket_url = "ws://127.0.0.1:9090"
http_host = "127.0.0.1"
http_port = 0
start_key = "s"
stop_key = "e"
keep_key = "k"
discard_key = "x"
"#;
    let err = UiRuntimeConfig::from_str(toml_text).expect_err("invalid http port should fail");
    assert!(
        err.to_string()
            .contains("http_port must be greater than zero"),
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
