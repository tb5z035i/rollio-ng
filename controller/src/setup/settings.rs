use super::state::{rotate_index, SetupSession, SetupStep};
use rollio_types::config::{CollectionMode, EpisodeFormat, StorageBackend, UiRuntimeConfig};
use std::error::Error;

/// Which single-character UI binding `set_ui_single_char_key` is editing.
#[derive(Debug, Clone, Copy)]
enum UiKeyField {
    Start,
    Stop,
    Keep,
    Discard,
}

impl UiKeyField {
    fn label(self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Stop => "Stop",
            Self::Keep => "Keep",
            Self::Discard => "Discard",
        }
    }

    fn slot_mut(self, ui: &mut UiRuntimeConfig) -> &mut String {
        match self {
            Self::Start => &mut ui.start_key,
            Self::Stop => &mut ui.stop_key,
            Self::Keep => &mut ui.keep_key,
            Self::Discard => &mut ui.discard_key,
        }
    }
}

impl SetupSession {
    pub(super) fn cycle_episode_format(&mut self, delta: i32) -> Result<bool, Box<dyn Error>> {
        let options = [
            EpisodeFormat::LeRobotV2_1,
            EpisodeFormat::LeRobotV3_0,
            EpisodeFormat::Mcap,
        ];
        let current_index = options
            .iter()
            .position(|format| *format == self.config.episode.format)
            .unwrap_or(0);
        let next_index = rotate_index(current_index, options.len(), delta);
        self.config.episode.format = options[next_index];
        self.config.validate()?;
        Ok(true)
    }

    pub(super) fn cycle_storage_backend(&mut self, delta: i32) -> Result<bool, Box<dyn Error>> {
        let options = [
            StorageBackend::Local,
            StorageBackend::Http,
            StorageBackend::Dataloop,
        ];
        let current_index = options
            .iter()
            .position(|backend| *backend == self.config.storage.backend)
            .unwrap_or(0);
        let next_index = rotate_index(current_index, options.len(), delta);
        self.config.storage.backend = options[next_index];
        match self.config.storage.backend {
            StorageBackend::Local => {
                self.config.storage.endpoint = None;
                if self
                    .config
                    .storage
                    .output_path
                    .as_deref()
                    .is_none_or(|path| path.trim().is_empty())
                {
                    self.config.storage.output_path = Some("./output".into());
                }
            }
            StorageBackend::Http => {
                if self.config.storage.endpoint.is_none() {
                    self.config.storage.endpoint = Some("http://127.0.0.1:8080/upload".into());
                }
            }
            StorageBackend::Dataloop => {
                if self
                    .config
                    .storage
                    .endpoint
                    .as_deref()
                    .is_none_or(|v| v.trim().is_empty() || v.contains("/upload"))
                {
                    self.config.storage.endpoint = Some("http://127.0.0.1/".into());
                }
                if self
                    .config
                    .storage
                    .dataloop_project_id
                    .as_deref()
                    .is_none_or(|v| v.trim().is_empty())
                {
                    self.config.storage.dataloop_project_id = Some("1".into());
                }
            }
        }
        self.config.validate()?;
        Ok(true)
    }

    pub(super) fn cycle_collection_mode(&mut self, _delta: i32) -> Result<bool, Box<dyn Error>> {
        // The wizard now exposes only `Teleop`. Pin the value here so any
        // legacy `Intervention` config that lands in the session (e.g.
        // resumed from an older save) is normalized on the first cycle
        // attempt. Otherwise this is a no-op cycle.
        if self.config.mode == CollectionMode::Teleop {
            return Ok(false);
        }
        self.config.mode = CollectionMode::Teleop;
        self.ensure_visible_current_step();
        self.config.validate()?;
        Ok(true)
    }

    pub(super) fn set_project_name(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.message = Some("Project name must not be empty.".into());
            return Ok(false);
        }
        if self.config.project_name == trimmed {
            return Ok(false);
        }
        self.config.project_name = trimmed.into();
        self.config.validate()?;
        Ok(true)
    }

    pub(super) fn set_storage_output_path(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.message = Some("Local storage output path must not be empty.".into());
            return Ok(false);
        }
        if self.config.storage.output_path.as_deref() == Some(trimmed) {
            return Ok(false);
        }
        self.config.storage.output_path = Some(trimmed.into());
        self.config.validate()?;
        Ok(true)
    }

    pub(super) fn set_storage_endpoint(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.message = Some("Storage endpoint must not be empty.".into());
            return Ok(false);
        }
        if self.config.storage.endpoint.as_deref() == Some(trimmed) {
            return Ok(false);
        }
        self.config.storage.endpoint = Some(trimmed.into());
        self.config.validate()?;
        Ok(true)
    }

    pub(super) fn set_dataloop_project_id(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.message = Some("Dataloop project ID must not be empty.".into());
            return Ok(false);
        }
        if self.config.storage.dataloop_project_id.as_deref() == Some(trimmed) {
            return Ok(false);
        }
        self.config.storage.dataloop_project_id = Some(trimmed.into());
        self.config.validate()?;
        Ok(true)
    }

    pub(super) fn set_dataloop_token(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.message = Some("Dataloop token must not be empty.".into());
            return Ok(false);
        }
        if self.config.storage.dataloop_token.as_deref() == Some(trimmed) {
            return Ok(false);
        }
        self.config.storage.dataloop_token = Some(trimmed.into());
        self.config.validate()?;
        Ok(true)
    }

    /// Update the host `rollio-web-gateway` should bind to. Mutating the
    /// field through the wizard avoids forcing the operator to hand-edit
    /// the saved TOML when they need to expose the UI on a different
    /// interface (e.g. switching the default `0.0.0.0` to `127.0.0.1` for
    /// loopback-only access).
    pub(super) fn set_ui_http_host(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.message = Some("UI host must not be empty.".into());
            return Ok(false);
        }
        if self.config.ui.http_host == trimmed {
            return Ok(false);
        }
        let previous = std::mem::replace(&mut self.config.ui.http_host, trimmed.into());
        if let Err(error) = self.config.validate() {
            self.config.ui.http_host = previous;
            self.message = Some(format!("UI host rejected: {error}"));
            return Ok(false);
        }
        Ok(true)
    }

    pub(super) fn set_episode_fps(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.message = Some("Episode fps must not be empty.".into());
            return Ok(false);
        }
        let fps: u32 = match trimmed.parse() {
            Ok(v) => v,
            Err(_) => {
                self.message = Some(format!(
                    "Episode fps must be an integer (1..1000), got {trimmed:?}."
                ));
                return Ok(false);
            }
        };
        if fps == 0 || fps > 1000 {
            self.message = Some(format!("Episode fps must be 1..1000, got {fps}."));
            return Ok(false);
        }
        if self.config.episode.fps == fps {
            return Ok(false);
        }
        self.config.episode.fps = fps;
        if let Err(error) = self.config.validate() {
            return Err(error.into());
        }
        Ok(true)
    }

    pub(super) fn set_episode_chunk_size(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        let parsed: u32 = match trimmed.parse() {
            Ok(v) if v > 0 => v,
            _ => {
                self.message = Some(format!(
                    "Episode chunk_size must be a positive integer, got {trimmed:?}."
                ));
                return Ok(false);
            }
        };
        if self.config.episode.chunk_size == parsed {
            return Ok(false);
        }
        self.config.episode.chunk_size = parsed;
        self.config.validate()?;
        Ok(true)
    }

    pub(super) fn set_controller_shutdown_timeout_ms(
        &mut self,
        value: &str,
    ) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        let parsed: u64 = match trimmed.parse() {
            Ok(v) => v,
            Err(_) => {
                self.message = Some(format!(
                    "Controller shutdown_timeout_ms must be a non-negative integer, got {trimmed:?}."
                ));
                return Ok(false);
            }
        };
        if self.config.controller.shutdown_timeout_ms == parsed {
            return Ok(false);
        }
        self.config.controller.shutdown_timeout_ms = parsed;
        self.config.validate()?;
        Ok(true)
    }

    pub(super) fn set_controller_child_poll_interval_ms(
        &mut self,
        value: &str,
    ) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        let parsed: u64 = match trimmed.parse() {
            Ok(v) if v > 0 => v,
            _ => {
                self.message = Some(format!(
                    "Controller child_poll_interval_ms must be a positive integer, got {trimmed:?}."
                ));
                return Ok(false);
            }
        };
        if self.config.controller.child_poll_interval_ms == parsed {
            return Ok(false);
        }
        self.config.controller.child_poll_interval_ms = parsed;
        self.config.validate()?;
        Ok(true)
    }

    pub(super) fn set_visualizer_port(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        let parsed: u16 = match trimmed.parse() {
            Ok(v) if v > 0 => v,
            _ => {
                self.message = Some(format!(
                    "Visualizer port must be a positive integer (1..65535), got {trimmed:?}."
                ));
                return Ok(false);
            }
        };
        if self.config.visualizer.port == parsed {
            return Ok(false);
        }
        self.config.visualizer.port = parsed;
        self.config.validate()?;
        Ok(true)
    }

    pub(super) fn set_ui_http_port(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        let parsed: u16 = match trimmed.parse() {
            Ok(v) if v > 0 => v,
            _ => {
                self.message = Some(format!(
                    "UI http_port must be a positive integer (1..65535), got {trimmed:?}."
                ));
                return Ok(false);
            }
        };
        if self.config.ui.http_port == parsed {
            return Ok(false);
        }
        let previous = std::mem::replace(&mut self.config.ui.http_port, parsed);
        if let Err(error) = self.config.validate() {
            self.config.ui.http_port = previous;
            self.message = Some(format!("UI http_port rejected: {error}"));
            return Ok(false);
        }
        Ok(true)
    }

    /// Mutate a single-char UI keybinding. Each of start/stop/keep/discard
    /// must be one character, must not collide with the other three, and
    /// must not collide with the reserved shortcuts the wizard hard-codes
    /// (`d`, `r`). Validation runs through `ProjectConfig::validate` which
    /// already enforces those constraints when the config is reloaded.
    fn set_ui_single_char_key(
        &mut self,
        which: UiKeyField,
        value: &str,
    ) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        if trimmed.chars().count() != 1 {
            self.message = Some(format!(
                "{} key must be a single character, got {trimmed:?}.",
                which.label()
            ));
            return Ok(false);
        }
        let new_key = trimmed.to_owned();
        let slot = which.slot_mut(&mut self.config.ui);
        if *slot == new_key {
            return Ok(false);
        }
        let previous = std::mem::replace(slot, new_key);
        if let Err(error) = self.config.validate() {
            *which.slot_mut(&mut self.config.ui) = previous;
            self.message = Some(format!("{} key rejected: {error}", which.label()));
            return Ok(false);
        }
        Ok(true)
    }

    pub(super) fn set_ui_start_key(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        self.set_ui_single_char_key(UiKeyField::Start, value)
    }

    pub(super) fn set_ui_stop_key(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        self.set_ui_single_char_key(UiKeyField::Stop, value)
    }

    pub(super) fn set_ui_keep_key(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        self.set_ui_single_char_key(UiKeyField::Keep, value)
    }

    pub(super) fn set_ui_discard_key(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        self.set_ui_single_char_key(UiKeyField::Discard, value)
    }

    pub(super) fn set_assembler_missing_eos_timeout_ms(
        &mut self,
        value: &str,
    ) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        let parsed: u64 = match trimmed.parse() {
            Ok(v) => v,
            Err(_) => {
                self.message = Some(format!(
                    "Assembler missing_eos_timeout_ms must be a non-negative integer, got {trimmed:?}."
                ));
                return Ok(false);
            }
        };
        if self.config.assembler.missing_eos_timeout_ms == parsed {
            return Ok(false);
        }
        self.config.assembler.missing_eos_timeout_ms = parsed;
        self.config.validate()?;
        Ok(true)
    }

    pub(super) fn set_assembler_staging_dir(
        &mut self,
        value: &str,
    ) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.message = Some("Assembler staging_dir must not be empty.".into());
            return Ok(false);
        }
        if self.config.assembler.staging_dir == trimmed {
            return Ok(false);
        }
        self.config.assembler.staging_dir = trimmed.into();
        self.config.validate()?;
        Ok(true)
    }

    pub(super) fn set_assembler_staging_slots(
        &mut self,
        value: &str,
    ) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        let parsed: u32 = match trimmed.parse() {
            Ok(v) if (1..=64).contains(&v) => v,
            _ => {
                self.message = Some(format!(
                    "Assembler staging_slots must be 1..=64, got {trimmed:?}."
                ));
                return Ok(false);
            }
        };
        if self.config.assembler.staging_slots == parsed {
            return Ok(false);
        }
        let previous = self.config.assembler.staging_slots;
        self.config.assembler.staging_slots = parsed;
        if let Err(error) = self.config.validate() {
            self.config.assembler.staging_slots = previous;
            self.message = Some(format!("staging_slots rejected: {error}"));
            return Ok(false);
        }
        Ok(true)
    }

    pub(super) fn set_storage_queue_size(&mut self, value: &str) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        let parsed: u32 = match trimmed.parse() {
            Ok(v) if v > 0 => v,
            _ => {
                self.message = Some(format!(
                    "Storage queue_size must be a positive integer, got {trimmed:?}."
                ));
                return Ok(false);
            }
        };
        if self.config.storage.queue_size == parsed {
            return Ok(false);
        }
        self.config.storage.queue_size = parsed;
        self.config.validate()?;
        Ok(true)
    }

    pub(super) fn set_monitor_metrics_frequency_hz(
        &mut self,
        value: &str,
    ) -> Result<bool, Box<dyn Error>> {
        let trimmed = value.trim();
        let parsed: f64 = match trimmed.parse::<f64>() {
            Ok(v) if v.is_finite() && v > 0.0 => v,
            _ => {
                self.message = Some(format!(
                    "Monitor metrics_frequency_hz must be a positive finite number, got {trimmed:?}."
                ));
                return Ok(false);
            }
        };
        if (self.config.monitor.metrics_frequency_hz - parsed).abs() < f64::EPSILON {
            return Ok(false);
        }
        self.config.monitor.metrics_frequency_hz = parsed;
        self.config.validate()?;
        Ok(true)
    }

    pub(super) fn jump_to_step(&mut self, value: &str) -> bool {
        let target = match value {
            "devices" | "discovery" | "selection" | "parameters" => SetupStep::Devices,
            "states" => SetupStep::States,
            "storage" => SetupStep::Storage,
            "pairing" => SetupStep::Pairing,
            "preview" => SetupStep::Preview,
            _ => return false,
        };
        if !self.visible_steps().contains(&target) {
            return false;
        }
        let changed = self.current_step != target;
        self.current_step = target;
        if self.current_step != SetupStep::Devices {
            self.clear_identify_state();
            self.subpanel_target_name = None;
        }
        changed
    }
}
