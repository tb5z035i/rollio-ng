use rollio_types::config::*;
use std::str::FromStr;

#[test]
fn parse_example_config() {
    let toml_text = include_str!("../../config/config.example.toml");
    let config = Config::from_str(toml_text).expect("config.example.toml should parse");
    assert_eq!(config.devices.len(), 4);
    assert_eq!(config.pairing.len(), 1);
    assert_eq!(config.episode.fps, 30);
    assert_eq!(config.episode.format, EpisodeFormat::LeRobotV2_1);
    assert_eq!(config.encoder.codec, "libx264");
    assert_eq!(config.storage.backend, StorageBackend::Local);
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

[encoder]
codec = "libx264"

[storage]
backend = "local"
output_path = "./out"
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

[[devices]]
name = "cam"
type = "camera"
driver = "pseudo"
id = "c1"

[encoder]
codec = "libx264"

[storage]
backend = "local"
output_path = "./out"
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

[encoder]
codec = "nonexistent"

[storage]
backend = "local"
output_path = "./out"
"#;
    let err = Config::from_str(toml_text).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("nonexistent"),
        "error should name the bad codec: {msg}"
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
