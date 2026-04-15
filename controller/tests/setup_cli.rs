use rollio_types::config::Config;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root should resolve")
        .to_path_buf()
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{suffix}"));
    fs::create_dir_all(&path).expect("temp dir should be created");
    path
}

#[cfg(unix)]
fn write_fake_camera_driver(dir: &Path) {
    let script_path = dir.join("rollio-camera-pseudo");
    fs::write(
        &script_path,
        r#"#!/usr/bin/env bash
set -euo pipefail
subcommand="${1:-}"
case "${subcommand}" in
  probe)
    printf '[{"id":"pseudo_cam_0","name":"Pseudo Top","driver":"pseudo","type":"camera"},{"id":"pseudo_cam_1","name":"Pseudo Side","driver":"pseudo","type":"camera"}]\n'
    ;;
  validate)
    printf '{"valid":true,"id":"%s"}\n' "${2:-pseudo_cam_0}"
    ;;
  capabilities)
    printf '{"id":"%s","pixel_formats":["rgb24"],"streams":["color"],"profiles":[{"width":640,"height":480,"fps":30},{"width":1280,"height":720,"fps":30}]}\n' "${2:-pseudo_cam_0}"
    ;;
  *)
    echo "unexpected subcommand: ${subcommand}" >&2
    exit 1
    ;;
esac
"#,
    )
    .expect("fake camera driver should be written");
    let mut permissions = fs::metadata(&script_path)
        .expect("metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script_path, permissions).expect("script should be executable");
}

#[cfg(unix)]
fn path_with_fake_camera_driver(dir: &Path) -> String {
    let existing = std::env::var("PATH").unwrap_or_default();
    format!("{}:{existing}", dir.display())
}

#[cfg(unix)]
#[test]
fn setup_accept_defaults_writes_valid_config() {
    let workspace_root = workspace_root();
    let fixture_dir = unique_temp_dir("rollio-setup-fixture");
    let output_path = fixture_dir.join("generated.setup.toml");
    write_fake_camera_driver(&fixture_dir);

    let output = Command::new(env!("CARGO_BIN_EXE_rollio"))
        .arg("setup")
        .arg("--sim-cameras")
        .arg("2")
        .arg("--sim-arms")
        .arg("2")
        .arg("--accept-defaults")
        .arg("--output")
        .arg(&output_path)
        .env("PATH", path_with_fake_camera_driver(&fixture_dir))
        .current_dir(&workspace_root)
        .output()
        .expect("setup command should run");

    assert!(
        output.status.success(),
        "setup should succeed, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output_path.exists(), "setup should write the config file");

    let config = Config::from_file(&output_path).expect("generated config should parse");
    assert_eq!(config.project_name, "default");
    assert_eq!(config.mode, rollio_types::config::CollectionMode::Teleop);
    assert_eq!(config.encoder.video_codec.as_str(), "h264");
    assert_eq!(config.encoder.depth_codec.as_str(), "rvl");
    let camera_names = config.camera_names();
    let robot_names = config.robot_names();
    assert!(camera_names.iter().any(|name| name == "pseudo_camera"));
    assert!(camera_names.iter().any(|name| name == "pseudo_camera_2"));
    assert!(robot_names.iter().any(|name| name == "pseudo_arm"));
    assert!(robot_names.iter().any(|name| name == "pseudo_arm_2"));
    assert!(
        config
            .pairing
            .iter()
            .any(|pair| pair.leader == "pseudo_arm" && pair.follower == "pseudo_arm_2"),
        "default setup should include an arm leader/follower pairing"
    );
}

#[cfg(unix)]
#[test]
fn setup_resume_path_rewrites_existing_config() {
    let workspace_root = workspace_root();
    let fixture_dir = unique_temp_dir("rollio-setup-resume");
    let original_path = fixture_dir.join("original.setup.toml");
    let resumed_path = fixture_dir.join("resumed.setup.toml");
    write_fake_camera_driver(&fixture_dir);

    let first = Command::new(env!("CARGO_BIN_EXE_rollio"))
        .arg("setup")
        .arg("--sim-cameras")
        .arg("2")
        .arg("--sim-arms")
        .arg("2")
        .arg("--accept-defaults")
        .arg("--output")
        .arg(&original_path)
        .env("PATH", path_with_fake_camera_driver(&fixture_dir))
        .current_dir(&workspace_root)
        .output()
        .expect("initial setup should run");
    assert!(
        first.status.success(),
        "initial setup should succeed, stderr={}",
        String::from_utf8_lossy(&first.stderr)
    );

    let resumed = Command::new(env!("CARGO_BIN_EXE_rollio"))
        .arg("setup")
        .arg("--config")
        .arg(&original_path)
        .arg("--accept-defaults")
        .arg("--output")
        .arg(&resumed_path)
        .env("PATH", path_with_fake_camera_driver(&fixture_dir))
        .current_dir(&workspace_root)
        .output()
        .expect("resume setup should run");
    assert!(
        resumed.status.success(),
        "resume setup should succeed, stderr={}",
        String::from_utf8_lossy(&resumed.stderr)
    );

    let original = Config::from_file(&original_path).expect("original config should parse");
    let resumed = Config::from_file(&resumed_path).expect("resumed config should parse");
    assert_eq!(original.project_name, resumed.project_name);
    assert_eq!(original.mode, resumed.mode);
    assert_eq!(original.encoder.video_codec, resumed.encoder.video_codec);
    assert_eq!(original.encoder.depth_codec, resumed.encoder.depth_codec);
    assert_eq!(original.camera_names(), resumed.camera_names());
    assert_eq!(original.robot_names(), resumed.robot_names());
    let original_pairs = original
        .pairing
        .iter()
        .map(|pair| {
            (
                pair.leader.as_str(),
                pair.follower.as_str(),
                pair.mapping,
                pair.joint_index_map.as_slice(),
                pair.joint_scales.as_slice(),
            )
        })
        .collect::<Vec<_>>();
    let resumed_pairs = resumed
        .pairing
        .iter()
        .map(|pair| {
            (
                pair.leader.as_str(),
                pair.follower.as_str(),
                pair.mapping,
                pair.joint_index_map.as_slice(),
                pair.joint_scales.as_slice(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(original_pairs, resumed_pairs);
    assert_eq!(original.episode.format, resumed.episode.format);
}
