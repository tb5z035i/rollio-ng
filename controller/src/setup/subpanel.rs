//! Per-channel subpanel state machine + field-edit handlers.
//!
//! Step 1's `s` key opens a modal overlay rendered by the Ink UI that
//! edits the focused channel's per-channel fields:
//!
//!   * For every channel: the three Bool flags (`enabled`,
//!     `preview_enabled`, `record_enabled`) and the channel name.
//!   * For camera channels: `profile` cycle (reuses
//!     `cycle_device_profile`), and the `record` / `preview_settings`
//!     blocks (later — currently the wizard inherits per-channel
//!     defaults from `ChannelRecordConfig::default()` /
//!     `ChannelPreviewConfig::default()` until the operator opts in).
//!   * For robot channels: `mode` cycle (reuses `cycle_robot_mode`),
//!     `publish_states` / `recorded_states` multiselect (reuses
//!     `toggle_publish_state` / `toggle_recorded_state`).
//!
//! The subpanel is intentionally lightweight on the controller side —
//! it tracks just the target channel name and exposes handlers the
//! dispatcher routes to. The Ink UI owns cursor position, draft text,
//! and modal rendering.

use super::state::{rotate_index, AvailableDevice, SetupSession};
use crate::runtime_paths::default_device_executable_name;
use rollio_types::config::{
    BinaryDeviceConfig, CameraChannelProfile, ChannelCommandDefaults, ChannelPreviewConfig,
    ChannelRecordConfig, ChromaSubsampling, DeviceChannelConfigV2, DeviceType,
    DirectJointCompatibility, EncoderBackend, EncoderCodec, EncoderColorSpace, PreviewOutputMode,
    RobotMode, RobotStateKind,
};
use rollio_types::messages::PixelFormat;
use std::error::Error;

/// Cycle list for `ChannelRecordConfig::video_codec` / `color_codec`.
/// Covers every variant of `EncoderCodec` the rollio-encoder crate
/// can emit for an RGB / IR stream. `Rvl` is intentionally excluded:
/// it's depth-only and the libav encoder rejects non-depth16 frames,
/// so cycling onto it for a color channel would trip validation.
const RECORD_VIDEO_CODECS: &[EncoderCodec] = &[
    EncoderCodec::H264,
    EncoderCodec::H265,
    EncoderCodec::Av1,
    EncoderCodec::Mjpg,
];

/// Cycle list for `ChannelRecordConfig::depth_codec`. RVL is the only
/// supported depth codec today — the libav depth backends were a
/// planned alternative but never wired through the depth backend
/// registry, so we deliberately expose just the single option. The
/// subpanel UI renders this field as read-only ("rvl only"), but
/// keeping a one-element list here means an `h`/`l` press is a
/// no-op rather than dispatching to an arm that doesn't exist.
const RECORD_DEPTH_CODECS: &[EncoderCodec] = &[EncoderCodec::Rvl];

/// Cycle list for `ChannelRecordConfig::backend` / `video_backend` /
/// `depth_backend`. `Auto` is intentionally first so a freshly-saved
/// config doesn't pin the encoder to one host's hardware lineup;
/// `HorizonX5` is included so operators on aarch64 boards running
/// the Horizon BSP can route through the hardware VPU.
const RECORD_BACKENDS: &[EncoderBackend] = &[
    EncoderBackend::Auto,
    EncoderBackend::Cpu,
    EncoderBackend::Nvidia,
    EncoderBackend::Vaapi,
    EncoderBackend::Passthrough,
    EncoderBackend::HorizonX5,
];

const RECORD_CHROMA_SUBSAMPLINGS: &[ChromaSubsampling] =
    &[ChromaSubsampling::S422, ChromaSubsampling::S420];

const RECORD_BIT_DEPTHS: &[u8] = &[8, 10];

/// Cycle list for `ChannelRecordConfig::preset`. Mirrors the legacy
/// `cycle_encoder_preset` options — the standard x264 / x265 / NVENC
/// preset names plus `None` (= libav default). Operators don't
/// generally type these by hand; cycling through the canonical list
/// is friendlier than asking for a free-form string.
const RECORD_PRESETS: &[Option<&str>] = &[
    None,
    Some("ultrafast"),
    Some("veryfast"),
    Some("fast"),
    Some("medium"),
    Some("slow"),
    Some("slower"),
    Some("veryslow"),
];

const RECORD_COLOR_SPACES: &[EncoderColorSpace] = &[
    EncoderColorSpace::Auto,
    EncoderColorSpace::Bt709Limited,
    EncoderColorSpace::Bt601Limited,
];

const PREVIEW_OUTPUT_MODES: &[PreviewOutputMode] =
    &[PreviewOutputMode::Jpeg, PreviewOutputMode::Encoded];

impl SetupSession {
    /// Open the subpanel for the named available_device row. Returns
    /// false (and leaves state unchanged) when the name doesn't match a
    /// known row. Returning true here triggers a state push so the Ink
    /// UI sees `subpanel_target` populated and renders the modal.
    pub(super) fn open_subpanel(&mut self, name: &str) -> bool {
        if self.available_device(name).is_none() {
            return false;
        }
        if self.subpanel_target_name.as_deref() == Some(name) {
            return false;
        }
        self.subpanel_target_name = Some(name.to_owned());
        true
    }

    /// Close the subpanel. Returns true when there was a panel to close
    /// so the dispatcher knows to re-publish state.
    pub(super) fn close_subpanel(&mut self) -> bool {
        self.subpanel_target_name.take().is_some()
    }

    /// V1 add-device flow: opening the picker is currently equivalent
    /// to pressing the "pseudo camera" option, since pseudo camera is
    /// the only add affordance fully wired up. The full picker that
    /// asks (pseudo camera | pseudo robot | command device stub) lands
    /// in a follow-up; for now, `a` adds one default pseudo camera
    /// each time it's pressed.
    pub(super) fn open_add_picker(&mut self) -> bool {
        match self.add_pseudo_camera() {
            Ok(true) => true,
            Ok(false) => false,
            Err(error) => {
                self.message = Some(format!("Could not add pseudo camera: {error}"));
                true
            }
        }
    }

    /// Append a fresh pseudo camera to the project + the available
    /// device list, using safe defaults (`640x480 @ 30 fps rgb24`). The
    /// pseudo driver accepts arbitrary unique ids; we generate one by
    /// scanning existing pseudo devices and picking the next index.
    /// Returns true on success so the dispatcher publishes the
    /// updated state.
    pub(super) fn add_pseudo_camera(&mut self) -> Result<bool, Box<dyn Error>> {
        let suffix = self.next_pseudo_id_index("pseudo_camera");
        let id = format!("pseudo_camera_{suffix}");
        let display = format!("Pseudo Camera {suffix}");
        let channel = DeviceChannelConfigV2 {
            channel_type: "color".to_owned(),
            kind: DeviceType::Camera,
            enabled: true,
            name: Some(id.clone()),
            channel_label: Some(display.clone()),
            mode: None,
            dof: None,
            publish_states: Vec::new(),
            recorded_states: Vec::new(),
            control_frequency_hz: None,
            profile: Some(CameraChannelProfile {
                width: 640,
                height: 480,
                fps: 30,
                pixel_format: PixelFormat::Rgb24,
                native_pixel_format: None,
                mjpeg_quality: None,
                h264_bitrate_bps: None,
                h264_gop: None,
                h264_preset: None,
                h264_tune: None,
                h264_profile: None,
            }),
            preview_enabled: true,
            record_enabled: true,
            record: None,
            preview_settings: None,
            command_defaults: ChannelCommandDefaults::default(),
            value_limits: Vec::new(),
            direct_joint_compatibility: DirectJointCompatibility::default(),
            supported_commands: Vec::new(),
            extra: toml::Table::new(),
        };
        let mut extra = toml::Table::new();
        extra.insert("transport".into(), toml::Value::String("simulated".into()));
        let device = BinaryDeviceConfig {
            name: id.clone(),
            executable: Some(default_device_executable_name("pseudo")),
            driver: "pseudo".to_owned(),
            id: id.clone(),
            bus_root: id.clone(),
            dds_domain_id: None,
            dds_shm_segment_size: None,
            dds_callback_threads: None,
            channels: vec![channel.clone()],
            extra,
        };
        let available_name = format!("camera|pseudo|{}|color|-", id);
        let available = AvailableDevice {
            name: available_name,
            display_name: display.clone(),
            device_type: DeviceType::Camera,
            driver: "pseudo".to_owned(),
            id: id.clone(),
            camera_profiles: Vec::new(),
            supported_modes: Vec::new(),
            supported_states: Vec::new(),
            supported_commands: Vec::new(),
            direct_joint_compatibility: DirectJointCompatibility::default(),
            current: device.clone(),
        };
        let snapshot_devices = self.config.devices.clone();
        let snapshot_available = self.available_devices.clone();
        self.config.devices.push(device);
        self.available_devices.push(available);
        if let Err(error) = self.config.validate() {
            self.config.devices = snapshot_devices;
            self.available_devices = snapshot_available;
            self.message = Some(format!("Pseudo camera rejected by validator: {error}"));
            return Ok(false);
        }
        self.message = Some(format!("Added pseudo camera {id}."));
        Ok(true)
    }

    /// Append a fresh 6-DOF pseudo robot arm. Mirrors
    /// `add_pseudo_camera` for the robot side: hard-coded sensible
    /// defaults today, full picker that asks for dof lands later.
    pub(super) fn add_pseudo_robot(&mut self) -> Result<bool, Box<dyn Error>> {
        let suffix = self.next_pseudo_id_index("pseudo_robot");
        let id = format!("pseudo_robot_{suffix}_dof_6");
        let display = format!("Pseudo Robot {suffix}");
        let channel = DeviceChannelConfigV2 {
            channel_type: "arm".to_owned(),
            kind: DeviceType::Robot,
            enabled: true,
            name: Some(id.clone()),
            channel_label: Some(display.clone()),
            mode: Some(RobotMode::FreeDrive),
            dof: Some(6),
            publish_states: vec![
                RobotStateKind::JointPosition,
                RobotStateKind::JointVelocity,
                RobotStateKind::JointEffort,
            ],
            recorded_states: vec![
                RobotStateKind::JointPosition,
                RobotStateKind::JointVelocity,
                RobotStateKind::JointEffort,
            ],
            control_frequency_hz: Some(60.0),
            profile: None,
            preview_enabled: true,
            record_enabled: true,
            record: None,
            preview_settings: None,
            command_defaults: ChannelCommandDefaults::default(),
            value_limits: Vec::new(),
            direct_joint_compatibility: DirectJointCompatibility::default(),
            supported_commands: Vec::new(),
            extra: toml::Table::new(),
        };
        let mut extra = toml::Table::new();
        extra.insert("transport".into(), toml::Value::String("simulated".into()));
        let device = BinaryDeviceConfig {
            name: id.clone(),
            executable: Some(default_device_executable_name("pseudo")),
            driver: "pseudo".to_owned(),
            id: id.clone(),
            bus_root: id.clone(),
            dds_domain_id: None,
            dds_shm_segment_size: None,
            dds_callback_threads: None,
            channels: vec![channel.clone()],
            extra,
        };
        let available = AvailableDevice {
            name: format!("robot|pseudo|{}|arm|-", id),
            display_name: display.clone(),
            device_type: DeviceType::Robot,
            driver: "pseudo".to_owned(),
            id: id.clone(),
            camera_profiles: Vec::new(),
            supported_modes: vec![RobotMode::FreeDrive, RobotMode::CommandFollowing],
            supported_states: vec![
                RobotStateKind::JointPosition,
                RobotStateKind::JointVelocity,
                RobotStateKind::JointEffort,
            ],
            supported_commands: Vec::new(),
            direct_joint_compatibility: DirectJointCompatibility::default(),
            current: device.clone(),
        };
        let snapshot_devices = self.config.devices.clone();
        let snapshot_available = self.available_devices.clone();
        self.config.devices.push(device);
        self.available_devices.push(available);
        if let Err(error) = self.config.validate() {
            self.config.devices = snapshot_devices;
            self.available_devices = snapshot_available;
            self.message = Some(format!("Pseudo robot rejected by validator: {error}"));
            return Ok(false);
        }
        self.message = Some(format!("Added pseudo robot {id}."));
        Ok(true)
    }

    /// Append a stub "command" device. The actual `rollio-device-command`
    /// executable doesn't exist yet — this just reserves a slot in the
    /// config so the operator can wire up an inference-output channel
    /// before the runtime supports it. The wizard's validator is
    /// lenient about unknown drivers (executable resolution happens at
    /// process spawn, not at config save time).
    pub(super) fn add_command_device(&mut self) -> Result<bool, Box<dyn Error>> {
        let suffix = self.next_pseudo_id_index("command");
        let id = format!("command_{suffix}");
        let display = format!("Command Device {suffix} (stub)");
        let channel = DeviceChannelConfigV2 {
            channel_type: "arm".to_owned(),
            kind: DeviceType::Robot,
            enabled: true,
            name: Some(id.clone()),
            channel_label: Some(display.clone()),
            mode: Some(RobotMode::CommandFollowing),
            dof: Some(6),
            publish_states: vec![RobotStateKind::JointPosition],
            recorded_states: Vec::new(),
            control_frequency_hz: Some(60.0),
            profile: None,
            preview_enabled: true,
            record_enabled: false,
            record: None,
            preview_settings: None,
            command_defaults: ChannelCommandDefaults::default(),
            value_limits: Vec::new(),
            direct_joint_compatibility: DirectJointCompatibility::default(),
            supported_commands: Vec::new(),
            extra: toml::Table::new(),
        };
        let device = BinaryDeviceConfig {
            name: id.clone(),
            executable: Some(default_device_executable_name("command")),
            driver: "command".to_owned(),
            id: id.clone(),
            bus_root: id.clone(),
            dds_domain_id: None,
            dds_shm_segment_size: None,
            dds_callback_threads: None,
            channels: vec![channel.clone()],
            extra: toml::Table::new(),
        };
        let available = AvailableDevice {
            name: format!("robot|command|{}|arm|-", id),
            display_name: display.clone(),
            device_type: DeviceType::Robot,
            driver: "command".to_owned(),
            id: id.clone(),
            camera_profiles: Vec::new(),
            supported_modes: vec![RobotMode::CommandFollowing],
            supported_states: Vec::new(),
            supported_commands: Vec::new(),
            direct_joint_compatibility: DirectJointCompatibility::default(),
            current: device.clone(),
        };
        let snapshot_devices = self.config.devices.clone();
        let snapshot_available = self.available_devices.clone();
        self.config.devices.push(device);
        self.available_devices.push(available);
        if let Err(error) = self.config.validate() {
            self.config.devices = snapshot_devices;
            self.available_devices = snapshot_available;
            self.message = Some(format!("Command device rejected: {error}"));
            return Ok(false);
        }
        self.message = Some(format!(
            "Added command device stub {id} — the runtime driver doesn't exist yet."
        ));
        Ok(true)
    }

    /// Pick the next integer suffix for a generated pseudo / command
    /// device id (e.g. `pseudo_camera_3` when ids 0..=2 already exist).
    /// Scans `self.config.devices` for any id starting with `{prefix}_`
    /// and parses the trailing digits; returns max + 1, falling back
    /// to 0 when no matches exist.
    fn next_pseudo_id_index(&self, prefix: &str) -> u32 {
        let prefix_with_sep = format!("{prefix}_");
        let mut next: u32 = 0;
        for device in &self.config.devices {
            if let Some(rest) = device.id.strip_prefix(&prefix_with_sep) {
                let head: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(n) = head.parse::<u32>() {
                    next = next.max(n + 1);
                }
            }
        }
        next
    }

    /// Toggle the focused channel's `preview_enabled` flag. The
    /// corresponding preview encoder process is spawned (or skipped)
    /// the next time the preview runtime starts. Validates the
    /// project to keep the toggle from violating any cross-field
    /// invariant (e.g. robot channels rejecting `preview_enabled =
    /// false` in some driver schemas). Mirrors the new value into
    /// the AvailableDevice snapshot so the Ink subpanel UI sees the
    /// change on the next state publish.
    pub(super) fn subpanel_toggle_preview_enabled(
        &mut self,
        name: &str,
    ) -> Result<bool, Box<dyn Error>> {
        let Some((device_index, channel_index)) = self.configured_device_channel_index(name) else {
            return Ok(false);
        };
        let channel = &mut self.config.devices[device_index].channels[channel_index];
        let previous = channel.preview_enabled;
        channel.preview_enabled = !previous;
        if let Err(error) = self.config.validate() {
            self.config.devices[device_index].channels[channel_index].preview_enabled = previous;
            self.message = Some(format!("preview_enabled toggle rejected: {error}"));
            return Ok(false);
        }
        let new_value = self.config.devices[device_index].channels[channel_index].preview_enabled;
        if let Some(available) = self.available_device_mut(name) {
            if let Some(channel) = available.current.channels.first_mut() {
                channel.preview_enabled = new_value;
            }
        }
        Ok(true)
    }

    /// Toggle the focused channel's `record_enabled` flag. Recording
    /// is skipped when false; the channel still appears on the bus
    /// (frames published) but the recording encoder doesn't write
    /// packets to disk during an episode. Mirrors into the
    /// AvailableDevice snapshot for the same reason as
    /// `subpanel_toggle_preview_enabled`.
    pub(super) fn subpanel_toggle_record_enabled(
        &mut self,
        name: &str,
    ) -> Result<bool, Box<dyn Error>> {
        let Some((device_index, channel_index)) = self.configured_device_channel_index(name) else {
            return Ok(false);
        };
        let channel = &mut self.config.devices[device_index].channels[channel_index];
        let previous = channel.record_enabled;
        channel.record_enabled = !previous;
        if let Err(error) = self.config.validate() {
            self.config.devices[device_index].channels[channel_index].record_enabled = previous;
            self.message = Some(format!("record_enabled toggle rejected: {error}"));
            return Ok(false);
        }
        let new_value = self.config.devices[device_index].channels[channel_index].record_enabled;
        if let Some(available) = self.available_device_mut(name) {
            if let Some(channel) = available.current.channels.first_mut() {
                channel.record_enabled = new_value;
            }
        }
        Ok(true)
    }

    /// Cycle the focused channel's primary domain field with the
    /// `h`/`l` keys: camera channels cycle the capture `profile`
    /// (delegates to `cycle_device_profile`), robot channels cycle
    /// `mode` (delegates to `cycle_robot_mode`). Channel kind is
    /// taken from the available_device snapshot.
    pub(super) fn subpanel_cycle_primary(
        &mut self,
        name: &str,
        delta: i32,
    ) -> Result<bool, Box<dyn Error>> {
        let kind = match self.available_device(name) {
            Some(available) => available.device_type,
            None => return Ok(false),
        };
        match kind {
            DeviceType::Camera => self.cycle_device_profile(name, delta),
            DeviceType::Robot => self.cycle_robot_mode(name, delta),
        }
    }

    /// Cycle one knob inside the focused camera channel's
    /// `[devices.channels.record]` block. `field` identifies the
    /// knob — codec, backend, chroma_subsampling, bit_depth, color_space.
    /// The record block is materialized (`Some(default)`) on first
    /// edit so the operator doesn't have to manually write the block
    /// before they can tune it. On validation failure we restore the
    /// channel snapshot taken before the mutation, so a rejected
    /// cycle never leaves the wizard in a half-mutated state.
    pub(super) fn subpanel_cycle_record_field(
        &mut self,
        name: &str,
        field: &str,
        delta: i32,
    ) -> Result<bool, Box<dyn Error>> {
        let Some((device_index, channel_index)) = self.configured_device_channel_index(name) else {
            return Ok(false);
        };
        if self.config.devices[device_index].channels[channel_index].kind != DeviceType::Camera {
            return Ok(false);
        }
        let snapshot = self.config.devices[device_index].channels[channel_index].clone();
        ensure_record_block(&mut self.config.devices[device_index].channels[channel_index]);
        let record = self.config.devices[device_index].channels[channel_index]
            .record
            .as_mut()
            .expect("record block ensured above");
        let known_field = match field {
            "video_codec" => {
                let next = cycle_enum_field(record.video_codec, RECORD_VIDEO_CODECS, delta);
                record.video_codec = Some(next);
                true
            }
            "depth_codec" => {
                let next = cycle_enum_field(record.depth_codec, RECORD_DEPTH_CODECS, delta);
                record.depth_codec = Some(next);
                true
            }
            "backend" => {
                let next = cycle_enum_field(record.backend, RECORD_BACKENDS, delta);
                record.backend = Some(next);
                true
            }
            "video_backend" => {
                let next = cycle_enum_field(record.video_backend, RECORD_BACKENDS, delta);
                record.video_backend = Some(next);
                true
            }
            "depth_backend" => {
                let next = cycle_enum_field(record.depth_backend, RECORD_BACKENDS, delta);
                record.depth_backend = Some(next);
                true
            }
            "chroma_subsampling" => {
                let next =
                    cycle_enum_field(record.chroma_subsampling, RECORD_CHROMA_SUBSAMPLINGS, delta);
                record.chroma_subsampling = Some(next);
                true
            }
            "bit_depth" => {
                let next = cycle_enum_field(record.bit_depth, RECORD_BIT_DEPTHS, delta);
                record.bit_depth = Some(next);
                true
            }
            "color_space" => {
                let next = cycle_enum_field(record.color_space, RECORD_COLOR_SPACES, delta);
                record.color_space = Some(next);
                true
            }
            "preset" => {
                // `preset` is a String but cycles through a fixed
                // list of x264/x265 preset names plus `None` (libav
                // default). Match by case-insensitive string compare
                // so a TOML-saved "Medium" still cycles cleanly.
                let current_str = record.preset.as_deref();
                let current_index = RECORD_PRESETS
                    .iter()
                    .position(|opt| match (*opt, current_str) {
                        (None, None) => true,
                        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                        _ => false,
                    })
                    .unwrap_or(0);
                let next = RECORD_PRESETS[rotate_index(current_index, RECORD_PRESETS.len(), delta)];
                record.preset = next.map(|s| s.to_owned());
                true
            }
            _ => false,
        };
        if !known_field {
            // Unknown field name from the UI shouldn't have any
            // visible effect — leave the snapshot in place.
            self.config.devices[device_index].channels[channel_index] = snapshot;
            self.message = Some(format!("Unknown record field: {field}"));
            return Ok(false);
        }
        if let Err(error) = self.config.validate() {
            self.config.devices[device_index].channels[channel_index] = snapshot;
            self.message = Some(format!("record.{field} rejected: {error}"));
            return Ok(false);
        }
        sync_channel_into_available(
            &mut self.available_devices,
            name,
            &self.config.devices[device_index].channels[channel_index],
        );
        Ok(true)
    }

    /// Set a text-input knob inside the focused camera channel's
    /// `record` block — `crf`, `tune`, `queue_size`. Parses
    /// the operator's draft into the field's native type; empty input
    /// clears the field (back to controller default). Rolls back to
    /// the pre-edit channel snapshot on any parse / validation error.
    pub(super) fn subpanel_set_record_field(
        &mut self,
        name: &str,
        field: &str,
        value: &str,
    ) -> Result<bool, Box<dyn Error>> {
        let Some((device_index, channel_index)) = self.configured_device_channel_index(name) else {
            return Ok(false);
        };
        if self.config.devices[device_index].channels[channel_index].kind != DeviceType::Camera {
            return Ok(false);
        }
        let snapshot = self.config.devices[device_index].channels[channel_index].clone();
        ensure_record_block(&mut self.config.devices[device_index].channels[channel_index]);
        let record = self.config.devices[device_index].channels[channel_index]
            .record
            .as_mut()
            .expect("record block ensured above");
        let trimmed = value.trim();
        let empty = trimmed.is_empty();
        let parse_outcome: Result<(), String> = match field {
            "crf" => {
                if empty {
                    record.crf = None;
                    Ok(())
                } else {
                    match trimmed.parse::<u8>() {
                        Ok(v) if v <= 51 => {
                            record.crf = Some(v);
                            Ok(())
                        }
                        _ => Err(format!("crf = {value:?} is not in range 0..=51")),
                    }
                }
            }
            "preset" => {
                record.preset = if empty {
                    None
                } else {
                    Some(trimmed.to_owned())
                };
                Ok(())
            }
            "tune" => {
                record.tune = if empty {
                    None
                } else {
                    Some(trimmed.to_owned())
                };
                Ok(())
            }
            "queue_size" => {
                if empty {
                    record.queue_size = None;
                    Ok(())
                } else {
                    match trimmed.parse::<u32>() {
                        Ok(v) if v > 0 => {
                            record.queue_size = Some(v);
                            Ok(())
                        }
                        _ => Err(format!("queue_size = {value:?} must be a positive integer")),
                    }
                }
            }
            _ => Err(format!("Unknown record field: {field}")),
        };
        if let Err(msg) = parse_outcome {
            self.config.devices[device_index].channels[channel_index] = snapshot;
            self.message = Some(msg);
            return Ok(false);
        }
        if let Err(error) = self.config.validate() {
            self.config.devices[device_index].channels[channel_index] = snapshot;
            self.message = Some(format!("record.{field} rejected: {error}"));
            return Ok(false);
        }
        sync_channel_into_available(
            &mut self.available_devices,
            name,
            &self.config.devices[device_index].channels[channel_index],
        );
        Ok(true)
    }

    /// Cycle one knob inside the focused camera channel's
    /// `[devices.channels.preview_config]` block. Mirrors
    /// `subpanel_cycle_record_field` for the preview encoder side —
    /// same snapshot-and-rollback policy on validation failure.
    pub(super) fn subpanel_cycle_preview_field(
        &mut self,
        name: &str,
        field: &str,
        delta: i32,
    ) -> Result<bool, Box<dyn Error>> {
        let Some((device_index, channel_index)) = self.configured_device_channel_index(name) else {
            return Ok(false);
        };
        if self.config.devices[device_index].channels[channel_index].kind != DeviceType::Camera {
            return Ok(false);
        }
        let snapshot = self.config.devices[device_index].channels[channel_index].clone();
        ensure_preview_block(&mut self.config.devices[device_index].channels[channel_index]);
        let preview = self.config.devices[device_index].channels[channel_index]
            .preview_settings
            .as_mut()
            .expect("preview block ensured above");
        let known_field = match field {
            "output_mode" => {
                let next = cycle_enum_field(preview.output_mode, PREVIEW_OUTPUT_MODES, delta);
                preview.output_mode = Some(next);
                true
            }
            "color_codec" => {
                let next = cycle_enum_field(preview.color_codec, RECORD_VIDEO_CODECS, delta);
                preview.color_codec = Some(next);
                true
            }
            "depth_codec" => {
                let next = cycle_enum_field(preview.depth_codec, RECORD_DEPTH_CODECS, delta);
                preview.depth_codec = Some(next);
                true
            }
            "backend" => {
                let next = cycle_enum_field(preview.backend, RECORD_BACKENDS, delta);
                preview.backend = Some(next);
                true
            }
            _ => false,
        };
        if !known_field {
            self.config.devices[device_index].channels[channel_index] = snapshot;
            self.message = Some(format!("Unknown preview field: {field}"));
            return Ok(false);
        }
        if let Err(error) = self.config.validate() {
            self.config.devices[device_index].channels[channel_index] = snapshot;
            self.message = Some(format!("preview.{field} rejected: {error}"));
            return Ok(false);
        }
        sync_channel_into_available(
            &mut self.available_devices,
            name,
            &self.config.devices[device_index].channels[channel_index],
        );
        Ok(true)
    }

    /// Set a text-input knob inside the preview block — width / height
    /// / fps / gop_seconds / crf / jpeg_quality.
    pub(super) fn subpanel_set_preview_field(
        &mut self,
        name: &str,
        field: &str,
        value: &str,
    ) -> Result<bool, Box<dyn Error>> {
        let Some((device_index, channel_index)) = self.configured_device_channel_index(name) else {
            return Ok(false);
        };
        if self.config.devices[device_index].channels[channel_index].kind != DeviceType::Camera {
            return Ok(false);
        }
        let snapshot = self.config.devices[device_index].channels[channel_index].clone();
        ensure_preview_block(&mut self.config.devices[device_index].channels[channel_index]);
        let preview = self.config.devices[device_index].channels[channel_index]
            .preview_settings
            .as_mut()
            .expect("preview block ensured above");
        let trimmed = value.trim();
        let empty = trimmed.is_empty();
        let parse_outcome: Result<(), String> = match field {
            "width" => parse_positive_u32(trimmed, empty, &mut preview.width, "width", value),
            "height" => parse_positive_u32(trimmed, empty, &mut preview.height, "height", value),
            "fps" => {
                if empty {
                    preview.fps = None;
                    Ok(())
                } else {
                    match trimmed.parse::<u32>() {
                        Ok(v) if (1..=1000).contains(&v) => {
                            preview.fps = Some(v);
                            Ok(())
                        }
                        _ => Err(format!("fps = {value:?} must be in 1..=1000")),
                    }
                }
            }
            "gop_seconds" => {
                if empty {
                    preview.gop_seconds = None;
                    Ok(())
                } else {
                    match trimmed.parse::<u32>() {
                        Ok(v) if v > 0 => {
                            preview.gop_seconds = Some(v);
                            Ok(())
                        }
                        _ => Err(format!(
                            "gop_seconds = {value:?} must be a positive integer"
                        )),
                    }
                }
            }
            "crf" => {
                if empty {
                    preview.crf = None;
                    Ok(())
                } else {
                    match trimmed.parse::<u8>() {
                        Ok(v) if v <= 51 => {
                            preview.crf = Some(v);
                            Ok(())
                        }
                        _ => Err(format!("crf = {value:?} is not in 0..=51")),
                    }
                }
            }
            "jpeg_quality" => {
                if empty {
                    preview.jpeg_quality = None;
                    Ok(())
                } else {
                    match trimmed.parse::<i32>() {
                        Ok(v) if (1..=100).contains(&v) => {
                            preview.jpeg_quality = Some(v);
                            Ok(())
                        }
                        _ => Err(format!("jpeg_quality = {value:?} must be in 1..=100")),
                    }
                }
            }
            _ => Err(format!("Unknown preview field: {field}")),
        };
        if let Err(msg) = parse_outcome {
            self.config.devices[device_index].channels[channel_index] = snapshot;
            self.message = Some(msg);
            return Ok(false);
        }
        if let Err(error) = self.config.validate() {
            self.config.devices[device_index].channels[channel_index] = snapshot;
            self.message = Some(format!("preview.{field} rejected: {error}"));
            return Ok(false);
        }
        sync_channel_into_available(
            &mut self.available_devices,
            name,
            &self.config.devices[device_index].channels[channel_index],
        );
        Ok(true)
    }

    /// Edit the focused channel's `control_frequency_hz` (robot-only).
    /// Rejects non-positive / non-finite values; cameras silently
    /// ignore the call so a stray keystroke in the wrong panel can't
    /// trip validation.
    pub(super) fn subpanel_set_control_frequency_hz(
        &mut self,
        name: &str,
        value: &str,
    ) -> Result<bool, Box<dyn Error>> {
        let Some((device_index, channel_index)) = self.configured_device_channel_index(name) else {
            return Ok(false);
        };
        if self.config.devices[device_index].channels[channel_index].kind != DeviceType::Robot {
            return Ok(false);
        }
        let parsed: f64 = match value.trim().parse::<f64>() {
            Ok(v) if v.is_finite() && v > 0.0 => v,
            _ => {
                self.message = Some(format!(
                    "control_frequency_hz must be a positive finite number, got {value:?}."
                ));
                return Ok(false);
            }
        };
        let previous =
            self.config.devices[device_index].channels[channel_index].control_frequency_hz;
        if previous == Some(parsed) {
            return Ok(false);
        }
        self.config.devices[device_index].channels[channel_index].control_frequency_hz =
            Some(parsed);
        if let Err(error) = self.config.validate() {
            self.config.devices[device_index].channels[channel_index].control_frequency_hz =
                previous;
            self.message = Some(format!("control_frequency_hz rejected: {error}"));
            return Ok(false);
        }
        sync_channel_into_available(
            &mut self.available_devices,
            name,
            &self.config.devices[device_index].channels[channel_index],
        );
        Ok(true)
    }
}

/// Materialize the channel's `record` block as `Some(default)` if it's
/// still None. Lets the subpanel edit individual record-encoder knobs
/// without forcing the operator to opt into the block first via a
/// separate keystroke.
fn ensure_record_block(channel: &mut DeviceChannelConfigV2) {
    if channel.record.is_none() {
        channel.record = Some(ChannelRecordConfig::default());
    }
}

/// Mirror a freshly-mutated channel from `self.config.devices` into
/// the matching `AvailableDevice.current` snapshot the Ink UI reads
/// from. Without this, edits to per-channel `record` /
/// `preview_settings` blocks are invisible in the wizard until the
/// next discovery refresh. Call after any subpanel mutation that
/// passes validation.
fn sync_channel_into_available(
    available_devices: &mut [AvailableDevice],
    name: &str,
    channel: &DeviceChannelConfigV2,
) {
    if let Some(available) = available_devices.iter_mut().find(|d| d.name == name) {
        if let Some(slot) = available.current.channels.first_mut() {
            *slot = channel.clone();
        }
    }
}

fn ensure_preview_block(channel: &mut DeviceChannelConfigV2) {
    if channel.preview_settings.is_none() {
        channel.preview_settings = Some(ChannelPreviewConfig::default());
    }
}

/// Step `delta` positions through `options`, wrapping at both ends.
/// `current` is the current value (None falls through to index 0).
/// Returned value is a copy of the new option entry.
fn cycle_enum_field<T: Copy + PartialEq>(current: Option<T>, options: &[T], delta: i32) -> T {
    if options.is_empty() {
        return current.expect("cycle_enum_field called with empty options");
    }
    let current_index = current
        .and_then(|v| options.iter().position(|o| *o == v))
        .unwrap_or(0);
    options[rotate_index(current_index, options.len(), delta)]
}

/// Helper used by `subpanel_set_preview_field` to share the
/// "empty clears, otherwise positive u32" pattern between `width` and
/// `height`. Writes the parsed value into `slot` on success; returns
/// the error string on parse failure so the caller can roll back.
fn parse_positive_u32(
    trimmed: &str,
    empty: bool,
    slot: &mut Option<u32>,
    field: &str,
    raw_value: &str,
) -> Result<(), String> {
    if empty {
        *slot = None;
        return Ok(());
    }
    match trimmed.parse::<u32>() {
        Ok(v) if v > 0 => {
            *slot = Some(v);
            Ok(())
        }
        _ => Err(format!(
            "{field} = {raw_value:?} must be a positive integer"
        )),
    }
}
