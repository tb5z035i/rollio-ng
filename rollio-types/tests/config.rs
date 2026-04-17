use rollio_types::config::*;
use rollio_types::messages::PixelFormat;
use rollio_types::schema::build_config_schema;
use std::str::FromStr;

#[test]
fn parse_example_project_config() {
    let toml_text = include_str!("../../config/config.example.toml");
    let config = ProjectConfig::from_str(toml_text).expect("config.example.toml should parse");
    assert_eq!(config.project_name, "default");
    assert_eq!(config.mode, CollectionMode::Teleop);
    assert_eq!(config.devices.len(), 4);
    assert_eq!(config.pairings.len(), 1);
    assert_eq!(config.episode.fps, 30);
    assert_eq!(config.episode.format, EpisodeFormat::LeRobotV2_1);
    assert_eq!(config.encoder.video_codec, EncoderCodec::H264);
    assert_eq!(config.encoder.depth_codec, EncoderCodec::Rvl);
    assert_eq!(config.storage.queue_size, 32);
    assert_eq!(config.visualizer.port, 19090);
    assert_eq!(config.controller.shutdown_timeout_ms, 3000);
    assert_eq!(
        config.ui_runtime_config().preview_websocket_url.as_deref(),
        Some("ws://127.0.0.1:19090")
    );
    assert!(
        config.ui_runtime_config().control_websocket_url.is_none(),
        "control_websocket_url is filled in by the controller at runtime"
    );

    let cameras = config.resolved_camera_channels();
    assert_eq!(cameras.len(), 2);
    assert_eq!(cameras[0].channel_id, "camera_top/color");
    assert_eq!(cameras[0].frame_topic, "camera_top/color/frames");
    assert_eq!(cameras[0].pixel_format, PixelFormat::Rgb24);

    let robots = config.resolved_robot_channels();
    assert_eq!(robots.len(), 2);
    assert_eq!(robots[0].channel_id, "leader_arm/arm");
    assert_eq!(
        robots[0].state_topics[0].1,
        "leader_arm/arm/states/joint_position"
    );

    let encoder_configs = config.encoder_runtime_configs_v2();
    assert_eq!(encoder_configs.len(), 2);
    assert_eq!(encoder_configs[0].channel_id, "camera_top/color");
    assert_eq!(encoder_configs[0].frame_topic, "camera_top/color/frames");

    let teleop_configs = config.teleop_runtime_configs_v2();
    assert_eq!(teleop_configs.len(), 1);
    assert_eq!(
        teleop_configs[0].leader_state_topic,
        "leader_arm/arm/states/joint_position"
    );
    assert_eq!(
        teleop_configs[0].follower_command_topic,
        "follower_arm/arm/commands/joint_position"
    );

    let assembler_runtime = config.assembler_runtime_config_v2(toml_text.to_string());
    assert_eq!(assembler_runtime.cameras.len(), 2);
    assert_eq!(assembler_runtime.observations.len(), 6);
    assert_eq!(assembler_runtime.actions.len(), 1);

    let storage_runtime = config.storage_runtime_config();
    assert_eq!(storage_runtime.process_id, "storage");
    assert_eq!(storage_runtime.queue_size, 32);
}

#[test]
fn visualizer_runtime_config_v2_derives_sources_from_enabled_channels() {
    let config = include_str!("../../config/config.example.toml")
        .parse::<ProjectConfig>()
        .expect("example config should parse");
    let visualizer = config.visualizer_runtime_config_v2();
    assert_eq!(visualizer.camera_sources.len(), 2);
    assert_eq!(visualizer.robot_sources.len(), 8);
    assert_eq!(visualizer.camera_sources[0].frame_topic, "camera_top/color/frames");
}

#[test]
fn pairings_require_existing_robot_channels() {
    let toml_text = r#"
project_name = "demo"
mode = "teleop"

[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "cam"
driver = "pseudo"
id = "pseudo_camera_0"
bus_root = "cam"

[[devices.channels]]
channel_type = "color"
kind = "camera"
enabled = true
profile = { width = 640, height = 480, fps = 30, pixel_format = "rgb24" }

[[pairings]]
leader_device = "cam"
leader_channel_type = "color"
follower_device = "cam"
follower_channel_type = "color"
mapping = "direct-joint"
leader_state = "joint_position"
follower_command = "joint_position"

[encoder]
video_codec = "h264"
depth_codec = "rvl"

[storage]
backend = "local"
output_path = "./out"
"#;
    let err = ProjectConfig::from_str(toml_text).expect_err("camera pairing should fail");
    assert!(
        err.to_string().contains("must target robot channels"),
        "unexpected error: {err}"
    );
}

#[test]
fn schema_export_is_v2_and_includes_nested_sections() {
    let schema = build_config_schema();
    assert_eq!(schema.format, "rollio-config-schema");
    assert_eq!(schema.version, 2);

    let section_ids = schema
        .sections
        .iter()
        .map(|section| section.name)
        .collect::<Vec<_>>();
    assert!(section_ids.contains(&"devices"));
    assert!(section_ids.contains(&"devices.channels"));
    assert!(section_ids.contains(&"pairings"));
}
