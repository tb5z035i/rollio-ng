use crate::messages::{PixelFormat, MAX_DOF, MAX_PARALLEL};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::str::FromStr;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("validation error: {0}")]
    Validation(String),
}

// ---------------------------------------------------------------------------
// Top-level config (legacy `Config` removed; use `ProjectConfig` instead).
// ---------------------------------------------------------------------------

fn default_project_name() -> String {
    "default".into()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionMode {
    Teleop,
    Intervention,
}

// ---------------------------------------------------------------------------
// Episode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeConfig {
    pub format: EpisodeFormat,
    pub fps: u32,
    #[serde(default = "default_chunk_size")]
    pub chunk_size: u32,
}

fn default_chunk_size() -> u32 {
    1000
}

impl Default for EpisodeConfig {
    fn default() -> Self {
        Self {
            format: EpisodeFormat::default(),
            fps: 30,
            chunk_size: default_chunk_size(),
        }
    }
}

impl EpisodeConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.fps == 0 || self.fps > 1000 {
            return Err(ConfigError::Validation(format!(
                "episode: fps must be 1..1000, got {}",
                self.fps
            )));
        }
        if self.chunk_size == 0 {
            return Err(ConfigError::Validation(
                "episode: chunk_size must be > 0".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EpisodeFormat {
    #[serde(rename = "lerobot-v2.1")]
    #[default]
    LeRobotV2_1,
    #[serde(rename = "lerobot-v3.0")]
    LeRobotV3_0,
    Mcap,
}

// (Legacy `DeviceConfig` removed; use `BinaryDeviceConfig` instead.)

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeviceType {
    Camera,
    #[default]
    Robot,
    Sensor,
}

/// Sample kinds a sensor channel can publish.
///
/// Each variant has a fixed memory layout consumers can rely on without
/// re-reading the schema:
/// - `ImuAccelGyro` is a 6-float packet `[ax, ay, az, gx, gy, gz]`.
///   Accel + gyro are combined so consumers cannot observe cross-topic
///   skew between halves of the same IMU sample. shape = `[6]`.
/// - `TactilePointCloud2` is `N_points × 6` floats per sample, each
///   point being `[x, y, z, fx, fy, fz]` (3D position + 3D contact
///   force). `N_points` is fixed per channel and reported by the driver
///   via `query --json` so the assembler can pre-allocate Parquet
///   columns. shape = `[N_points, 6]`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SensorStateKind {
    ImuAccelGyro,
    TactilePointCloud2,
}

impl SensorStateKind {
    /// Stable string segment used in IPC topic names (and elsewhere we
    /// need a canonical lowercase identifier).
    pub fn topic_suffix(self) -> &'static str {
        match self {
            Self::ImuAccelGyro => "imu_accel_gyro",
            Self::TactilePointCloud2 => "tactile_point_cloud2",
        }
    }

    /// Number of scalar values per sample, when fixed by the kind alone.
    /// `None` for variable-shape kinds (tactile clouds) where the
    /// driver reports the shape per channel.
    pub fn fixed_value_len(self) -> Option<u32> {
        match self {
            Self::ImuAccelGyro => Some(6),
            Self::TactilePointCloud2 => None,
        }
    }

    pub fn is_variable_shape(self) -> bool {
        self.fixed_value_len().is_none()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RobotMode {
    FreeDrive,
    CommandFollowing,
    Identifying,
    Disabled,
}

impl RobotMode {
    pub fn control_mode_value(self) -> u32 {
        match self {
            Self::FreeDrive => 0,
            Self::CommandFollowing => 1,
            Self::Identifying => 2,
            Self::Disabled => 3,
        }
    }

    pub fn from_control_mode_value(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::FreeDrive),
            1 => Some(Self::CommandFollowing),
            2 => Some(Self::Identifying),
            3 => Some(Self::Disabled),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::FreeDrive => "free-drive",
            Self::CommandFollowing => "command-following",
            Self::Identifying => "identifying",
            Self::Disabled => "disabled",
        }
    }
}

// ---------------------------------------------------------------------------
// Pairing
// ---------------------------------------------------------------------------
//
// Legacy `PairConfig` (device-name pairing built on `DeviceConfig`) was
// removed alongside the legacy `Config`/`DeviceConfig`. Channel-level
// pairing now goes through `ChannelPairingConfig`, validated against the
// per-channel `direct_joint_compatibility` blob refreshed from the
// driver's `query --json` response (see `ChannelPairingConfig::validate`).

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MappingStrategy {
    /// Identity (or operator-supplied joint_index_map / joint_scales)
    /// joint-position teleop. Requires both drivers to opt in via
    /// `direct_joint_compatibility` and matching DOF.
    DirectJoint,
    /// End-effector pose passthrough (FK on leader, IK on follower).
    Cartesian,
    /// Single-DOF parallel-gripper position teleop with a configurable
    /// linear scaling ratio stored as `joint_scales = [ratio]`. Both
    /// channels must have `dof == 1`.
    Parallel,
}

fn default_mapping() -> MappingStrategy {
    MappingStrategy::DirectJoint
}

// (Legacy `TeleopRuntimeConfig` removed; use `TeleopRuntimeConfigV2` instead.)

// ---------------------------------------------------------------------------
// Assembler
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblerConfig {
    /// Maximum wait, in milliseconds, after the controller publishes
    /// `RecordingStop` for every camera's recording packet stream to
    /// emit `EndOfStream`. The encoder normally sends EOS within a
    /// frame interval; the timeout bounds how long a crashed encoder
    /// can pin an episode in the assembler before it is removed.
    #[serde(
        alias = "missing_video_timeout_ms",
        default = "default_missing_eos_timeout_ms"
    )]
    pub missing_eos_timeout_ms: u64,
    #[serde(default = "default_staging_dir")]
    pub staging_dir: String,
    /// Upper bound on episodes simultaneously sitting in the staging
    /// directory. The assembler reserves a slot when it dispatches an
    /// episode to the stage worker and releases it on `EpisodeStored`
    /// from storage. When the slot pool is exhausted, newly ready
    /// episodes are dropped (artifacts cleaned up, `EpisodeDropped` and
    /// `BackpressureEvent` published) rather than blocking the
    /// assembler main loop.
    #[serde(default = "default_staging_slots")]
    pub staging_slots: u32,
}

fn default_missing_eos_timeout_ms() -> u64 {
    30_000
}

fn default_staging_slots() -> u32 {
    4
}

fn default_staging_dir() -> String {
    #[cfg(target_os = "linux")]
    {
        return "/dev/shm/rollio".into();
    }

    #[allow(unreachable_code)]
    std::env::temp_dir()
        .join("rollio")
        .to_string_lossy()
        .into_owned()
}

impl Default for AssemblerConfig {
    fn default() -> Self {
        Self {
            missing_eos_timeout_ms: default_missing_eos_timeout_ms(),
            staging_dir: default_staging_dir(),
            staging_slots: default_staging_slots(),
        }
    }
}

impl AssemblerConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.missing_eos_timeout_ms == 0 {
            return Err(ConfigError::Validation(
                "assembler: missing_eos_timeout_ms must be > 0".into(),
            ));
        }
        if self.staging_dir.trim().is_empty() {
            return Err(ConfigError::Validation(
                "assembler: staging_dir must not be empty".into(),
            ));
        }
        if self.staging_slots == 0 {
            return Err(ConfigError::Validation(
                "assembler: staging_slots must be > 0".into(),
            ));
        }
        if self.staging_slots > 64 {
            return Err(ConfigError::Validation(
                "assembler: staging_slots must be <= 64".into(),
            ));
        }
        Ok(())
    }
}

// (Legacy V1 assembler runtime types removed; use the V2 variants in the
// `AssemblerRuntimeConfigV2` family instead.)

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EncoderCodec {
    #[serde(alias = "libx264", alias = "h264_nvenc", alias = "h264_vaapi")]
    #[default]
    H264,
    #[serde(alias = "libx265", alias = "hevc_nvenc", alias = "hevc_vaapi")]
    H265,
    #[serde(
        alias = "libsvtav1",
        alias = "librav1e",
        alias = "av1_nvenc",
        alias = "av1_vaapi"
    )]
    Av1,
    /// Motion JPEG. Stores one self-contained JPEG per frame; useful as
    /// a low-CPU recording option and for camera-native MJPG passthrough.
    #[serde(alias = "mjpeg")]
    Mjpg,
    Rvl,
}

impl EncoderCodec {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::H265 => "h265",
            Self::Av1 => "av1",
            Self::Mjpg => "mjpg",
            Self::Rvl => "rvl",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum EncoderBackend {
    #[default]
    Auto,
    Cpu,
    Nvidia,
    Vaapi,
    /// Bytes-verbatim relay for pre-encoded camera streams (today
    /// only `PixelFormat::H264AnnexB` qualifies). No decode, no scale,
    /// no encode — the session rewrites packet headers (PTS, sequence,
    /// `source_timestamp_us`) and forwards NAL units as-is. Selected
    /// automatically by `Auto` when the input codec matches the
    /// configured output codec; selecting it explicitly without that
    /// match is a session-open error.
    Passthrough,
    /// Horizon Robotics X5 SoC hardware VPU encoder. Uses the
    /// `libmultimedia` codec API (`hb_mm_mc_*`) for zero-copy H.264
    /// and MJPEG encoding on the BPU/VPU pipeline. Only available on
    /// aarch64 boards running the Horizon Linux BSP.
    HorizonX5,
}

/// Final container the assembler muxes encoded packets into for one
/// camera. Derived from the codec via [`container_for`]; not a config
/// knob (the operator picks the codec, the container follows).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ContainerKind {
    Mp4,
    Mkv,
    /// Custom RVL container produced by [`rollio_episode_lerobot::muxer::rvl_frame`].
    /// Byte-identical to the file written by the legacy `RvlSession`.
    RvlNative,
}

impl ContainerKind {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::RvlNative => "rvl",
        }
    }
}

/// Map an `EncoderCodec` to the standard container the assembler uses
/// when muxing its packets. `H264 / H265 / Mjpg` go into MP4 (mov),
/// `Av1` into MKV (matroska), `Rvl` into the in-repo `.rvl` container.
pub fn container_for(codec: EncoderCodec) -> ContainerKind {
    match codec {
        EncoderCodec::H264 | EncoderCodec::H265 | EncoderCodec::Mjpg => ContainerKind::Mp4,
        EncoderCodec::Av1 => ContainerKind::Mkv,
        EncoderCodec::Rvl => ContainerKind::RvlNative,
    }
}

/// Chroma subsampling for the codec input pixel format. `S422` (the
/// default) preserves the native chroma resolution of YUYV / MJPG-422
/// camera sources at the cost of slightly larger files; `S420` mirrors
/// the legacy behaviour and downsamples chroma vertically by 2x.
///
/// The encoder will silently fall back to `S420` when the resolved
/// libav codec / backend pair can't accept 4:2:2 input (older NVENC
/// hardware, libsvtav1, certain VAAPI drivers); the configured default
/// only kicks in where the encoder actually advertises 4:2:2 support.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ChromaSubsampling {
    /// 4:2:0 chroma (legacy default, smallest files, broadest player
    /// compatibility, codec input is YUV420P / NV12).
    #[serde(alias = "420", alias = "yuv420p", alias = "s420")]
    S420,
    /// 4:2:2 chroma (preserves native chroma resolution from YUYV and
    /// MJPG-422 sources, codec input is YUV422P / NV16).
    #[default]
    #[serde(alias = "422", alias = "yuv422p", alias = "s422")]
    S422,
}

/// Color metadata written into the encoded bitstream so downstream
/// players know which color primaries / transfer / matrix to use when
/// converting YUV back to RGB. `Auto` (the default) leaves the fields
/// unset and lets each player guess from the resolution (typically
/// BT.709 for >=720p, BT.601 for SD).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum EncoderColorSpace {
    /// Don't write color metadata. Matches the pre-config behaviour and
    /// keeps existing recordings reproducible after this change rolls
    /// out.
    #[default]
    Auto,
    /// BT.709 limited-range (modern HD; recommended for >= 720p).
    #[serde(alias = "bt709", alias = "rec709")]
    Bt709Limited,
    /// BT.601 limited-range (SD; recommended for <= 480p webcam modes).
    #[serde(alias = "bt601", alias = "smpte170m", alias = "rec601")]
    Bt601Limited,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EncoderImplementationFamily {
    Libav,
    Rvl,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EncoderCapabilityDirection {
    Encode,
    Decode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncoderCapability {
    pub codec: EncoderCodec,
    pub implementation: EncoderImplementationFamily,
    pub direction: EncoderCapabilityDirection,
    pub backend: EncoderBackend,
    pub pixel_formats: Vec<PixelFormat>,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EncoderCapabilityReport {
    #[serde(default)]
    pub codecs: Vec<EncoderCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "EncoderConfigSerde")]
pub struct EncoderConfig {
    pub video_codec: EncoderCodec,
    pub depth_codec: EncoderCodec,
    /// Legacy global backend hint. New code should prefer `video_backend`
    /// and `depth_backend`; this field is preserved so older configs keep
    /// loading and so the per-codec backends fall back to it when unset.
    #[serde(default)]
    pub backend: EncoderBackend,
    /// Backend used for color (and Gray8 fallback) streams. Defaults to
    /// `backend` when omitted from the saved TOML so existing configs keep
    /// the same semantics.
    #[serde(default)]
    pub video_backend: EncoderBackend,
    /// Backend used for depth streams. RVL is CPU-only by construction;
    /// validation rejects non-CPU pairings with `Rvl` to catch
    /// misconfiguration early.
    #[serde(default)]
    pub depth_backend: EncoderBackend,
    /// Codec input chroma subsampling. Default `S422` preserves YUYV /
    /// MJPG-422 chroma; the encoder falls back to `S420` for codec /
    /// backend combinations that can't ingest 4:2:2.
    #[serde(default)]
    pub chroma_subsampling: ChromaSubsampling,
    /// Constant-quality target. Lower = better quality, larger files.
    /// Typical x264/x265 useful range is `0..=51` (`0` = lossless,
    /// `18` ≈ visually lossless, `23` is the libavcodec default,
    /// `28` is "OK for streaming"). Mapped to `cq` for NVENC and to
    /// `qp` (CQP rate-control) for VAAPI. `None` keeps the encoder's
    /// built-in default and matches the pre-config behaviour.
    #[serde(default)]
    pub crf: Option<u8>,
    /// Encoder preset (e.g. `"ultrafast"..="veryslow"` for x264/x265,
    /// numeric `"0".."13"` for SVT-AV1, `"p1".."p7"` for NVENC).
    /// Slower presets trade encoder CPU for either smaller files or
    /// higher quality at the same `crf`. `None` keeps the libavcodec
    /// default (typically `medium` for x264/x265).
    #[serde(default)]
    pub preset: Option<String>,
    /// Codec-specific psychovisual tuning hint (e.g. `"film"`,
    /// `"animation"`, `"grain"`, `"stillimage"` for x264; `"grain"`
    /// or `"psnr"` for x265). `None` keeps the libavcodec default.
    #[serde(default)]
    pub tune: Option<String>,
    /// Codec input bit depth. Currently `8` (default) and `10` are
    /// supported. 10-bit eliminates banding in dark scenes at the cost
    /// of ~25% larger files; requires the host's `libx264` / `libx265`
    /// to be the 10-bit build (most distros ship one that handles
    /// both).
    #[serde(default = "default_bit_depth")]
    pub bit_depth: u8,
    /// Color metadata to write into the bitstream. `Auto` (default)
    /// leaves the fields unset; pick `Bt709Limited` or `Bt601Limited`
    /// to give downstream players a definitive answer.
    #[serde(default)]
    pub color_space: EncoderColorSpace,
    #[serde(default = "default_queue_size")]
    pub queue_size: u32,
    /// `[encoder.preview]` sub-block. Owns every preview-only
    /// production knob (size, fps, codec, jpeg_quality, output_mode).
    #[serde(default)]
    pub preview: EncoderPreviewConfig,
}

#[derive(Debug, Deserialize)]
struct EncoderConfigSerde {
    #[serde(default)]
    codec: Option<EncoderCodec>,
    #[serde(default)]
    video_codec: Option<EncoderCodec>,
    #[serde(default)]
    depth_codec: Option<EncoderCodec>,
    #[serde(default)]
    backend: EncoderBackend,
    #[serde(default)]
    video_backend: Option<EncoderBackend>,
    #[serde(default)]
    depth_backend: Option<EncoderBackend>,
    #[serde(default)]
    chroma_subsampling: ChromaSubsampling,
    #[serde(default)]
    crf: Option<u8>,
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    tune: Option<String>,
    #[serde(default = "default_bit_depth")]
    bit_depth: u8,
    #[serde(default)]
    color_space: EncoderColorSpace,
    #[serde(default = "default_queue_size")]
    queue_size: u32,
    #[serde(default)]
    preview: EncoderPreviewConfig,
}

impl From<EncoderConfigSerde> for EncoderConfig {
    fn from(value: EncoderConfigSerde) -> Self {
        let legacy_codec = value.codec;
        let video_codec = value.video_codec.or(legacy_codec).unwrap_or_default();
        let depth_codec = value
            .depth_codec
            .or(legacy_codec)
            .unwrap_or_else(default_depth_codec);
        // Per-codec backends inherit from the legacy global field so
        // existing TOML files keep producing exactly the same encoder
        // configuration. The wizard always fills the per-codec fields
        // explicitly going forward.
        let video_backend = value.video_backend.unwrap_or(value.backend);
        let depth_backend = value
            .depth_backend
            .unwrap_or_else(|| default_backend_for_codec(depth_codec, value.backend));
        Self {
            video_codec,
            depth_codec,
            backend: value.backend,
            video_backend,
            depth_backend,
            chroma_subsampling: value.chroma_subsampling,
            crf: value.crf,
            preset: value.preset,
            tune: value.tune,
            bit_depth: value.bit_depth,
            color_space: value.color_space,
            queue_size: value.queue_size,
            preview: value.preview,
        }
    }
}

/// Pick a default backend for a codec when the saved TOML does not specify
/// one explicitly. RVL has no GPU acceleration path, so we force CPU even
/// when the legacy global `backend` requests something else; this avoids a
/// validation error on a project that was migrated from the
/// pre-per-codec-backend schema.
fn default_backend_for_codec(codec: EncoderCodec, fallback: EncoderBackend) -> EncoderBackend {
    if codec == EncoderCodec::Rvl
        && matches!(fallback, EncoderBackend::Nvidia | EncoderBackend::Vaapi)
    {
        EncoderBackend::Cpu
    } else {
        fallback
    }
}

fn default_queue_size() -> u32 {
    32
}

fn default_bit_depth() -> u8 {
    8
}

impl Default for EncoderConfig {
    fn default() -> Self {
        let video_codec = EncoderCodec::default();
        let depth_codec = default_depth_codec();
        let backend = EncoderBackend::default();
        Self {
            video_codec,
            depth_codec,
            backend,
            video_backend: backend,
            depth_backend: default_backend_for_codec(depth_codec, backend),
            chroma_subsampling: ChromaSubsampling::default(),
            crf: None,
            preset: None,
            tune: None,
            bit_depth: default_bit_depth(),
            color_space: EncoderColorSpace::default(),
            queue_size: default_queue_size(),
            preview: EncoderPreviewConfig::default(),
        }
    }
}

impl EncoderConfig {
    pub fn codec_for_pixel_format(&self, pixel_format: PixelFormat) -> EncoderCodec {
        match pixel_format {
            PixelFormat::Depth16 => self.depth_codec,
            // Gray8 (e.g. RealSense infrared) is plain monochrome video.
            // It nominally shares depth_codec, but the RVL encoder is
            // depth-specific and physically rejects non-depth16 frames, so
            // fall back to video_codec when depth_codec=rvl. Without this
            // fallback the infrared encoder process exits at episode start
            // with `rvl requires depth16 frames, got Gray8`.
            PixelFormat::Gray8 => self.depth_codec_with_gray8_fallback(),
            _ => self.video_codec,
        }
    }

    /// Resolve the encoder backend that should be paired with the codec
    /// produced by [`Self::codec_for_pixel_format`]. Color streams use
    /// `video_backend`; depth streams use `depth_backend`. Gray8 fallbacks
    /// follow the same rule as the codec fallback so an infrared channel
    /// re-routed to the video codec also gets the video backend.
    pub fn backend_for_pixel_format(&self, pixel_format: PixelFormat) -> EncoderBackend {
        match pixel_format {
            PixelFormat::Depth16 => self.depth_backend,
            PixelFormat::Gray8 => {
                if self.depth_codec == EncoderCodec::Rvl {
                    self.video_backend
                } else {
                    self.depth_backend
                }
            }
            _ => self.video_backend,
        }
    }

    fn depth_codec_with_gray8_fallback(&self) -> EncoderCodec {
        if self.depth_codec == EncoderCodec::Rvl {
            self.video_codec
        } else {
            self.depth_codec
        }
    }

    /// Container that the assembler will mux this encoder's video
    /// codec into. Derived from the codec via [`container_for`]; not a
    /// configurable knob anymore (the operator picks the codec, the
    /// container follows).
    pub fn resolved_container(&self) -> ContainerKind {
        container_for(self.video_codec)
    }

    pub fn resolved_depth_container(&self) -> ContainerKind {
        container_for(self.depth_codec)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.queue_size == 0 {
            return Err(ConfigError::Validation(
                "encoder: queue_size must be > 0".into(),
            ));
        }
        if let Some(crf) = self.crf {
            if crf > 51 {
                return Err(ConfigError::Validation(format!(
                    "encoder: crf must be in 0..=51, got {crf}"
                )));
            }
        }
        if !(self.bit_depth == 8 || self.bit_depth == 10) {
            return Err(ConfigError::Validation(format!(
                "encoder: bit_depth must be 8 or 10, got {}",
                self.bit_depth
            )));
        }
        if let Some(preset) = self.preset.as_deref() {
            if preset.trim().is_empty() {
                return Err(ConfigError::Validation(
                    "encoder: preset must not be an empty string (omit the field to use the libavcodec default)".into(),
                ));
            }
        }
        if let Some(tune) = self.tune.as_deref() {
            if tune.trim().is_empty() {
                return Err(ConfigError::Validation(
                    "encoder: tune must not be an empty string (omit the field to use the libavcodec default)".into(),
                ));
            }
        }
        // The previous `Rvl + (Nvidia|Vaapi) → error` validation lived
        // here; it was removed once the encoder crate split depth and
        // color into separate backend traits (see
        // `encoder/src/backend/{color,depth}`). RVL now flows through
        // `DepthBackendRegistry` and never sees `EncoderBackend`, so
        // the pairing isn't representable. Don't reintroduce this
        // check unless the runtime regains a unified backend axis.
        self.preview.validate()?;
        Ok(())
    }
}

fn default_depth_codec() -> EncoderCodec {
    EncoderCodec::Rvl
}

// ---------------------------------------------------------------------------
// Encoder role + preview block
// ---------------------------------------------------------------------------

/// Selects which runtime an instance of `rollio-encoder` runs. The
/// controller spawns up to two encoders per camera channel (one
/// recording, one preview); the role discriminator decides which
/// fields on `EncoderRuntimeConfigV2` are required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum EncoderRole {
    /// Episode-scoped encoder: subscribes to ControlEvent, opens a
    /// codec session on `RecordingStart`, publishes packets to the
    /// per-camera `recording-config` / `recording-packets` topics, and
    /// emits `EndOfStream` on `RecordingStop`.
    #[default]
    Recording,
    /// Always-on encoder: ignores RecordingStart/Stop, listens to the
    /// per-camera `preview-control` topic, and publishes either
    /// encoded preview packets or JPEG bytes depending on
    /// `[encoder.preview] output_mode`.
    Preview,
}

impl EncoderRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Preview => "preview",
        }
    }
}

/// Project-level preview output mode. Selects which preview transport
/// the encoder emits and which iceoryx2 topic the visualizer
/// subscribes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PreviewOutputMode {
    /// Encoder publishes one self-contained JPEG per preview frame on
    /// `…/preview-jpeg`. Visualizer bridges JPEG bytes verbatim; web
    /// UI renders via `<img>`.
    Jpeg,
    /// Encoder publishes encoded packets (H.264 for color channels,
    /// RVL for depth) on `…/preview-config` + `…/preview-packets`.
    /// Visualizer relays as new binary WS message kinds; web UI
    /// decodes via WebCodecs.
    #[default]
    Encoded,
}

impl PreviewOutputMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jpeg => "jpeg",
            Self::Encoded => "encoded",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PreviewResizePolicy {
    #[default]
    Dynamic,
    FixedSource,
}

impl PreviewResizePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dynamic => "dynamic",
            Self::FixedSource => "fixed-source",
        }
    }

    pub fn is_resizable(self) -> bool {
        self == Self::Dynamic
    }
}

/// `[encoder.preview]` project-config block. Owns every preview-only
/// production setting (size, fps, quality, codec). Recording knobs
/// stay on `[encoder]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncoderPreviewConfig {
    #[serde(default)]
    pub output_mode: PreviewOutputMode,
    /// Color/IR preview codec. Only `H264` is allowed when
    /// `output_mode = Encoded` (validated). Ignored when
    /// `output_mode = Jpeg` (the encoder always JPEG-encodes its
    /// internal RGB24 buffer).
    #[serde(default = "default_preview_color_codec")]
    pub color_codec: EncoderCodec,
    /// Depth preview codec. Only `Rvl` is allowed when
    /// `output_mode = Encoded` (validated). Ignored when
    /// `output_mode = Jpeg` (depth → grayscale RGB24 → JPEG).
    #[serde(default = "default_preview_depth_codec")]
    pub depth_codec: EncoderCodec,
    #[serde(default)]
    pub backend: EncoderBackend,
    #[serde(default = "default_preview_width")]
    pub width: u32,
    #[serde(default = "default_preview_height")]
    pub height: u32,
    #[serde(default = "default_preview_fps")]
    pub fps: u32,
    #[serde(default = "default_preview_gop_seconds")]
    pub gop_seconds: u32,
    /// CRF / quality knob for the preview codec. Tuned to favour
    /// low-CPU low-latency encoding rather than archival quality.
    #[serde(default = "default_preview_crf")]
    pub crf: Option<u8>,
    #[serde(default = "default_preview_jpeg_quality")]
    pub jpeg_quality: i32,
}

fn default_preview_color_codec() -> EncoderCodec {
    EncoderCodec::H264
}

fn default_preview_depth_codec() -> EncoderCodec {
    EncoderCodec::Rvl
}

fn default_preview_width() -> u32 {
    320
}

fn default_preview_height() -> u32 {
    240
}

fn default_preview_fps() -> u32 {
    15
}

fn default_preview_gop_seconds() -> u32 {
    1
}

fn default_preview_crf() -> Option<u8> {
    Some(26)
}

fn default_preview_jpeg_quality() -> i32 {
    50
}

impl Default for EncoderPreviewConfig {
    fn default() -> Self {
        Self {
            output_mode: PreviewOutputMode::default(),
            color_codec: default_preview_color_codec(),
            depth_codec: default_preview_depth_codec(),
            backend: EncoderBackend::default(),
            width: default_preview_width(),
            height: default_preview_height(),
            fps: default_preview_fps(),
            gop_seconds: default_preview_gop_seconds(),
            crf: default_preview_crf(),
            jpeg_quality: default_preview_jpeg_quality(),
        }
    }
}

impl EncoderPreviewConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.width == 0 || self.height == 0 {
            return Err(ConfigError::Validation(
                "encoder.preview: width and height must be > 0".into(),
            ));
        }
        if self.fps == 0 || self.fps > 1000 {
            return Err(ConfigError::Validation(format!(
                "encoder.preview: fps must be 1..1000, got {}",
                self.fps
            )));
        }
        if self.gop_seconds == 0 {
            return Err(ConfigError::Validation(
                "encoder.preview: gop_seconds must be > 0".into(),
            ));
        }
        if let Some(crf) = self.crf {
            if crf > 51 {
                return Err(ConfigError::Validation(format!(
                    "encoder.preview: crf must be 0..=51, got {crf}"
                )));
            }
        }
        if !(1..=100).contains(&self.jpeg_quality) {
            return Err(ConfigError::Validation(
                "encoder.preview: jpeg_quality must be 1..100".into(),
            ));
        }
        if self.output_mode == PreviewOutputMode::Encoded && self.color_codec != EncoderCodec::H264
        {
            return Err(ConfigError::Validation(format!(
                "encoder.preview: output_mode=encoded requires color_codec=h264, got {}",
                self.color_codec.as_str()
            )));
        }
        if self.output_mode == PreviewOutputMode::Encoded && self.depth_codec != EncoderCodec::Rvl {
            return Err(ConfigError::Validation(format!(
                "encoder.preview: output_mode=encoded requires depth_codec=rvl, got {}",
                self.depth_codec.as_str()
            )));
        }
        Ok(())
    }
}

// (Legacy `EncoderRuntimeConfig` removed; use `EncoderRuntimeConfigV2`.)

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub backend: StorageBackend,
    pub output_path: Option<String>,
    pub endpoint: Option<String>,
    #[serde(default = "default_storage_queue_size")]
    pub queue_size: u32,
}

fn default_storage_queue_size() -> u32 {
    32
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: StorageBackend::default(),
            output_path: Some("./output".into()),
            endpoint: None,
            queue_size: default_storage_queue_size(),
        }
    }
}

impl StorageConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.queue_size == 0 {
            return Err(ConfigError::Validation(
                "storage: queue_size must be > 0".into(),
            ));
        }
        match self.backend {
            StorageBackend::Local => {
                if self
                    .output_path
                    .as_deref()
                    .is_none_or(|path| path.trim().is_empty())
                {
                    return Err(ConfigError::Validation(
                        "storage: local backend requires output_path".into(),
                    ));
                }
            }
            StorageBackend::Http => {
                if self
                    .endpoint
                    .as_deref()
                    .is_none_or(|endpoint| endpoint.trim().is_empty())
                {
                    return Err(ConfigError::Validation(
                        "storage: http backend requires endpoint".into(),
                    ));
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    #[default]
    Local,
    Http,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageRuntimeConfig {
    pub process_id: String,
    pub backend: StorageBackend,
    pub output_path: Option<String>,
    pub endpoint: Option<String>,
    #[serde(default = "default_storage_queue_size")]
    pub queue_size: u32,
}

impl StorageRuntimeConfig {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.process_id.trim().is_empty() {
            return Err(ConfigError::Validation(
                "storage runtime: process_id must not be empty".into(),
            ));
        }
        StorageConfig {
            backend: self.backend,
            output_path: self.output_path.clone(),
            endpoint: self.endpoint.clone(),
            queue_size: self.queue_size,
        }
        .validate()
    }
}

impl FromStr for StorageRuntimeConfig {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: StorageRuntimeConfig = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

// ---------------------------------------------------------------------------
// Monitor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControllerConfig {
    #[serde(default = "default_shutdown_timeout_ms")]
    pub shutdown_timeout_ms: u64,
    #[serde(default = "default_child_poll_interval_ms")]
    pub child_poll_interval_ms: u64,
}

fn default_shutdown_timeout_ms() -> u64 {
    30_000
}

fn default_child_poll_interval_ms() -> u64 {
    100
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            shutdown_timeout_ms: default_shutdown_timeout_ms(),
            child_poll_interval_ms: default_child_poll_interval_ms(),
        }
    }
}

impl ControllerConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.shutdown_timeout_ms == 0 {
            return Err(ConfigError::Validation(
                "controller: shutdown_timeout_ms must be > 0".into(),
            ));
        }
        if self.child_poll_interval_ms == 0 {
            return Err(ConfigError::Validation(
                "controller: child_poll_interval_ms must be > 0".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizerRuntimeConfig {
    #[serde(default = "default_visualizer_port")]
    pub port: u16,
    #[serde(default)]
    pub cameras: Vec<String>,
    #[serde(default)]
    pub robots: Vec<String>,
    /// Preview transport this visualizer is wired up to bridge.
    /// Must match the project-level `[encoder.preview] output_mode`
    /// chosen by the controller; cross-validated by the controller.
    #[serde(default)]
    pub preview_output_mode: PreviewOutputMode,
}

fn default_visualizer_port() -> u16 {
    19090
}

impl Default for VisualizerRuntimeConfig {
    fn default() -> Self {
        Self {
            port: default_visualizer_port(),
            cameras: Vec::new(),
            robots: Vec::new(),
            preview_output_mode: PreviewOutputMode::default(),
        }
    }
}

impl VisualizerRuntimeConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.port == 0 {
            return Err(ConfigError::Validation(
                "visualizer: port must be > 0".into(),
            ));
        }
        Ok(())
    }
}

impl FromStr for VisualizerRuntimeConfig {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: VisualizerRuntimeConfig = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiRuntimeConfig {
    /// Optional explicit upstream URL for the control plane WebSocket
    /// (proxied at `/ws/control`). Defaults to
    /// `ws://127.0.0.1:<controller-control-port>` when the controller
    /// derives the runtime config; runtime callers must populate it before
    /// passing into `rollio-web-gateway`.
    pub control_websocket_url: Option<String>,
    /// Optional explicit upstream URL for the preview plane WebSocket
    /// (proxied at `/ws/preview`). Defaults to
    /// `ws://127.0.0.1:<visualizer-port>`.
    pub preview_websocket_url: Option<String>,
    #[serde(default = "default_ui_http_host")]
    pub http_host: String,
    #[serde(default = "default_ui_http_port")]
    pub http_port: u16,
    #[serde(default = "default_ui_start_key")]
    pub start_key: String,
    #[serde(default = "default_ui_stop_key")]
    pub stop_key: String,
    #[serde(default = "default_ui_keep_key")]
    pub keep_key: String,
    #[serde(default = "default_ui_discard_key")]
    pub discard_key: String,
}

fn default_ui_http_host() -> String {
    // `0.0.0.0` so the web gateway is reachable from every interface by
    // default. Operators that want to lock it down to loopback can edit
    // the field in the wizard's settings step (or the saved TOML).
    "0.0.0.0".into()
}

fn default_ui_http_port() -> u16 {
    3000
}

fn default_ui_start_key() -> String {
    "s".into()
}

fn default_ui_stop_key() -> String {
    "e".into()
}

fn default_ui_keep_key() -> String {
    "k".into()
}

fn default_ui_discard_key() -> String {
    "x".into()
}

impl Default for UiRuntimeConfig {
    fn default() -> Self {
        Self {
            control_websocket_url: None,
            preview_websocket_url: None,
            http_host: default_ui_http_host(),
            http_port: default_ui_http_port(),
            start_key: default_ui_start_key(),
            stop_key: default_ui_stop_key(),
            keep_key: default_ui_keep_key(),
            discard_key: default_ui_discard_key(),
        }
    }
}

impl UiRuntimeConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        for (label, url) in [
            ("control_websocket_url", &self.control_websocket_url),
            ("preview_websocket_url", &self.preview_websocket_url),
        ] {
            if let Some(url) = url {
                if url.trim().is_empty() {
                    return Err(ConfigError::Validation(format!(
                        "ui: {label} must not be empty"
                    )));
                }
            }
        }

        if self.http_host.trim().is_empty() {
            return Err(ConfigError::Validation(
                "ui: http_host must not be empty".into(),
            ));
        }
        if self.http_port == 0 {
            return Err(ConfigError::Validation(
                "ui: http_port must be greater than zero".into(),
            ));
        }

        let mut seen = HashSet::new();
        for (label, key) in [
            ("start_key", &self.start_key),
            ("stop_key", &self.stop_key),
            ("keep_key", &self.keep_key),
            ("discard_key", &self.discard_key),
        ] {
            let normalized = normalize_ui_key(label, key)?;
            if normalized == "d" || normalized == "r" {
                return Err(ConfigError::Validation(format!(
                    "ui: {label} conflicts with reserved UI shortcut \"{normalized}\""
                )));
            }
            if !seen.insert(normalized.clone()) {
                return Err(ConfigError::Validation(format!(
                    "ui: duplicate key binding \"{normalized}\""
                )));
            }
        }
        Ok(())
    }
}

fn normalize_ui_key(label: &str, raw: &str) -> Result<String, ConfigError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ConfigError::Validation(format!(
            "ui: {label} must not be empty"
        )));
    }

    let mut chars = trimmed.chars();
    let ch = chars.next().ok_or_else(|| {
        ConfigError::Validation(format!("ui: {label} must be a single printable character"))
    })?;
    if chars.next().is_some() {
        return Err(ConfigError::Validation(format!(
            "ui: {label} must be a single printable character"
        )));
    }
    if ch.is_control() {
        return Err(ConfigError::Validation(format!(
            "ui: {label} must be a printable character"
        )));
    }
    Ok(ch.to_ascii_lowercase().to_string())
}

impl FromStr for UiRuntimeConfig {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: UiRuntimeConfig = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

// (Legacy V1 topic / process-id helpers removed; the V2 equivalents
// (`encoder_process_id_v2`, `camera_frames_topic_v2`, etc.) live further
// down in this file alongside the channel-aware bus contract.)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorConfig {
    #[serde(default = "default_metrics_freq")]
    pub metrics_frequency_hz: f64,
    #[serde(default)]
    pub thresholds: std::collections::HashMap<String, ThresholdGroup>,
}

fn default_metrics_freq() -> f64 {
    1.0
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            metrics_frequency_hz: default_metrics_freq(),
            thresholds: HashMap::new(),
        }
    }
}

impl MonitorConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if !self.metrics_frequency_hz.is_finite() || self.metrics_frequency_hz <= 0.0 {
            return Err(ConfigError::Validation(
                "monitor: metrics_frequency_hz must be a positive finite number".into(),
            ));
        }

        for (process_id, thresholds) in &self.thresholds {
            if process_id.trim().is_empty() {
                return Err(ConfigError::Validation(
                    "monitor: threshold group names must not be empty".into(),
                ));
            }
            for (metric_name, threshold) in thresholds {
                if metric_name.trim().is_empty() {
                    return Err(ConfigError::Validation(format!(
                        "monitor: threshold metric name must not be empty in group \"{process_id}\""
                    )));
                }
                threshold.validate(process_id, metric_name)?;
            }
        }

        Ok(())
    }
}

/// Thresholds keyed by metric name within a given process.
pub type ThresholdGroup = HashMap<String, ThresholdDef>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdDef {
    pub explanation: String,
    pub gt: Option<f64>,
    pub lt: Option<f64>,
    pub gte: Option<f64>,
    pub lte: Option<f64>,
    pub outside: Option<[f64; 2]>,
    pub inside: Option<[f64; 2]>,
    pub occurred: Option<bool>,
    pub gap: Option<f64>,
    pub repeated: Option<bool>,
}

impl ThresholdDef {
    fn validate(&self, process_id: &str, metric_name: &str) -> Result<(), ConfigError> {
        if self.explanation.trim().is_empty() {
            return Err(ConfigError::Validation(format!(
                "monitor: threshold \"{process_id}.{metric_name}\" requires an explanation"
            )));
        }

        let configured_conditions = [
            self.gt.is_some(),
            self.lt.is_some(),
            self.gte.is_some(),
            self.lte.is_some(),
            self.outside.is_some(),
            self.inside.is_some(),
            self.occurred.is_some(),
            self.gap.is_some(),
            self.repeated.is_some(),
        ]
        .into_iter()
        .filter(|value| *value)
        .count();

        if configured_conditions == 0 {
            return Err(ConfigError::Validation(format!(
                "monitor: threshold \"{process_id}.{metric_name}\" must declare at least one condition"
            )));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Sprint Extra A hierarchical device-binary config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "ProjectConfigSerde")]
pub struct ProjectConfig {
    pub project_name: String,
    pub mode: CollectionMode,
    pub episode: EpisodeConfig,
    pub devices: Vec<BinaryDeviceConfig>,
    #[serde(default)]
    pub pairings: Vec<ChannelPairingConfig>,
    #[serde(default)]
    pub assembler: AssemblerConfig,
    pub storage: StorageConfig,
    #[serde(default)]
    pub monitor: MonitorConfig,
    #[serde(default)]
    pub controller: ControllerConfig,
    #[serde(default)]
    pub visualizer: VisualizerConfig,
    #[serde(default)]
    pub ui: UiRuntimeConfig,
    #[serde(default)]
    pub runtime: RuntimeConfig,
}

/// Process-wide runtime knobs that aren't tied to a single subsystem.
///
/// Currently just `advanced_pipeline_logs`, a switch the encoder /
/// visualizer / assembler consult to decide whether to emit verbose
/// per-frame telemetry. The controller forwards the value to child
/// processes via the `ROLLIO_ADVANCED_PIPELINE_LOGS` env var so the
/// subsystems don't need to re-parse the project TOML.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub advanced_pipeline_logs: bool,
}

impl RuntimeConfig {
    pub const ENV_ADVANCED_PIPELINE_LOGS: &'static str = "ROLLIO_ADVANCED_PIPELINE_LOGS";

    /// True if the calling process inherited `ROLLIO_ADVANCED_PIPELINE_LOGS`
    /// set to a truthy value (`1`, `true`, `yes`, case-insensitive).
    pub fn advanced_pipeline_logs_enabled() -> bool {
        std::env::var(Self::ENV_ADVANCED_PIPELINE_LOGS)
            .ok()
            .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false)
    }
}

#[derive(Debug, Deserialize)]
struct ProjectConfigSerde {
    #[serde(default = "default_project_name")]
    project_name: String,
    #[serde(default)]
    mode: Option<CollectionMode>,
    episode: EpisodeConfig,
    devices: Vec<BinaryDeviceConfig>,
    #[serde(default)]
    pairings: Vec<ChannelPairingConfig>,
    #[serde(default)]
    assembler: AssemblerConfig,
    storage: StorageConfig,
    #[serde(default)]
    monitor: MonitorConfig,
    #[serde(default)]
    controller: ControllerConfig,
    #[serde(default)]
    visualizer: VisualizerConfig,
    #[serde(default)]
    ui: UiRuntimeConfig,
    #[serde(default)]
    runtime: RuntimeConfig,
}

impl From<ProjectConfigSerde> for ProjectConfig {
    fn from(value: ProjectConfigSerde) -> Self {
        let mode = value
            .mode
            .unwrap_or_else(|| infer_collection_mode_v2(&value.pairings));
        Self {
            project_name: value.project_name,
            mode,
            episode: value.episode,
            devices: value.devices,
            pairings: value.pairings,
            assembler: value.assembler,
            storage: value.storage,
            monitor: value.monitor,
            controller: value.controller,
            visualizer: value.visualizer,
            ui: value.ui,
            runtime: value.runtime,
        }
    }
}

fn infer_collection_mode_v2(_pairings: &[ChannelPairingConfig]) -> CollectionMode {
    // Teleop is the only collection mode the setup wizard exposes; the
    // implicit default for any project (with or without pairings) is
    // teleop. Intervention configs left over from older saves still
    // round-trip through the explicit `mode = "intervention"` TOML key.
    CollectionMode::Teleop
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryDeviceConfig {
    pub name: String,
    #[serde(default)]
    pub executable: Option<String>,
    pub driver: String,
    pub id: String,
    pub bus_root: String,
    /// DDS domain id (Cora bridge only). When `None` the bridge consults
    /// `ROLLIO_DDS_DOMAIN_ID` and finally the SDK default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dds_domain_id: Option<u32>,
    /// DDS shared-memory transport segment size in bytes; 0 is rejected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dds_shm_segment_size: Option<u32>,
    /// Cora DDS callback thread pool size; 0 keeps the SDK default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dds_callback_threads: Option<u32>,
    #[serde(default)]
    pub channels: Vec<DeviceChannelConfigV2>,
    #[serde(flatten, default)]
    pub extra: toml::Table,
}

impl BinaryDeviceConfig {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.name.trim().is_empty() {
            return Err(ConfigError::Validation(
                "device: name must not be empty".into(),
            ));
        }
        if self.driver.trim().is_empty() {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": driver must not be empty",
                self.name
            )));
        }
        if self
            .executable
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": executable must not be empty when set",
                self.name
            )));
        }
        if self.id.trim().is_empty() {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": id must not be empty",
                self.name
            )));
        }
        if self.bus_root.trim().is_empty() {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": bus_root must not be empty",
                self.name
            )));
        }
        if self.channels.is_empty() {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": at least one [[devices.channels]] entry is required",
                self.name
            )));
        }
        let mut channel_types = HashSet::new();
        for channel in &self.channels {
            if !channel_types.insert(channel.channel_type.as_str()) {
                return Err(ConfigError::Validation(format!(
                    "device \"{}\": duplicate channel_type \"{}\"",
                    self.name, channel.channel_type
                )));
            }
            channel.validate(self)?;
        }
        Ok(())
    }

    pub fn channel_named(&self, channel_type: &str) -> Option<&DeviceChannelConfigV2> {
        self.channels
            .iter()
            .find(|channel| channel.channel_type == channel_type)
    }
}

impl FromStr for BinaryDeviceConfig {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: BinaryDeviceConfig = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

/// Per-channel recording encoder configuration. All fields are optional;
/// omitted fields use sensible defaults matching the old global `[encoder]`
/// defaults.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelRecordConfig {
    #[serde(default)]
    pub video_codec: Option<EncoderCodec>,
    #[serde(default)]
    pub depth_codec: Option<EncoderCodec>,
    #[serde(default)]
    pub backend: Option<EncoderBackend>,
    #[serde(default)]
    pub video_backend: Option<EncoderBackend>,
    #[serde(default)]
    pub depth_backend: Option<EncoderBackend>,
    #[serde(default)]
    pub chroma_subsampling: Option<ChromaSubsampling>,
    #[serde(default)]
    pub crf: Option<u8>,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub tune: Option<String>,
    #[serde(default)]
    pub bit_depth: Option<u8>,
    #[serde(default)]
    pub color_space: Option<EncoderColorSpace>,
    #[serde(default)]
    pub queue_size: Option<u32>,
}

impl ChannelRecordConfig {
    /// Resolve into a full EncoderConfig using defaults for any unset field.
    pub fn resolve(&self) -> EncoderConfig {
        EncoderConfig {
            video_codec: self.video_codec.unwrap_or_default(),
            depth_codec: self.depth_codec.unwrap_or(EncoderCodec::Rvl),
            backend: self.backend.unwrap_or_default(),
            video_backend: self
                .video_backend
                .unwrap_or(self.backend.unwrap_or_default()),
            depth_backend: self
                .depth_backend
                .unwrap_or(self.backend.unwrap_or_default()),
            chroma_subsampling: self.chroma_subsampling.unwrap_or_default(),
            crf: self.crf,
            preset: self.preset.clone(),
            tune: self.tune.clone(),
            bit_depth: self.bit_depth.unwrap_or(8),
            color_space: self.color_space.unwrap_or_default(),
            queue_size: self.queue_size.unwrap_or(default_queue_size()),
            preview: EncoderPreviewConfig::default(),
        }
    }
}

/// Per-channel preview encoder configuration. All fields are optional;
/// omitted fields use sensible defaults matching the old global
/// `[encoder.preview]` defaults.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelPreviewConfig {
    #[serde(default)]
    pub output_mode: Option<PreviewOutputMode>,
    #[serde(default)]
    pub color_codec: Option<EncoderCodec>,
    #[serde(default)]
    pub depth_codec: Option<EncoderCodec>,
    #[serde(default)]
    pub backend: Option<EncoderBackend>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub fps: Option<u32>,
    #[serde(default)]
    pub gop_seconds: Option<u32>,
    #[serde(default)]
    pub crf: Option<u8>,
    #[serde(default)]
    pub jpeg_quality: Option<i32>,
}

impl ChannelPreviewConfig {
    /// Resolve into a full EncoderPreviewConfig using defaults for unset fields.
    pub fn resolve(&self) -> EncoderPreviewConfig {
        EncoderPreviewConfig {
            output_mode: self.output_mode.unwrap_or_default(),
            color_codec: self.color_codec.unwrap_or(default_preview_color_codec()),
            depth_codec: self.depth_codec.unwrap_or(default_preview_depth_codec()),
            backend: self.backend.unwrap_or_default(),
            width: self.width.unwrap_or(default_preview_width()),
            height: self.height.unwrap_or(default_preview_height()),
            fps: self.fps.unwrap_or(default_preview_fps()),
            gop_seconds: self.gop_seconds.unwrap_or(default_preview_gop_seconds()),
            crf: self.crf.or(default_preview_crf()),
            jpeg_quality: self.jpeg_quality.unwrap_or(default_preview_jpeg_quality()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceChannelConfigV2 {
    pub channel_type: String,
    pub kind: DeviceType,
    #[serde(default = "default_enabled_true")]
    pub enabled: bool,
    /// User-facing name for this channel (independent per channel even when
    /// multiple channels share one device process / bus_root). Defaults to
    /// the device-supplied `default_name` when present, otherwise the
    /// channel_type. Renaming a row in the wizard mutates *only* this field.
    #[serde(default)]
    pub name: Option<String>,
    /// Human-readable label for this channel as reported by the device
    /// executable's `query --json` response (e.g. "AIRBOT E2", "V4L2 Camera").
    /// Used purely for display; the controller no longer derives this from
    /// the driver name.
    #[serde(default)]
    pub channel_label: Option<String>,
    #[serde(default)]
    pub mode: Option<RobotMode>,
    #[serde(default)]
    pub dof: Option<u32>,
    #[serde(default)]
    pub publish_states: Vec<RobotStateKind>,
    #[serde(default)]
    pub recorded_states: Vec<RobotStateKind>,
    #[serde(default)]
    pub control_frequency_hz: Option<f64>,
    #[serde(default)]
    pub profile: Option<CameraChannelProfile>,
    /// Per-camera opt-in for the always-on preview encoder. When `true`
    /// (default), the controller spawns a `role=preview` `rollio-encoder`
    /// alongside the `role=recording` one for this channel; when
    /// `false`, no preview is produced for this channel and the
    /// visualizer omits it from its camera list.
    /// Validation rejects a non-default value on robot channels.
    #[serde(default = "default_enabled_true")]
    pub preview_enabled: bool,
    /// Whether this channel's streams are recorded to the episode dataset.
    /// When `false`, the channel is live-only (preview still works if
    /// `preview_enabled` is true). Default: `true`.
    #[serde(default = "default_enabled_true")]
    pub record_enabled: bool,
    /// Per-channel recording encoder settings. When `None`, sensible
    /// defaults are used (H264/Rvl, CPU backend, crf=None, etc.).
    #[serde(default)]
    pub record: Option<ChannelRecordConfig>,
    /// Per-channel preview encoder settings. When `None`, sensible
    /// defaults are used (Encoded mode, H264, 320×240@15fps, etc.).
    #[serde(default, rename = "preview_config")]
    pub preview_settings: Option<ChannelPreviewConfig>,
    #[serde(default)]
    pub command_defaults: ChannelCommandDefaults,
    /// Hardware-reported value limits for each published state kind.
    /// Skipped during (de)serialization: the controller refreshes these from
    /// a fresh `query` invocation on every startup (setup and `collect`)
    /// and feeds them to downstream consumers (visualizer) in-memory only.
    /// Storing them in the TOML caused stale limits when drivers updated
    /// and offered no operator value (operators never edit them).
    #[serde(skip)]
    pub value_limits: Vec<StateValueLimitsEntry>,
    /// Direct-joint pairing peers as reported by the device's `query --json`
    /// response. Refreshed on every controller startup so pairing legality
    /// follows the live driver schema instead of any hard-coded vendor table.
    /// Skipped during (de)serialization for the same reason `value_limits`
    /// is: stale TOML caused subtle pairing breakages when drivers shipped
    /// new compatibility metadata, and operators never hand-edit it.
    #[serde(skip)]
    pub direct_joint_compatibility: DirectJointCompatibility,
    /// Robot command kinds the channel accepts, as reported by the driver's
    /// `query --json`. Refreshed on every controller startup; not persisted.
    #[serde(skip)]
    pub supported_commands: Vec<RobotCommandKind>,
    /// Sensor sample kinds this channel publishes. Only meaningful when
    /// `kind = "sensor"`; rejected by validation otherwise. Robot
    /// channels keep using `publish_states`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub publish_sensors: Vec<SensorStateKind>,
    /// Sample rate for `kind = "sensor"` channels. Required (positive,
    /// finite) when the channel is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate_hz: Option<f64>,
    /// Shape per published sensor kind. The driver reports this in
    /// `query --json`; not persisted because operators never hand-edit
    /// it and a stale shape silently corrupts downstream Parquet.
    #[serde(skip)]
    pub sensor_shape_hints: std::collections::BTreeMap<SensorStateKind, Vec<u32>>,
    #[serde(flatten, default)]
    pub extra: toml::Table,
}

fn default_enabled_true() -> bool {
    true
}

impl DeviceChannelConfigV2 {
    fn validate(&self, device: &BinaryDeviceConfig) -> Result<(), ConfigError> {
        if self.channel_type.trim().is_empty() {
            return Err(ConfigError::Validation(format!(
                "device \"{}\": channel_type must not be empty",
                device.name
            )));
        }
        match self.kind {
            DeviceType::Camera => {
                if self.mode.is_some() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": camera channels do not accept mode",
                        device.name, self.channel_type
                    )));
                }
                if self.dof.is_some() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": camera channels do not accept dof",
                        device.name, self.channel_type
                    )));
                }
                if !self.publish_states.is_empty() || !self.recorded_states.is_empty() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": camera channels do not accept publish_states or recorded_states",
                        device.name, self.channel_type
                    )));
                }
                if self.control_frequency_hz.is_some() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": camera channels do not accept control_frequency_hz",
                        device.name, self.channel_type
                    )));
                }
                self.profile.as_ref().ok_or_else(|| {
                    ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": enabled camera channels require profile",
                        device.name, self.channel_type
                    ))
                })?;
                if let Some(profile) = &self.profile {
                    profile.validate(device, &self.channel_type)?;
                }
                self.command_defaults
                    .validate(device, &self.channel_type, 0)?;
            }
            DeviceType::Robot => {
                if !self.preview_enabled {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": robot channels do not accept preview_enabled = false",
                        device.name, self.channel_type
                    )));
                }
                let dof = self.dof.ok_or_else(|| {
                    ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": robot channels require dof",
                        device.name, self.channel_type
                    ))
                })?;
                if dof == 0 || dof as usize > MAX_DOF {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": dof must be 1..{}, got {}",
                        device.name, self.channel_type, MAX_DOF, dof
                    )));
                }
                if self.enabled && self.mode.is_none() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": enabled robot channels require mode",
                        device.name, self.channel_type
                    )));
                }
                if self.enabled && self.publish_states.is_empty() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": enabled robot channels require publish_states",
                        device.name, self.channel_type
                    )));
                }
                if self
                    .recorded_states
                    .iter()
                    .any(|state| !self.publish_states.contains(state))
                {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": recorded_states must be a subset of publish_states",
                        device.name, self.channel_type
                    )));
                }
                if let Some(profile) = &self.profile {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": robot channels do not accept profile ({:?})",
                        device.name, self.channel_type, profile
                    )));
                }
                if let Some(freq) = self.control_frequency_hz {
                    if !freq.is_finite() || freq <= 0.0 {
                        return Err(ConfigError::Validation(format!(
                            "device \"{}\" channel \"{}\": control_frequency_hz must be a positive finite number",
                            device.name, self.channel_type
                        )));
                    }
                }
                self.command_defaults
                    .validate(device, &self.channel_type, dof as usize)?;
            }
            DeviceType::Sensor => {
                if self.mode.is_some() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": sensor channels do not accept mode",
                        device.name, self.channel_type
                    )));
                }
                if self.dof.is_some() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": sensor channels do not accept dof",
                        device.name, self.channel_type
                    )));
                }
                if self.control_frequency_hz.is_some() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": sensor channels do not accept control_frequency_hz",
                        device.name, self.channel_type
                    )));
                }
                if !self.publish_states.is_empty() || !self.recorded_states.is_empty() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": sensor channels use publish_sensors, not publish_states/recorded_states",
                        device.name, self.channel_type
                    )));
                }
                if let Some(profile) = &self.profile {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": sensor channels do not accept profile ({:?})",
                        device.name, self.channel_type, profile
                    )));
                }
                if self.enabled {
                    if self.publish_sensors.is_empty() {
                        return Err(ConfigError::Validation(format!(
                            "device \"{}\" channel \"{}\": enabled sensor channels require publish_sensors",
                            device.name, self.channel_type
                        )));
                    }
                    let rate = self.sample_rate_hz.ok_or_else(|| {
                        ConfigError::Validation(format!(
                            "device \"{}\" channel \"{}\": enabled sensor channels require sample_rate_hz",
                            device.name, self.channel_type
                        ))
                    })?;
                    if !rate.is_finite() || rate <= 0.0 {
                        return Err(ConfigError::Validation(format!(
                            "device \"{}\" channel \"{}\": sample_rate_hz must be a positive finite number",
                            device.name, self.channel_type
                        )));
                    }
                }
            }
        }
        if !matches!(self.kind, DeviceType::Sensor)
            && (!self.publish_sensors.is_empty() || self.sample_rate_hz.is_some())
        {
            return Err(ConfigError::Validation(format!(
                "device \"{}\" channel \"{}\": publish_sensors and sample_rate_hz are sensor-only fields",
                device.name, self.channel_type
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CameraChannelProfile {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub pixel_format: PixelFormat,
    #[serde(default)]
    pub native_pixel_format: Option<String>,
    /// MJPEG quality (libavcodec qscale, 1-31, lower = better). Default: 5.
    #[serde(default)]
    pub mjpeg_quality: Option<u32>,
    /// H.264 target bitrate in bits per second. Default: width*height*fps/10.
    #[serde(default)]
    pub h264_bitrate_bps: Option<u32>,
    /// H.264 GOP size (keyframe interval). Default: fps.
    #[serde(default)]
    pub h264_gop: Option<u32>,
    /// H.264 x264 preset. Default: "ultrafast".
    #[serde(default)]
    pub h264_preset: Option<String>,
    /// H.264 x264 tune. Default: "zerolatency".
    #[serde(default)]
    pub h264_tune: Option<String>,
    /// H.264 x264 profile. Default: "baseline".
    #[serde(default)]
    pub h264_profile: Option<String>,
}

impl CameraChannelProfile {
    fn validate(&self, device: &BinaryDeviceConfig, channel_type: &str) -> Result<(), ConfigError> {
        if self.width == 0 || self.height == 0 {
            return Err(ConfigError::Validation(format!(
                "device \"{}\" channel \"{}\": camera profile requires non-zero width and height",
                device.name, channel_type
            )));
        }
        if self.fps == 0 || self.fps > 1000 {
            return Err(ConfigError::Validation(format!(
                "device \"{}\" channel \"{}\": camera profile fps must be 1..1000, got {}",
                device.name, channel_type, self.fps
            )));
        }
        if self
            .native_pixel_format
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ConfigError::Validation(format!(
                "device \"{}\" channel \"{}\": native_pixel_format must not be empty",
                device.name, channel_type
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChannelCommandDefaults {
    #[serde(default)]
    pub joint_mit_kp: Vec<f64>,
    #[serde(default)]
    pub joint_mit_kd: Vec<f64>,
    #[serde(default)]
    pub parallel_mit_kp: Vec<f64>,
    #[serde(default)]
    pub parallel_mit_kd: Vec<f64>,
}

impl ChannelCommandDefaults {
    fn validate(
        &self,
        device: &BinaryDeviceConfig,
        channel_type: &str,
        dof: usize,
    ) -> Result<(), ConfigError> {
        for (label, values, limit) in [
            ("joint_mit_kp", &self.joint_mit_kp, MAX_DOF),
            ("joint_mit_kd", &self.joint_mit_kd, MAX_DOF),
            ("parallel_mit_kp", &self.parallel_mit_kp, MAX_PARALLEL),
            ("parallel_mit_kd", &self.parallel_mit_kd, MAX_PARALLEL),
        ] {
            if values.iter().any(|value| !value.is_finite()) {
                return Err(ConfigError::Validation(format!(
                    "device \"{}\" channel \"{}\": {} values must be finite",
                    device.name, channel_type, label
                )));
            }
            if values.len() > limit {
                return Err(ConfigError::Validation(format!(
                    "device \"{}\" channel \"{}\": {} may contain at most {} values",
                    device.name, channel_type, label, limit
                )));
            }
        }
        if !self.joint_mit_kp.is_empty() && dof != 0 && self.joint_mit_kp.len() != dof {
            return Err(ConfigError::Validation(format!(
                "device \"{}\" channel \"{}\": joint_mit_kp length must match dof {}",
                device.name, channel_type, dof
            )));
        }
        if !self.joint_mit_kd.is_empty() && dof != 0 && self.joint_mit_kd.len() != dof {
            return Err(ConfigError::Validation(format!(
                "device \"{}\" channel \"{}\": joint_mit_kd length must match dof {}",
                device.name, channel_type, dof
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RobotStateKind {
    /// `JointPosition` is the default so `StateValueLimitsEntry::default()`
    /// (used by serde when the entry is absent or partially specified) does
    /// not silently bind the wrong kind.
    #[default]
    JointPosition,
    JointVelocity,
    JointEffort,
    EndEffectorPose,
    EndEffectorTwist,
    EndEffectorWrench,
    ParallelPosition,
    ParallelVelocity,
    ParallelEffort,
}

impl RobotStateKind {
    pub fn topic_suffix(self) -> &'static str {
        match self {
            Self::JointPosition => "joint_position",
            Self::JointVelocity => "joint_velocity",
            Self::JointEffort => "joint_effort",
            Self::EndEffectorPose => "end_effector_pose",
            Self::EndEffectorTwist => "end_effector_twist",
            Self::EndEffectorWrench => "end_effector_wrench",
            Self::ParallelPosition => "parallel_position",
            Self::ParallelVelocity => "parallel_velocity",
            Self::ParallelEffort => "parallel_effort",
        }
    }

    pub fn value_len(self, dof: u32) -> u32 {
        match self {
            Self::JointPosition | Self::JointVelocity | Self::JointEffort => dof,
            Self::ParallelPosition | Self::ParallelVelocity | Self::ParallelEffort => {
                dof.min(MAX_PARALLEL as u32)
            }
            Self::EndEffectorPose => 7,
            Self::EndEffectorTwist | Self::EndEffectorWrench => 6,
        }
    }

    pub fn uses_pose_payload(self) -> bool {
        matches!(self, Self::EndEffectorPose)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RobotCommandKind {
    JointPosition,
    JointMit,
    EndPose,
    ParallelPosition,
    ParallelMit,
}

impl RobotCommandKind {
    pub fn topic_suffix(self) -> &'static str {
        match self {
            Self::JointPosition => "joint_position",
            Self::JointMit => "joint_mit",
            Self::EndPose => "end_pose",
            Self::ParallelPosition => "parallel_position",
            Self::ParallelMit => "parallel_mit",
        }
    }

    pub fn uses_pose_payload(self) -> bool {
        matches!(self, Self::EndPose)
    }
}

/// Per-state value limits for visualization and (future) safety checks.
///
/// Each entry binds a `RobotStateKind` to per-element `min`/`max` bounds.
/// Lengths should match `RobotStateKind::value_len(dof)` when the limits
/// originate from the robot driver, but consumers must tolerate empty or
/// shorter slices and fall back to sensible defaults (the visualization
/// layer treats missing values as "unknown" and uses fallback ranges).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct StateValueLimitsEntry {
    pub state_kind: RobotStateKind,
    #[serde(default)]
    pub min: Vec<f64>,
    #[serde(default)]
    pub max: Vec<f64>,
}

impl StateValueLimitsEntry {
    pub fn new(state_kind: RobotStateKind, min: Vec<f64>, max: Vec<f64>) -> Self {
        Self {
            state_kind,
            min,
            max,
        }
    }

    /// Symmetric ±`bound` limits of length `len` (e.g. ±π for joint position).
    pub fn symmetric(state_kind: RobotStateKind, bound: f64, len: usize) -> Self {
        Self {
            state_kind,
            min: vec![-bound; len],
            max: vec![bound; len],
        }
    }

    /// Asymmetric `[min, max]` limits applied uniformly to every element.
    pub fn uniform(state_kind: RobotStateKind, min: f64, max: f64, len: usize) -> Self {
        Self {
            state_kind,
            min: vec![min; len],
            max: vec![max; len],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelPairingConfig {
    pub leader_device: String,
    pub leader_channel_type: String,
    pub follower_device: String,
    pub follower_channel_type: String,
    #[serde(default = "default_mapping")]
    pub mapping: MappingStrategy,
    pub leader_state: RobotStateKind,
    pub follower_command: RobotCommandKind,
    #[serde(default)]
    pub joint_index_map: Vec<u32>,
    #[serde(default)]
    pub joint_scales: Vec<f64>,
}

impl ChannelPairingConfig {
    fn validate(&self, config: &ProjectConfig) -> Result<(), ConfigError> {
        let leader_device = config.device_named(&self.leader_device).ok_or_else(|| {
            ConfigError::Validation(format!(
                "pairing references unknown leader_device \"{}\"",
                self.leader_device
            ))
        })?;
        let follower_device = config.device_named(&self.follower_device).ok_or_else(|| {
            ConfigError::Validation(format!(
                "pairing references unknown follower_device \"{}\"",
                self.follower_device
            ))
        })?;
        let leader = leader_device
            .channel_named(&self.leader_channel_type)
            .ok_or_else(|| {
                ConfigError::Validation(format!(
                    "pairing references unknown leader channel {}:{}",
                    self.leader_device, self.leader_channel_type
                ))
            })?;
        let follower = follower_device
            .channel_named(&self.follower_channel_type)
            .ok_or_else(|| {
                ConfigError::Validation(format!(
                    "pairing references unknown follower channel {}:{}",
                    self.follower_device, self.follower_channel_type
                ))
            })?;
        if leader.kind != DeviceType::Robot || follower.kind != DeviceType::Robot {
            return Err(ConfigError::Validation(format!(
                "pairing {}:{} -> {}:{} must target robot channels",
                self.leader_device,
                self.leader_channel_type,
                self.follower_device,
                self.follower_channel_type
            )));
        }
        if self.leader_device == self.follower_device
            && self.leader_channel_type == self.follower_channel_type
        {
            return Err(ConfigError::Validation(
                "pairing leader and follower channel must differ".into(),
            ));
        }
        if !leader.publish_states.contains(&self.leader_state) {
            return Err(ConfigError::Validation(format!(
                "pairing {}:{} leader_state {:?} is not present in publish_states",
                self.leader_device, self.leader_channel_type, self.leader_state
            )));
        }
        match self.mapping {
            MappingStrategy::DirectJoint => {
                // Hard predicate per teleop-policy redesign: leader publishes
                // joint_position, follower accepts joint_position commands,
                // DOFs match, and BOTH drivers must opt in via
                // `direct_joint_compatibility`. Cross-vendor pairs are
                // intentionally rejected here -- drivers own the safety
                // story for joint-space teleop.
                if self.leader_state != RobotStateKind::JointPosition {
                    return Err(ConfigError::Validation(format!(
                        "direct-joint mapping requires leader_state=joint_position (got {:?}) for {}:{} -> {}:{}",
                        self.leader_state,
                        self.leader_device,
                        self.leader_channel_type,
                        self.follower_device,
                        self.follower_channel_type,
                    )));
                }
                if self.follower_command != RobotCommandKind::JointPosition {
                    return Err(ConfigError::Validation(format!(
                        "direct-joint mapping requires follower_command=joint_position (got {:?}) for {}:{} -> {}:{}",
                        self.follower_command,
                        self.leader_device,
                        self.leader_channel_type,
                        self.follower_device,
                        self.follower_channel_type,
                    )));
                }
                // `supported_commands` is `#[serde(skip)]`: persisted
                // configs don't carry it. Only enforce the predicate
                // when the field has been populated (i.e. by the
                // controller's runtime refresh of driver queries),
                // otherwise trust the loaded config and let runtime
                // surface the mismatch.
                if !follower.supported_commands.is_empty()
                    && !follower
                        .supported_commands
                        .contains(&RobotCommandKind::JointPosition)
                {
                    return Err(ConfigError::Validation(format!(
                        "direct-joint mapping: follower {}:{} does not advertise joint_position in supported_commands",
                        self.follower_device, self.follower_channel_type,
                    )));
                }
                self.validate_direct_joint_dof_and_index_map(leader, follower)?;
                // The whitelist check is also runtime-populated; skip
                // when neither side carries any whitelist entries
                // (e.g. parsed from TOML without a driver refresh) so
                // standalone config tests can round-trip without
                // surfacing a phantom rejection.
                let whitelist_present = !leader.direct_joint_compatibility.can_lead.is_empty()
                    || !follower.direct_joint_compatibility.can_follow.is_empty();
                if whitelist_present {
                    self.require_direct_joint_compatibility(
                        leader_device,
                        leader,
                        follower_device,
                        follower,
                    )?;
                }
            }
            MappingStrategy::Cartesian => {
                if self.leader_state != RobotStateKind::EndEffectorPose
                    || self.follower_command != RobotCommandKind::EndPose
                {
                    return Err(ConfigError::Validation(
                        "cartesian mapping requires leader_state=end_effector_pose and follower_command=end_pose"
                            .into(),
                    ));
                }
                if !follower.supported_commands.is_empty()
                    && !follower
                        .supported_commands
                        .contains(&RobotCommandKind::EndPose)
                {
                    return Err(ConfigError::Validation(format!(
                        "cartesian mapping: follower {}:{} does not advertise end_pose in supported_commands",
                        self.follower_device, self.follower_channel_type,
                    )));
                }
                if !self.joint_index_map.is_empty() || !self.joint_scales.is_empty() {
                    return Err(ConfigError::Validation(
                        "cartesian mapping does not use joint_index_map or joint_scales".into(),
                    ));
                }
            }
            MappingStrategy::Parallel => {
                // Single-DOF parallel-gripper teleop. The mapping ratio
                // lives in joint_scales[0] so the wire format stays
                // compatible with the router's existing scaling code path.
                if leader.dof != Some(1) || follower.dof != Some(1) {
                    return Err(ConfigError::Validation(format!(
                        "parallel mapping requires dof=1 on both sides for {}:{} -> {}:{} (got leader={:?}, follower={:?})",
                        self.leader_device,
                        self.leader_channel_type,
                        self.follower_device,
                        self.follower_channel_type,
                        leader.dof,
                        follower.dof,
                    )));
                }
                if self.leader_state != RobotStateKind::ParallelPosition {
                    return Err(ConfigError::Validation(format!(
                        "parallel mapping requires leader_state=parallel_position (got {:?}) for {}:{} -> {}:{}",
                        self.leader_state,
                        self.leader_device,
                        self.leader_channel_type,
                        self.follower_device,
                        self.follower_channel_type,
                    )));
                }
                if !matches!(
                    self.follower_command,
                    RobotCommandKind::ParallelPosition | RobotCommandKind::ParallelMit
                ) {
                    return Err(ConfigError::Validation(format!(
                        "parallel mapping requires follower_command in {{parallel_position, parallel_mit}} (got {:?}) for {}:{} -> {}:{}",
                        self.follower_command,
                        self.leader_device,
                        self.leader_channel_type,
                        self.follower_device,
                        self.follower_channel_type,
                    )));
                }
                if !follower.supported_commands.is_empty()
                    && !follower
                        .supported_commands
                        .contains(&RobotCommandKind::ParallelPosition)
                    && !follower
                        .supported_commands
                        .contains(&RobotCommandKind::ParallelMit)
                {
                    return Err(ConfigError::Validation(format!(
                        "parallel mapping: follower {}:{} does not advertise parallel_position or parallel_mit in supported_commands",
                        self.follower_device, self.follower_channel_type,
                    )));
                }
                if !self.joint_index_map.is_empty() {
                    return Err(ConfigError::Validation(
                        "parallel mapping does not use joint_index_map (ratio lives in joint_scales[0])".into(),
                    ));
                }
                if self.joint_scales.len() != 1 {
                    return Err(ConfigError::Validation(format!(
                        "parallel mapping requires exactly one joint_scales entry (the ratio); got {} for {}:{} -> {}:{}",
                        self.joint_scales.len(),
                        self.leader_device,
                        self.leader_channel_type,
                        self.follower_device,
                        self.follower_channel_type,
                    )));
                }
                let ratio = self.joint_scales[0];
                if !ratio.is_finite() || ratio == 0.0 {
                    return Err(ConfigError::Validation(format!(
                        "parallel mapping ratio must be finite and non-zero (got {ratio}) for {}:{} -> {}:{}",
                        self.leader_device,
                        self.leader_channel_type,
                        self.follower_device,
                        self.follower_channel_type,
                    )));
                }
            }
        }
        if self.joint_scales.iter().any(|value| !value.is_finite()) {
            return Err(ConfigError::Validation(
                "pairing joint_scales must be finite".into(),
            ));
        }
        Ok(())
    }

    /// Technical (driver-agnostic) DOF and `joint_index_map` checks for a
    /// direct-joint mapping. Ported from the legacy `PairConfig` validator
    /// so the channel-pairing path catches the same misconfigurations the
    /// device-pairing path used to:
    ///
    /// - Identity mapping requires `leader.dof >= follower.dof`.
    /// - An explicit `joint_index_map` must have one entry per follower
    ///   joint, and every leader index must be in range.
    /// - `joint_scales`, when present, must align with either the
    ///   `joint_index_map` length or the follower DOF.
    ///
    /// Driver name is intentionally NOT consulted here: cross-vendor
    /// direct-joint pairings (e.g. AIRBOT Play leader -> AGX Nero
    /// follower) are allowed whenever the joint shapes line up.
    fn validate_direct_joint_dof_and_index_map(
        &self,
        leader: &DeviceChannelConfigV2,
        follower: &DeviceChannelConfigV2,
    ) -> Result<(), ConfigError> {
        let leader_dof = leader.dof.unwrap_or(0);
        let follower_dof = follower.dof.unwrap_or(0);

        if self.joint_index_map.is_empty() {
            if leader_dof < follower_dof {
                return Err(ConfigError::Validation(format!(
                    "pairing {}:{} -> {}:{}: direct-joint identity mapping requires leader dof ({leader_dof}) >= follower dof ({follower_dof}); add a joint_index_map to remap",
                    self.leader_device,
                    self.leader_channel_type,
                    self.follower_device,
                    self.follower_channel_type,
                )));
            }
        } else {
            if self.joint_index_map.len() != follower_dof as usize {
                return Err(ConfigError::Validation(format!(
                    "pairing {}:{} -> {}:{}: joint_index_map length ({}) must match follower dof ({follower_dof})",
                    self.leader_device,
                    self.leader_channel_type,
                    self.follower_device,
                    self.follower_channel_type,
                    self.joint_index_map.len(),
                )));
            }
            for (index, leader_joint) in self.joint_index_map.iter().enumerate() {
                if *leader_joint >= leader_dof {
                    return Err(ConfigError::Validation(format!(
                        "pairing {}:{} -> {}:{}: joint_index_map[{index}]={} exceeds leader dof ({leader_dof})",
                        self.leader_device,
                        self.leader_channel_type,
                        self.follower_device,
                        self.follower_channel_type,
                        leader_joint,
                    )));
                }
            }
        }

        if !self.joint_scales.is_empty() {
            let expected_len = if self.joint_index_map.is_empty() {
                follower_dof as usize
            } else {
                self.joint_index_map.len()
            };
            if self.joint_scales.len() != expected_len {
                return Err(ConfigError::Validation(format!(
                    "pairing {}:{} -> {}:{}: joint_scales length ({}) must match {expected_len} mapped follower joints",
                    self.leader_device,
                    self.leader_channel_type,
                    self.follower_device,
                    self.follower_channel_type,
                    self.joint_scales.len(),
                )));
            }
        }

        Ok(())
    }

    /// Hard predicate on the per-channel `direct_joint_compatibility`
    /// blob each driver reports in its `query --json`. BOTH directions
    /// must opt in (leader's `can_lead` lists the follower peer, AND
    /// follower's `can_follow` lists the leader peer); otherwise the
    /// pairing is rejected. This is the safety story for joint-space
    /// teleop: drivers vouch for the peer they accept, and the wizard
    /// refuses pairings that haven't been declared. Operators wanting
    /// cross-vendor direct-joint teleop must update both device
    /// executables to advertise each other before the pairing will
    /// validate.
    fn require_direct_joint_compatibility(
        &self,
        leader_device: &BinaryDeviceConfig,
        leader_channel: &DeviceChannelConfigV2,
        follower_device: &BinaryDeviceConfig,
        follower_channel: &DeviceChannelConfigV2,
    ) -> Result<(), ConfigError> {
        let leader_meta = &leader_channel.direct_joint_compatibility;
        let follower_meta = &follower_channel.direct_joint_compatibility;
        let leader_endorses = leader_meta.can_lead.iter().any(|peer| {
            peer.driver == follower_device.driver
                && peer.channel_type == follower_channel.channel_type
        });
        let follower_endorses = follower_meta.can_follow.iter().any(|peer| {
            peer.driver == leader_device.driver && peer.channel_type == leader_channel.channel_type
        });
        if leader_endorses && follower_endorses {
            return Ok(());
        }
        let mut missing = Vec::new();
        if !leader_endorses {
            missing.push(format!(
                "leader driver \"{}\" must list follower peer (driver=\"{}\", channel_type=\"{}\") in direct_joint_compatibility.can_lead",
                leader_device.driver,
                follower_device.driver,
                follower_channel.channel_type,
            ));
        }
        if !follower_endorses {
            missing.push(format!(
                "follower driver \"{}\" must list leader peer (driver=\"{}\", channel_type=\"{}\") in direct_joint_compatibility.can_follow",
                follower_device.driver,
                leader_device.driver,
                leader_channel.channel_type,
            ));
        }
        Err(ConfigError::Validation(format!(
            "direct-joint mapping rejects {}:{} -> {}:{}: {}",
            self.leader_device,
            self.leader_channel_type,
            self.follower_device,
            self.follower_channel_type,
            missing.join("; "),
        )))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VisualizerConfig {
    #[serde(default = "default_visualizer_port")]
    pub port: u16,
}

impl VisualizerConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        VisualizerRuntimeConfig {
            port: self.port,
            cameras: Vec::new(),
            robots: Vec::new(),
            preview_output_mode: PreviewOutputMode::default(),
        }
        .validate()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizerCameraSourceConfig {
    pub channel_id: String,
    /// Topic identifying the channel's bus root + channel type. The
    /// visualizer derives the actual subscribed topic name from the
    /// `preview_output_mode` it was launched in:
    /// - `Jpeg`  -> `<bus_root>/<channel_type>/preview-jpeg`
    /// - `Encoded` -> `<bus_root>/<channel_type>/preview-config` +
    ///   `<bus_root>/<channel_type>/preview-packets`
    pub bus_root: String,
    pub channel_type: String,
    #[serde(default)]
    pub preview_resize_policy: PreviewResizePolicy,
    #[serde(default)]
    pub source_width: Option<u32>,
    #[serde(default)]
    pub source_height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizerRobotSourceConfig {
    pub channel_id: String,
    pub state_kind: RobotStateKind,
    pub state_topic: String,
    /// Optional per-element value bounds for this state kind. The visualizer
    /// forwards them to the UI so bars can be normalized against the real
    /// hardware envelope instead of an arbitrary fallback.
    #[serde(default)]
    pub value_min: Vec<f64>,
    #[serde(default)]
    pub value_max: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizerRuntimeConfigV2 {
    pub port: u16,
    #[serde(default)]
    pub camera_sources: Vec<VisualizerCameraSourceConfig>,
    #[serde(default)]
    pub robot_sources: Vec<VisualizerRobotSourceConfig>,
    /// Mode the encoder is configured to emit; selects which iceoryx2
    /// topics the visualizer subscribes to per camera and which binary
    /// WS message kind the visualizer broadcasts to UI clients.
    #[serde(default)]
    pub preview_output_mode: PreviewOutputMode,
}

impl VisualizerRuntimeConfigV2 {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        VisualizerRuntimeConfig {
            port: self.port,
            cameras: self
                .camera_sources
                .iter()
                .map(|source| source.channel_id.clone())
                .collect(),
            robots: self
                .robot_sources
                .iter()
                .map(|source| source.channel_id.clone())
                .collect(),
            preview_output_mode: self.preview_output_mode,
        }
        .validate()?;
        Ok(())
    }
}

impl FromStr for VisualizerRuntimeConfigV2 {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: VisualizerRuntimeConfigV2 = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeleopRuntimeConfigV2 {
    pub process_id: String,
    pub leader_channel_id: String,
    pub follower_channel_id: String,
    pub leader_state_kind: RobotStateKind,
    pub leader_state_topic: String,
    pub follower_command_kind: RobotCommandKind,
    pub follower_command_topic: String,
    /// Optional follower-state subscription used by the initial syncing
    /// phase so the router can ramp commands toward the leader at
    /// `sync_max_step_rad` per cycle until the follower is within
    /// `sync_complete_threshold_rad` on every joint. Set to `None` for
    /// pure pass-through behaviour (legacy).
    #[serde(default)]
    pub follower_state_kind: Option<RobotStateKind>,
    #[serde(default)]
    pub follower_state_topic: Option<String>,
    /// Maximum per-cycle step (rad) while the follower is still syncing
    /// toward the leader. Defaults to 0.005 rad (~0.29°).
    #[serde(default)]
    pub sync_max_step_rad: Option<f64>,
    /// Once `max(|leader[i] - follower[i]|) <= threshold` for every joint,
    /// the router exits the syncing phase and forwards leader targets
    /// directly. Defaults to 0.05 rad (~2.86°) — large enough that an
    /// operator does not have to perfectly align the two arms before
    /// teleop kicks in, but still tight enough that the follower has
    /// visibly reached the leader's pose by the time pass-through engages.
    #[serde(default)]
    pub sync_complete_threshold_rad: Option<f64>,
    #[serde(default = "default_mapping")]
    pub mapping: MappingStrategy,
    #[serde(default)]
    pub joint_index_map: Vec<u32>,
    #[serde(default)]
    pub joint_scales: Vec<f64>,
    #[serde(default)]
    pub command_defaults: ChannelCommandDefaults,
}

/// Default maximum per-cycle joint step (rad) while syncing the follower
/// toward the leader. Conservative value chosen to avoid jerky motion at
/// 250 Hz control rates (~1.25 rad/s peak slewing speed).
pub const DEFAULT_TELEOP_SYNC_MAX_STEP_RAD: f64 = 0.005;
/// Default per-joint distance under which the syncing phase is considered
/// complete and pass-through forwarding takes over. Picked to be loose
/// enough that operators do not have to align the leader and follower
/// arms perfectly before teleop engages, while still tight enough that
/// the follower has visibly tracked the leader by the time the router
/// switches modes.
pub const DEFAULT_TELEOP_SYNC_COMPLETE_THRESHOLD_RAD: f64 = 0.05;

/// Maximum number of camera tiles rendered side-by-side in **one** preview
/// row.
///
/// Configuring more camera channels still keeps every channel in the
/// recording pipeline AND in the live preview — the visualizer subscribes
/// to all enabled cameras and the UIs render every channel — but tiles
/// wrap onto additional rows once a row already holds this many tiles.
/// This keeps each tile wide enough to honour the requested 16:10 box
/// without shrinking below the readability threshold, while no longer
/// silently hiding extra channels from the operator.
///
/// Encoded as a `u32` for trivial FFI compatibility with the C++ camera
/// drivers.
pub const MAX_PREVIEW_CAMERAS: usize = 3;

impl TeleopRuntimeConfigV2 {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.process_id.trim().is_empty() {
            return Err(ConfigError::Validation(
                "teleop runtime v2: process_id must not be empty".into(),
            ));
        }
        if self.leader_channel_id.trim().is_empty() || self.follower_channel_id.trim().is_empty() {
            return Err(ConfigError::Validation(
                "teleop runtime v2: leader_channel_id and follower_channel_id must not be empty"
                    .into(),
            ));
        }
        if self.leader_state_topic.trim().is_empty()
            || self.follower_command_topic.trim().is_empty()
        {
            return Err(ConfigError::Validation(
                "teleop runtime v2: state and command topics must not be empty".into(),
            ));
        }
        if self.joint_scales.iter().any(|scale| !scale.is_finite()) {
            return Err(ConfigError::Validation(
                "teleop runtime v2: joint_scales must be finite".into(),
            ));
        }
        Ok(())
    }
}

impl FromStr for TeleopRuntimeConfigV2 {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: TeleopRuntimeConfigV2 = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

/// Recording-role specific fields. Required when
/// `EncoderRuntimeConfigV2.role = Recording`; rejected at validation
/// otherwise.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingEncoderConfig {
    pub codec: EncoderCodec,
    #[serde(default)]
    pub backend: EncoderBackend,
    #[serde(default = "default_queue_size")]
    pub queue_size: u32,
    pub fps: u32,
    /// iceoryx2 service name for the per-camera codec config topic.
    /// Carries one `EncodedPacketHeader` with `kind = Config` at
    /// session-open and is opened with `history_size = 1` so late
    /// subscribers (e.g. an assembler restarted mid-recording) can
    /// recover the codec extradata.
    pub config_topic: String,
    /// iceoryx2 service name for the per-camera recording packet
    /// stream. Strict delivery (no overflow, publisher blocks on slow
    /// subscriber).
    pub packet_topic: String,
    #[serde(default)]
    pub chroma_subsampling: ChromaSubsampling,
    #[serde(default)]
    pub crf: Option<u8>,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub tune: Option<String>,
    #[serde(default = "default_bit_depth")]
    pub bit_depth: u8,
    #[serde(default)]
    pub color_space: EncoderColorSpace,
}

/// Preview-role specific fields. Required when
/// `EncoderRuntimeConfigV2.role = Preview`; rejected at validation
/// otherwise.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewEncoderConfig {
    pub output_mode: PreviewOutputMode,
    pub color_codec: EncoderCodec,
    pub depth_codec: EncoderCodec,
    #[serde(default)]
    pub backend: EncoderBackend,
    #[serde(default)]
    pub resize_policy: PreviewResizePolicy,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub gop_seconds: u32,
    #[serde(default)]
    pub crf: Option<u8>,
    pub jpeg_quality: i32,
    /// Service name for the per-camera codec config topic (encoded mode
    /// only). Carries `kind = Config` with `history_size = 1`.
    #[serde(default)]
    pub config_topic: Option<String>,
    /// Service name for the per-camera encoded preview packet topic
    /// (encoded mode only). Best-effort delivery; safe overflow on.
    #[serde(default)]
    pub packet_topic: Option<String>,
    /// Service name for the per-camera preview JPEG topic (jpeg mode
    /// only). Carries `CameraFrameHeader` with `pixel_format = Mjpeg`.
    #[serde(default)]
    pub jpeg_topic: Option<String>,
    /// Service name for the per-camera `PreviewControl` subscription.
    /// Always required for preview encoders (used by `set_preview_size`).
    pub control_topic: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncoderRuntimeConfigV2 {
    pub process_id: String,
    pub channel_id: String,
    pub frame_topic: String,
    /// Selects which runtime fans out below. The controller spawns one
    /// `Recording` and (optionally) one `Preview` encoder per camera
    /// channel.
    pub role: EncoderRole,
    /// Required iff `role == Recording`. Errors at validation
    /// otherwise.
    #[serde(default)]
    pub recording: Option<RecordingEncoderConfig>,
    /// Required iff `role == Preview`. Errors at validation otherwise.
    #[serde(default)]
    pub preview: Option<PreviewEncoderConfig>,
}

impl EncoderRuntimeConfigV2 {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.process_id.trim().is_empty()
            || self.channel_id.trim().is_empty()
            || self.frame_topic.trim().is_empty()
        {
            return Err(ConfigError::Validation(
                "encoder runtime v2: process_id, channel_id, and frame_topic must not be empty"
                    .into(),
            ));
        }
        match self.role {
            EncoderRole::Recording => {
                let rec = self.recording.as_ref().ok_or_else(|| {
                    ConfigError::Validation(
                        "encoder runtime v2: role=recording requires [recording] block".into(),
                    )
                })?;
                if self.preview.is_some() {
                    return Err(ConfigError::Validation(
                        "encoder runtime v2: role=recording must not include [preview] block"
                            .into(),
                    ));
                }
                rec.validate()?;
            }
            EncoderRole::Preview => {
                let prev = self.preview.as_ref().ok_or_else(|| {
                    ConfigError::Validation(
                        "encoder runtime v2: role=preview requires [preview] block".into(),
                    )
                })?;
                if self.recording.is_some() {
                    return Err(ConfigError::Validation(
                        "encoder runtime v2: role=preview must not include [recording] block"
                            .into(),
                    ));
                }
                prev.validate()?;
            }
        }
        Ok(())
    }
}

impl RecordingEncoderConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.config_topic.trim().is_empty() || self.packet_topic.trim().is_empty() {
            return Err(ConfigError::Validation(
                "encoder runtime v2: recording.config_topic and recording.packet_topic must not be empty"
                    .into(),
            ));
        }
        if self.fps == 0 || self.fps > 1000 {
            return Err(ConfigError::Validation(format!(
                "encoder runtime v2: recording.fps must be 1..1000, got {}",
                self.fps
            )));
        }
        EncoderConfig {
            video_codec: self.codec,
            depth_codec: self.codec,
            backend: self.backend,
            video_backend: self.backend,
            depth_backend: self.backend,
            chroma_subsampling: self.chroma_subsampling,
            crf: self.crf,
            preset: self.preset.clone(),
            tune: self.tune.clone(),
            bit_depth: self.bit_depth,
            color_space: self.color_space,
            queue_size: self.queue_size,
            preview: EncoderPreviewConfig::default(),
        }
        .validate()
    }
}

impl PreviewEncoderConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.control_topic.trim().is_empty() {
            return Err(ConfigError::Validation(
                "encoder runtime v2: preview.control_topic must not be empty".into(),
            ));
        }
        if self.width == 0 || self.height == 0 {
            return Err(ConfigError::Validation(
                "encoder runtime v2: preview.width and preview.height must be > 0".into(),
            ));
        }
        if self.fps == 0 || self.fps > 1000 {
            return Err(ConfigError::Validation(format!(
                "encoder runtime v2: preview.fps must be 1..1000, got {}",
                self.fps
            )));
        }
        if self.gop_seconds == 0 {
            return Err(ConfigError::Validation(
                "encoder runtime v2: preview.gop_seconds must be > 0".into(),
            ));
        }
        if !(1..=100).contains(&self.jpeg_quality) {
            return Err(ConfigError::Validation(
                "encoder runtime v2: preview.jpeg_quality must be 1..100".into(),
            ));
        }
        match self.output_mode {
            PreviewOutputMode::Encoded => {
                if self.color_codec != EncoderCodec::H264 {
                    return Err(ConfigError::Validation(format!(
                        "encoder runtime v2: preview.output_mode=encoded requires color_codec=h264, got {}",
                        self.color_codec.as_str()
                    )));
                }
                if self.depth_codec != EncoderCodec::Rvl {
                    return Err(ConfigError::Validation(format!(
                        "encoder runtime v2: preview.output_mode=encoded requires depth_codec=rvl, got {}",
                        self.depth_codec.as_str()
                    )));
                }
                if self
                    .config_topic
                    .as_deref()
                    .is_none_or(|t| t.trim().is_empty())
                    || self
                        .packet_topic
                        .as_deref()
                        .is_none_or(|t| t.trim().is_empty())
                {
                    return Err(ConfigError::Validation(
                        "encoder runtime v2: preview.output_mode=encoded requires config_topic and packet_topic"
                            .into(),
                    ));
                }
            }
            PreviewOutputMode::Jpeg => {
                if self.resize_policy == PreviewResizePolicy::FixedSource {
                    return Err(ConfigError::Validation(
                        "encoder runtime v2: preview.resize_policy=fixed-source requires output_mode=encoded"
                            .into(),
                    ));
                }
                if self
                    .jpeg_topic
                    .as_deref()
                    .is_none_or(|t| t.trim().is_empty())
                {
                    return Err(ConfigError::Validation(
                        "encoder runtime v2: preview.output_mode=jpeg requires jpeg_topic".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

impl FromStr for EncoderRuntimeConfigV2 {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: EncoderRuntimeConfigV2 = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblerCameraRuntimeConfigV2 {
    pub channel_id: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub pixel_format: PixelFormat,
    pub codec: EncoderCodec,
    /// Per-camera codec config topic the assembler subscribes to.
    /// Carries `kind = Config` packets with `history_size = 1` so
    /// late subscribers can replay codec extradata.
    pub recording_config_topic: String,
    /// Per-camera recording packet topic. Carries `kind = Packet` and
    /// terminating `kind = EndOfStream` per episode.
    pub recording_packet_topic: String,
}

impl AssemblerCameraRuntimeConfigV2 {
    pub fn container(&self) -> ContainerKind {
        container_for(self.codec)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblerObservationRuntimeConfigV2 {
    pub channel_id: String,
    pub state_kind: RobotStateKind,
    pub state_topic: String,
    pub value_len: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblerActionRuntimeConfigV2 {
    pub channel_id: String,
    pub command_kind: RobotCommandKind,
    pub command_topic: String,
    pub value_len: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblerRuntimeConfigV2 {
    pub process_id: String,
    pub format: EpisodeFormat,
    pub fps: u32,
    pub chunk_size: u32,
    /// Maximum wait, in milliseconds, after `RecordingStop` for every
    /// camera's recording packet stream to emit `EndOfStream`. Bounds
    /// how long a crashed encoder can pin an episode in the assembler
    /// before the episode is removed.
    #[serde(alias = "missing_video_timeout_ms")]
    pub missing_eos_timeout_ms: u64,
    pub staging_dir: String,
    /// See `AssemblerConfig::staging_slots`.
    #[serde(default = "default_staging_slots")]
    pub staging_slots: u32,
    #[serde(default)]
    pub cameras: Vec<AssemblerCameraRuntimeConfigV2>,
    #[serde(default)]
    pub observations: Vec<AssemblerObservationRuntimeConfigV2>,
    #[serde(default)]
    pub actions: Vec<AssemblerActionRuntimeConfigV2>,
    pub embedded_config_toml: String,
}

impl AssemblerRuntimeConfigV2 {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.process_id.trim().is_empty() || self.staging_dir.trim().is_empty() {
            return Err(ConfigError::Validation(
                "assembler runtime v2: process_id and staging_dir must not be empty".into(),
            ));
        }
        if self.fps == 0 || self.fps > 1000 || self.chunk_size == 0 {
            return Err(ConfigError::Validation(
                "assembler runtime v2: fps must be 1..1000 and chunk_size must be > 0".into(),
            ));
        }
        if self.missing_eos_timeout_ms == 0 {
            return Err(ConfigError::Validation(
                "assembler runtime v2: missing_eos_timeout_ms must be > 0".into(),
            ));
        }
        if self.staging_slots == 0 || self.staging_slots > 64 {
            return Err(ConfigError::Validation(
                "assembler runtime v2: staging_slots must be in 1..=64".into(),
            ));
        }
        if self.cameras.is_empty() {
            return Err(ConfigError::Validation(
                "assembler runtime v2: at least one camera is required".into(),
            ));
        }
        for camera in &self.cameras {
            if camera.recording_config_topic.trim().is_empty()
                || camera.recording_packet_topic.trim().is_empty()
            {
                return Err(ConfigError::Validation(format!(
                    "assembler runtime v2: camera \"{}\" requires non-empty recording_config_topic and recording_packet_topic",
                    camera.channel_id,
                )));
            }
        }
        if self.embedded_config_toml.trim().is_empty() {
            return Err(ConfigError::Validation(
                "assembler runtime v2: embedded_config_toml must not be empty".into(),
            ));
        }
        Ok(())
    }
}

impl FromStr for AssemblerRuntimeConfigV2 {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: AssemblerRuntimeConfigV2 = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectJointCompatibilityPeer {
    pub driver: String,
    pub channel_type: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectJointCompatibility {
    #[serde(default)]
    pub can_lead: Vec<DirectJointCompatibilityPeer>,
    #[serde(default)]
    pub can_follow: Vec<DirectJointCompatibilityPeer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceQueryChannel {
    pub channel_type: String,
    pub kind: DeviceType,
    pub available: bool,
    /// Display label for this channel (e.g., "AIRBOT E2", "V4L2 Camera").
    /// Falls back to the parent device's `device_label` when None.
    #[serde(default)]
    pub channel_label: Option<String>,
    /// Default user-facing name to use when this channel is first added to
    /// a project (e.g., "airbot_play_arm", "airbot_e2", "camera"). The
    /// controller stores this in `DeviceChannelConfigV2.name`. Falls back
    /// to channel_type when None.
    #[serde(default)]
    pub default_name: Option<String>,
    #[serde(default)]
    pub modes: Vec<String>,
    #[serde(default)]
    pub profiles: Vec<CameraChannelProfile>,
    #[serde(default)]
    pub supported_states: Vec<RobotStateKind>,
    #[serde(default)]
    pub supported_commands: Vec<RobotCommandKind>,
    #[serde(default)]
    pub supports_fk: bool,
    #[serde(default)]
    pub supports_ik: bool,
    pub dof: Option<u32>,
    pub default_control_frequency_hz: Option<f64>,
    #[serde(default)]
    pub direct_joint_compatibility: DirectJointCompatibility,
    #[serde(default)]
    pub defaults: ChannelCommandDefaults,
    /// Per-state value limits reported by the driver. The controller
    /// persists these on the channel config so the visualizer can render
    /// limit-aware bars and (later) the safety layer can clip targets.
    #[serde(default)]
    pub value_limits: Vec<StateValueLimitsEntry>,
    /// Sensor sample kinds this channel can publish. Only populated for
    /// `kind = "sensor"` driver entries.
    #[serde(default)]
    pub supported_sensor_kinds: Vec<SensorStateKind>,
    /// Driver-suggested sample period for sensor channels (`None` for
    /// camera/robot). Only populated for `kind = "sensor"`.
    #[serde(default)]
    pub default_sample_rate_hz: Option<f64>,
    /// Per-kind shape hints reported by the driver (`[N, 6]` for a
    /// tactile cloud, etc.). Only populated for `kind = "sensor"`.
    #[serde(default)]
    pub sensor_shape_hints: std::collections::BTreeMap<SensorStateKind, Vec<u32>>,
    #[serde(default)]
    pub optional_info: toml::Table,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceQueryDevice {
    pub id: String,
    pub device_class: String,
    pub device_label: String,
    /// Default user-facing name for the *device* row when the wizard collapses
    /// multiple channels into a single device entry (e.g. "airbot_play",
    /// "realsense", "agx_nero"). Falls back to `driver.replace('-', '_')`
    /// when absent.
    #[serde(default)]
    pub default_device_name: Option<String>,
    #[serde(default)]
    pub optional_info: toml::Table,
    #[serde(default)]
    pub channels: Vec<DeviceQueryChannel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceQueryResponse {
    pub driver: String,
    #[serde(default)]
    pub devices: Vec<DeviceQueryDevice>,
}

#[derive(Debug, Clone)]
pub struct ResolvedCameraChannel {
    pub channel_id: String,
    pub device_name: String,
    pub bus_root: String,
    pub channel_type: String,
    pub frame_topic: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub pixel_format: PixelFormat,
}

#[derive(Debug, Clone)]
pub struct ResolvedRobotChannel {
    pub channel_id: String,
    pub device_name: String,
    pub driver: String,
    pub bus_root: String,
    pub channel_type: String,
    pub dof: u32,
    pub state_topics: Vec<(RobotStateKind, String)>,
    pub recorded_states: Vec<RobotStateKind>,
    pub control_frequency_hz: f64,
    pub command_defaults: ChannelCommandDefaults,
    pub value_limits: Vec<StateValueLimitsEntry>,
}

fn preview_resize_policy(
    pixel_format: PixelFormat,
    preview: &EncoderPreviewConfig,
) -> PreviewResizePolicy {
    if pixel_format == PixelFormat::H264AnnexB
        && preview.output_mode == PreviewOutputMode::Encoded
        && preview.color_codec == EncoderCodec::H264
    {
        PreviewResizePolicy::FixedSource
    } else {
        PreviewResizePolicy::Dynamic
    }
}

impl ProjectConfig {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    /// Empty project shell used by interactive setup before devices are finalized.
    pub fn draft_setup_template() -> Self {
        Self {
            project_name: default_project_name(),
            mode: CollectionMode::Intervention,
            episode: EpisodeConfig::default(),
            devices: Vec::new(),
            pairings: Vec::new(),
            assembler: AssemblerConfig::default(),
            storage: StorageConfig::default(),
            monitor: MonitorConfig::default(),
            controller: ControllerConfig::default(),
            visualizer: VisualizerConfig { port: 19090 },
            ui: UiRuntimeConfig::default(),
            runtime: RuntimeConfig::default(),
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.project_name.trim().is_empty() {
            return Err(ConfigError::Validation(
                "project_name must not be empty".into(),
            ));
        }
        self.episode.validate()?;
        if self.devices.is_empty() {
            return Err(ConfigError::Validation(
                "at least one [[devices]] entry is required".into(),
            ));
        }
        let mut names = HashSet::new();
        let mut bus_roots = HashSet::new();
        for device in &self.devices {
            if !names.insert(device.name.as_str()) {
                return Err(ConfigError::Validation(format!(
                    "duplicate device name: \"{}\"",
                    device.name
                )));
            }
            if !bus_roots.insert(device.bus_root.as_str()) {
                return Err(ConfigError::Validation(format!(
                    "duplicate bus_root: \"{}\"",
                    device.bus_root
                )));
            }
            device.validate()?;
        }
        for pairing in &self.pairings {
            pairing.validate(self)?;
        }
        // Both modes accept any pairing count: teleop is the only mode the
        // wizard exposes now, and the operator may save a teleop project
        // before assembling pairings (e.g. while iterating on device
        // selection in step 1). Downstream consumers (`teleop_runtime_configs_v2`)
        // simply emit zero teleop runtimes when no pairings exist; intervention
        // configs still tolerate stray pairings without taking action.
        let _ = self.mode;
        self.validate_per_channel_encoder_configs()?;
        self.validate_preview_compatibility()?;
        self.assembler.validate()?;
        self.storage.validate()?;
        self.monitor.validate()?;
        self.controller.validate()?;
        self.visualizer.validate()?;
        self.ui.validate()?;
        Ok(())
    }

    /// Validate per-channel encoder configs by resolving each one and
    /// delegating to `EncoderConfig::validate()`.
    fn validate_per_channel_encoder_configs(&self) -> Result<(), ConfigError> {
        for device in &self.devices {
            for channel in &device.channels {
                if channel.kind != DeviceType::Camera || !channel.enabled {
                    continue;
                }
                let resolved = channel
                    .record
                    .as_ref()
                    .map(|r| r.resolve())
                    .unwrap_or_default();
                resolved.validate().map_err(|e| {
                    ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": record config: {}",
                        device.name, channel.channel_type, e
                    ))
                })?;
            }
        }
        Ok(())
    }

    fn validate_preview_compatibility(&self) -> Result<(), ConfigError> {
        for device in &self.devices {
            for channel in &device.channels {
                if channel.kind != DeviceType::Camera
                    || !channel.enabled
                    || !channel.preview_enabled
                {
                    continue;
                }
                let preview_cfg = channel
                    .preview_settings
                    .as_ref()
                    .map(|p| p.resolve())
                    .unwrap_or_default();
                if preview_cfg.output_mode == PreviewOutputMode::Encoded {
                    continue;
                }
                if channel
                    .profile
                    .as_ref()
                    .is_some_and(|profile| profile.pixel_format == PixelFormat::H264AnnexB)
                {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": h264-annex-b previews require preview_config.output_mode = \"encoded\"",
                        device.name, channel.channel_type
                    )));
                }
            }
        }

        Ok(())
    }

    pub fn device_named(&self, name: &str) -> Option<&BinaryDeviceConfig> {
        self.devices.iter().find(|device| device.name == name)
    }

    pub fn resolved_camera_channels(&self) -> Vec<ResolvedCameraChannel> {
        let mut channels = Vec::new();
        for device in &self.devices {
            for channel in &device.channels {
                if channel.kind != DeviceType::Camera || !channel.enabled {
                    continue;
                }
                let Some(profile) = channel.profile.as_ref() else {
                    continue;
                };
                let channel_id = device_channel_id(&device.name, &channel.channel_type);
                channels.push(ResolvedCameraChannel {
                    channel_id,
                    device_name: device.name.clone(),
                    bus_root: device.bus_root.clone(),
                    channel_type: channel.channel_type.clone(),
                    frame_topic: camera_frames_topic_v2(&device.bus_root, &channel.channel_type),
                    width: profile.width,
                    height: profile.height,
                    fps: profile.fps,
                    pixel_format: profile.pixel_format,
                });
            }
        }
        channels
    }

    pub fn resolved_robot_channels(&self) -> Vec<ResolvedRobotChannel> {
        let mut channels = Vec::new();
        for device in &self.devices {
            for channel in &device.channels {
                if channel.kind != DeviceType::Robot || !channel.enabled {
                    continue;
                }
                let dof = channel.dof.unwrap_or_default();
                let recorded_states = if channel.recorded_states.is_empty() {
                    channel.publish_states.clone()
                } else {
                    channel.recorded_states.clone()
                };
                let state_topics = channel
                    .publish_states
                    .iter()
                    .copied()
                    .map(|state| {
                        (
                            state,
                            robot_state_topic_v2(&device.bus_root, &channel.channel_type, state),
                        )
                    })
                    .collect::<Vec<_>>();
                channels.push(ResolvedRobotChannel {
                    channel_id: device_channel_id(&device.name, &channel.channel_type),
                    device_name: device.name.clone(),
                    driver: device.driver.clone(),
                    bus_root: device.bus_root.clone(),
                    channel_type: channel.channel_type.clone(),
                    dof,
                    state_topics,
                    recorded_states,
                    control_frequency_hz: channel.control_frequency_hz.unwrap_or(60.0),
                    command_defaults: channel.command_defaults.clone(),
                    value_limits: channel.value_limits.clone(),
                });
            }
        }
        channels
    }

    pub fn ui_runtime_config(&self) -> UiRuntimeConfig {
        let mut config = self.ui.clone();
        if config.preview_websocket_url.is_none() {
            config.preview_websocket_url = Some(format!("ws://127.0.0.1:{}", self.visualizer.port));
        }
        config
    }

    pub fn visualizer_runtime_config_v2(&self) -> VisualizerRuntimeConfigV2 {
        // Subscribe only to cameras that opt into a live preview
        // (`preview_enabled = true`, default). The visualizer derives
        // the per-camera iceoryx2 topic name from `bus_root` +
        // `channel_type` plus the per-channel preview output_mode.
        let mut first_preview_output_mode = PreviewOutputMode::default();
        let camera_sources = self
            .resolved_camera_channels()
            .into_iter()
            .filter(|camera| {
                self.device_named(&camera.device_name)
                    .and_then(|device| {
                        device
                            .channels
                            .iter()
                            .find(|c| c.channel_type == camera.channel_type)
                    })
                    .is_some_and(|channel| channel.preview_enabled)
            })
            .map(|camera| {
                let channel_cfg = self.device_named(&camera.device_name).and_then(|device| {
                    device
                        .channels
                        .iter()
                        .find(|c| c.channel_type == camera.channel_type)
                });
                let preview_cfg = channel_cfg
                    .and_then(|ch| ch.preview_settings.as_ref())
                    .map(|p| p.resolve())
                    .unwrap_or_default();
                first_preview_output_mode = preview_cfg.output_mode;
                let preview_resize_policy =
                    preview_resize_policy(camera.pixel_format, &preview_cfg);
                VisualizerCameraSourceConfig {
                    channel_id: camera.channel_id,
                    bus_root: camera.bus_root,
                    channel_type: camera.channel_type,
                    preview_resize_policy,
                    source_width: Some(camera.width),
                    source_height: Some(camera.height),
                }
            })
            .collect();
        let robot_sources = self
            .resolved_robot_channels()
            .into_iter()
            .flat_map(|robot| {
                let channel_id = robot.channel_id.clone();
                let value_limits = robot.value_limits.clone();
                robot
                    .state_topics
                    .into_iter()
                    .map(move |(state_kind, state_topic)| {
                        let entry = value_limits
                            .iter()
                            .find(|entry| entry.state_kind == state_kind);
                        VisualizerRobotSourceConfig {
                            channel_id: channel_id.clone(),
                            state_kind,
                            state_topic,
                            value_min: entry.map(|e| e.min.clone()).unwrap_or_default(),
                            value_max: entry.map(|e| e.max.clone()).unwrap_or_default(),
                        }
                    })
            })
            .collect();
        VisualizerRuntimeConfigV2 {
            port: self.visualizer.port,
            camera_sources,
            robot_sources,
            preview_output_mode: first_preview_output_mode,
        }
    }

    pub fn encoder_runtime_configs_v2(&self) -> Vec<EncoderRuntimeConfigV2> {
        let mut configs = Vec::new();
        for camera in self.resolved_camera_channels() {
            let channel_cfg = self.device_named(&camera.device_name).and_then(|device| {
                device
                    .channels
                    .iter()
                    .find(|c| c.channel_type == camera.channel_type)
            });
            let record_cfg = channel_cfg
                .and_then(|ch| ch.record.as_ref())
                .map(|r| r.resolve())
                .unwrap_or_default();
            let codec = record_cfg.codec_for_pixel_format(camera.pixel_format);
            let backend = record_cfg.backend_for_pixel_format(camera.pixel_format);
            let preview_enabled = channel_cfg.is_some_and(|channel| channel.preview_enabled);
            let record_enabled = channel_cfg.is_some_and(|channel| channel.record_enabled);

            // Recording-role encoder for every camera with record_enabled.
            if record_enabled {
                configs.push(EncoderRuntimeConfigV2 {
                    process_id: recording_encoder_process_id(&camera.channel_id),
                    channel_id: camera.channel_id.clone(),
                    frame_topic: camera.frame_topic.clone(),
                    role: EncoderRole::Recording,
                    recording: Some(RecordingEncoderConfig {
                        codec,
                        backend,
                        queue_size: record_cfg.queue_size,
                        fps: camera.fps,
                        config_topic: rollio_bus::recording_config_service_name(
                            &camera.bus_root,
                            &camera.channel_type,
                        ),
                        packet_topic: rollio_bus::recording_packet_service_name(
                            &camera.bus_root,
                            &camera.channel_type,
                        ),
                        chroma_subsampling: record_cfg.chroma_subsampling,
                        crf: record_cfg.crf,
                        preset: record_cfg.preset.clone(),
                        tune: record_cfg.tune.clone(),
                        bit_depth: record_cfg.bit_depth,
                        color_space: record_cfg.color_space,
                    }),
                    preview: None,
                });
            }

            // Preview-role encoder, if the channel opts in.
            if preview_enabled {
                let preview_cfg = channel_cfg
                    .and_then(|ch| ch.preview_settings.as_ref())
                    .map(|p| p.resolve())
                    .unwrap_or_default();
                let (preview_codec_color, preview_codec_depth) =
                    (preview_cfg.color_codec, preview_cfg.depth_codec);
                let resize_policy = preview_resize_policy(camera.pixel_format, &preview_cfg);
                let (preview_width, preview_height) = match resize_policy {
                    PreviewResizePolicy::Dynamic => (preview_cfg.width, preview_cfg.height),
                    PreviewResizePolicy::FixedSource => (camera.width, camera.height),
                };
                let preview_backend = match resize_policy {
                    PreviewResizePolicy::Dynamic => preview_cfg.backend,
                    PreviewResizePolicy::FixedSource => EncoderBackend::Passthrough,
                };
                let depth_channel = camera.pixel_format == PixelFormat::Depth16;
                let _effective_codec = if depth_channel {
                    preview_codec_depth
                } else {
                    preview_codec_color
                };

                let (config_topic, packet_topic, jpeg_topic) = match preview_cfg.output_mode {
                    PreviewOutputMode::Encoded => (
                        Some(rollio_bus::preview_config_service_name(
                            &camera.bus_root,
                            &camera.channel_type,
                        )),
                        Some(rollio_bus::preview_packet_service_name(
                            &camera.bus_root,
                            &camera.channel_type,
                        )),
                        None,
                    ),
                    PreviewOutputMode::Jpeg => (
                        None,
                        None,
                        Some(rollio_bus::preview_jpeg_service_name(
                            &camera.bus_root,
                            &camera.channel_type,
                        )),
                    ),
                };

                configs.push(EncoderRuntimeConfigV2 {
                    process_id: preview_encoder_process_id(&camera.channel_id),
                    channel_id: camera.channel_id.clone(),
                    frame_topic: camera.frame_topic,
                    role: EncoderRole::Preview,
                    recording: None,
                    preview: Some(PreviewEncoderConfig {
                        output_mode: preview_cfg.output_mode,
                        color_codec: preview_codec_color,
                        depth_codec: preview_codec_depth,
                        backend: preview_backend,
                        resize_policy,
                        width: preview_width,
                        height: preview_height,
                        fps: preview_cfg.fps,
                        gop_seconds: preview_cfg.gop_seconds,
                        crf: preview_cfg.crf,
                        jpeg_quality: preview_cfg.jpeg_quality,
                        config_topic,
                        packet_topic,
                        jpeg_topic,
                        control_topic: rollio_bus::preview_control_service_name(
                            &camera.bus_root,
                            &camera.channel_type,
                        ),
                    }),
                });
            }
        }
        configs
    }

    pub fn assembler_runtime_config_v2(
        &self,
        embedded_config_toml: String,
    ) -> AssemblerRuntimeConfigV2 {
        let cameras = self
            .resolved_camera_channels()
            .into_iter()
            .filter(|camera| {
                // Only include cameras with record_enabled
                self.device_named(&camera.device_name)
                    .and_then(|device| {
                        device
                            .channels
                            .iter()
                            .find(|c| c.channel_type == camera.channel_type)
                    })
                    .is_some_and(|channel| channel.record_enabled)
            })
            .map(|camera| {
                let channel_cfg = self.device_named(&camera.device_name).and_then(|device| {
                    device
                        .channels
                        .iter()
                        .find(|c| c.channel_type == camera.channel_type)
                });
                let record_cfg = channel_cfg
                    .and_then(|ch| ch.record.as_ref())
                    .map(|r| r.resolve())
                    .unwrap_or_default();
                let codec = record_cfg.codec_for_pixel_format(camera.pixel_format);
                AssemblerCameraRuntimeConfigV2 {
                    channel_id: camera.channel_id.clone(),
                    width: camera.width,
                    height: camera.height,
                    fps: camera.fps,
                    pixel_format: camera.pixel_format,
                    codec,
                    recording_config_topic: rollio_bus::recording_config_service_name(
                        &camera.bus_root,
                        &camera.channel_type,
                    ),
                    recording_packet_topic: rollio_bus::recording_packet_service_name(
                        &camera.bus_root,
                        &camera.channel_type,
                    ),
                }
            })
            .collect();
        let observations = self
            .resolved_robot_channels()
            .into_iter()
            .flat_map(|robot| {
                let recorded = robot.recorded_states.clone();
                robot
                    .state_topics
                    .into_iter()
                    .filter(move |(state_kind, _)| recorded.contains(state_kind))
                    .map(
                        move |(state_kind, state_topic)| AssemblerObservationRuntimeConfigV2 {
                            channel_id: robot.channel_id.clone(),
                            state_kind,
                            state_topic,
                            value_len: state_kind.value_len(robot.dof),
                        },
                    )
            })
            .collect();
        let actions = self
            .pairings
            .iter()
            .filter_map(|pairing| {
                let follower = self
                    .device_named(&pairing.follower_device)?
                    .channel_named(&pairing.follower_channel_type)?;
                let dof = follower.dof.unwrap_or_default();
                let bus_root = &self.device_named(&pairing.follower_device)?.bus_root;
                Some(AssemblerActionRuntimeConfigV2 {
                    channel_id: device_channel_id(
                        &pairing.follower_device,
                        &pairing.follower_channel_type,
                    ),
                    command_kind: pairing.follower_command,
                    command_topic: robot_command_topic_v2(
                        bus_root,
                        &pairing.follower_channel_type,
                        pairing.follower_command,
                    ),
                    value_len: match pairing.follower_command {
                        RobotCommandKind::JointPosition | RobotCommandKind::JointMit => dof,
                        RobotCommandKind::ParallelPosition | RobotCommandKind::ParallelMit => {
                            dof.min(MAX_PARALLEL as u32)
                        }
                        RobotCommandKind::EndPose => 7,
                    },
                })
            })
            .collect();
        AssemblerRuntimeConfigV2 {
            process_id: assembler_process_id_for_format(self.episode.format),
            format: self.episode.format,
            fps: self.episode.fps,
            chunk_size: self.episode.chunk_size,
            missing_eos_timeout_ms: self.assembler.missing_eos_timeout_ms,
            staging_dir: episode_staging_root_v2(&self.assembler.staging_dir),
            staging_slots: self.assembler.staging_slots,
            cameras,
            observations,
            actions,
            embedded_config_toml,
        }
    }

    pub fn teleop_runtime_configs_v2(&self) -> Vec<TeleopRuntimeConfigV2> {
        self.pairings
            .iter()
            .filter_map(|pairing| {
                let leader_device = self.device_named(&pairing.leader_device)?;
                let follower_device = self.device_named(&pairing.follower_device)?;
                let follower_channel =
                    follower_device.channel_named(&pairing.follower_channel_type)?;
                // Pick the follower state-kind that matches the command kind so
                // the router can compute joint-space deltas during the initial
                // sync phase (rate-limited steps until follower catches up).
                let follower_state_kind = match pairing.follower_command {
                    RobotCommandKind::JointPosition | RobotCommandKind::JointMit => {
                        Some(RobotStateKind::JointPosition)
                    }
                    RobotCommandKind::ParallelPosition | RobotCommandKind::ParallelMit => {
                        Some(RobotStateKind::ParallelPosition)
                    }
                    RobotCommandKind::EndPose => Some(RobotStateKind::EndEffectorPose),
                };
                let follower_state_topic = follower_state_kind.and_then(|kind| {
                    if follower_channel.publish_states.contains(&kind) {
                        Some(robot_state_topic_v2(
                            &follower_device.bus_root,
                            &pairing.follower_channel_type,
                            kind,
                        ))
                    } else {
                        None
                    }
                });
                Some(TeleopRuntimeConfigV2 {
                    process_id: format!(
                        "teleop.{}.{}.to.{}.{}",
                        pairing.leader_device,
                        pairing.leader_channel_type,
                        pairing.follower_device,
                        pairing.follower_channel_type
                    ),
                    leader_channel_id: device_channel_id(
                        &pairing.leader_device,
                        &pairing.leader_channel_type,
                    ),
                    follower_channel_id: device_channel_id(
                        &pairing.follower_device,
                        &pairing.follower_channel_type,
                    ),
                    leader_state_kind: pairing.leader_state,
                    leader_state_topic: robot_state_topic_v2(
                        &leader_device.bus_root,
                        &pairing.leader_channel_type,
                        pairing.leader_state,
                    ),
                    follower_command_kind: pairing.follower_command,
                    follower_command_topic: robot_command_topic_v2(
                        &follower_device.bus_root,
                        &pairing.follower_channel_type,
                        pairing.follower_command,
                    ),
                    follower_state_kind: follower_state_topic.is_some().then_some(
                        follower_state_kind.expect("follower_state_kind set when topic resolved"),
                    ),
                    follower_state_topic,
                    sync_max_step_rad: Some(DEFAULT_TELEOP_SYNC_MAX_STEP_RAD),
                    sync_complete_threshold_rad: Some(DEFAULT_TELEOP_SYNC_COMPLETE_THRESHOLD_RAD),
                    mapping: pairing.mapping,
                    joint_index_map: pairing.joint_index_map.clone(),
                    joint_scales: pairing.joint_scales.clone(),
                    command_defaults: follower_channel.command_defaults.clone(),
                })
            })
            .collect()
    }

    pub fn storage_runtime_config(&self) -> StorageRuntimeConfig {
        StorageRuntimeConfig {
            process_id: storage_process_id_for(self.episode.format, self.storage.backend).into(),
            backend: self.storage.backend,
            output_path: self.storage.output_path.clone(),
            endpoint: self.storage.endpoint.clone(),
            queue_size: self.storage.queue_size,
        }
    }
}

/// Stable identifier — matches the binary name spawned by the controller —
/// for the storage child chosen by `(format, backend)`. Used as
/// `process_id` in `StorageRuntimeConfig`, `BackpressureEvent`, log lines.
pub fn storage_process_id_for(format: EpisodeFormat, backend: StorageBackend) -> &'static str {
    match (format, backend) {
        (EpisodeFormat::LeRobotV2_1 | EpisodeFormat::LeRobotV3_0, StorageBackend::Local) => {
            "storage-local-lerobot"
        }
        (EpisodeFormat::Mcap, StorageBackend::Local) => "storage-local",
        (_, StorageBackend::Http) => "storage-http",
    }
}

impl FromStr for ProjectConfig {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: ProjectConfig = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

fn device_channel_id(device_name: &str, channel_type: &str) -> String {
    format!("{device_name}/{channel_type}")
}

fn channel_prefix_v2(bus_root: &str, channel_type: &str) -> String {
    format!("{bus_root}/{channel_type}")
}

fn camera_frames_topic_v2(bus_root: &str, channel_type: &str) -> String {
    format!("{}/frames", channel_prefix_v2(bus_root, channel_type))
}

fn robot_state_topic_v2(bus_root: &str, channel_type: &str, state: RobotStateKind) -> String {
    format!(
        "{}/states/{}",
        channel_prefix_v2(bus_root, channel_type),
        state.topic_suffix()
    )
}

fn robot_command_topic_v2(bus_root: &str, channel_type: &str, command: RobotCommandKind) -> String {
    format!(
        "{}/commands/{}",
        channel_prefix_v2(bus_root, channel_type),
        command.topic_suffix()
    )
}

/// Encoder process_id for the recording-role child the controller
/// spawns per camera. Distinct from the preview-role process_id so
/// the two children appear as separate iceoryx2 nodes.
pub fn recording_encoder_process_id(channel_id: &str) -> String {
    format!("encoder.{}", channel_id.replace('/', "."))
}

/// Encoder process_id for the preview-role child.
pub fn preview_encoder_process_id(channel_id: &str) -> String {
    format!("preview-encoder.{}", channel_id.replace('/', "."))
}

/// Pick the assembler process_id (matches the eventual binary name)
/// for the project's chosen episode format. The controller uses this
/// to spawn the right sibling crate (`rollio-episode-lerobot` /
/// `rollio-episode-mcap`).
pub fn assembler_process_id_for_format(format: EpisodeFormat) -> String {
    match format {
        EpisodeFormat::LeRobotV2_1 => "episode-lerobot".into(),
        EpisodeFormat::LeRobotV3_0 => "episode-lerobot".into(),
        EpisodeFormat::Mcap => "episode-mcap".into(),
    }
}

fn episode_staging_root_v2(staging_root: &str) -> String {
    Path::new(staging_root)
        .join("episodes")
        .to_string_lossy()
        .into_owned()
}
