//! Integration test: write a synthetic episode to MCAP and read it back.

use rollio_episode_mcap::encode;
use rollio_episode_mcap::mcap_writer::{us_to_ns, McapEpisodeWriter, SchemaType};
use std::collections::BTreeMap;
use std::path::Path;

/// Resolve the bfbs directory for tests.
///
/// Mirrors `resolve_bfbs_dir` in `src/runtime.rs`: env var override first,
/// then the installed location populated by the deb package.
fn bfbs_dir() -> &'static Path {
    // 1. Env var override (set by `eval "$(make set-env)"` for in-tree dev)
    if let Ok(dir) = std::env::var("ROLLIO_BFBS_DIR") {
        let p = Path::new(Box::leak(dir.into_boxed_str()));
        if p.is_dir() {
            return p;
        }
    }
    // 2. Installed location (populated by the rollio deb package)
    let installed = Path::new("/usr/share/rollio/bfbs");
    if installed.is_dir() {
        return installed;
    }
    panic!(
        "Cannot find bfbs directory for tests. \
         Set ROLLIO_BFBS_DIR (e.g. `eval \"$(make set-env)\"`) or install the rollio deb."
    );
}

#[test]
fn test_write_and_read_mcap_episode() {
    let dir = tempfile::tempdir().unwrap();
    let mcap_path = dir.path().join("test_episode.mcap");
    let bfbs = bfbs_dir();

    // Create writer and add channels
    let mut writer = McapEpisodeWriter::new(&mcap_path, bfbs).unwrap();

    let video_ch = writer
        .add_channel("/camera/cam_left/video", SchemaType::CompressedVideo, bfbs)
        .unwrap();
    let obs_ch = writer
        .add_channel(
            "/observation/leader/joint_position",
            SchemaType::JointStates,
            bfbs,
        )
        .unwrap();
    let action_ch = writer
        .add_channel(
            "/action/follower/joint_position",
            SchemaType::JointStates,
            bfbs,
        )
        .unwrap();

    // Write some video frames
    for i in 0..10u64 {
        let ts_us = 1_000_000 + i * 33_333; // ~30fps
        let fake_nal = vec![0x00, 0x00, 0x00, 0x01, 0x65, i as u8];
        let fb_data = encode::encode_compressed_video(ts_us, "cam_left", "h264", &fake_nal);
        writer
            .write_message(video_ch, us_to_ns(ts_us), &fb_data)
            .unwrap();
    }

    // Write observation samples
    for i in 0..30u64 {
        let ts_us = 1_000_000 + i * 10_000; // 100Hz
        let values: Vec<f64> = (0..6).map(|j| (i * 6 + j) as f64 * 0.01).collect();
        let fb_data = encode::encode_joint_states(ts_us, &values, None);
        writer
            .write_message(obs_ch, us_to_ns(ts_us), &fb_data)
            .unwrap();
    }

    // Write action samples
    for i in 0..30u64 {
        let ts_us = 1_000_000 + i * 10_000;
        let values: Vec<f64> = (0..6).map(|j| (i * 6 + j) as f64 * 0.02).collect();
        let fb_data = encode::encode_joint_states(ts_us, &values, None);
        writer
            .write_message(action_ch, us_to_ns(ts_us), &fb_data)
            .unwrap();
    }

    // Write metadata
    let mut meta = BTreeMap::new();
    meta.insert("episode_index".to_string(), "0".to_string());
    meta.insert("start_time_us".to_string(), "1000000".to_string());
    meta.insert("stop_time_us".to_string(), "1300000".to_string());
    writer.write_metadata("episode", meta).unwrap();

    // Finish
    let output = writer.finish().unwrap();
    assert!(output.exists());

    // Read back and verify structure
    let mapped = std::fs::read(&output).unwrap();
    let messages: Vec<_> = mcap::MessageStream::new(&mapped)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    // Should have 10 video + 30 obs + 30 action = 70 messages
    assert_eq!(
        messages.len(),
        70,
        "Expected 70 messages, got {}",
        messages.len()
    );

    // Verify channel topics
    let topics: std::collections::HashSet<&str> =
        messages.iter().map(|m| m.channel.topic.as_str()).collect();
    assert!(topics.contains("/camera/cam_left/video"));
    assert!(topics.contains("/observation/leader/joint_position"));
    assert!(topics.contains("/action/follower/joint_position"));

    // Verify schemas are flatbuffer-encoded
    for msg in &messages {
        if let Some(schema) = &msg.channel.schema {
            assert_eq!(schema.encoding, "flatbuffer");
            assert!(!schema.data.is_empty(), "Schema data should not be empty");
        }
    }

    // Verify first video message can be decoded
    let first_video = messages
        .iter()
        .find(|m| m.channel.topic == "/camera/cam_left/video")
        .unwrap();
    let video =
        flatbuffers::root::<rollio_episode_mcap::fb::foxglove::CompressedVideo>(&first_video.data)
            .unwrap();
    assert_eq!(video.frame_id(), Some("cam_left"));
    assert_eq!(video.format(), Some("h264"));
}
