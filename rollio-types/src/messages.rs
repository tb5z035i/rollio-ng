use iceoryx2::prelude::*;
use serde::{Deserialize, Serialize};

pub const MAX_JOINTS: usize = 16;
pub const MAX_PROCESS_ID_LEN: usize = 64;
pub const MAX_METRIC_NAME_LEN: usize = 64;
pub const MAX_METRICS: usize = 32;
pub const MAX_FILE_PATH_LEN: usize = 256;
pub const MAX_EXPLANATION_LEN: usize = 256;
pub const MAX_QUEUE_NAME_LEN: usize = 64;

/// Fixed-size byte string for use in `#[repr(C)]` shared-memory types.
#[derive(Debug, Clone, Copy, ZeroCopySend)]
#[type_name("FixedString64")]
#[repr(C)]
pub struct FixedString64 {
    pub data: [u8; 64],
    pub len: u32,
}

impl FixedString64 {
    pub fn new(s: &str) -> Self {
        let bytes = s.as_bytes();
        let len = bytes.len().min(64);
        let mut data = [0u8; 64];
        data[..len].copy_from_slice(&bytes[..len]);
        Self {
            data,
            len: len as u32,
        }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.data[..self.len as usize]).unwrap_or("")
    }
}

impl Default for FixedString64 {
    fn default() -> Self {
        Self {
            data: [0u8; 64],
            len: 0,
        }
    }
}

/// Fixed-size byte string (256 bytes) for file paths and longer text.
#[derive(Debug, Clone, Copy, ZeroCopySend)]
#[type_name("FixedString256")]
#[repr(C)]
pub struct FixedString256 {
    pub data: [u8; 256],
    pub len: u32,
}

impl FixedString256 {
    pub fn new(s: &str) -> Self {
        let bytes = s.as_bytes();
        let len = bytes.len().min(256);
        let mut data = [0u8; 256];
        data[..len].copy_from_slice(&bytes[..len]);
        Self {
            data,
            len: len as u32,
        }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.data[..self.len as usize]).unwrap_or("")
    }
}

impl Default for FixedString256 {
    fn default() -> Self {
        Self {
            data: [0u8; 256],
            len: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Pixel format
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, ZeroCopySend, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[type_name("PixelFormat")]
#[repr(C)]
pub enum PixelFormat {
    Rgb24 = 0,
    Bgr24 = 1,
    Yuyv = 2,
    Mjpeg = 3,
    Depth16 = 4,
    Gray8 = 5,
}

impl PixelFormat {
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            Self::Rgb24 | Self::Bgr24 => 3,
            Self::Yuyv => 2,
            Self::Mjpeg => 0, // variable-length compressed
            Self::Depth16 => 2,
            Self::Gray8 => 1,
        }
    }
}

// ---------------------------------------------------------------------------
// CameraFrameHeader — user header for publish_subscribe::<[u8]>()
// ---------------------------------------------------------------------------

/// Metadata for a camera frame. Used as a user header on an iceoryx2
/// `publish_subscribe::<[u8]>()` service so the raw pixel payload stays
/// zero-copy.
#[derive(Debug, Clone, Copy, ZeroCopySend)]
#[type_name("CameraFrameHeader")]
#[repr(C)]
pub struct CameraFrameHeader {
    pub timestamp_ns: u64,
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
    pub frame_index: u64,
}

impl Default for CameraFrameHeader {
    fn default() -> Self {
        Self {
            timestamp_ns: 0,
            width: 0,
            height: 0,
            pixel_format: PixelFormat::Rgb24,
            frame_index: 0,
        }
    }
}

impl CameraFrameHeader {
    pub fn payload_size(&self) -> usize {
        self.width as usize * self.height as usize * self.pixel_format.bytes_per_pixel()
    }
}

// ---------------------------------------------------------------------------
// RobotState
// ---------------------------------------------------------------------------

/// Published by robot drivers at their configured rate.
#[derive(Debug, Clone, Copy, ZeroCopySend)]
#[type_name("RobotState")]
#[repr(C)]
pub struct RobotState {
    pub timestamp_ns: u64,
    pub num_joints: u32,
    pub positions: [f64; MAX_JOINTS],
    pub velocities: [f64; MAX_JOINTS],
    pub efforts: [f64; MAX_JOINTS],
    /// End-effector pose: [x, y, z, qx, qy, qz, qw].
    pub ee_pose: [f64; 7],
    pub has_ee_pose: bool,
}

impl Default for RobotState {
    fn default() -> Self {
        Self {
            timestamp_ns: 0,
            num_joints: 0,
            positions: [0.0; MAX_JOINTS],
            velocities: [0.0; MAX_JOINTS],
            efforts: [0.0; MAX_JOINTS],
            ee_pose: [0.0; 7],
            has_ee_pose: false,
        }
    }
}

// ---------------------------------------------------------------------------
// RobotCommand
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, ZeroCopySend)]
#[type_name("CommandMode")]
#[repr(C)]
pub enum CommandMode {
    Joint = 0,
    Cartesian = 1,
}

/// Sent to a follower robot's command topic by the Teleop Router.
#[derive(Debug, Clone, Copy, ZeroCopySend)]
#[type_name("RobotCommand")]
#[repr(C)]
pub struct RobotCommand {
    pub timestamp_ns: u64,
    pub mode: CommandMode,
    pub num_joints: u32,
    /// Joint-space targets (used when mode == Joint).
    pub joint_targets: [f64; MAX_JOINTS],
    /// Cartesian target: [x, y, z, qx, qy, qz, qw] (used when mode == Cartesian).
    pub cartesian_target: [f64; 7],
}

impl Default for RobotCommand {
    fn default() -> Self {
        Self {
            timestamp_ns: 0,
            mode: CommandMode::Joint,
            num_joints: 0,
            joint_targets: [0.0; MAX_JOINTS],
            cartesian_target: [0.0; 7],
        }
    }
}

// ---------------------------------------------------------------------------
// ControlEvent
// ---------------------------------------------------------------------------

/// Lifecycle and control events published by the Controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ZeroCopySend)]
#[type_name("ControlEvent")]
#[repr(C)]
pub enum ControlEvent {
    RecordingStart { episode_index: u32 },
    RecordingStop { episode_index: u32 },
    EpisodeKeep { episode_index: u32 },
    EpisodeDiscard { episode_index: u32 },
    Shutdown,
    ModeSwitch { target_mode: u32 },
}

// ---------------------------------------------------------------------------
// MetricsReport
// ---------------------------------------------------------------------------

/// A single metric entry: name → value.
#[derive(Debug, Clone, Copy, ZeroCopySend)]
#[type_name("MetricEntry")]
#[repr(C)]
pub struct MetricEntry {
    pub name: FixedString64,
    pub value: f64,
}

impl Default for MetricEntry {
    fn default() -> Self {
        Self {
            name: FixedString64::default(),
            value: 0.0,
        }
    }
}

/// Periodic health/performance metrics published by every module.
#[derive(Debug, Clone, Copy, ZeroCopySend)]
#[type_name("MetricsReport")]
#[repr(C)]
pub struct MetricsReport {
    pub process_id: FixedString64,
    pub timestamp_ns: u64,
    pub num_entries: u32,
    pub entries: [MetricEntry; MAX_METRICS],
}

impl Default for MetricsReport {
    fn default() -> Self {
        Self {
            process_id: FixedString64::default(),
            timestamp_ns: 0,
            num_entries: 0,
            entries: [MetricEntry::default(); MAX_METRICS],
        }
    }
}

// ---------------------------------------------------------------------------
// WarningEvent
// ---------------------------------------------------------------------------

/// Published by the Monitor when a threshold is breached.
#[derive(Debug, Clone, Copy, ZeroCopySend)]
#[type_name("WarningEvent")]
#[repr(C)]
pub struct WarningEvent {
    pub process_id: FixedString64,
    pub metric_name: FixedString64,
    pub current_value: f64,
    pub explanation: FixedString256,
}

// ---------------------------------------------------------------------------
// VideoReady
// ---------------------------------------------------------------------------

/// Published by an Encoder after it flushes and closes a video file.
#[derive(Debug, Clone, Copy, ZeroCopySend)]
#[type_name("VideoReady")]
#[repr(C)]
pub struct VideoReady {
    pub process_id: FixedString64,
    pub episode_index: u32,
    pub file_path: FixedString256,
}

// ---------------------------------------------------------------------------
// BackpressureEvent
// ---------------------------------------------------------------------------

/// Published when an internal queue is full (Encoder or Storage).
#[derive(Debug, Clone, Copy, ZeroCopySend)]
#[type_name("BackpressureEvent")]
#[repr(C)]
pub struct BackpressureEvent {
    pub process_id: FixedString64,
    pub queue_name: FixedString64,
}
