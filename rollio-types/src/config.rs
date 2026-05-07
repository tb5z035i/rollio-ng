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
    /// Sensor channel carrying inertial data (orientation, angular velocity,
    /// linear acceleration). Backed by the `Imu` POD message in
    /// [`crate::messages`] and published on `{bus_root}/{channel_type}/states/imu`.
    /// Used by bridge devices like `rollio-device-umi` that ingest IMU streams
    /// from external middlewares (FastDDS, ROS2) and republish them as
    /// first-class rollio channels.
    Imu,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum EncodedHandoffMode {
    #[default]
    File,
    Iceoryx2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblerConfig {
    #[serde(default = "default_missing_video_timeout_ms")]
    pub missing_video_timeout_ms: u64,
    #[serde(default = "default_staging_dir")]
    pub staging_dir: String,
    #[serde(default)]
    pub encoded_handoff: EncodedHandoffMode,
}

fn default_missing_video_timeout_ms() -> u64 {
    5_000
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
            missing_video_timeout_ms: default_missing_video_timeout_ms(),
            staging_dir: default_staging_dir(),
            encoded_handoff: EncodedHandoffMode::File,
        }
    }
}

impl AssemblerConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.missing_video_timeout_ms == 0 {
            return Err(ConfigError::Validation(
                "assembler: missing_video_timeout_ms must be > 0".into(),
            ));
        }
        if self.staging_dir.trim().is_empty() {
            return Err(ConfigError::Validation(
                "assembler: staging_dir must not be empty".into(),
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
    Rvl,
}

impl EncoderCodec {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::H265 => "h265",
            Self::Av1 => "av1",
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
    /// Pseudo-backend: the input frames already carry compressed bytes
    /// (currently only H264 from the UMI bridge), so the encoder mux'es
    /// them straight into MP4 without invoking any encoder. Reported in
    /// telemetry to make it observable that no real encode happened.
    Passthrough,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum EncoderArtifactFormat {
    #[default]
    Auto,
    Mp4,
    Mkv,
    Rvl,
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

impl EncoderArtifactFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Auto => "bin",
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::Rvl => "rvl",
        }
    }
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
    pub artifact_formats: Vec<EncoderArtifactFormat>,
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
    #[serde(default)]
    pub artifact_format: EncoderArtifactFormat,
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
    artifact_format: EncoderArtifactFormat,
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
            artifact_format: value.artifact_format,
            chroma_subsampling: value.chroma_subsampling,
            crf: value.crf,
            preset: value.preset,
            tune: value.tune,
            bit_depth: value.bit_depth,
            color_space: value.color_space,
            queue_size: value.queue_size,
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
            artifact_format: EncoderArtifactFormat::default(),
            chroma_subsampling: ChromaSubsampling::default(),
            crf: None,
            preset: None,
            tune: None,
            bit_depth: default_bit_depth(),
            color_space: EncoderColorSpace::default(),
            queue_size: default_queue_size(),
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

    pub fn resolved_artifact_format(&self) -> EncoderArtifactFormat {
        self.resolved_artifact_format_for(self.video_codec)
    }

    pub fn resolved_depth_artifact_format(&self) -> EncoderArtifactFormat {
        self.resolved_artifact_format_for(self.depth_codec)
    }

    pub fn resolved_artifact_format_for(&self, codec: EncoderCodec) -> EncoderArtifactFormat {
        if self.artifact_format != EncoderArtifactFormat::Auto {
            return self.artifact_format;
        }

        match codec {
            EncoderCodec::H264 | EncoderCodec::H265 => EncoderArtifactFormat::Mp4,
            EncoderCodec::Av1 => EncoderArtifactFormat::Mkv,
            EncoderCodec::Rvl => EncoderArtifactFormat::Rvl,
        }
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
        self.validate_codec("video_codec", self.video_codec, self.video_backend)?;
        self.validate_codec("depth_codec", self.depth_codec, self.depth_backend)?;
        Ok(())
    }

    fn validate_codec(
        &self,
        field_name: &str,
        codec: EncoderCodec,
        backend: EncoderBackend,
    ) -> Result<(), ConfigError> {
        if codec == EncoderCodec::Rvl && backend == EncoderBackend::Vaapi {
            return Err(ConfigError::Validation(format!(
                "encoder: {field_name}=rvl only supports cpu or auto backends"
            )));
        }
        if codec == EncoderCodec::Rvl && backend == EncoderBackend::Nvidia {
            return Err(ConfigError::Validation(format!(
                "encoder: {field_name}=rvl only supports cpu or auto backends"
            )));
        }

        match (codec, self.resolved_artifact_format_for(codec)) {
            (EncoderCodec::Rvl, EncoderArtifactFormat::Rvl)
            | (EncoderCodec::H264, EncoderArtifactFormat::Mp4)
            | (EncoderCodec::H265, EncoderArtifactFormat::Mp4)
            | (EncoderCodec::Av1, EncoderArtifactFormat::Mkv) => Ok(()),
            (EncoderCodec::Rvl, other) => Err(ConfigError::Validation(format!(
                "encoder: {field_name}=rvl requires artifact_format=rvl, got {:?}",
                other
            ))),
            (_, EncoderArtifactFormat::Rvl) => Err(ConfigError::Validation(format!(
                "encoder: artifact_format=rvl requires {field_name}=rvl"
            ))),
            (codec, other) => Err(ConfigError::Validation(format!(
                "encoder: {field_name}={} does not support artifact_format {:?}",
                codec.as_str(),
                other
            ))),
        }
    }
}

fn default_depth_codec() -> EncoderCodec {
    EncoderCodec::Rvl
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
    #[serde(default = "default_max_preview_width")]
    pub max_preview_width: u32,
    #[serde(default = "default_max_preview_height")]
    pub max_preview_height: u32,
    #[serde(default = "default_jpeg_quality")]
    pub jpeg_quality: i32,
    #[serde(default = "default_preview_fps")]
    pub preview_fps: u32,
    #[serde(default)]
    pub preview_workers: Option<usize>,
}

fn default_visualizer_port() -> u16 {
    19090
}

fn default_max_preview_width() -> u32 {
    320
}

fn default_max_preview_height() -> u32 {
    240
}

fn default_jpeg_quality() -> i32 {
    30
}

fn default_preview_fps() -> u32 {
    60
}

impl Default for VisualizerRuntimeConfig {
    fn default() -> Self {
        Self {
            port: default_visualizer_port(),
            cameras: Vec::new(),
            robots: Vec::new(),
            max_preview_width: default_max_preview_width(),
            max_preview_height: default_max_preview_height(),
            jpeg_quality: default_jpeg_quality(),
            preview_fps: default_preview_fps(),
            preview_workers: None,
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
        if self.max_preview_width == 0 || self.max_preview_height == 0 {
            return Err(ConfigError::Validation(
                "visualizer: preview dimensions must be > 0".into(),
            ));
        }
        if !(1..=100).contains(&self.jpeg_quality) {
            return Err(ConfigError::Validation(
                "visualizer: jpeg_quality must be 1..100".into(),
            ));
        }
        if self.preview_fps > 1000 {
            return Err(ConfigError::Validation(
                "visualizer: preview_fps must be <= 1000".into(),
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
    /// passing into the UI server.
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
    // `0.0.0.0` so the UI server is reachable from every interface by
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
    pub encoder: EncoderConfig,
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
    encoder: EncoderConfig,
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
            encoder: value.encoder,
            assembler: value.assembler,
            storage: value.storage,
            monitor: value.monitor,
            controller: value.controller,
            visualizer: value.visualizer,
            ui: value.ui,
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
            DeviceType::Imu => {
                if let Some(profile) = &self.profile {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": imu channels do not accept profile ({:?})",
                        device.name, self.channel_type, profile
                    )));
                }
                if self.dof.is_some() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": imu channels do not accept dof",
                        device.name, self.channel_type
                    )));
                }
                if self.mode.is_some() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": imu channels do not accept mode",
                        device.name, self.channel_type
                    )));
                }
                if !self.publish_states.is_empty() || !self.recorded_states.is_empty() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": imu channels do not accept publish_states or recorded_states (the IMU service is implicit)",
                        device.name, self.channel_type
                    )));
                }
                if self.control_frequency_hz.is_some() {
                    return Err(ConfigError::Validation(format!(
                        "device \"{}\" channel \"{}\": imu channels do not accept control_frequency_hz",
                        device.name, self.channel_type
                    )));
                }
            }
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
    #[serde(default = "default_max_preview_width")]
    pub max_preview_width: u32,
    #[serde(default = "default_max_preview_height")]
    pub max_preview_height: u32,
    #[serde(default = "default_jpeg_quality")]
    pub jpeg_quality: i32,
    #[serde(default = "default_preview_fps")]
    pub preview_fps: u32,
    #[serde(default)]
    pub preview_workers: Option<usize>,
}

impl VisualizerConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        VisualizerRuntimeConfig {
            port: self.port,
            cameras: Vec::new(),
            robots: Vec::new(),
            max_preview_width: self.max_preview_width,
            max_preview_height: self.max_preview_height,
            jpeg_quality: self.jpeg_quality,
            preview_fps: self.preview_fps,
            preview_workers: self.preview_workers,
        }
        .validate()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizerCameraSourceConfig {
    pub channel_id: String,
    /// Topic the visualizer subscribes to for this camera. As of the
    /// "native camera pixel formats" change this is the encoder's
    /// downsized RGB24 preview tap (`channel_preview_service_name`),
    /// not the raw camera frames topic. The encoder always runs (even
    /// when not recording), so the preview tap is always populated as
    /// long as the camera driver is publishing.
    pub preview_topic: String,
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

/// Visualizer subscriber descriptor for an IMU channel. The visualizer
/// currently only plumbs the subscription so IMU data is observable via
/// bus-tap; rendering is a follow-up.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizerImuSourceConfig {
    pub channel_id: String,
    pub state_topic: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizerRuntimeConfigV2 {
    pub port: u16,
    #[serde(default)]
    pub camera_sources: Vec<VisualizerCameraSourceConfig>,
    #[serde(default)]
    pub robot_sources: Vec<VisualizerRobotSourceConfig>,
    #[serde(default)]
    pub imu_sources: Vec<VisualizerImuSourceConfig>,
    pub max_preview_width: u32,
    pub max_preview_height: u32,
    pub jpeg_quality: i32,
    pub preview_fps: u32,
    #[serde(default)]
    pub preview_workers: Option<usize>,
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
            max_preview_width: self.max_preview_width,
            max_preview_height: self.max_preview_height,
            jpeg_quality: self.jpeg_quality,
            preview_fps: self.preview_fps,
            preview_workers: self.preview_workers,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncoderRuntimeConfigV2 {
    pub process_id: String,
    pub channel_id: String,
    pub frame_topic: String,
    pub output_dir: String,
    pub codec: EncoderCodec,
    #[serde(default)]
    pub backend: EncoderBackend,
    #[serde(default)]
    pub artifact_format: EncoderArtifactFormat,
    #[serde(default = "default_queue_size")]
    pub queue_size: u32,
    pub fps: u32,
    /// Identifies the bus topic family used for the always-on RGB24 preview
    /// tap that the visualizer subscribes to. The encoder reads from
    /// `frame_topic` (the raw camera feed) and republishes a downsized RGB24
    /// frame on `preview_topic`. Both topics share the same `bus_root` /
    /// `channel_type` pair; see `channel_preview_service_name` in
    /// `rollio-bus`. This field is required so an old encoder binary
    /// reading a new runtime config — or vice versa — fails loudly instead
    /// of silently dropping the preview tap.
    pub preview_topic: String,
    /// Width of the preview tap that the encoder publishes for the
    /// visualizer. Mirrors `[visualizer] max_preview_width` so a single
    /// dimension request travels through the project config.
    pub preview_width: u32,
    pub preview_height: u32,
    /// Throttled preview-tap publish rate. Mirrors `[visualizer]
    /// preview_fps` so the encoder produces at most this many preview
    /// frames per second regardless of the camera's native fps.
    pub preview_fps: u32,
    /// Configured chroma subsampling for the codec input pixel format.
    /// `S422` (default) preserves YUYV / MJPG-422 chroma; `S420` matches
    /// the legacy 4:2:0 behaviour. The encoder downgrades to `S420` at
    /// session-open time when the resolved codec / backend pair can't
    /// accept 4:2:2 input (older NVENC, libsvtav1, certain VAAPI
    /// drivers).
    #[serde(default)]
    pub chroma_subsampling: ChromaSubsampling,
    /// Optional constant-quality target (mapped to `crf` for x264/x265/
    /// AV1, `cq` for NVENC, `qp` for VAAPI). `None` keeps the
    /// libavcodec default.
    #[serde(default)]
    pub crf: Option<u8>,
    /// Optional codec preset name (`"slow"`, `"veryslow"`, etc.).
    /// `None` keeps the libavcodec default.
    #[serde(default)]
    pub preset: Option<String>,
    /// Optional codec tuning hint (`"film"`, `"animation"`, ...).
    /// `None` keeps the libavcodec default.
    #[serde(default)]
    pub tune: Option<String>,
    /// Codec input bit depth (`8` or `10`). `8` matches legacy
    /// behaviour.
    #[serde(default = "default_bit_depth")]
    pub bit_depth: u8,
    /// Color metadata to write into the encoded bitstream. `Auto`
    /// matches legacy behaviour (no metadata, players guess).
    #[serde(default)]
    pub color_space: EncoderColorSpace,
}

impl EncoderRuntimeConfigV2 {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        text.parse()
    }

    pub fn resolved_artifact_format(&self) -> EncoderArtifactFormat {
        EncoderConfig {
            video_codec: self.codec,
            depth_codec: self.codec,
            backend: self.backend,
            video_backend: self.backend,
            depth_backend: self.backend,
            artifact_format: self.artifact_format,
            chroma_subsampling: self.chroma_subsampling,
            crf: self.crf,
            preset: self.preset.clone(),
            tune: self.tune.clone(),
            bit_depth: self.bit_depth,
            color_space: self.color_space,
            queue_size: self.queue_size,
        }
        .resolved_artifact_format()
    }

    pub fn output_extension(&self) -> &'static str {
        self.resolved_artifact_format().extension()
    }

    pub fn output_file_name(&self, episode_index: u32) -> String {
        let stem = self
            .process_id
            .chars()
            .map(|ch| match ch {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
                _ => '_',
            })
            .collect::<String>();
        format!(
            "{stem}_episode_{episode_index:06}.{}",
            self.output_extension()
        )
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.process_id.trim().is_empty()
            || self.channel_id.trim().is_empty()
            || self.frame_topic.trim().is_empty()
            || self.output_dir.trim().is_empty()
            || self.preview_topic.trim().is_empty()
        {
            return Err(ConfigError::Validation(
                "encoder runtime v2: process_id, channel_id, frame_topic, preview_topic, and output_dir must not be empty"
                    .into(),
            ));
        }
        if self.fps == 0 || self.fps > 1000 {
            return Err(ConfigError::Validation(format!(
                "encoder runtime v2: fps must be 1..1000, got {}",
                self.fps
            )));
        }
        if self.preview_width == 0 || self.preview_height == 0 {
            return Err(ConfigError::Validation(
                "encoder runtime v2: preview_width and preview_height must be > 0".into(),
            ));
        }
        if self.preview_fps == 0 || self.preview_fps > 1000 {
            return Err(ConfigError::Validation(format!(
                "encoder runtime v2: preview_fps must be 1..1000, got {}",
                self.preview_fps
            )));
        }
        EncoderConfig {
            video_codec: self.codec,
            depth_codec: self.codec,
            backend: self.backend,
            video_backend: self.backend,
            depth_backend: self.backend,
            artifact_format: self.artifact_format,
            chroma_subsampling: self.chroma_subsampling,
            crf: self.crf,
            preset: self.preset.clone(),
            tune: self.tune.clone(),
            bit_depth: self.bit_depth,
            color_space: self.color_space,
            queue_size: self.queue_size,
        }
        .validate()
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
    pub encoder_process_id: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub pixel_format: PixelFormat,
    pub codec: EncoderCodec,
    pub artifact_format: EncoderArtifactFormat,
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
    pub missing_video_timeout_ms: u64,
    pub staging_dir: String,
    #[serde(default)]
    pub encoded_handoff: EncodedHandoffMode,
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
        if self.cameras.is_empty() {
            return Err(ConfigError::Validation(
                "assembler runtime v2: at least one camera is required".into(),
            ));
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

/// Resolved view of an IMU channel — used by the visualizer / bus-tap to
/// subscribe to inertial streams published by bridge devices like
/// `rollio-device-umi`.
#[derive(Debug, Clone)]
pub struct ResolvedImuChannel {
    pub channel_id: String,
    pub device_name: String,
    pub driver: String,
    pub bus_root: String,
    pub channel_type: String,
    /// Topic where the bridge publishes `Imu` POD samples
    /// (`{bus_root}/{channel_type}/states/imu`).
    pub state_topic: String,
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
            encoder: EncoderConfig::default(),
            assembler: AssemblerConfig::default(),
            storage: StorageConfig::default(),
            monitor: MonitorConfig::default(),
            controller: ControllerConfig::default(),
            // `VisualizerConfig` uses `#[derive(Default)]` with serde defaults only for
            // deserialization — explicit values keep draft templates valid before TOML parse.
            visualizer: VisualizerConfig {
                port: 19090,
                max_preview_width: 320,
                max_preview_height: 240,
                jpeg_quality: 30,
                preview_fps: 60,
                preview_workers: None,
            },
            ui: UiRuntimeConfig::default(),
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
        self.encoder.validate()?;
        self.assembler.validate()?;
        self.storage.validate()?;
        self.monitor.validate()?;
        self.controller.validate()?;
        self.visualizer.validate()?;
        self.ui.validate()?;
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

    /// Walk every `[[devices.channels]]` entry whose `kind = "imu"` is enabled and
    /// produce a flat list of subscribe-ready descriptors. Mirrors the
    /// pattern of [`Self::resolved_camera_channels`] / [`Self::resolved_robot_channels`].
    pub fn resolved_imu_channels(&self) -> Vec<ResolvedImuChannel> {
        let mut channels = Vec::new();
        for device in &self.devices {
            for channel in &device.channels {
                if channel.kind != DeviceType::Imu || !channel.enabled {
                    continue;
                }
                channels.push(ResolvedImuChannel {
                    channel_id: device_channel_id(&device.name, &channel.channel_type),
                    device_name: device.name.clone(),
                    driver: device.driver.clone(),
                    bus_root: device.bus_root.clone(),
                    channel_type: channel.channel_type.clone(),
                    state_topic: imu_state_topic_v2(&device.bus_root, &channel.channel_type),
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
        // Subscribe to every enabled camera so the operator can see all
        // configured streams in the live preview. `MAX_PREVIEW_CAMERAS`
        // applies to layout (max tiles per row) — overflow wraps onto a
        // second row in the UI rather than being silently dropped.
        //
        // The visualizer subscribes to the encoder's downsized RGB24
        // preview tap, never to the raw camera frames topic — so it
        // never has to decode YUYV/MJPG bytes itself.
        let camera_sources = self
            .resolved_camera_channels()
            .into_iter()
            .map(|camera| VisualizerCameraSourceConfig {
                channel_id: camera.channel_id,
                preview_topic: camera_preview_topic_v2(&camera.bus_root, &camera.channel_type),
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
        let imu_sources = self
            .resolved_imu_channels()
            .into_iter()
            .map(|imu| VisualizerImuSourceConfig {
                channel_id: imu.channel_id,
                state_topic: imu.state_topic,
            })
            .collect();
        VisualizerRuntimeConfigV2 {
            port: self.visualizer.port,
            camera_sources,
            robot_sources,
            imu_sources,
            max_preview_width: self.visualizer.max_preview_width,
            max_preview_height: self.visualizer.max_preview_height,
            jpeg_quality: self.visualizer.jpeg_quality,
            preview_fps: self.visualizer.preview_fps,
            preview_workers: self.visualizer.preview_workers,
        }
    }

    pub fn encoder_runtime_configs_v2(&self) -> Vec<EncoderRuntimeConfigV2> {
        self.resolved_camera_channels()
            .into_iter()
            .map(|camera| {
                let codec = self.encoder.codec_for_pixel_format(camera.pixel_format);
                // Per-codec backend so a project that wants e.g. nvidia
                // for color and cpu for depth gets each encoder bound to
                // the right device — no global field is good enough here.
                let backend = self.encoder.backend_for_pixel_format(camera.pixel_format);
                let preview_topic = camera_preview_topic_v2(&camera.bus_root, &camera.channel_type);
                EncoderRuntimeConfigV2 {
                    process_id: encoder_process_id_v2(&camera.channel_id),
                    channel_id: camera.channel_id.clone(),
                    frame_topic: camera.frame_topic,
                    output_dir: encoder_output_dir_v2(
                        &self.assembler.staging_dir,
                        &camera.channel_id,
                    ),
                    codec,
                    backend,
                    artifact_format: self.encoder.resolved_artifact_format_for(codec),
                    queue_size: self.encoder.queue_size,
                    fps: camera.fps,
                    preview_topic,
                    preview_width: self.visualizer.max_preview_width,
                    preview_height: self.visualizer.max_preview_height,
                    preview_fps: self.visualizer.preview_fps,
                    chroma_subsampling: self.encoder.chroma_subsampling,
                    crf: self.encoder.crf,
                    preset: self.encoder.preset.clone(),
                    tune: self.encoder.tune.clone(),
                    bit_depth: self.encoder.bit_depth,
                    color_space: self.encoder.color_space,
                }
            })
            .collect()
    }

    pub fn assembler_runtime_config_v2(
        &self,
        embedded_config_toml: String,
    ) -> AssemblerRuntimeConfigV2 {
        let cameras = self
            .resolved_camera_channels()
            .into_iter()
            .map(|camera| {
                let codec = self.encoder.codec_for_pixel_format(camera.pixel_format);
                AssemblerCameraRuntimeConfigV2 {
                    channel_id: camera.channel_id.clone(),
                    encoder_process_id: encoder_process_id_v2(&camera.channel_id),
                    width: camera.width,
                    height: camera.height,
                    fps: camera.fps,
                    pixel_format: camera.pixel_format,
                    codec,
                    artifact_format: self.encoder.resolved_artifact_format_for(codec),
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
            process_id: "episode-assembler".into(),
            format: self.episode.format,
            fps: self.episode.fps,
            chunk_size: self.episode.chunk_size,
            missing_video_timeout_ms: self.assembler.missing_video_timeout_ms,
            staging_dir: episode_staging_root_v2(&self.assembler.staging_dir),
            encoded_handoff: self.assembler.encoded_handoff,
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
            process_id: "storage".into(),
            backend: self.storage.backend,
            output_path: self.storage.output_path.clone(),
            endpoint: self.storage.endpoint.clone(),
            queue_size: self.storage.queue_size,
        }
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

fn camera_preview_topic_v2(bus_root: &str, channel_type: &str) -> String {
    format!("{}/preview", channel_prefix_v2(bus_root, channel_type))
}

fn robot_state_topic_v2(bus_root: &str, channel_type: &str, state: RobotStateKind) -> String {
    format!(
        "{}/states/{}",
        channel_prefix_v2(bus_root, channel_type),
        state.topic_suffix()
    )
}

/// IMU state topic. Sibling of [`robot_state_topic_v2`] but with the suffix
/// pinned to `imu` since IMU streams aren't represented as a `RobotStateKind`
/// (they're a separate `DeviceType::Imu`).
fn imu_state_topic_v2(bus_root: &str, channel_type: &str) -> String {
    format!("{}/states/imu", channel_prefix_v2(bus_root, channel_type))
}

fn robot_command_topic_v2(bus_root: &str, channel_type: &str, command: RobotCommandKind) -> String {
    format!(
        "{}/commands/{}",
        channel_prefix_v2(bus_root, channel_type),
        command.topic_suffix()
    )
}

fn encoder_process_id_v2(channel_id: &str) -> String {
    format!("encoder.{}", channel_id.replace('/', "."))
}

fn encoder_output_dir_v2(staging_root: &str, channel_id: &str) -> String {
    Path::new(staging_root)
        .join("encoders")
        .join(channel_id.replace('/', "__"))
        .to_string_lossy()
        .into_owned()
}

fn episode_staging_root_v2(staging_root: &str) -> String {
    Path::new(staging_root)
        .join("episodes")
        .to_string_lossy()
        .into_owned()
}
