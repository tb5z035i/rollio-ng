use iceoryx2::prelude::*;
use serde::{Deserialize, Serialize};

pub const MAX_JOINTS: usize = 16;
pub const MAX_DOF: usize = 15;
pub const MAX_PARALLEL: usize = 2;
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

/// Fixed-size byte string (4096 bytes) for JSON control payloads.
#[derive(Debug, Clone, Copy, ZeroCopySend)]
#[type_name("FixedString4096")]
#[repr(C)]
pub struct FixedString4096 {
    pub data: [u8; 4096],
    pub len: u32,
}

impl FixedString4096 {
    pub fn new(s: &str) -> Self {
        let bytes = s.as_bytes();
        let len = bytes.len().min(4096);
        let mut data = [0u8; 4096];
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

impl Default for FixedString4096 {
    fn default() -> Self {
        Self {
            data: [0u8; 4096],
            len: 0,
        }
    }
}

/// Fixed-size byte string (262144 bytes) for setup state snapshots.
///
/// Setup-state envelopes can include large discovered capability sets (for
/// example V4L2 cameras exposing many modes), so the payload needs headroom
/// well beyond typical control messages.
#[derive(Debug, Clone, Copy, ZeroCopySend)]
#[type_name("FixedString262144")]
#[repr(C)]
pub struct FixedString262144 {
    pub data: [u8; 262_144],
    pub len: u32,
}

impl FixedString262144 {
    pub const MAX_LEN: usize = 262_144;

    pub fn new(s: &str) -> Self {
        let bytes = s.as_bytes();
        let len = bytes.len().min(Self::MAX_LEN);
        let mut data = [0u8; Self::MAX_LEN];
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

impl Default for FixedString262144 {
    fn default() -> Self {
        Self {
            data: [0u8; 262_144],
            len: 0,
        }
    }
}

/// JSON-encoded setup command sent from the terminal UI to the controller.
#[derive(Debug, Clone, Copy, Default, ZeroCopySend)]
#[type_name("SetupCommandMessage")]
#[repr(C)]
pub struct SetupCommandMessage {
    pub payload: FixedString4096,
}

impl SetupCommandMessage {
    pub fn new(payload: &str) -> Self {
        Self {
            payload: FixedString4096::new(payload),
        }
    }

    pub fn as_str(&self) -> &str {
        self.payload.as_str()
    }
}

/// JSON-encoded setup state snapshot sent from the controller to the UI.
#[derive(Debug, Clone, Copy, Default, ZeroCopySend)]
#[type_name("SetupStateMessage")]
#[repr(C)]
pub struct SetupStateMessage {
    pub payload: FixedString262144,
}

impl SetupStateMessage {
    pub const MAX_LEN: usize = FixedString262144::MAX_LEN;

    pub fn new(payload: &str) -> Self {
        Self {
            payload: FixedString262144::new(payload),
        }
    }

    pub fn as_str(&self) -> &str {
        self.payload.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::SetupStateMessage;

    #[test]
    fn setup_state_message_round_trips_large_payloads() {
        let payload = format!(
            r#"{{"type":"setup_state","padding":"{}"}}"#,
            "x".repeat(20_000)
        );
        assert!(payload.len() > 16_384);

        let msg = SetupStateMessage::new(&payload);
        assert_eq!(msg.as_str(), payload);
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
// Hierarchical device payloads
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ZeroCopySend, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[type_name("DeviceStatus")]
#[repr(C)]
pub enum DeviceStatus {
    #[default]
    Okay = 0,
    Degraded = 1,
    Error = 2,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ZeroCopySend, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[type_name("DeviceChannelMode")]
#[repr(C)]
pub enum DeviceChannelMode {
    #[default]
    Disabled = 0,
    Enabled = 1,
    FreeDrive = 2,
    CommandFollowing = 3,
    Identifying = 4,
}

impl DeviceChannelMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Enabled => "enabled",
            Self::FreeDrive => "free-drive",
            Self::CommandFollowing => "command-following",
            Self::Identifying => "identifying",
        }
    }
}

#[derive(Debug, Clone, Copy, ZeroCopySend, Serialize, Deserialize)]
#[type_name("JointVector15")]
#[repr(C)]
pub struct JointVector15 {
    pub timestamp_us: u64,
    pub len: u32,
    pub values: [f64; MAX_DOF],
}

impl Default for JointVector15 {
    fn default() -> Self {
        Self {
            timestamp_us: 0,
            len: 0,
            values: [0.0; MAX_DOF],
        }
    }
}

impl JointVector15 {
    pub fn from_slice(timestamp_us: u64, values: &[f64]) -> Self {
        let mut payload = Self {
            timestamp_us,
            len: values.len().min(MAX_DOF) as u32,
            ..Self::default()
        };
        payload.values[..payload.len as usize].copy_from_slice(&values[..payload.len as usize]);
        payload
    }
}

#[derive(Debug, Clone, Copy, ZeroCopySend, Serialize, Deserialize)]
#[type_name("ParallelVector2")]
#[repr(C)]
pub struct ParallelVector2 {
    pub timestamp_us: u64,
    pub len: u32,
    pub values: [f64; MAX_PARALLEL],
}

impl Default for ParallelVector2 {
    fn default() -> Self {
        Self {
            timestamp_us: 0,
            len: 0,
            values: [0.0; MAX_PARALLEL],
        }
    }
}

impl ParallelVector2 {
    pub fn from_slice(timestamp_us: u64, values: &[f64]) -> Self {
        let mut payload = Self {
            timestamp_us,
            len: values.len().min(MAX_PARALLEL) as u32,
            ..Self::default()
        };
        payload.values[..payload.len as usize].copy_from_slice(&values[..payload.len as usize]);
        payload
    }
}

#[derive(Debug, Clone, Copy, ZeroCopySend, Serialize, Deserialize)]
#[type_name("Pose7")]
#[repr(C)]
pub struct Pose7 {
    pub timestamp_us: u64,
    pub values: [f64; 7],
}

impl Default for Pose7 {
    fn default() -> Self {
        Self {
            timestamp_us: 0,
            values: [0.0; 7],
        }
    }
}

#[derive(Debug, Clone, Copy, ZeroCopySend, Serialize, Deserialize)]
#[type_name("JointMitCommand15")]
#[repr(C)]
pub struct JointMitCommand15 {
    pub timestamp_us: u64,
    pub len: u32,
    pub position: [f64; MAX_DOF],
    pub velocity: [f64; MAX_DOF],
    pub effort: [f64; MAX_DOF],
    pub kp: [f64; MAX_DOF],
    pub kd: [f64; MAX_DOF],
}

impl Default for JointMitCommand15 {
    fn default() -> Self {
        Self {
            timestamp_us: 0,
            len: 0,
            position: [0.0; MAX_DOF],
            velocity: [0.0; MAX_DOF],
            effort: [0.0; MAX_DOF],
            kp: [0.0; MAX_DOF],
            kd: [0.0; MAX_DOF],
        }
    }
}

#[derive(Debug, Clone, Copy, ZeroCopySend, Serialize, Deserialize)]
#[type_name("ParallelMitCommand2")]
#[repr(C)]
pub struct ParallelMitCommand2 {
    pub timestamp_us: u64,
    pub len: u32,
    pub position: [f64; MAX_PARALLEL],
    pub velocity: [f64; MAX_PARALLEL],
    pub effort: [f64; MAX_PARALLEL],
    pub kp: [f64; MAX_PARALLEL],
    pub kd: [f64; MAX_PARALLEL],
}

impl Default for ParallelMitCommand2 {
    fn default() -> Self {
        Self {
            timestamp_us: 0,
            len: 0,
            position: [0.0; MAX_PARALLEL],
            velocity: [0.0; MAX_PARALLEL],
            effort: [0.0; MAX_PARALLEL],
            kp: [0.0; MAX_PARALLEL],
            kd: [0.0; MAX_PARALLEL],
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
    pub timestamp_us: u64,
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
    pub frame_index: u64,
}

impl Default for CameraFrameHeader {
    fn default() -> Self {
        Self {
            timestamp_us: 0,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ZeroCopySend, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[type_name("EndEffectorStatus")]
#[repr(C)]
pub enum EndEffectorStatus {
    #[default]
    Unknown = 0,
    Disabled = 1,
    Enabled = 2,
}

impl EndEffectorStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Disabled => "disabled",
            Self::Enabled => "enabled",
        }
    }
}

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
    /// Optional status for standalone end-effector devices.
    pub end_effector_status: EndEffectorStatus,
    pub has_end_effector_status: bool,
    pub end_effector_feedback_valid: bool,
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
            end_effector_status: EndEffectorStatus::Unknown,
            has_end_effector_status: false,
            end_effector_feedback_valid: false,
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
///
/// `RecordingStart` / `RecordingStop` carry the controller's wall-clock
/// timestamp (`controller_ts_us`, UNIX-epoch microseconds) at the moment
/// the user clicked record / stop. Subscribers (encoder, episode-assembler)
/// use this anchor instead of stamping their own `SystemTime::now()` on
/// receipt so every artifact for a given episode is anchored to a single
/// shared instant — irrespective of bus latency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ZeroCopySend)]
#[type_name("ControlEvent")]
#[repr(C)]
pub enum ControlEvent {
    RecordingStart {
        episode_index: u32,
        controller_ts_us: u64,
    },
    RecordingStop {
        episode_index: u32,
        controller_ts_us: u64,
    },
    EpisodeKeep {
        episode_index: u32,
    },
    EpisodeDiscard {
        episode_index: u32,
    },
    Shutdown,
    ModeSwitch {
        target_mode: u32,
    },
}

// ---------------------------------------------------------------------------
// EpisodeCommand
// ---------------------------------------------------------------------------

/// UI-originated episode control command forwarded by the Visualizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ZeroCopySend)]
#[type_name("EpisodeCommand")]
#[repr(C)]
pub enum EpisodeCommand {
    Start,
    Stop,
    Keep,
    Discard,
}

impl EpisodeCommand {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "episode_start",
            Self::Stop => "episode_stop",
            Self::Keep => "episode_keep",
            Self::Discard => "episode_discard",
        }
    }
}

// ---------------------------------------------------------------------------
// EpisodeStatus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, ZeroCopySend)]
#[type_name("EpisodeState")]
#[repr(C)]
pub enum EpisodeState {
    Idle = 0,
    Recording = 1,
    Pending = 2,
}

impl EpisodeState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Recording => "recording",
            Self::Pending => "pending",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ZeroCopySend)]
#[type_name("EpisodeStatus")]
#[repr(C)]
pub struct EpisodeStatus {
    pub state: EpisodeState,
    pub episode_count: u32,
    pub elapsed_ms: u64,
}

impl Default for EpisodeStatus {
    fn default() -> Self {
        Self {
            state: EpisodeState::Idle,
            episode_count: 0,
            elapsed_ms: 0,
        }
    }
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
// EpisodeReady
// ---------------------------------------------------------------------------

/// Published by the Episode Assembler when a staged episode directory is ready
/// for persistence by Storage.
#[derive(Debug, Clone, Copy, ZeroCopySend)]
#[type_name("EpisodeReady")]
#[repr(C)]
pub struct EpisodeReady {
    pub episode_index: u32,
    pub staging_dir: FixedString256,
}

// ---------------------------------------------------------------------------
// EpisodeStored
// ---------------------------------------------------------------------------

/// Published by Storage after an episode has been durably persisted.
#[derive(Debug, Clone, Copy, ZeroCopySend)]
#[type_name("EpisodeStored")]
#[repr(C)]
pub struct EpisodeStored {
    pub episode_index: u32,
    pub output_path: FixedString256,
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
