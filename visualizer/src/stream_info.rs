use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use rollio_types::messages::CameraFrameHeader;
use serde::Serialize;

const FPS_EMA_ALPHA: f64 = 0.2;

#[derive(Debug)]
pub struct StreamInfoRegistry {
    camera_order: Vec<String>,
    robot_order: Vec<String>,
    configured_preview_fps: u32,
    max_preview_width: u32,
    max_preview_height: u32,
    active_preview_width: u32,
    active_preview_height: u32,
    preview_workers: usize,
    jpeg_quality: i32,
    cameras: HashMap<String, CameraRuntimeInfo>,
}

#[derive(Debug, Default)]
struct CameraRuntimeInfo {
    source_width: Option<u32>,
    source_height: Option<u32>,
    latest_timestamp_ms: Option<u64>,
    latest_frame_index: Option<u64>,
    source_fps_estimate: Option<f64>,
    published_fps_estimate: Option<f64>,
    last_source_sample: Option<FpsSample>,
    last_published_sample: Option<FpsSample>,
    published_frame_count: u64,
    last_published_timestamp_ms: Option<u64>,
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
    pub configured_preview_fps: u32,
    pub max_preview_width: u32,
    pub max_preview_height: u32,
    pub active_preview_width: u32,
    pub active_preview_height: u32,
    pub preview_workers: usize,
    pub jpeg_quality: i32,
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
    pub source_fps_estimate: Option<f64>,
    pub published_fps_estimate: Option<f64>,
    pub last_published_timestamp_ms: Option<u64>,
}

impl StreamInfoRegistry {
    pub fn new(
        camera_names: &[String],
        robot_names: &[String],
        configured_preview_fps: u32,
        max_preview_width: u32,
        max_preview_height: u32,
        preview_workers: usize,
        jpeg_quality: i32,
    ) -> Self {
        let mut cameras = HashMap::with_capacity(camera_names.len());
        for name in camera_names {
            cameras.insert(name.clone(), CameraRuntimeInfo::default());
        }

        Self {
            camera_order: camera_names.to_vec(),
            robot_order: robot_names.to_vec(),
            configured_preview_fps,
            max_preview_width,
            max_preview_height,
            active_preview_width: max_preview_width,
            active_preview_height: max_preview_height,
            preview_workers,
            jpeg_quality,
            cameras,
        }
    }

    pub fn set_active_preview_bounds(&mut self, width: u32, height: u32) {
        self.active_preview_width = width;
        self.active_preview_height = height;
    }

    pub fn observe_source_frame(&mut self, name: &str, header: &CameraFrameHeader) {
        let camera = self.camera_entry(name);
        camera.source_width = Some(header.width);
        camera.source_height = Some(header.height);
        // Source bus timestamps are now microseconds (see CameraFrameHeader).
        // Display values stay in milliseconds for the UI; convert at the boundary.
        let timestamp_ms = header.timestamp_us / 1_000;
        camera.latest_timestamp_ms = Some(timestamp_ms);
        camera.latest_frame_index = Some(header.frame_index);
        update_fps_estimate(
            &mut camera.source_fps_estimate,
            &mut camera.last_source_sample,
            header.frame_index,
            timestamp_ms,
        );
    }

    pub fn observe_published_frame(&mut self, name: &str) {
        let camera = self.camera_entry(name);
        camera.published_frame_count = camera.published_frame_count.saturating_add(1);
        let now_ms = wall_time_ms();
        camera.last_published_timestamp_ms = Some(now_ms);
        update_fps_estimate(
            &mut camera.published_fps_estimate,
            &mut camera.last_published_sample,
            camera.published_frame_count,
            now_ms,
        );
    }

    pub fn snapshot(&self) -> StreamInfoSnapshot {
        let mut cameras = Vec::with_capacity(self.camera_order.len());
        for name in &self.camera_order {
            let camera = self.cameras.get(name);
            cameras.push(CameraInfoSnapshot {
                name: name.clone(),
                source_width: camera.and_then(|info| info.source_width),
                source_height: camera.and_then(|info| info.source_height),
                latest_timestamp_ms: camera.and_then(|info| info.latest_timestamp_ms),
                latest_frame_index: camera.and_then(|info| info.latest_frame_index),
                source_fps_estimate: camera.and_then(|info| info.source_fps_estimate),
                published_fps_estimate: camera.and_then(|info| info.published_fps_estimate),
                last_published_timestamp_ms: camera
                    .and_then(|info| info.last_published_timestamp_ms),
            });
        }

        StreamInfoSnapshot {
            msg_type: "stream_info",
            server_timestamp_ms: wall_time_ms(),
            configured_preview_fps: self.configured_preview_fps,
            max_preview_width: self.max_preview_width,
            max_preview_height: self.max_preview_height,
            active_preview_width: self.active_preview_width,
            active_preview_height: self.active_preview_height,
            preview_workers: self.preview_workers,
            jpeg_quality: self.jpeg_quality,
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

fn wall_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
