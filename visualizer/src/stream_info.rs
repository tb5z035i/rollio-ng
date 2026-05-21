//! Per-camera metadata the visualizer reports to UI clients via the
//! `stream_info` JSON message. Replaces the legacy JPEG-flavoured
//! fields with packet-stream metrics.

use rollio_types::config::{PreviewResizePolicy, VisualizerCameraSourceConfig};
use rollio_types::messages::EncodedPacketHeader;
use serde::Serialize;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

const FPS_EMA_ALPHA: f64 = 0.2;

#[derive(Debug)]
pub struct StreamInfoRegistry {
    camera_order: Vec<String>,
    robot_order: Vec<String>,
    output_mode: &'static str,
    advanced_pipeline_logs: bool,
    active_preview_width: u32,
    active_preview_height: u32,
    cameras: HashMap<String, CameraRuntimeInfo>,
}

#[derive(Debug, Default)]
struct CameraRuntimeInfo {
    preview_resize_policy: PreviewResizePolicy,
    source_width: Option<u32>,
    source_height: Option<u32>,
    latest_timestamp_ms: Option<u64>,
    latest_frame_index: Option<u64>,
    received_fps_estimate: Option<f64>,
    received_count: u64,
    last_sample: Option<FpsSample>,
    last_keyframe_ms: Option<u64>,
    bytes_per_sec: Option<f64>,
    bytes_window_start_ms: Option<u64>,
    bytes_in_window: u64,
    /// Observed from `EncodedPacketHeader.flags` —
    /// `ENCODED_PACKET_FLAG_SCALING_LOCKED` is set by the passthrough
    /// backend (and any other backend whose output dims are pinned
    /// to source dims). UI uses this to suppress `set_preview_size`.
    scaling_locked: bool,
}

#[derive(Debug, Clone, Copy)]
struct FpsSample {
    counter: u64,
    timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamInfoSnapshot {
    #[serde(rename = "type")]
    pub msg_type: &'static str,
    pub server_timestamp_ms: u64,
    pub preview_output_mode: &'static str,
    pub advanced_pipeline_logs: bool,
    pub active_preview_width: u32,
    pub active_preview_height: u32,
    pub cameras: Vec<CameraInfoSnapshot>,
    pub robots: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CameraInfoSnapshot {
    pub name: String,
    pub source_width: Option<u32>,
    pub source_height: Option<u32>,
    pub preview_resizable: bool,
    pub preview_resize_policy: &'static str,
    pub latest_timestamp_ms: Option<u64>,
    pub latest_frame_index: Option<u64>,
    pub received_fps_estimate: Option<f64>,
    pub bytes_per_sec: Option<f64>,
    pub keyframe_age_ms: Option<u64>,
    /// True when the encoder's output dims are pinned to source dims
    /// (passthrough mode). UI should suppress `set_preview_size`
    /// when set; sending it produces a no-op rejection from the
    /// encoder and clutters the visualizer log.
    pub scaling_locked: bool,
}

impl StreamInfoRegistry {
    pub fn new(
        camera_sources: &[VisualizerCameraSourceConfig],
        robot_names: &[String],
        output_mode: &'static str,
        advanced_pipeline_logs: bool,
        active_preview_width: u32,
        active_preview_height: u32,
    ) -> Self {
        let mut cameras = HashMap::with_capacity(camera_sources.len());
        let mut camera_order = Vec::with_capacity(camera_sources.len());
        for source in camera_sources {
            camera_order.push(source.channel_id.clone());
            cameras.insert(
                source.channel_id.clone(),
                CameraRuntimeInfo {
                    preview_resize_policy: source.preview_resize_policy,
                    source_width: source.source_width,
                    source_height: source.source_height,
                    ..CameraRuntimeInfo::default()
                },
            );
        }
        Self {
            camera_order,
            robot_order: robot_names.to_vec(),
            output_mode,
            advanced_pipeline_logs,
            active_preview_width,
            active_preview_height,
            cameras,
        }
    }

    pub fn set_active_preview_bounds(&mut self, width: u32, height: u32) {
        self.active_preview_width = width;
        self.active_preview_height = height;
    }

    pub fn observe_jpeg_frame(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        timestamp_us: u64,
        frame_index: u64,
        payload_bytes: usize,
    ) {
        let camera = self.camera_entry(name);
        camera.source_width = Some(width);
        camera.source_height = Some(height);
        let timestamp_ms = timestamp_us / 1_000;
        camera.latest_timestamp_ms = Some(timestamp_ms);
        camera.latest_frame_index = Some(frame_index);
        camera.received_count = camera.received_count.saturating_add(1);
        update_fps_estimate(
            &mut camera.received_fps_estimate,
            &mut camera.last_sample,
            camera.received_count,
            timestamp_ms,
        );
        update_bytes_per_sec(camera, payload_bytes);
    }

    pub fn observe_encoded_packet(
        &mut self,
        name: &str,
        header: &EncodedPacketHeader,
        payload_bytes: usize,
    ) {
        let camera = self.camera_entry(name);
        camera.source_width = Some(header.width);
        camera.source_height = Some(header.height);
        let timestamp_ms = header.source_timestamp_us / 1_000;
        camera.latest_timestamp_ms = Some(timestamp_ms);
        camera.latest_frame_index = Some(header.source_frame_index);
        camera.received_count = camera.received_count.saturating_add(1);
        update_fps_estimate(
            &mut camera.received_fps_estimate,
            &mut camera.last_sample,
            camera.received_count,
            timestamp_ms,
        );
        update_bytes_per_sec(camera, payload_bytes);
        if header.is_keyframe() {
            camera.last_keyframe_ms = Some(wall_time_ms());
        }
        // The encoder stamps SCALING_LOCKED on every header it emits
        // for the session, so it's safe to overwrite on every packet
        // observation — there's no flip-flop in a healthy stream.
        // (When the session is re-opened, the new first Config
        // packet refreshes this.)
        camera.scaling_locked = header.is_scaling_locked();
    }

    pub fn snapshot(&self) -> StreamInfoSnapshot {
        let now_ms = wall_time_ms();
        let mut cameras = Vec::with_capacity(self.camera_order.len());
        for name in &self.camera_order {
            let camera = self.cameras.get(name);
            let preview_resize_policy = camera.map(|c| c.preview_resize_policy).unwrap_or_default();
            cameras.push(CameraInfoSnapshot {
                name: name.clone(),
                source_width: camera.and_then(|c| c.source_width),
                source_height: camera.and_then(|c| c.source_height),
                preview_resizable: preview_resize_policy.is_resizable(),
                preview_resize_policy: preview_resize_policy.as_str(),
                latest_timestamp_ms: camera.and_then(|c| c.latest_timestamp_ms),
                latest_frame_index: camera.and_then(|c| c.latest_frame_index),
                received_fps_estimate: camera.and_then(|c| c.received_fps_estimate),
                bytes_per_sec: camera.and_then(|c| c.bytes_per_sec),
                keyframe_age_ms: camera
                    .and_then(|c| c.last_keyframe_ms)
                    .map(|ms| now_ms.saturating_sub(ms)),
                scaling_locked: camera.map(|c| c.scaling_locked).unwrap_or(false),
            });
        }
        StreamInfoSnapshot {
            msg_type: "stream_info",
            server_timestamp_ms: now_ms,
            preview_output_mode: self.output_mode,
            advanced_pipeline_logs: self.advanced_pipeline_logs,
            active_preview_width: self.active_preview_width,
            active_preview_height: self.active_preview_height,
            cameras,
            robots: self.robot_order.clone(),
        }
    }

    fn camera_entry(&mut self, name: &str) -> &mut CameraRuntimeInfo {
        if !self
            .camera_order
            .iter()
            .any(|camera_name| camera_name == name)
        {
            self.camera_order.push(name.to_string());
        }
        self.cameras.entry(name.to_string()).or_default()
    }
}

fn update_fps_estimate(
    fps_estimate: &mut Option<f64>,
    last_sample: &mut Option<FpsSample>,
    counter: u64,
    timestamp_ms: u64,
) {
    if let Some(previous) = last_sample {
        let counter_delta = counter.saturating_sub(previous.counter);
        let time_delta_ms = timestamp_ms.saturating_sub(previous.timestamp_ms);
        if counter_delta > 0 && time_delta_ms > 0 {
            let instant_fps = counter_delta as f64 / (time_delta_ms as f64 / 1_000.0);
            *fps_estimate = Some(match *fps_estimate {
                Some(current) => current * (1.0 - FPS_EMA_ALPHA) + instant_fps * FPS_EMA_ALPHA,
                None => instant_fps,
            });
        }
    }
    *last_sample = Some(FpsSample {
        counter,
        timestamp_ms,
    });
}

fn update_bytes_per_sec(camera: &mut CameraRuntimeInfo, payload_bytes: usize) {
    let now_ms = wall_time_ms();
    let window_start = camera.bytes_window_start_ms.get_or_insert(now_ms);
    camera.bytes_in_window = camera.bytes_in_window.saturating_add(payload_bytes as u64);
    let elapsed = now_ms.saturating_sub(*window_start);
    if elapsed >= 1_000 {
        let bps = (camera.bytes_in_window as f64) * 1_000.0 / elapsed as f64;
        camera.bytes_per_sec = Some(bps);
        camera.bytes_in_window = 0;
        camera.bytes_window_start_ms = Some(now_ms);
    }
}

fn wall_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_exposes_fixed_source_preview_metadata() {
        let sources = vec![VisualizerCameraSourceConfig {
            channel_id: "cam/color".into(),
            bus_root: "cam".into(),
            channel_type: "color".into(),
            preview_resize_policy: PreviewResizePolicy::FixedSource,
            source_width: Some(640),
            source_height: Some(480),
        }];
        let registry = StreamInfoRegistry::new(&sources, &[], "encoded", false, 320, 240);

        let snapshot = registry.snapshot();
        assert_eq!(snapshot.cameras.len(), 1);
        assert_eq!(snapshot.cameras[0].source_width, Some(640));
        assert_eq!(snapshot.cameras[0].source_height, Some(480));
        assert!(!snapshot.cameras[0].preview_resizable);
        assert_eq!(snapshot.cameras[0].preview_resize_policy, "fixed-source");
        // Default before any packet observed.
        assert!(!snapshot.cameras[0].scaling_locked);
    }

    fn header(flags: u32) -> EncodedPacketHeader {
        let mut h = EncodedPacketHeader::default();
        h.flags = flags;
        h.width = 1920;
        h.height = 1080;
        h.source_timestamp_us = 1_000_000;
        h.source_frame_index = 0;
        h
    }

    #[test]
    fn observe_packet_reflects_scaling_locked_flag_into_snapshot() {
        let sources = vec![VisualizerCameraSourceConfig {
            channel_id: "cam/color".into(),
            bus_root: "cam".into(),
            channel_type: "color".into(),
            preview_resize_policy: PreviewResizePolicy::Dynamic,
            source_width: None,
            source_height: None,
        }];
        let mut registry = StreamInfoRegistry::new(&sources, &[], "encoded", false, 320, 240);

        // Initially the flag is false until we see a packet.
        assert!(!registry.snapshot().cameras[0].scaling_locked);

        // A packet without the flag keeps it false.
        registry.observe_encoded_packet(
            "cam/color",
            &header(rollio_types::messages::ENCODED_PACKET_FLAG_KEYFRAME),
            128,
        );
        assert!(!registry.snapshot().cameras[0].scaling_locked);

        // A packet with the flag flips it to true.
        registry.observe_encoded_packet(
            "cam/color",
            &header(
                rollio_types::messages::ENCODED_PACKET_FLAG_KEYFRAME
                    | rollio_types::messages::ENCODED_PACKET_FLAG_SCALING_LOCKED,
            ),
            128,
        );
        assert!(registry.snapshot().cameras[0].scaling_locked);

        // And a subsequent packet without the flag flips it back —
        // the encoder is the source of truth, the visualizer just
        // mirrors what's on the wire.
        registry.observe_encoded_packet("cam/color", &header(0), 128);
        assert!(!registry.snapshot().cameras[0].scaling_locked);
    }
}
