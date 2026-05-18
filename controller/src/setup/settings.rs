use super::state::{rotate_index, SetupSession, SetupStep};
use rollio_types::config::{CollectionMode, EpisodeFormat, StorageBackend};
use std::error::Error;

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
        let options = [StorageBackend::Local, StorageBackend::Http];
        let current_index = options
            .iter()
            .position(|backend| *backend == self.config.storage.backend)
            .unwrap_or(0);
        let next_index = rotate_index(current_index, options.len(), delta);
        self.config.storage.backend = options[next_index];
        if matches!(self.config.storage.backend, StorageBackend::Local) {
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
        } else if self.config.storage.endpoint.is_none() {
            self.config.storage.endpoint = Some("http://127.0.0.1:8080/upload".into());
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
            self.message = Some("HTTP storage endpoint must not be empty.".into());
            return Ok(false);
        }
        if self.config.storage.endpoint.as_deref() == Some(trimmed) {
            return Ok(false);
        }
        self.config.storage.endpoint = Some(trimmed.into());
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
        }
        changed
    }
}
