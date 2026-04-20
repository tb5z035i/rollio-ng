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
    // The follower publishes joint_position state, so the controller must
    // have wired the optional follower-state fields so the teleop router can
    // run its initial syncing phase. Defaults match the user-spec values
    // (0.005 rad max step, 0.05 rad completion threshold).
    assert_eq!(
        teleop_configs[0].follower_state_kind,
        Some(RobotStateKind::JointPosition)
    );
    assert_eq!(
        teleop_configs[0].follower_state_topic.as_deref(),
        Some("follower_arm/arm/states/joint_position"),
    );
    assert_eq!(teleop_configs[0].sync_max_step_rad, Some(0.005));
    assert_eq!(teleop_configs[0].sync_complete_threshold_rad, Some(0.05));

    let assembler_runtime = config.assembler_runtime_config_v2(toml_text.to_string());
    assert_eq!(assembler_runtime.cameras.len(), 2);
    assert_eq!(assembler_runtime.observations.len(), 6);
    assert_eq!(assembler_runtime.actions.len(), 1);

    let storage_runtime = config.storage_runtime_config();
    assert_eq!(storage_runtime.process_id, "storage");
    assert_eq!(storage_runtime.queue_size, 32);
}

/// The wizard binds the browser UI server to all interfaces by default so a
/// fresh project can be opened from another machine on the LAN without
/// hand-editing the TOML. Operators that want loopback-only access can
/// edit the field in the wizard's settings step.
#[test]
fn ui_runtime_config_defaults_to_all_interfaces() {
    assert_eq!(UiRuntimeConfig::default().http_host, "0.0.0.0");
    let config = ProjectConfig::draft_setup_template();
    assert_eq!(config.ui.http_host, "0.0.0.0");
}

/// Per-codec backends should default to inheriting the legacy global
/// `backend` field so loading an older TOML produces the same encoder
/// configuration. Reading a TOML with explicit per-codec backends must
/// override the shared default.
#[test]
fn encoder_config_inherits_backend_per_codec_for_legacy_configs() {
    let inherited: EncoderConfig = toml::from_str(
        r#"
video_codec = "h264"
depth_codec = "rvl"
backend = "nvidia"
"#,
    )
    .expect("legacy encoder config should parse");
    assert_eq!(inherited.video_backend, EncoderBackend::Nvidia);
    // RVL has no GPU acceleration path; the migration path must downgrade
    // depth_backend to CPU rather than emit an invalid Nvidia/RVL pair.
    assert_eq!(inherited.depth_backend, EncoderBackend::Cpu);

    let explicit: EncoderConfig = toml::from_str(
        r#"
video_codec = "av1"
depth_codec = "h265"
backend = "auto"
video_backend = "vaapi"
depth_backend = "nvidia"
"#,
    )
    .expect("encoder config with per-codec backends should parse");
    assert_eq!(explicit.video_backend, EncoderBackend::Vaapi);
    assert_eq!(explicit.depth_backend, EncoderBackend::Nvidia);
}

/// `EncoderRuntimeConfigV2` is what the controller hands to each encoder
/// child process. Its `backend` field must come from the per-codec
/// `video_backend` / `depth_backend` selection instead of the shared
/// fallback so an operator that picked, e.g., NVIDIA AV1 for color and
/// CPU RVL for depth gets each encoder bound to the right device.
#[test]
fn encoder_runtime_configs_v2_use_per_pixel_format_backend() {
    let toml_text = r#"
project_name = "mixed-backends"
mode = "intervention"

[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "rs"
driver = "realsense"
id = "332322071743"
bus_root = "rs"

[[devices.channels]]
channel_type = "color"
kind = "camera"
profile = { width = 640, height = 480, fps = 30, pixel_format = "rgb24" }

[[devices.channels]]
channel_type = "depth"
kind = "camera"
profile = { width = 640, height = 480, fps = 30, pixel_format = "depth16" }

[encoder]
video_codec = "av1"
video_backend = "nvidia"
depth_codec = "rvl"
depth_backend = "cpu"

[storage]
backend = "local"
output_path = "./out"

[visualizer]
port = 19090
"#;
    let config = ProjectConfig::from_str(toml_text).expect("config should parse");
    let encoders = config.encoder_runtime_configs_v2();
    let color = encoders
        .iter()
        .find(|cfg| cfg.channel_id == "rs/color")
        .expect("color encoder runtime should be derived");
    assert_eq!(color.codec, EncoderCodec::Av1);
    assert_eq!(color.backend, EncoderBackend::Nvidia);
    let depth = encoders
        .iter()
        .find(|cfg| cfg.channel_id == "rs/depth")
        .expect("depth encoder runtime should be derived");
    assert_eq!(depth.codec, EncoderCodec::Rvl);
    assert_eq!(depth.backend, EncoderBackend::Cpu);
}

/// Validation must catch an attempt to pair RVL with a GPU backend so the
/// wizard never offers the operator a configuration that the encoder
/// process would reject at startup.
#[test]
fn encoder_config_rejects_rvl_with_gpu_backend() {
    let toml_text = r#"
project_name = "rvl-vaapi"
mode = "intervention"

[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "rs"
driver = "realsense"
id = "332322071743"
bus_root = "rs"

[[devices.channels]]
channel_type = "depth"
kind = "camera"
profile = { width = 640, height = 480, fps = 30, pixel_format = "depth16" }

[encoder]
video_codec = "h264"
depth_codec = "rvl"
depth_backend = "vaapi"

[storage]
backend = "local"
output_path = "./out"

[visualizer]
port = 19090
"#;
    let error = ProjectConfig::from_str(toml_text).expect_err("rvl + vaapi should fail validation");
    assert!(
        error.to_string().contains("rvl only supports cpu"),
        "unexpected error: {error}"
    );
}

#[test]
fn visualizer_runtime_config_v2_derives_sources_from_enabled_channels() {
    let config = include_str!("../../config/config.example.toml")
        .parse::<ProjectConfig>()
        .expect("example config should parse");
    let visualizer = config.visualizer_runtime_config_v2();
    assert_eq!(visualizer.camera_sources.len(), 2);
    assert_eq!(visualizer.robot_sources.len(), 8);
    assert_eq!(
        visualizer.camera_sources[0].frame_topic,
        "camera_top/color/frames"
    );
}

/// Configuring more than three camera channels keeps the recording pipeline
/// intact (each camera still gets its own encoder) but the visualizer-bound
/// preview list is truncated so the UI never has to render more than
/// `MAX_PREVIEW_CAMERAS` tiles. This guarantees each tile keeps the 16:10
/// box without shrinking below the readability threshold.
#[test]
fn visualizer_runtime_config_v2_caps_camera_sources_at_max_preview() {
    let toml_text = r#"
project_name = "many-cameras"
mode = "intervention"

[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "cam_a"
driver = "pseudo"
id = "pseudo_camera_a"
bus_root = "cam_a"
[[devices.channels]]
channel_type = "color"
kind = "camera"
profile = { width = 640, height = 480, fps = 30, pixel_format = "rgb24" }

[[devices]]
name = "cam_b"
driver = "pseudo"
id = "pseudo_camera_b"
bus_root = "cam_b"
[[devices.channels]]
channel_type = "color"
kind = "camera"
profile = { width = 640, height = 480, fps = 30, pixel_format = "rgb24" }

[[devices]]
name = "cam_c"
driver = "pseudo"
id = "pseudo_camera_c"
bus_root = "cam_c"
[[devices.channels]]
channel_type = "color"
kind = "camera"
profile = { width = 640, height = 480, fps = 30, pixel_format = "rgb24" }

[[devices]]
name = "cam_d"
driver = "pseudo"
id = "pseudo_camera_d"
bus_root = "cam_d"
[[devices.channels]]
channel_type = "color"
kind = "camera"
profile = { width = 640, height = 480, fps = 30, pixel_format = "rgb24" }

[[devices]]
name = "cam_e"
driver = "pseudo"
id = "pseudo_camera_e"
bus_root = "cam_e"
[[devices.channels]]
channel_type = "color"
kind = "camera"
profile = { width = 640, height = 480, fps = 30, pixel_format = "rgb24" }

[encoder]
video_codec = "h264"
depth_codec = "rvl"

[storage]
backend = "local"
output_path = "./out"

[visualizer]
port = 19090
"#;
    let config = ProjectConfig::from_str(toml_text).expect("config should parse");
    // Sanity check: every camera is still in the encoder pipeline so they
    // all get recorded — the cap only affects the preview tiles.
    let encoders = config.encoder_runtime_configs_v2();
    assert_eq!(encoders.len(), 5, "every camera should still be recorded");

    let visualizer = config.visualizer_runtime_config_v2();
    assert_eq!(
        visualizer.camera_sources.len(),
        MAX_PREVIEW_CAMERAS,
        "preview row should be capped at MAX_PREVIEW_CAMERAS"
    );
    let preview_names: Vec<_> = visualizer
        .camera_sources
        .iter()
        .map(|source| source.channel_id.as_str())
        .collect();
    assert_eq!(
        preview_names,
        vec!["cam_a/color", "cam_b/color", "cam_c/color"]
    );
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

/// Regression: when `depth_codec=rvl` and a camera channel publishes
/// `gray8` frames (e.g. RealSense infrared), the encoder must use the
/// video codec instead. RVL is depth-only and physically rejects
/// non-`Depth16` frames, so without the fallback the infrared encoder
/// process exits at episode start with `rvl requires depth16 frames,
/// got Gray8`.
#[test]
fn gray8_infrared_falls_back_to_video_codec_when_depth_codec_is_rvl() {
    let encoder = EncoderConfig {
        video_codec: EncoderCodec::H264,
        depth_codec: EncoderCodec::Rvl,
        ..EncoderConfig::default()
    };

    assert_eq!(
        encoder.codec_for_pixel_format(PixelFormat::Depth16),
        EncoderCodec::Rvl,
        "depth16 still routes to depth_codec",
    );
    assert_eq!(
        encoder.codec_for_pixel_format(PixelFormat::Gray8),
        EncoderCodec::H264,
        "gray8 must fall back to video_codec when depth_codec=rvl",
    );
    assert_eq!(
        encoder.codec_for_pixel_format(PixelFormat::Rgb24),
        EncoderCodec::H264,
    );
}

/// When the operator picks a depth codec that *can* encode grayscale
/// frames (any libav-backed codec), `gray8` should keep using it so that
/// infrared streams stay grouped with depth in the produced artifacts.
#[test]
fn gray8_infrared_uses_depth_codec_when_depth_codec_supports_it() {
    let encoder = EncoderConfig {
        video_codec: EncoderCodec::H264,
        depth_codec: EncoderCodec::H265,
        ..EncoderConfig::default()
    };

    assert_eq!(
        encoder.codec_for_pixel_format(PixelFormat::Gray8),
        EncoderCodec::H265,
    );
}

/// `value_limits` are no longer persisted in TOML: the controller refreshes
/// them from a fresh `query` invocation on every startup and feeds them into
/// the in-memory channel config before downstream consumers (visualizer)
/// build their runtime configs. This test confirms that
/// 1) the field is silently ignored if it appears in older configs (no parse
///    error), and 2) once populated programmatically, the visualizer runtime
///    config still surfaces the per-source value envelopes.
#[test]
fn value_limits_are_runtime_only() {
    let toml_text = r#"
project_name = "limits"
mode = "intervention"

[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "arm"
driver = "pseudo"
id = "pseudo_robot_0_dof_6"
bus_root = "arm"

[[devices.channels]]
channel_type = "arm"
kind = "robot"
enabled = true
mode = "free-drive"
dof = 6
publish_states = ["joint_position", "joint_velocity"]

[encoder]
video_codec = "h264"
depth_codec = "rvl"

[storage]
backend = "local"
output_path = "./out"

[visualizer]
port = 19090
"#;
    let mut config = ProjectConfig::from_str(toml_text).expect("config should parse");
    let channel = &mut config.devices[0].channels[0];
    assert!(
        channel.value_limits.is_empty(),
        "fresh load should carry no value_limits until the controller refreshes them",
    );

    // Simulate the controller injecting the latest driver-reported limits
    // into the in-memory config (this happens after the device `query`).
    channel.value_limits = vec![
        StateValueLimitsEntry::new(
            RobotStateKind::JointPosition,
            vec![-3.14, -2.96, -0.087, -3.01, -1.76, -3.01],
            vec![2.094, 0.174, 3.14, 3.01, 1.76, 3.01],
        ),
        StateValueLimitsEntry::symmetric(RobotStateKind::JointVelocity, 3.14, 6),
    ];

    // The visualizer runtime config exposes per-source value envelopes so
    // the WebSocket payload can carry them to the UI bars.
    let visualizer = config.visualizer_runtime_config_v2();
    let position_source = visualizer
        .robot_sources
        .iter()
        .find(|source| source.state_kind == RobotStateKind::JointPosition)
        .expect("joint_position source should be present");
    assert_eq!(position_source.value_min[0], -3.14);
    assert_eq!(position_source.value_max[0], 2.094);

    // Re-serializing must NOT include value_limits, even if we just populated
    // the in-memory copy.
    let serialized = toml::to_string(&config).expect("config should re-serialize");
    assert!(
        !serialized.contains("value_limits"),
        "serialized TOML must omit value_limits; got:\n{serialized}",
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

// -----------------------------------------------------------------------
// Teleop policy validator tests (introduced with the three-policy redesign).
// -----------------------------------------------------------------------

fn parallel_pair_config_template() -> ProjectConfig {
    let toml_text = r#"
project_name = "parallel-test"
mode = "teleop"

[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "lead"
driver = "pseudo"
id = "lead0"
bus_root = "lead"

[[devices.channels]]
channel_type = "gripper"
kind = "robot"
enabled = true
mode = "free-drive"
dof = 1
publish_states = ["parallel_position"]
recorded_states = ["parallel_position"]

[[devices]]
name = "follow"
driver = "pseudo"
id = "follow0"
bus_root = "follow"

[[devices.channels]]
channel_type = "gripper"
kind = "robot"
enabled = true
mode = "command-following"
dof = 1
publish_states = ["parallel_position"]
recorded_states = ["parallel_position"]

[[pairings]]
leader_device = "lead"
leader_channel_type = "gripper"
follower_device = "follow"
follower_channel_type = "gripper"
mapping = "parallel"
leader_state = "parallel_position"
follower_command = "parallel_position"
joint_index_map = []
joint_scales = [1.0]

[encoder]
video_codec = "h264"
depth_codec = "rvl"

[storage]
backend = "local"
output_path = "./output"

[visualizer]
port = 19090
"#;
    ProjectConfig::from_str(toml_text).expect("parallel template should parse")
}

#[test]
fn parallel_pairing_requires_dof_one_and_parallel_position() {
    let mut config = parallel_pair_config_template();

    // Baseline: dof=1 + parallel_position both sides + ratio=1.0 validates.
    config
        .validate()
        .expect("baseline parallel pair should validate");

    // dof != 1 on the leader trips the dof-1 predicate.
    config.devices[0].channels[0].dof = Some(2);
    let err = config
        .validate()
        .expect_err("dof=2 leader must be rejected for parallel mapping");
    assert!(
        format!("{err}").contains("requires dof=1"),
        "rejection should name the dof requirement, got: {err}",
    );

    // Restore dof + replace parallel_position with joint_position in
    // publish_states: parallel mapping should reject because the leader
    // no longer publishes the kind it requires.
    config.devices[0].channels[0].dof = Some(1);
    config.devices[0].channels[0].publish_states = vec![RobotStateKind::JointPosition];
    config.devices[0].channels[0].recorded_states = vec![RobotStateKind::JointPosition];
    let err = config
        .validate()
        .expect_err("missing parallel_position must be rejected");
    assert!(
        format!("{err}").to_lowercase().contains("parallelposition")
            || format!("{err}").contains("parallel_position"),
        "rejection should mention parallel_position, got: {err}",
    );
}

#[test]
fn parallel_pairing_rejects_zero_and_non_finite_ratio() {
    let mut config = parallel_pair_config_template();

    config.pairings[0].joint_scales = vec![0.0];
    let err = config.validate().expect_err("ratio=0 must be rejected");
    assert!(
        format!("{err}").contains("non-zero"),
        "ratio=0 rejection should mention non-zero, got: {err}",
    );

    config.pairings[0].joint_scales = vec![f64::NAN];
    let err = config.validate().expect_err("ratio=NaN must be rejected");
    assert!(
        format!("{err}").contains("finite"),
        "NaN rejection should mention finite, got: {err}",
    );
}

fn direct_joint_pair_config_template() -> ProjectConfig {
    let toml_text = r#"
project_name = "direct-joint-test"
mode = "teleop"

[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "lead"
driver = "pseudo-a"
id = "lead0"
bus_root = "lead"

[[devices.channels]]
channel_type = "arm"
kind = "robot"
enabled = true
mode = "free-drive"
dof = 6
publish_states = ["joint_position"]
recorded_states = ["joint_position"]

[[devices]]
name = "follow"
driver = "pseudo-b"
id = "follow0"
bus_root = "follow"

[[devices.channels]]
channel_type = "arm"
kind = "robot"
enabled = true
mode = "command-following"
dof = 6
publish_states = ["joint_position"]
recorded_states = ["joint_position"]

[[pairings]]
leader_device = "lead"
leader_channel_type = "arm"
follower_device = "follow"
follower_channel_type = "arm"
mapping = "direct-joint"
leader_state = "joint_position"
follower_command = "joint_position"
joint_index_map = [0, 1, 2, 3, 4, 5]
joint_scales = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0]

[encoder]
video_codec = "h264"
depth_codec = "rvl"

[storage]
backend = "local"
output_path = "./output"

[visualizer]
port = 19090
"#;
    ProjectConfig::from_str(toml_text).expect("direct-joint template should parse")
}

#[test]
fn direct_joint_requires_two_sided_whitelist() {
    let mut config = direct_joint_pair_config_template();

    // Without any whitelist entries the validator skips the whitelist
    // check (loaded TOML can't carry the runtime field). Round-trip OK.
    config
        .validate()
        .expect("baseline (no whitelist) should validate");

    // Now opt the leader's `can_lead` in but leave the follower's
    // `can_follow` empty -- expect the two-sided check to reject.
    config.devices[0].channels[0]
        .direct_joint_compatibility
        .can_lead = vec![DirectJointCompatibilityPeer {
        driver: "pseudo-b".into(),
        channel_type: "arm".into(),
    }];
    let err = config
        .validate()
        .expect_err("one-sided whitelist must be rejected");
    let msg = format!("{err}");
    assert!(
        msg.contains("can_follow") && msg.contains("pseudo-b"),
        "rejection should name the missing follower endorsement, got: {err}",
    );

    // Symmetric opt-in: the pair becomes valid again.
    config.devices[1].channels[0]
        .direct_joint_compatibility
        .can_follow = vec![DirectJointCompatibilityPeer {
        driver: "pseudo-a".into(),
        channel_type: "arm".into(),
    }];
    config
        .validate()
        .expect("two-sided whitelist match should validate");

    // Drop the leader endorsement: rejected again, this time naming
    // the missing leader-side entry.
    config.devices[0].channels[0]
        .direct_joint_compatibility
        .can_lead
        .clear();
    let err = config
        .validate()
        .expect_err("removing leader endorsement must re-reject");
    assert!(
        format!("{err}").contains("can_lead"),
        "rejection should name the missing leader endorsement, got: {err}",
    );
}
