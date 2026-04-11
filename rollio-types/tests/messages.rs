use rollio_types::messages::*;

// ---------------------------------------------------------------------------
// Round-trip: write a #[repr(C)] struct to bytes, read it back, compare.
// ---------------------------------------------------------------------------

unsafe fn roundtrip<T: Clone + Sized>(val: &T) -> T {
    let size = core::mem::size_of::<T>();
    let src = val as *const T as *const u8;
    let bytes = core::slice::from_raw_parts(src, size);
    let mut buf = vec![0u8; size];
    buf.copy_from_slice(bytes);
    let ptr = buf.as_ptr() as *const T;
    (*ptr).clone()
}

#[test]
fn camera_frame_header_roundtrip() {
    let hdr = CameraFrameHeader {
        timestamp_ns: 123_456_789,
        width: 640,
        height: 480,
        pixel_format: PixelFormat::Rgb24,
        frame_index: 42,
    };
    let hdr2 = unsafe { roundtrip(&hdr) };
    assert_eq!(hdr2.timestamp_ns, 123_456_789);
    assert_eq!(hdr2.width, 640);
    assert_eq!(hdr2.height, 480);
    assert!(matches!(hdr2.pixel_format, PixelFormat::Rgb24));
    assert_eq!(hdr2.frame_index, 42);
}

#[test]
fn camera_frame_header_payload_size_rgb24() {
    let hdr = CameraFrameHeader {
        timestamp_ns: 0,
        width: 640,
        height: 480,
        pixel_format: PixelFormat::Rgb24,
        frame_index: 0,
    };
    assert_eq!(hdr.payload_size(), 640 * 480 * 3);
}

#[test]
fn camera_frame_header_payload_size_depth16() {
    let hdr = CameraFrameHeader {
        timestamp_ns: 0,
        width: 640,
        height: 480,
        pixel_format: PixelFormat::Depth16,
        frame_index: 0,
    };
    assert_eq!(hdr.payload_size(), 640 * 480 * 2);
}

// ---------------------------------------------------------------------------
// RobotState
// ---------------------------------------------------------------------------

#[test]
fn robot_state_6_joints_roundtrip() {
    let mut state = RobotState::default();
    state.num_joints = 6;
    state.positions[0] = 0.1;
    state.positions[1] = 0.2;
    state.positions[2] = 0.3;
    state.positions[3] = 0.4;
    state.positions[4] = 0.5;
    state.positions[5] = 0.6;
    state.velocities[0] = 1.0;
    state.efforts[5] = -2.5;
    state.timestamp_ns = 999;

    let state2 = unsafe { roundtrip(&state) };
    assert_eq!(state2.num_joints, 6);
    assert_eq!(state2.timestamp_ns, 999);
    for i in 0..6 {
        assert_eq!(state2.positions[i], state.positions[i]);
    }
    assert_eq!(state2.velocities[0], 1.0);
    assert_eq!(state2.efforts[5], -2.5);
    assert!(!state2.has_ee_pose);
}

#[test]
fn robot_state_1_joint_roundtrip() {
    let mut state = RobotState::default();
    state.num_joints = 1;
    state.positions[0] = 0.035;
    state.velocities[0] = -0.25;
    state.efforts[0] = 1.2;
    state.end_effector_status = EndEffectorStatus::Enabled;
    state.has_end_effector_status = true;
    state.end_effector_feedback_valid = true;

    let state2 = unsafe { roundtrip(&state) };
    assert_eq!(state2.num_joints, 1);
    assert_eq!(state2.positions[0], 0.035);
    assert_eq!(state2.velocities[0], -0.25);
    assert_eq!(state2.efforts[0], 1.2);
    assert_eq!(state2.end_effector_status, EndEffectorStatus::Enabled);
    assert!(state2.has_end_effector_status);
    assert!(state2.end_effector_feedback_valid);
}

#[test]
fn robot_state_ee_pose_present() {
    let mut state = RobotState::default();
    state.num_joints = 6;
    state.has_ee_pose = true;
    state.ee_pose = [0.3, 0.0, 0.5, 0.0, 0.0, 0.0, 1.0];

    let state2 = unsafe { roundtrip(&state) };
    assert!(state2.has_ee_pose);
    assert_eq!(state2.ee_pose[0], 0.3);
    assert_eq!(state2.ee_pose[6], 1.0);
}

#[test]
fn robot_state_ee_pose_absent() {
    let state = RobotState::default();
    assert!(!state.has_ee_pose);
}

#[test]
fn end_effector_status_as_str_matches_wire_values() {
    assert_eq!(EndEffectorStatus::Unknown.as_str(), "unknown");
    assert_eq!(EndEffectorStatus::Disabled.as_str(), "disabled");
    assert_eq!(EndEffectorStatus::Enabled.as_str(), "enabled");
}

// ---------------------------------------------------------------------------
// RobotCommand
// ---------------------------------------------------------------------------

#[test]
fn robot_command_roundtrip() {
    let mut cmd = RobotCommand::default();
    cmd.num_joints = 6;
    cmd.joint_targets[0] = 1.5;
    cmd.mode = CommandMode::Joint;
    cmd.timestamp_ns = 42;

    let cmd2 = unsafe { roundtrip(&cmd) };
    assert_eq!(cmd2.num_joints, 6);
    assert_eq!(cmd2.joint_targets[0], 1.5);
    assert!(matches!(cmd2.mode, CommandMode::Joint));
}

// ---------------------------------------------------------------------------
// ControlEvent — all variants
// ---------------------------------------------------------------------------

#[test]
fn control_event_all_variants() {
    let variants = [
        ControlEvent::RecordingStart { episode_index: 0 },
        ControlEvent::RecordingStop { episode_index: 1 },
        ControlEvent::EpisodeKeep { episode_index: 2 },
        ControlEvent::EpisodeDiscard { episode_index: 3 },
        ControlEvent::Shutdown,
        ControlEvent::ModeSwitch { target_mode: 1 },
    ];
    for evt in &variants {
        let evt2 = unsafe { roundtrip(evt) };
        assert_eq!(core::mem::discriminant(evt), core::mem::discriminant(&evt2));
    }
}

#[test]
fn control_event_recording_start_payload() {
    let evt = ControlEvent::RecordingStart { episode_index: 7 };
    let evt2 = unsafe { roundtrip(&evt) };
    match evt2 {
        ControlEvent::RecordingStart { episode_index } => assert_eq!(episode_index, 7),
        _ => panic!("wrong variant"),
    }
}

// ---------------------------------------------------------------------------
// EpisodeCommand / EpisodeStatus
// ---------------------------------------------------------------------------

#[test]
fn episode_command_all_variants_roundtrip() {
    let variants = [
        EpisodeCommand::Start,
        EpisodeCommand::Stop,
        EpisodeCommand::Keep,
        EpisodeCommand::Discard,
    ];
    for command in &variants {
        let roundtripped = unsafe { roundtrip(command) };
        assert_eq!(
            core::mem::discriminant(command),
            core::mem::discriminant(&roundtripped)
        );
    }
}

#[test]
fn episode_status_roundtrip() {
    let status = EpisodeStatus {
        state: EpisodeState::Recording,
        episode_count: 3,
        elapsed_ms: 5_250,
    };
    let roundtripped = unsafe { roundtrip(&status) };
    assert_eq!(roundtripped.state, EpisodeState::Recording);
    assert_eq!(roundtripped.episode_count, 3);
    assert_eq!(roundtripped.elapsed_ms, 5_250);
}

// ---------------------------------------------------------------------------
// MetricsReport
// ---------------------------------------------------------------------------

#[test]
fn metrics_report_roundtrip() {
    let mut report = MetricsReport::default();
    report.process_id = FixedString64::new("encoder.camera_top");
    report.timestamp_ns = 1_000_000;
    report.num_entries = 2;
    report.entries[0] = MetricEntry {
        name: FixedString64::new("queue_depth"),
        value: 12.0,
    };
    report.entries[1] = MetricEntry {
        name: FixedString64::new("latency_ms"),
        value: 3.5,
    };

    let r2 = unsafe { roundtrip(&report) };
    assert_eq!(r2.process_id.as_str(), "encoder.camera_top");
    assert_eq!(r2.num_entries, 2);
    assert_eq!(r2.entries[0].name.as_str(), "queue_depth");
    assert_eq!(r2.entries[0].value, 12.0);
    assert_eq!(r2.entries[1].name.as_str(), "latency_ms");
}

// ---------------------------------------------------------------------------
// WarningEvent
// ---------------------------------------------------------------------------

#[test]
fn warning_event_roundtrip() {
    let w = WarningEvent {
        process_id: FixedString64::new("camera.top"),
        metric_name: FixedString64::new("actual_fps"),
        current_value: 15.0,
        explanation: FixedString256::new("FPS dropped below target"),
    };
    let w2 = unsafe { roundtrip(&w) };
    assert_eq!(w2.process_id.as_str(), "camera.top");
    assert_eq!(w2.metric_name.as_str(), "actual_fps");
    assert_eq!(w2.current_value, 15.0);
    assert_eq!(w2.explanation.as_str(), "FPS dropped below target");
}

// ---------------------------------------------------------------------------
// VideoReady
// ---------------------------------------------------------------------------

#[test]
fn video_ready_roundtrip() {
    let v = VideoReady {
        process_id: FixedString64::new("encoder.cam0"),
        episode_index: 5,
        file_path: FixedString256::new("/tmp/ep_005.mp4"),
    };
    let v2 = unsafe { roundtrip(&v) };
    assert_eq!(v2.process_id.as_str(), "encoder.cam0");
    assert_eq!(v2.episode_index, 5);
    assert_eq!(v2.file_path.as_str(), "/tmp/ep_005.mp4");
}

// ---------------------------------------------------------------------------
// BackpressureEvent
// ---------------------------------------------------------------------------

#[test]
fn backpressure_event_roundtrip() {
    let b = BackpressureEvent {
        process_id: FixedString64::new("storage.main"),
        queue_name: FixedString64::new("upload_queue"),
    };
    let b2 = unsafe { roundtrip(&b) };
    assert_eq!(b2.process_id.as_str(), "storage.main");
    assert_eq!(b2.queue_name.as_str(), "upload_queue");
}

// ---------------------------------------------------------------------------
// FixedString edge cases
// ---------------------------------------------------------------------------

#[test]
fn fixed_string64_truncates_long_input() {
    let long = "a".repeat(200);
    let fs = FixedString64::new(&long);
    assert_eq!(fs.len, 64);
    assert_eq!(fs.as_str().len(), 64);
}

#[test]
fn fixed_string256_truncates_long_input() {
    let long = "b".repeat(500);
    let fs = FixedString256::new(&long);
    assert_eq!(fs.len, 256);
    assert_eq!(fs.as_str().len(), 256);
}

#[test]
fn fixed_string64_empty() {
    let fs = FixedString64::new("");
    assert_eq!(fs.len, 0);
    assert_eq!(fs.as_str(), "");
}
