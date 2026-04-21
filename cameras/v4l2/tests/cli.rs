use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_rollio-device-v4l2")
}

#[test]
fn probe_outputs_json_array() {
    let output = Command::new(bin())
        .args(["probe", "--json"])
        .output()
        .expect("probe command should run");

    assert!(
        output.status.success(),
        "probe should succeed, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    assert!(
        trimmed.starts_with('[') && trimmed.ends_with(']'),
        "probe output should be a JSON array, got {trimmed:?}"
    );
}

#[test]
fn validate_rejects_non_v4l2_path() {
    let output = Command::new(bin())
        .args(["validate", "/dev/null"])
        .output()
        .expect("validate command should run");

    assert!(
        !output.status.success(),
        "validate should fail for /dev/null"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not a V4L2 capture device")
            || stderr.contains("Inappropriate ioctl")
            || stderr.contains("rollio-device-v4l2"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn run_rejects_unsupported_output_format() {
    // depth16 is not supported by V4L2 webcams; the driver must reject
    // it before opening the device. mjpeg / yuyv are now valid bus
    // formats and must NOT be rejected here.
    let config = r#"name = "cam"
driver = "v4l2"
id = "/dev/video0"
bus_root = "cam"

[[channels]]
channel_type = "color"
kind = "camera"
profile = { width = 640, height = 480, fps = 30, pixel_format = "depth16" }
"#;

    let output = Command::new(bin())
        .args(["run", "--config-inline", config])
        .output()
        .expect("run command should run");

    assert!(
        !output.status.success(),
        "run should fail for depth16 output config"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("rgb24, bgr24, yuyv, mjpeg"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn run_rejects_non_device_path() {
    let config = r#"name = "cam"
driver = "v4l2"
id = "/dev/null"
bus_root = "cam"

[[channels]]
channel_type = "color"
kind = "camera"
profile = { width = 640, height = 480, fps = 30, pixel_format = "rgb24" }
"#;

    let output = Command::new(bin())
        .args(["run", "--config-inline", config])
        .output()
        .expect("run command should run");

    assert!(
        !output.status.success(),
        "run should fail for non-device path"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not a V4L2 capture device")
            || stderr.contains("Inappropriate ioctl")
            || stderr.contains("rollio-device-v4l2"),
        "unexpected stderr: {stderr}"
    );
}
