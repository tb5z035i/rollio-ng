//! Per-camera metadata the visualizer reports to UI clients via the
//! `stream_info` JSON message. Replaces the legacy JPEG-flavoured
//! fields with packet-stream metrics.

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
    active_preview_width: u32,
    active_preview_height: u32,
    cameras: HashMap<String, CameraRuntimeInfo>,
}

#[derive(Debug, Default)]
struct CameraRuntimeInfo {
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
    pub latest_timestamp_ms: Option<u64>,
    pub latest_frame_index: Option<u64>,
    pub received_fps_estimate: Option<f64>,
    pub bytes_per_sec: Option<f64>,
    pub keyframe_age_ms: Option<u64>,
}

impl StreamInfoRegistry {
    pub fn new(
        camera_names: &[String],
        robot_names: &[String],
        output_mode: &'static str,
        active_preview_width: u32,
        active_preview_height: u32,
    ) -> Self {
        let mut cameras = HashMap::with_capacity(camera_names.len());
        for name in camera_names {
            cameras.insert(name.clone(), CameraRuntimeInfo::default());
        }
        Self {
            camera_order: camera_names.to_vec(),
            robot_order: robot_names.to_vec(),
            output_mode,
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
    }

    pub fn snapshot(&self) -> StreamInfoSnapshot {
        let now_ms = wall_time_ms();
        let mut cameras = Vec::with_capacity(self.camera_order.len());
        for name in &self.camera_order {
            let camera = self.cameras.get(name);
            cameras.push(CameraInfoSnapshot {
                name: name.clone(),
                source_width: camera.and_then(|c| c.source_width),
                source_height: camera.and_then(|c| c.source_height),
                latest_timestamp_ms: camera.and_then(|c| c.latest_timestamp_ms),
                latest_frame_index: camera.and_then(|c| c.latest_frame_index),
                received_fps_estimate: camera.and_then(|c| c.received_fps_estimate),
                bytes_per_sec: camera.and_then(|c| c.bytes_per_sec),
                keyframe_age_ms: camera
                    .and_then(|c| c.last_keyframe_ms)
                    .map(|ms| now_ms.saturating_sub(ms)),
            });
        }
        StreamInfoSnapshot {
            msg_type: "stream_info",
            server_timestamp_ms: now_ms,
            preview_output_mode: self.output_mode,
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
