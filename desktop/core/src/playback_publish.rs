use crate::playback::PlaybackSessionState;

/// mpv reports `time-pos` about ten times a second and `demuxer-cache-time`
/// on top of that. Each published Playback Session re-renders the webview and
/// updates the tray, window title, and MPRIS, so playhead-only progress is
/// coalesced to roughly four publishes a second, like the browser's
/// `timeupdate`.
const PLAYHEAD_STEP_SECONDS: f64 = 0.25;
const BUFFERED_STEP_SECONDS: f64 = 1.0;

/// Drops Playback Session updates that only move the playhead or buffer
/// forward a little since the last published state. Any other change, a seek
/// backwards, or a larger jump publishes immediately and carries the latest
/// playhead with it.
#[derive(Debug, Default)]
pub(crate) struct PlayheadThrottle {
    last_published: Option<PlaybackSessionState>,
}

impl PlayheadThrottle {
    pub(crate) fn should_publish(&mut self, next: &PlaybackSessionState) -> bool {
        if self
            .last_published
            .as_ref()
            .is_some_and(|last| is_small_progress(last, next))
        {
            return false;
        }
        self.last_published = Some(next.clone());
        true
    }
}

fn is_small_progress(last: &PlaybackSessionState, next: &PlaybackSessionState) -> bool {
    let advanced = next.current_time - last.current_time;
    if !(0.0..PLAYHEAD_STEP_SECONDS).contains(&advanced) {
        return false;
    }
    let buffered_is_close = match (last.buffered_end, next.buffered_end) {
        (Some(last), Some(next)) => (next - last).abs() < BUFFERED_STEP_SECONDS,
        (last, next) => last == next,
    };
    if !buffered_is_close {
        return false;
    }
    let mut aligned = next.clone();
    aligned.current_time = last.current_time;
    aligned.buffered_end = last.buffered_end;
    aligned == *last
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::PlaybackStatus;

    fn playing_at(current_time: f64) -> PlaybackSessionState {
        let mut state = PlaybackSessionState::default();
        state.status = PlaybackStatus::Playing;
        state.current_time = current_time;
        state.duration = 200.0;
        state.buffered_end = Some(current_time + 10.0);
        state
    }

    fn paused_at(current_time: f64) -> PlaybackSessionState {
        let mut state = playing_at(current_time);
        state.status = PlaybackStatus::Paused;
        state
    }

    #[test]
    fn publishes_the_first_state() {
        let mut throttle = PlayheadThrottle::default();
        assert!(throttle.should_publish(&playing_at(0.0)));
    }

    #[test]
    fn coalesces_small_playhead_steps_until_a_quarter_second_has_played() {
        let mut throttle = PlayheadThrottle::default();
        assert!(throttle.should_publish(&playing_at(10.0)));
        assert!(!throttle.should_publish(&playing_at(10.09)));
        assert!(!throttle.should_publish(&playing_at(10.18)));
        assert!(throttle.should_publish(&playing_at(10.27)));
        assert!(!throttle.should_publish(&playing_at(10.36)));
    }

    #[test]
    fn publishes_seeks_backwards_and_large_jumps_immediately() {
        let mut throttle = PlayheadThrottle::default();
        assert!(throttle.should_publish(&playing_at(10.0)));
        assert!(throttle.should_publish(&playing_at(4.0)));
        assert!(throttle.should_publish(&playing_at(90.0)));
    }

    #[test]
    fn publishes_any_change_besides_progress_immediately() {
        let mut throttle = PlayheadThrottle::default();
        assert!(throttle.should_publish(&playing_at(10.0)));
        assert!(throttle.should_publish(&paused_at(10.05)));
    }

    #[test]
    fn publishes_buffer_growth_of_a_second_or_more() {
        let mut throttle = PlayheadThrottle::default();
        assert!(throttle.should_publish(&paused_at(10.0)));
        let buffered = |end| {
            let mut state = paused_at(10.0);
            state.buffered_end = Some(end);
            state
        };
        assert!(!throttle.should_publish(&buffered(20.5)));
        assert!(throttle.should_publish(&buffered(21.0)));
    }
}
