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
    /// Pre-encoded H.264 in Annex B framing (start-code-prefixed
    /// NAL units, possibly including in-band SPS/PPS before each IDR).
    /// Today no rollio camera publishes this format; the variant exists
    /// to let the encoder route compressed-from-camera streams to the
    /// passthrough backend (which forwards bytes verbatim, no decode +
    /// re-encode, no scaling).
    H264AnnexB = 6,
}

impl PixelFormat {
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            Self::Rgb24 | Self::Bgr24 => 3,
            Self::Yuyv => 2,
            Self::Mjpeg => 0, // variable-length compressed
            Self::Depth16 => 2,
            Self::Gray8 => 1,
            Self::H264AnnexB => 0, // variable-length compressed
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
// EncodedPacketHeader — user header for the recording- and preview-packet
// services. Carries every packet's metadata in a fixed-size, ZeroCopySend
// header so the iceoryx2 payload (`[u8]`) can stay byte-for-byte equal to
// the codec's elementary access unit.
// ---------------------------------------------------------------------------

/// Discriminates the three message shapes carried over a packet topic:
/// codec stream configuration (carries extradata/SPS/PPS), encoded access
/// unit, and end-of-stream sentinel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ZeroCopySend, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[type_name("EncodedPacketKind")]
#[repr(C)]
pub enum EncodedPacketKind {
    /// Codec configuration / extradata. Sent once at session-open and
    /// retained on the per-camera `…/recording-config` /
    /// `…/preview-config` topic with `history_size = 1` so late
    /// subscribers can replay it.
    Config = 0,
    /// One encoded access unit (NALU sequence in Annex B for H.264/H.265,
    /// temporal unit / OBU for AV1, single JPEG for MJPG, single RVL
    /// frame for RVL).
    Packet = 1,
    /// Marks the end of a recording session for the assembler. The
    /// payload is empty.
    EndOfStream = 2,
}

/// Codec carried in `EncodedPacketHeader.codec`. Mirrors
/// `rollio_types::config::EncoderCodec` but is `#[repr(C)]` for
/// shared-memory transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ZeroCopySend, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[type_name("EncodedCodecId")]
#[repr(C)]
pub enum EncodedCodecId {
    H264 = 0,
    H265 = 1,
    Av1 = 2,
    Rvl = 3,
    Mjpg = 4,
}

/// Bit set in `EncodedPacketHeader.flags` when the packet is a keyframe
/// (independently decodable). Subscribers use this to recover after a
/// best-effort drop on the preview path.
pub const ENCODED_PACKET_FLAG_KEYFRAME: u32 = 1 << 0;

/// Bit set in `EncodedPacketHeader.flags` when the packet has codec
/// configuration / extradata inlined ahead of the access unit (in
/// addition to the once-per-session `Config` message). H.264/H.265 NVENC
/// and some VAAPI builds repeat SPS/PPS at every keyframe; the flag lets
/// the assembler/visualizer know it can skip caching the duplicate.
pub const ENCODED_PACKET_FLAG_CONFIG_INLINE: u32 = 1 << 1;

/// Bit set in `EncodedPacketHeader.flags` when the encoder cannot
/// rescale the stream — output dims are pinned to source dims. The
/// passthrough backend always sets this because its very contract is
/// "rewrite headers and relay NAL bytes verbatim". The visualizer
/// surfaces this flag on `stream_info` so the UI knows not to send
/// `set_preview_size` requests (which the passthrough session would
/// reject anyway). Set on the `Config` packet and inherited by every
/// `Packet` from the same session.
pub const ENCODED_PACKET_FLAG_SCALING_LOCKED: u32 = 1 << 2;

/// Metadata for one encoded packet. Used as a user header on an iceoryx2
/// `publish_subscribe::<[u8]>()` service so the encoded access unit
/// stays zero-copy. The same header type is used for recording and
/// preview topics; the topic name (`…/recording-*` vs `…/preview-*`)
/// distinguishes them.
#[derive(Debug, Clone, Copy, ZeroCopySend)]
#[type_name("EncodedPacketHeader")]
#[repr(C)]
pub struct EncodedPacketHeader {
    pub kind: EncodedPacketKind,
    pub codec: EncodedCodecId,
    /// Bitwise-OR of `ENCODED_PACKET_FLAG_*` constants.
    pub flags: u32,
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
    /// Reserved for future flag/tag fields; keeps the struct
    /// 8-byte-aligned regardless of how the compiler lays out the
    /// preceding `#[repr(C)]` enums on the host platform.
    pub _reserved0: u32,
    pub time_base_num: u32,
    pub time_base_den: u32,
    pub pts_us: i64,
    pub dts_us: i64,
    pub duration_us: i64,
    pub sequence_number: u64,
    pub source_timestamp_us: u64,
    pub source_frame_index: u64,
    pub episode_index: u32,
    pub payload_len: u32,
}

impl Default for EncodedPacketHeader {
    fn default() -> Self {
        Self {
            kind: EncodedPacketKind::Packet,
            codec: EncodedCodecId::H264,
            flags: 0,
            width: 0,
            height: 0,
            pixel_format: PixelFormat::Rgb24,
            _reserved0: 0,
            time_base_num: 1,
            time_base_den: 1_000_000,
            pts_us: 0,
            dts_us: 0,
            duration_us: 0,
            sequence_number: 0,
            source_timestamp_us: 0,
            source_frame_index: 0,
            episode_index: 0,
            payload_len: 0,
        }
    }
}

impl EncodedPacketHeader {
    pub fn is_keyframe(&self) -> bool {
        self.flags & ENCODED_PACKET_FLAG_KEYFRAME != 0
    }

    pub fn has_inline_config(&self) -> bool {
        self.flags & ENCODED_PACKET_FLAG_CONFIG_INLINE != 0
    }

    pub fn set_keyframe(&mut self, value: bool) {
        if value {
            self.flags |= ENCODED_PACKET_FLAG_KEYFRAME;
        } else {
            self.flags &= !ENCODED_PACKET_FLAG_KEYFRAME;
        }
    }

    pub fn set_inline_config(&mut self, value: bool) {
        if value {
            self.flags |= ENCODED_PACKET_FLAG_CONFIG_INLINE;
        } else {
            self.flags &= !ENCODED_PACKET_FLAG_CONFIG_INLINE;
        }
    }

    pub fn is_scaling_locked(&self) -> bool {
        self.flags & ENCODED_PACKET_FLAG_SCALING_LOCKED != 0
    }

    pub fn set_scaling_locked(&mut self, value: bool) {
        if value {
            self.flags |= ENCODED_PACKET_FLAG_SCALING_LOCKED;
        } else {
            self.flags &= !ENCODED_PACKET_FLAG_SCALING_LOCKED;
        }
    }
}

// ---------------------------------------------------------------------------
// PreviewControl — UI -> preview encoder runtime control
// ---------------------------------------------------------------------------

/// Control message published by the visualizer to a preview encoder when
/// the operator changes the preview raster size. The encoder finalizes
/// the current session, rebuilds the codec session at the new dims, and
/// emits a fresh `Config` + keyframe so subscribers can re-init their
/// decoders without losing more than a few frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ZeroCopySend)]
#[type_name("PreviewControl")]
#[repr(C)]
pub enum PreviewControl {
    SetSize { width: u32, height: u32 },
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
/// the user clicked record / stop. Subscribers (encoder, `rollio-episode-lerobot`
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
