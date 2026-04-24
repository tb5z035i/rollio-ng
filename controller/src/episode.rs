use rollio_types::messages::{ControlEvent, EpisodeCommand, EpisodeState, EpisodeStatus};
use std::collections::BTreeSet;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct EpisodeLifecycle {
    state: EpisodeState,
    episode_count: u32,
    next_episode_index: u32,
    recording_started_at: Option<Instant>,
    pending_elapsed_ms: u64,
    stored_episode_indices: BTreeSet<u32>,
}

impl Default for EpisodeLifecycle {
    fn default() -> Self {
        Self {
            state: EpisodeState::Idle,
            episode_count: 0,
            next_episode_index: 0,
            recording_started_at: None,
            pending_elapsed_ms: 0,
            stored_episode_indices: BTreeSet::new(),
        }
    }
}

impl EpisodeLifecycle {
    pub fn state(&self) -> EpisodeState {
        self.state
    }

    pub fn status(&self, now: Instant) -> EpisodeStatus {
        EpisodeStatus {
            state: self.state,
            episode_count: self.episode_count,
            elapsed_ms: self.elapsed_ms(now),
        }
    }

    /// After probing an on-disk dataset, align the in-memory counters so new
    /// recordings continue past the last stored episode instead of restarting
    /// at index 0 (which would overwrite merged LeRobot shards).
    pub fn resume_from_prior_recordings(
        &mut self,
        next_episode_index: u32,
        prior_stored_episode_count: u32,
    ) {
        self.next_episode_index = next_episode_index;
        self.episode_count = prior_stored_episode_count;
    }

    pub fn record_episode_stored(&mut self, episode_index: u32) -> bool {
        if episode_index >= self.next_episode_index {
            return false;
        }
        if !self.stored_episode_indices.insert(episode_index) {
            return false;
        }
        self.episode_count = self.episode_count.saturating_add(1);
        true
    }

    pub fn handle_command(
        &mut self,
        command: EpisodeCommand,
        now: Instant,
    ) -> Result<ControlEvent, String> {
        match (self.state, command) {
            (EpisodeState::Idle, EpisodeCommand::Start) => {
                self.state = EpisodeState::Recording;
                self.recording_started_at = Some(now);
                self.pending_elapsed_ms = 0;
                Ok(ControlEvent::RecordingStart {
                    episode_index: self.next_episode_index,
                    controller_ts_us: controller_now_us(),
                })
            }
            (EpisodeState::Recording, EpisodeCommand::Stop) => {
                self.state = EpisodeState::Pending;
                self.pending_elapsed_ms = self.elapsed_ms(now);
                self.recording_started_at = None;
                Ok(ControlEvent::RecordingStop {
                    episode_index: self.next_episode_index,
                    controller_ts_us: controller_now_us(),
                })
            }
            (EpisodeState::Pending, EpisodeCommand::Keep) => {
                self.state = EpisodeState::Idle;
                let episode_index = self.next_episode_index;
                self.next_episode_index = self.next_episode_index.saturating_add(1);
                self.pending_elapsed_ms = 0;
                Ok(ControlEvent::EpisodeKeep { episode_index })
            }
            (EpisodeState::Pending, EpisodeCommand::Discard) => {
                self.state = EpisodeState::Idle;
                let episode_index = self.next_episode_index;
                self.next_episode_index = self.next_episode_index.saturating_add(1);
                self.pending_elapsed_ms = 0;
                Ok(ControlEvent::EpisodeDiscard { episode_index })
            }
            (state, command) => Err(format!(
                "invalid episode transition: state={} command={}",
                state.as_str(),
                command.as_str()
            )),
        }
    }

    fn elapsed_ms(&self, now: Instant) -> u64 {
        match self.state {
            EpisodeState::Idle => 0,
            EpisodeState::Recording => self
                .recording_started_at
                .map(|started_at| now.saturating_duration_since(started_at).as_millis() as u64)
                .unwrap_or(0),
            EpisodeState::Pending => self.pending_elapsed_ms,
        }
    }
}

/// Returns the controller's wall-clock timestamp in UNIX-epoch microseconds.
///
/// Used as the `controller_ts_us` anchor on `ControlEvent::RecordingStart` /
/// `RecordingStop` so every downstream subscriber (encoder, episode-assembler)
/// references the *same* moment instead of stamping their own clock on
/// receipt.
fn controller_now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn idle_start_enters_recording() {
        let now = Instant::now();
        let mut lifecycle = EpisodeLifecycle::default();
        let event = lifecycle
            .handle_command(EpisodeCommand::Start, now)
            .expect("start should be valid");
        assert_eq!(lifecycle.state(), EpisodeState::Recording);
        assert!(matches!(
            event,
            ControlEvent::RecordingStart {
                episode_index: 0,
                controller_ts_us
            } if controller_ts_us > 0
        ));
    }

    #[test]
    fn recording_stop_enters_pending() {
        let now = Instant::now();
        let mut lifecycle = EpisodeLifecycle::default();
        lifecycle
            .handle_command(EpisodeCommand::Start, now)
            .expect("start should be valid");
        let event = lifecycle
            .handle_command(EpisodeCommand::Stop, now + Duration::from_secs(2))
            .expect("stop should be valid");
        assert_eq!(lifecycle.state(), EpisodeState::Pending);
        assert!(matches!(
            event,
            ControlEvent::RecordingStop {
                episode_index: 0,
                controller_ts_us
            } if controller_ts_us > 0
        ));
        assert_eq!(lifecycle.status(now).state, EpisodeState::Pending);
    }

    #[test]
    fn pending_keep_increments_episode_count() {
        let now = Instant::now();
        let mut lifecycle = EpisodeLifecycle::default();
        lifecycle
            .handle_command(EpisodeCommand::Start, now)
            .expect("start should be valid");
        lifecycle
            .handle_command(EpisodeCommand::Stop, now + Duration::from_millis(50))
            .expect("stop should be valid");
        let event = lifecycle
            .handle_command(EpisodeCommand::Keep, now + Duration::from_millis(75))
            .expect("keep should be valid");
        assert_eq!(lifecycle.state(), EpisodeState::Idle);
        assert_eq!(lifecycle.episode_count, 0);
        assert_eq!(event, ControlEvent::EpisodeKeep { episode_index: 0 });
        assert!(lifecycle.record_episode_stored(0));
        assert_eq!(lifecycle.episode_count, 1);
    }

    #[test]
    fn pending_discard_does_not_increment_episode_count() {
        let now = Instant::now();
        let mut lifecycle = EpisodeLifecycle::default();
        lifecycle
            .handle_command(EpisodeCommand::Start, now)
            .expect("start should be valid");
        lifecycle
            .handle_command(EpisodeCommand::Stop, now + Duration::from_millis(50))
            .expect("stop should be valid");
        let event = lifecycle
            .handle_command(EpisodeCommand::Discard, now + Duration::from_millis(75))
            .expect("discard should be valid");
        assert_eq!(lifecycle.state(), EpisodeState::Idle);
        assert_eq!(lifecycle.episode_count, 0);
        assert_eq!(event, ControlEvent::EpisodeDiscard { episode_index: 0 });
    }

    #[test]
    fn invalid_transitions_leave_state_unchanged() {
        let now = Instant::now();
        let mut lifecycle = EpisodeLifecycle::default();
        assert!(lifecycle.handle_command(EpisodeCommand::Stop, now).is_err());
        assert_eq!(lifecycle.state(), EpisodeState::Idle);

        lifecycle
            .handle_command(EpisodeCommand::Start, now)
            .expect("start should be valid");
        assert!(lifecycle
            .handle_command(EpisodeCommand::Keep, now + Duration::from_millis(10))
            .is_err());
        assert_eq!(lifecycle.state(), EpisodeState::Recording);

        lifecycle
            .handle_command(EpisodeCommand::Stop, now + Duration::from_millis(20))
            .expect("stop should be valid");
        assert!(lifecycle
            .handle_command(EpisodeCommand::Start, now + Duration::from_millis(30))
            .is_err());
        assert_eq!(lifecycle.state(), EpisodeState::Pending);
    }

    #[test]
    fn rapid_transition_sequence_counts_only_kept_episode() {
        let start = Instant::now();
        let mut lifecycle = EpisodeLifecycle::default();
        lifecycle
            .handle_command(EpisodeCommand::Start, start)
            .expect("start should be valid");
        lifecycle
            .handle_command(EpisodeCommand::Stop, start + Duration::from_millis(20))
            .expect("stop should be valid");
        lifecycle
            .handle_command(EpisodeCommand::Keep, start + Duration::from_millis(30))
            .expect("keep should be valid");
        lifecycle
            .handle_command(EpisodeCommand::Start, start + Duration::from_millis(40))
            .expect("start should be valid");
        lifecycle
            .handle_command(EpisodeCommand::Stop, start + Duration::from_millis(50))
            .expect("stop should be valid");
        lifecycle
            .handle_command(EpisodeCommand::Discard, start + Duration::from_millis(60))
            .expect("discard should be valid");

        assert_eq!(lifecycle.state(), EpisodeState::Idle);
        assert_eq!(lifecycle.episode_count, 0);
        assert!(lifecycle.record_episode_stored(0));
        assert_eq!(lifecycle.episode_count, 1);
        assert!(!lifecycle.record_episode_stored(0));
    }

    #[test]
    fn record_episode_stored_ignores_unknown_episode_indices() {
        let mut lifecycle = EpisodeLifecycle::default();
        assert!(!lifecycle.record_episode_stored(0));
    }

    #[test]
    fn resume_from_prior_recordings_advances_next_index() {
        let now = Instant::now();
        let mut lifecycle = EpisodeLifecycle::default();
        lifecycle.resume_from_prior_recordings(7, 7);
        let event = lifecycle
            .handle_command(EpisodeCommand::Start, now)
            .expect("start after resume");
        assert!(matches!(
            event,
            ControlEvent::RecordingStart {
                episode_index: 7,
                ..
            }
        ));
    }
}
