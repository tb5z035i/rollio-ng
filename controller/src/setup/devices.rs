use super::overview::camera_channel_type_for_profile;
use super::state::{rotate_index, SetupSession};
use rollio_types::config::{
    BinaryDeviceConfig, CameraChannelProfile, DeviceType, RobotMode, RobotStateKind,
};
use std::error::Error;

impl SetupSession {
    pub(super) fn set_device_name(
        &mut self,
        name: &str,
        value: &str,
    ) -> Result<bool, Box<dyn Error>> {
        // The "rename" action now targets a single channel's user-facing
        // name; the BinaryDeviceConfig.name (= bus_root, iceoryx2 service
        // root, pairing key) is treated as an internal, immutable
        // identifier. This avoids the previous behavior where renaming the
        // arm row also renamed the e2 row because they shared the parent
        // BinaryDeviceConfig.name.
        let Some((selected_index, channel_index)) = self.selected_device_index(name) else {
            return Ok(false);
        };
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.message = Some("Channel name must not be empty.".into());
            return Ok(false);
        }

        // Uniqueness check: channel names must not collide across rows
        // (whether on the same device or another), excluding the row we are
        // editing.
        let duplicate_name = self.available_devices.iter().any(|device| {
            device.name != name
                && device
                    .current
                    .channels
                    .first()
                    .and_then(|channel| channel.name.as_deref())
                    .is_some_and(|existing| existing == trimmed)
        });
        if duplicate_name {
            self.message = Some(format!("Channel name \"{trimmed}\" is already in use."));
            return Ok(false);
        }

        let current_name = self.config.devices[selected_index].channels[channel_index]
            .name
            .clone();
        if current_name.as_deref() == Some(trimmed) {
            return Ok(false);
        }

        // Mirror the rename into both the persisted project config and the
        // matching available_device row's snapshot. We do NOT touch
        // BinaryDeviceConfig.name / bus_root or pairings.
        self.config.devices[selected_index].channels[channel_index].name = Some(trimmed.to_owned());
        if let Some(available) = self.available_device_mut(name) {
            if let Some(channel) = available.current.channels.first_mut() {
                channel.name = Some(trimmed.to_owned());
            }
        }
        self.config.validate()?;

        Ok(true)
    }

    pub(super) fn set_identify_device(&mut self, name: Option<&str>) -> bool {
        if name.is_none() {
            return self.clear_identify_state();
        }
        if self.identify_device_name.as_deref() == name {
            return false;
        }
        self.identify_device_name = name.map(ToOwned::to_owned);
        if let Some(name) = name {
            if let Some(device) = self.available_device(name) {
                self.message = Some(format!(
                    "Identify active for {} ({})",
                    device.display_name, device.id
                ));
            }
        }
        true
    }

    pub(super) fn cycle_device_profile(
        &mut self,
        name: &str,
        delta: i32,
    ) -> Result<bool, Box<dyn Error>> {
        let Some((device_index, channel_index)) = self.selected_device_index(name) else {
            return Ok(false);
        };
        let updated_current = {
            let Some(available) = self.available_device_mut(name) else {
                return Ok(false);
            };
            if available.camera_profiles.is_empty() {
                return Ok(false);
            }
            let Some(ch) = available.current.channels.first_mut() else {
                return Ok(false);
            };
            if ch.kind != DeviceType::Camera {
                return Ok(false);
            }
            let prof = ch.profile.as_ref();
            let current_profile = available
                .camera_profiles
                .iter()
                .position(|profile| {
                    prof.is_some_and(|p| {
                        p.width == profile.width
                            && p.height == profile.height
                            && p.fps == profile.fps
                            && p.pixel_format == profile.pixel_format
                            && p.native_pixel_format == profile.native_pixel_format
                    }) && camera_channel_type_for_profile(profile) == ch.channel_type
                })
                .unwrap_or(0);
            let next_index = rotate_index(current_profile, available.camera_profiles.len(), delta);
            let profile = available.camera_profiles[next_index].clone();
            ch.channel_type = camera_channel_type_for_profile(&profile);
            ch.profile = Some(CameraChannelProfile {
                width: profile.width,
                height: profile.height,
                fps: profile.fps,
                pixel_format: profile.pixel_format,
                native_pixel_format: profile.native_pixel_format.clone(),
                mjpeg_quality: None,
                h264_bitrate_bps: None,
                h264_gop: None,
                h264_preset: None,
                h264_tune: None,
                h264_profile: None,
            });
            available.current.clone()
        };
        self.config.devices[device_index].channels[channel_index] =
            updated_current.channels[0].clone();
        Ok(true)
    }

    pub(super) fn cycle_robot_mode(
        &mut self,
        name: &str,
        delta: i32,
    ) -> Result<bool, Box<dyn Error>> {
        let Some((device_index, channel_index)) = self.selected_device_index(name) else {
            return Ok(false);
        };
        let updated_current = {
            let Some(available) = self.available_device_mut(name) else {
                return Ok(false);
            };
            let selectable = wizard_selectable_modes(&available.supported_modes);
            if selectable.is_empty() {
                return Ok(false);
            }
            let Some(ch) = available.current.channels.first_mut() else {
                return Ok(false);
            };
            if ch.kind != DeviceType::Robot {
                return Ok(false);
            }
            // Snap to the first selectable mode if the persisted mode (e.g.
            // a legacy `Disabled`/`Identifying`) isn't part of the cycle.
            let current_index = ch
                .mode
                .and_then(|mode| selectable.iter().position(|candidate| *candidate == mode))
                .unwrap_or(0);
            let next_index = rotate_index(current_index, selectable.len(), delta);
            ch.mode = Some(selectable[next_index]);
            available.current.clone()
        };
        self.config.devices[device_index].channels[channel_index] =
            updated_current.channels[0].clone();
        Ok(true)
    }

    pub(super) fn toggle_device_selection(&mut self, name: &str) -> Result<bool, Box<dyn Error>> {
        if let Some((device_index, channel_index)) = self.selected_device_index(name) {
            let enabled_channels = self.config.devices[device_index]
                .channels
                .iter()
                .filter(|channel| channel.enabled)
                .count();
            if enabled_channels <= 1 {
                self.config.devices.remove(device_index);
            } else {
                self.config.devices[device_index].channels[channel_index].enabled = false;
            }
            if self.identify_device_name.as_deref() == Some(name) {
                self.clear_identify_state();
            }
            self.prune_invalid_pairings();
            self.config.validate()?;
            return Ok(true);
        }

        let Some(available) = self
            .available_devices
            .iter()
            .find(|device| device.name == name)
            .cloned()
        else {
            return Ok(false);
        };
        if let Some((device_index, channel_index)) = self.configured_device_channel_index(name) {
            self.config.devices[device_index].channels[channel_index] =
                available.current.channels[0].clone();
            self.config.devices[device_index].channels[channel_index].enabled = true;
        } else if let Some(device) = self.build_selected_device_from_available(name) {
            self.config.devices.push(device);
        } else {
            return Ok(false);
        }
        self.prune_invalid_pairings();
        self.config.validate()?;
        Ok(true)
    }

    /// Flip whether `state_kind` appears in the addressed channel's
    /// `publish_states`. When turning a kind off, also drop it from
    /// `recorded_states` to preserve the subset invariant. Reject the
    /// toggle if any active pairing currently relies on the kind as
    /// `leader_state` (we can't quietly break a configured teleop pair).
    pub(super) fn toggle_publish_state(
        &mut self,
        name: &str,
        state_kind: RobotStateKind,
    ) -> Result<bool, Box<dyn Error>> {
        let Some((device_index, channel_index)) = self.selected_device_index(name) else {
            return Ok(false);
        };
        if self.config.devices[device_index].channels[channel_index].kind != DeviceType::Robot {
            return Ok(false);
        }
        let device_name = self.config.devices[device_index].name.clone();
        let channel_type = self.config.devices[device_index].channels[channel_index]
            .channel_type
            .clone();
        let supported_states: Vec<RobotStateKind> = self
            .available_devices
            .iter()
            .find(|available| available.name == name)
            .map(|available| available.supported_states.clone())
            .unwrap_or_default();

        let channel = &mut self.config.devices[device_index].channels[channel_index];
        let currently_enabled = channel.publish_states.contains(&state_kind);
        if currently_enabled {
            // Block removal if a pairing depends on this state as its
            // leader_state.
            if let Some(pairing) = self.config.pairings.iter().find(|pair| {
                pair.leader_device == device_name
                    && pair.leader_channel_type == channel_type
                    && pair.leader_state == state_kind
            }) {
                self.message = Some(format!(
                    "{:?} is required by pairing {}:{} (leader_state); change the pairing first.",
                    state_kind, pairing.leader_device, pairing.leader_channel_type
                ));
                return Ok(false);
            }
            channel.publish_states.retain(|kind| *kind != state_kind);
            channel.recorded_states.retain(|kind| *kind != state_kind);
        } else {
            // Refuse to toggle on a kind the driver doesn't advertise so the
            // wizard cannot publish unsupported topics.
            if !supported_states.is_empty() && !supported_states.contains(&state_kind) {
                self.message = Some(format!(
                    "{:?} is not advertised as supported by this device.",
                    state_kind
                ));
                return Ok(false);
            }
            channel.publish_states.push(state_kind);
        }
        self.config.validate()?;
        // Mirror the latest publish/recorded sets into the AvailableDevice
        // snapshot the wizard UI renders from. The wizard otherwise keeps
        // showing the stale glyphs because every other toggle in the
        // session writes through both `config.devices` and
        // `available_devices` (see `cycle_robot_mode`).
        self.sync_available_channel_state_lists(name, device_index, channel_index);
        // Pairing defaults can change once the publish set changes (e.g.
        // `parallel_position` becoming available enables parallel teleop).
        self.teleop_pairing_cache = self.config.pairings.clone();
        Ok(true)
    }

    /// Flip whether `state_kind` appears in the addressed channel's
    /// `recorded_states`. The validator already enforces
    /// `recorded_states ⊆ publish_states`; we surface a clearer message
    /// here when the operator tries to record a kind that isn't being
    /// published.
    pub(super) fn toggle_recorded_state(
        &mut self,
        name: &str,
        state_kind: RobotStateKind,
    ) -> Result<bool, Box<dyn Error>> {
        let Some((device_index, channel_index)) = self.selected_device_index(name) else {
            return Ok(false);
        };
        if self.config.devices[device_index].channels[channel_index].kind != DeviceType::Robot {
            return Ok(false);
        }
        let channel = &mut self.config.devices[device_index].channels[channel_index];
        let currently_enabled = channel.recorded_states.contains(&state_kind);
        if currently_enabled {
            channel.recorded_states.retain(|kind| *kind != state_kind);
        } else {
            if !channel.publish_states.contains(&state_kind) {
                self.message = Some(format!(
                    "{:?} must be published before it can be recorded.",
                    state_kind
                ));
                return Ok(false);
            }
            channel.recorded_states.push(state_kind);
        }
        self.config.validate()?;
        self.sync_available_channel_state_lists(name, device_index, channel_index);
        Ok(true)
    }

    /// Copy `publish_states` / `recorded_states` from
    /// `self.config.devices[device_index].channels[channel_index]` onto the
    /// matching `AvailableDevice.current` snapshot so the wizard UI sees
    /// the freshest values on the next state publish.
    pub(super) fn sync_available_channel_state_lists(
        &mut self,
        name: &str,
        device_index: usize,
        channel_index: usize,
    ) {
        let publish_states = self.config.devices[device_index].channels[channel_index]
            .publish_states
            .clone();
        let recorded_states = self.config.devices[device_index].channels[channel_index]
            .recorded_states
            .clone();
        let Some(available) = self.available_device_mut(name) else {
            return;
        };
        let Some(channel) = available.current.channels.first_mut() else {
            return;
        };
        channel.publish_states = publish_states;
        channel.recorded_states = recorded_states;
    }

    pub(super) fn build_selected_device_from_available(
        &self,
        name: &str,
    ) -> Option<BinaryDeviceConfig> {
        let target = self.available_device(name)?;
        let mut device = target.current.clone();
        let enabled_channel = device.channels.first()?.channel_type.clone();
        let mut channels = self
            .available_devices
            .iter()
            .filter(|available| available.driver == target.driver && available.id == target.id)
            .filter_map(|available| available.current.channels.first().cloned())
            .collect::<Vec<_>>();
        channels.sort_by(|left, right| left.channel_type.cmp(&right.channel_type));
        for channel in &mut channels {
            channel.enabled = channel.channel_type == enabled_channel;
        }
        device.channels = channels;
        Some(device)
    }
}

pub(super) fn select_supported_mode(supported_modes: &[RobotMode], preferred: RobotMode) -> RobotMode {
    // Prefer a wizard-selectable mode when available so freshly discovered
    // channels never default to `Identifying`/`Disabled` (those modes
    // exist for the identify flow / channel disable, not for steady-state
    // operation). Falls back to the first advertised mode, then
    // `FreeDrive`, when the driver doesn't list any selectable mode.
    let selectable = wizard_selectable_modes(supported_modes);
    if selectable.contains(&preferred) {
        preferred
    } else if let Some(first) = selectable.first().copied() {
        first
    } else if supported_modes.contains(&preferred) {
        preferred
    } else {
        supported_modes
            .first()
            .copied()
            .unwrap_or(RobotMode::FreeDrive)
    }
}

/// The subset of `RobotMode` values the setup wizard offers via the cycle
/// keys. `Identifying` is set transiently by the identify flow and
/// `Disabled` is set via channel disable / removal — neither is a steady
/// runtime mode the operator should pick from the cycle. Returned in a
/// fixed order (`FreeDrive` first, then `CommandFollowing`) so cycling is
/// predictable across drivers regardless of the order they list modes in.
pub(super) fn wizard_selectable_modes(supported_modes: &[RobotMode]) -> Vec<RobotMode> {
    [RobotMode::FreeDrive, RobotMode::CommandFollowing]
        .into_iter()
        .filter(|mode| supported_modes.contains(mode))
        .collect()
}

