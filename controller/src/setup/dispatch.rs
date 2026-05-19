use super::pairings::PairingEndpoint;
use super::save::save_project_config;
use super::state::{SessionMutation, SetupCommandEnvelope, SetupSession, TeleopPairCreate};
use rollio_types::config::{MappingStrategy, RobotStateKind};
use std::error::Error;

impl SetupSession {
    /// Helper: dispatch a text-input command. Pulls `command.value`,
    /// returns `SessionMutation::default()` when absent (the wizard
    /// must send a value for every text field), otherwise delegates to
    /// the field-specific setter and wraps the bool result in
    /// `SessionMutation::config_changed`.
    #[allow(clippy::type_complexity)]
    fn set_text_field(
        &mut self,
        value: Option<&str>,
        setter: fn(&mut Self, &str) -> Result<bool, Box<dyn Error>>,
    ) -> Result<SessionMutation, Box<dyn Error>> {
        let Some(value) = value else {
            return Ok(SessionMutation::default());
        };
        Ok(SessionMutation::config_changed(setter(self, value)?))
    }
}

impl SetupSession {
    pub(super) fn apply_raw_command(
        &mut self,
        raw_json: &str,
    ) -> Result<SessionMutation, Box<dyn Error>> {
        let command: SetupCommandEnvelope = serde_json::from_str(raw_json)?;
        if command.msg_type != "command" {
            return Ok(SessionMutation::default());
        }
        let delta = normalized_delta(command.delta);
        match command.action.as_str() {
            "setup_get_state" => Ok(SessionMutation::state_only(true)),
            "setup_prev_step" => Ok(SessionMutation::step_changed(self.retreat_step())),
            "setup_next_step" => Ok(SessionMutation::step_changed(self.advance_step())),
            "setup_jump_step" => {
                let Some(value) = command.value.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::step_changed(self.jump_to_step(value)))
            }
            "setup_toggle_device" => {
                let Some(name) = command.name.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.toggle_device_selection(name)?,
                ))
            }
            "setup_set_device_name" => {
                let (Some(name), Some(value)) = (command.name.as_deref(), command.value.as_deref())
                else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.set_device_name(name, value)?,
                ))
            }
            "setup_open_subpanel" => {
                let Some(name) = command.name.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::state_only(self.open_subpanel(name)))
            }
            "setup_close_subpanel" => Ok(SessionMutation::state_only(self.close_subpanel())),
            "setup_subpanel_toggle_preview_enabled" => {
                let Some(name) = command.name.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.subpanel_toggle_preview_enabled(name)?,
                ))
            }
            "setup_subpanel_toggle_record_enabled" => {
                let Some(name) = command.name.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.subpanel_toggle_record_enabled(name)?,
                ))
            }
            "setup_subpanel_cycle_primary" => {
                let Some(name) = command.name.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.subpanel_cycle_primary(name, delta)?,
                ))
            }
            "setup_subpanel_cycle_record_field" => {
                let (Some(name), Some(field)) = (command.name.as_deref(), command.field.as_deref())
                else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.subpanel_cycle_record_field(name, field, delta)?,
                ))
            }
            "setup_subpanel_set_record_field" => {
                let (Some(name), Some(field), Some(value)) = (
                    command.name.as_deref(),
                    command.field.as_deref(),
                    command.value.as_deref(),
                ) else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.subpanel_set_record_field(name, field, value)?,
                ))
            }
            "setup_subpanel_cycle_preview_field" => {
                let (Some(name), Some(field)) = (command.name.as_deref(), command.field.as_deref())
                else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.subpanel_cycle_preview_field(name, field, delta)?,
                ))
            }
            "setup_subpanel_set_preview_field" => {
                let (Some(name), Some(field), Some(value)) = (
                    command.name.as_deref(),
                    command.field.as_deref(),
                    command.value.as_deref(),
                ) else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.subpanel_set_preview_field(name, field, value)?,
                ))
            }
            "setup_subpanel_set_control_frequency_hz" => {
                let (Some(name), Some(value)) = (command.name.as_deref(), command.value.as_deref())
                else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.subpanel_set_control_frequency_hz(name, value)?,
                ))
            }
            "setup_open_add_picker" => Ok(SessionMutation::state_only(self.open_add_picker())),
            "setup_add_pseudo_camera" => {
                Ok(SessionMutation::config_changed(self.add_pseudo_camera()?))
            }
            "setup_add_pseudo_robot" => {
                Ok(SessionMutation::config_changed(self.add_pseudo_robot()?))
            }
            "setup_add_command_device" => {
                Ok(SessionMutation::config_changed(self.add_command_device()?))
            }
            "setup_toggle_identify" => {
                let Some(name) = command.name.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                if self.identify_device_name.as_deref() != Some(name)
                    && !self.is_device_selected(name)
                {
                    return Ok(SessionMutation::default());
                }
                let target = if self.identify_device_name.as_deref() == Some(name) {
                    None
                } else {
                    Some(name)
                };
                Ok(SessionMutation::state_only(
                    self.set_identify_device(target),
                ))
            }
            "setup_cycle_camera_profile" => {
                let Some(name) = command.name.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.cycle_device_profile(name, delta)?,
                ))
            }
            "setup_cycle_robot_mode" => {
                let Some(name) = command.name.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.cycle_robot_mode(name, delta)?,
                ))
            }
            "setup_cycle_pair_mapping" => {
                let Some(index) = command.index else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.cycle_pair_mapping(index, delta)?,
                ))
            }
            "setup_create_pairing" => {
                // Optional `value` carries the operator's leader+follower
                // pick from the modal picker, encoded as
                // `"<leader_device>|<leader_channel_type>;<follower_device>|<follower_channel_type>"`.
                // When absent, the controller falls back to auto-seeding.
                let explicit = command
                    .value
                    .as_deref()
                    .and_then(parse_create_pairing_value);
                Ok(SessionMutation::config_changed(
                    self.create_pairing(explicit)?.is_some(),
                ))
            }
            "setup_remove_pairing" => {
                let Some(index) = command.index else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(self.remove_pairing(index)?))
            }
            "setup_set_pairing_leader" | "setup_set_pairing_follower" => {
                let (Some(index), Some(value)) = (command.index, command.value.as_deref()) else {
                    return Ok(SessionMutation::default());
                };
                let Some((device, channel_type)) = value.split_once('|') else {
                    return Ok(SessionMutation::default());
                };
                let endpoint = if command.action == "setup_set_pairing_leader" {
                    PairingEndpoint::Leader
                } else {
                    PairingEndpoint::Follower
                };
                Ok(SessionMutation::config_changed(self.set_pairing_endpoint(
                    index,
                    endpoint,
                    device,
                    channel_type,
                )?))
            }
            "setup_set_pairing_ratio" => {
                let (Some(index), Some(value)) = (command.index, command.value.as_deref()) else {
                    return Ok(SessionMutation::default());
                };
                let Ok(ratio) = value.parse::<f64>() else {
                    self.message = Some(format!(
                        "Could not parse parallel ratio \"{value}\": expected a finite, non-zero number."
                    ));
                    return Ok(SessionMutation::state_only(true));
                };
                Ok(SessionMutation::config_changed(
                    self.set_pairing_ratio(index, ratio)?,
                ))
            }
            "setup_toggle_publish_state" => {
                let (Some(name), Some(value)) = (command.name.as_deref(), command.value.as_deref())
                else {
                    return Ok(SessionMutation::default());
                };
                let Some(state_kind) = parse_robot_state_kind(value) else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.toggle_publish_state(name, state_kind)?,
                ))
            }
            "setup_toggle_recorded_state" => {
                let (Some(name), Some(value)) = (command.name.as_deref(), command.value.as_deref())
                else {
                    return Ok(SessionMutation::default());
                };
                let Some(state_kind) = parse_robot_state_kind(value) else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.toggle_recorded_state(name, state_kind)?,
                ))
            }
            "setup_cycle_episode_format" => Ok(SessionMutation::config_changed(
                self.cycle_episode_format(delta)?,
            )),
            "setup_cycle_storage_backend" => Ok(SessionMutation::config_changed(
                self.cycle_storage_backend(delta)?,
            )),
            "setup_cycle_collection_mode" => Ok(SessionMutation::config_changed(
                self.cycle_collection_mode(delta)?,
            )),
            "setup_set_project_name" => {
                let Some(value) = command.value.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.set_project_name(value)?,
                ))
            }
            "setup_set_storage_output_path" => {
                let Some(value) = command.value.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.set_storage_output_path(value)?,
                ))
            }
            "setup_set_storage_endpoint" => {
                let Some(value) = command.value.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.set_storage_endpoint(value)?,
                ))
            }
            "setup_set_dataloop_project_id" => {
                let Some(value) = command.value.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.set_dataloop_project_id(value)?,
                ))
            }
            "setup_set_dataloop_token" => {
                let Some(value) = command.value.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.set_dataloop_token(value)?,
                ))
            }
            "setup_set_ui_http_host" => {
                let Some(value) = command.value.as_deref() else {
                    return Ok(SessionMutation::default());
                };
                Ok(SessionMutation::config_changed(
                    self.set_ui_http_host(value)?,
                ))
            }
            "setup_set_episode_fps" => {
                self.set_text_field(command.value.as_deref(), Self::set_episode_fps)
            }
            "setup_set_episode_chunk_size" => {
                self.set_text_field(command.value.as_deref(), Self::set_episode_chunk_size)
            }
            "setup_set_controller_shutdown_timeout_ms" => self.set_text_field(
                command.value.as_deref(),
                Self::set_controller_shutdown_timeout_ms,
            ),
            "setup_set_controller_child_poll_interval_ms" => self.set_text_field(
                command.value.as_deref(),
                Self::set_controller_child_poll_interval_ms,
            ),
            "setup_set_visualizer_port" => {
                self.set_text_field(command.value.as_deref(), Self::set_visualizer_port)
            }
            "setup_set_ui_http_port" => {
                self.set_text_field(command.value.as_deref(), Self::set_ui_http_port)
            }
            "setup_set_ui_start_key" => {
                self.set_text_field(command.value.as_deref(), Self::set_ui_start_key)
            }
            "setup_set_ui_stop_key" => {
                self.set_text_field(command.value.as_deref(), Self::set_ui_stop_key)
            }
            "setup_set_ui_keep_key" => {
                self.set_text_field(command.value.as_deref(), Self::set_ui_keep_key)
            }
            "setup_set_ui_discard_key" => {
                self.set_text_field(command.value.as_deref(), Self::set_ui_discard_key)
            }
            "setup_set_assembler_missing_eos_timeout_ms" => self.set_text_field(
                command.value.as_deref(),
                Self::set_assembler_missing_eos_timeout_ms,
            ),
            "setup_set_assembler_staging_dir" => {
                self.set_text_field(command.value.as_deref(), Self::set_assembler_staging_dir)
            }
            "setup_set_assembler_staging_slots" => {
                self.set_text_field(command.value.as_deref(), Self::set_assembler_staging_slots)
            }
            "setup_set_storage_queue_size" => {
                self.set_text_field(command.value.as_deref(), Self::set_storage_queue_size)
            }
            "setup_set_monitor_metrics_frequency_hz" => self.set_text_field(
                command.value.as_deref(),
                Self::set_monitor_metrics_frequency_hz,
            ),
            "setup_toggle_advanced_pipeline_logs" => Ok(SessionMutation::config_changed(
                self.toggle_advanced_pipeline_logs()?,
            )),
            "setup_save" => {
                save_project_config(&self.config, &self.output_path)?;
                self.mark_saved();
                Ok(SessionMutation::state_only(true))
            }
            "setup_cancel" => {
                self.mark_cancelled();
                Ok(SessionMutation::state_only(true))
            }
            _ => Ok(SessionMutation::default()),
        }
    }
}

/// Parse the wire-format pair-create payload sent by the wizard when
/// the operator confirms a new pair in the picker:
/// `"<policy>;<leader_device>|<leader_channel_type>;<follower_device>|<follower_channel_type>[;ratio=<f64>]"`
///
/// `<policy>` is one of `direct-joint`, `cartesian`, `parallel`. The
/// optional `ratio=<f64>` segment is only meaningful for `parallel` and
/// carries the operator's mapping ratio (default `1.0` when omitted).
/// Returns `None` if any required segment is missing or malformed; the
/// caller then falls back to auto-seeding.
pub(super) fn parse_create_pairing_value(value: &str) -> Option<TeleopPairCreate> {
    let mut parts = value.split(';');
    let policy_str = parts.next()?;
    let leader_part = parts.next()?;
    let follower_part = parts.next()?;
    let policy = match policy_str {
        "direct-joint" => MappingStrategy::DirectJoint,
        "cartesian" => MappingStrategy::Cartesian,
        "parallel" => MappingStrategy::Parallel,
        _ => return None,
    };
    let (leader_device, leader_channel_type) = leader_part.split_once('|')?;
    let (follower_device, follower_channel_type) = follower_part.split_once('|')?;
    if leader_device.is_empty()
        || leader_channel_type.is_empty()
        || follower_device.is_empty()
        || follower_channel_type.is_empty()
    {
        return None;
    }
    let mut ratio: Option<f64> = None;
    for tail in parts {
        if let Some(rest) = tail.strip_prefix("ratio=") {
            ratio = rest
                .parse::<f64>()
                .ok()
                .filter(|r| r.is_finite() && *r != 0.0);
        }
    }
    Some(TeleopPairCreate {
        policy,
        leader: (leader_device.to_owned(), leader_channel_type.to_owned()),
        follower: (follower_device.to_owned(), follower_channel_type.to_owned()),
        ratio,
    })
}

pub(super) fn parse_robot_state_kind(value: &str) -> Option<RobotStateKind> {
    serde_json::from_value(serde_json::Value::String(value.to_owned())).ok()
}

pub(super) fn normalized_delta(delta: Option<i32>) -> i32 {
    match delta.unwrap_or(1).cmp(&0) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 1,
        std::cmp::Ordering::Greater => 1,
    }
}
