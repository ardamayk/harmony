//! The Shared Playback State and what each command does to it, free of
//! clocks and locks: the caller passes the server time in. One row per user
//! in SQLite, so it survives a server restart; a missing row reads as the
//! empty state with revision 0 (ADR 0040).

use serde::{Deserialize, Serialize};

/// A report older than this means the Active Device is gone: the state reads
/// as paused at the last reported playhead, without being rewritten.
pub const STALE_REPORT_MS: i64 = 20_000;

/// The contract's longest `deviceId` and `deviceName`.
pub const MAX_DEVICE_CHARS: usize = 64;

/// Which way a skip goes, as the contract spells it.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkipDirection {
    Next,
    Previous,
}

impl SkipDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Next => "next",
            Self::Previous => "previous",
        }
    }

    /// The direction as the database stores it.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "next" => Some(Self::Next),
            "previous" => Some(Self::Previous),
            _ => None,
        }
    }
}

/// A skip for the Active Device to carry out, once (`PlaybackSkipRequest`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SkipRequest {
    pub revision: i64,
    pub direction: SkipDirection,
}

/// One full snapshot, the `PlaybackState` schema.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackState {
    pub revision: i64,
    pub queue_item_id: Option<String>,
    pub track_id: Option<String>,
    pub playing: bool,
    pub position_ms: i64,
    pub position_at_ms: i64,
    pub active_device_id: Option<String>,
    pub active_device_name: Option<String>,
    pub remote_command_revision: i64,
    pub skip_request: Option<SkipRequest>,
    pub server_time_ms: i64,
}

/// The `PlaybackStateCommand` schema.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Command {
    Claim {
        device_id: String,
        device_name: String,
        queue_item_id: Option<String>,
        track_id: Option<String>,
        position_ms: i64,
        playing: bool,
    },
    Report {
        device_id: String,
        based_on_revision: i64,
        queue_item_id: Option<String>,
        track_id: Option<String>,
        position_ms: i64,
        playing: bool,
    },
    Pause,
    Seek {
        position_ms: i64,
    },
    Skip {
        direction: SkipDirection,
    },
}

impl Command {
    /// What can be refused without looking at the state: the contract's
    /// lengths and non-negative playheads.
    pub fn validate(&self) -> Result<(), &'static str> {
        fn device(id: &str, name: Option<&str>) -> Result<(), &'static str> {
            if id.is_empty() {
                return Err("deviceId cannot be empty");
            }
            if id.chars().count() > MAX_DEVICE_CHARS {
                return Err("deviceId holds too many characters");
            }
            if let Some(name) = name {
                if name.is_empty() {
                    return Err("deviceName cannot be empty");
                }
                if name.chars().count() > MAX_DEVICE_CHARS {
                    return Err("deviceName holds too many characters");
                }
            }
            Ok(())
        }
        let position = |position_ms: i64| {
            if position_ms < 0 {
                Err("positionMs cannot be negative")
            } else {
                Ok(())
            }
        };
        match self {
            Self::Claim {
                device_id,
                device_name,
                position_ms,
                ..
            } => {
                device(device_id, Some(device_name))?;
                position(*position_ms)
            }
            Self::Report {
                device_id,
                position_ms,
                ..
            } => {
                device(device_id, None)?;
                position(*position_ms)
            }
            Self::Seek { position_ms } => position(*position_ms),
            Self::Pause | Self::Skip { .. } => Ok(()),
        }
    }
}

/// Where the playing stands, as a claim or a report carries it. A claim has
/// it all; a report repeats what the device is at now.
#[derive(Clone, Debug, PartialEq)]
pub struct Playback {
    pub queue_item_id: Option<String>,
    pub track_id: Option<String>,
    pub position_ms: i64,
    pub playing: bool,
}

/// What [`StoredState::report`] refuses.
#[derive(Debug, PartialEq)]
pub enum ReportError {
    /// The report came from a device that is no longer the Active Device.
    NotActive,
}

/// The row as it is stored: everything the snapshot shows plus when the
/// Active Device was last heard from and the pending skip split into its
/// two columns.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StoredState {
    pub queue_item_id: Option<String>,
    pub track_id: Option<String>,
    pub playing: bool,
    pub position_ms: i64,
    pub position_at_ms: i64,
    pub last_report_at_ms: i64,
    pub active_device_id: Option<String>,
    pub active_device_name: Option<String>,
    pub revision: i64,
    pub remote_command_revision: i64,
    pub skip_revision: Option<i64>,
    pub skip_direction: Option<SkipDirection>,
}

impl StoredState {
    /// Whether the playhead is moving at `now_ms`: a stored `playing` only
    /// counts while the Active Device is heard from.
    fn playing_at(&self, now_ms: i64) -> bool {
        self.playing && now_ms - self.last_report_at_ms <= STALE_REPORT_MS
    }

    /// The playhead at `now_ms`. Paused or dead, it stays where it was.
    fn playhead(&self, now_ms: i64) -> i64 {
        if self.playing_at(now_ms) {
            let elapsed = now_ms.saturating_sub(self.position_at_ms).max(0);
            self.position_ms.saturating_add(elapsed)
        } else {
            self.position_ms
        }
    }

    fn skip_request(&self) -> Option<SkipRequest> {
        Some(SkipRequest {
            revision: self.skip_revision?,
            direction: self.skip_direction?,
        })
    }

    /// The snapshot a client sees at `now_ms`. A live-but-silent Active
    /// Device reads as paused at the stored playhead; nothing is rewritten,
    /// so a late report still lands.
    pub fn at(&self, now_ms: i64) -> PlaybackState {
        PlaybackState {
            revision: self.revision,
            queue_item_id: self.queue_item_id.clone(),
            track_id: self.track_id.clone(),
            playing: self.playing_at(now_ms),
            position_ms: self.position_ms,
            position_at_ms: self.position_at_ms,
            active_device_id: self.active_device_id.clone(),
            active_device_name: self.active_device_name.clone(),
            remote_command_revision: self.remote_command_revision,
            skip_request: self.skip_request(),
            server_time_ms: now_ms,
        }
    }

    /// The sender becomes the Active Device: everything is theirs now, and a
    /// pending skip dies with the old arrangement.
    pub fn claim(&mut self, device_id: &str, device_name: &str, playback: Playback, now_ms: i64) {
        self.queue_item_id = playback.queue_item_id;
        self.track_id = playback.track_id;
        self.playing = playback.playing;
        self.position_ms = playback.position_ms;
        self.position_at_ms = now_ms;
        self.last_report_at_ms = now_ms;
        self.active_device_id = Some(device_id.to_owned());
        self.active_device_name = Some(device_name.to_owned());
        self.skip_revision = None;
        self.skip_direction = None;
        self.revision += 1;
    }

    /// The Active Device says where it is. A report from another device is
    /// refused; one based on a revision older than the latest remote command
    /// changes nothing, so a heartbeat in flight cannot undo a remote pause.
    /// Returns whether the visible state changed.
    pub fn report(
        &mut self,
        device_id: &str,
        based_on_revision: i64,
        playback: Playback,
        now_ms: i64,
    ) -> Result<bool, ReportError> {
        if self.active_device_id.as_deref() != Some(device_id) {
            return Err(ReportError::NotActive);
        }
        if based_on_revision < self.remote_command_revision {
            return Ok(false);
        }
        // The device only carries a skip forward, so a report of a different
        // Track is the proof it carried it out.
        let track_changed =
            playback.queue_item_id != self.queue_item_id || playback.track_id != self.track_id;
        let cleared_skip = self.skip_revision.is_some() && track_changed;
        let changed = cleared_skip
            || playback.playing != self.playing
            || playback.position_ms != self.position_ms
            || track_changed;
        if cleared_skip {
            self.skip_revision = None;
            self.skip_direction = None;
        }
        self.queue_item_id = playback.queue_item_id;
        self.track_id = playback.track_id;
        self.playing = playback.playing;
        self.position_ms = playback.position_ms;
        self.position_at_ms = now_ms;
        // Every accepted report proves liveness, even a heartbeat that
        // changed nothing.
        self.last_report_at_ms = now_ms;
        if changed {
            self.revision += 1;
        }
        Ok(changed)
    }

    /// A remote pause: the playhead freezes where it was. Always raises the
    /// revision, so the Active Device's in-flight heartbeat loses.
    pub fn pause(&mut self, now_ms: i64) {
        let position_ms = self.playhead(now_ms);
        self.position_ms = position_ms;
        self.position_at_ms = now_ms;
        self.playing = false;
        self.become_remote_command();
    }

    /// A remote seek: the playhead moves, play or pause stays.
    pub fn seek(&mut self, position_ms: i64, now_ms: i64) {
        self.position_ms = position_ms;
        self.position_at_ms = now_ms;
        self.become_remote_command();
    }

    /// A remote skip: the server does not know the Active Device's shuffle,
    /// so the request is relayed for it to carry out once.
    pub fn skip(&mut self, direction: SkipDirection) {
        self.become_remote_command();
        self.skip_revision = Some(self.revision);
        self.skip_direction = Some(direction);
    }

    /// Marks the state as changed by a remote command: pause, seek and skip
    /// raise `remote_command_revision` to the new revision, which is what a
    /// report's `basedOnRevision` is judged against.
    fn become_remote_command(&mut self) {
        self.revision += 1;
        self.remote_command_revision = self.revision;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A claim or report at `q1`/`t1`, the common case in these tests.
    fn playback(track: &str, position_ms: i64, playing: bool) -> Playback {
        Playback {
            queue_item_id: Some("q1".to_owned()),
            track_id: Some(track.to_owned()),
            position_ms,
            playing,
        }
    }

    fn claimed() -> StoredState {
        let mut state = StoredState::default();
        state.claim("d1", "Desktop", playback("t1", 0, true), 1_000);
        state
    }

    fn report(
        device: &str,
        based_on: i64,
        track: &str,
        position_ms: i64,
        playing: bool,
    ) -> Command {
        Command::Report {
            device_id: device.to_owned(),
            based_on_revision: based_on,
            queue_item_id: Some("q1".to_owned()),
            track_id: Some(track.to_owned()),
            position_ms,
            playing,
        }
    }

    fn apply(state: &mut StoredState, command: Command, now_ms: i64) -> bool {
        match command {
            Command::Claim {
                device_id,
                device_name,
                queue_item_id,
                track_id,
                position_ms,
                playing,
            } => {
                state.claim(
                    &device_id,
                    &device_name,
                    Playback {
                        queue_item_id,
                        track_id,
                        position_ms,
                        playing,
                    },
                    now_ms,
                );
                true
            }
            Command::Report {
                device_id,
                based_on_revision,
                queue_item_id,
                track_id,
                position_ms,
                playing,
            } => state
                .report(
                    &device_id,
                    based_on_revision,
                    Playback {
                        queue_item_id,
                        track_id,
                        position_ms,
                        playing,
                    },
                    now_ms,
                )
                .unwrap(),
            Command::Pause => {
                state.pause(now_ms);
                true
            }
            Command::Seek { position_ms } => {
                state.seek(position_ms, now_ms);
                true
            }
            Command::Skip { direction } => {
                state.skip(direction);
                true
            }
        }
    }

    #[test]
    fn commands_decode_from_the_contract_shape() {
        let decode = |json: &str| serde_json::from_str::<Command>(json);
        assert_eq!(
            decode(
                r#"{"type":"claim","deviceId":"d","deviceName":"Desk","queueItemId":"q","trackId":"t","positionMs":5,"playing":true}"#
            )
            .unwrap(),
            Command::Claim {
                device_id: "d".to_owned(),
                device_name: "Desk".to_owned(),
                queue_item_id: Some("q".to_owned()),
                track_id: Some("t".to_owned()),
                position_ms: 5,
                playing: true
            }
        );
        assert_eq!(
            decode(
                r#"{"type":"report","deviceId":"d","basedOnRevision":2,"queueItemId":null,"trackId":null,"positionMs":0,"playing":false}"#
            )
            .unwrap(),
            Command::Report {
                device_id: "d".to_owned(),
                based_on_revision: 2,
                queue_item_id: None,
                track_id: None,
                position_ms: 0,
                playing: false
            }
        );
        assert_eq!(decode(r#"{"type":"pause"}"#).unwrap(), Command::Pause);
        assert_eq!(
            decode(r#"{"type":"seek","positionMs":5}"#).unwrap(),
            Command::Seek { position_ms: 5 }
        );
        assert_eq!(
            decode(r#"{"type":"skip","direction":"previous"}"#).unwrap(),
            Command::Skip {
                direction: SkipDirection::Previous
            }
        );
        for invalid in [
            r#"{"type":"explode"}"#,
            r#"{"type":"claim","deviceId":"d"}"#,
            r#"{"type":"skip","direction":"sideways"}"#,
            r#"{"type":"seek"}"#,
        ] {
            assert!(decode(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn validate_refuses_what_the_contract_refuses() {
        let claim = |id: &str, name: &str, position_ms: i64| Command::Claim {
            device_id: id.to_owned(),
            device_name: name.to_owned(),
            queue_item_id: None,
            track_id: None,
            position_ms,
            playing: false,
        };
        assert!(claim("", "Desk", 0).validate().is_err());
        assert!(claim(&"d".repeat(65), "Desk", 0).validate().is_err());
        assert!(claim("d", "", 0).validate().is_err());
        assert!(claim("d", &"D".repeat(65), 0).validate().is_err());
        assert!(claim("d", "Desk", -1).validate().is_err());
        assert!(claim("d", "Desk", 0).validate().is_ok());
        assert!(
            claim(&"d".repeat(64), &"D".repeat(64), 0)
                .validate()
                .is_ok()
        );
        assert!(report("", 0, "t", 0, false).validate().is_err());
        assert!(report("d", 0, "t", -1, false).validate().is_err());
        assert!(report("d", 0, "t", 0, false).validate().is_ok());
        assert!(Command::Seek { position_ms: -1 }.validate().is_err());
        assert!(Command::Seek { position_ms: 0 }.validate().is_ok());
        assert!(Command::Pause.validate().is_ok());
        assert!(
            Command::Skip {
                direction: SkipDirection::Next
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn a_claim_sets_everything_and_clears_a_pending_skip() {
        let mut state = claimed();
        state.skip(SkipDirection::Next);
        assert!(state.skip_request().is_some());
        state.claim(
            "d2",
            "Phone",
            Playback {
                queue_item_id: None,
                track_id: None,
                position_ms: 3_000,
                playing: false,
            },
            9_000,
        );
        assert_eq!(state.active_device_id.as_deref(), Some("d2"));
        assert_eq!(state.active_device_name.as_deref(), Some("Phone"));
        assert_eq!(
            (state.queue_item_id.as_deref(), state.track_id.as_deref()),
            (None, None)
        );
        assert!(!state.playing);
        assert_eq!((state.position_ms, state.position_at_ms), (3_000, 9_000));
        assert_eq!(state.last_report_at_ms, 9_000);
        assert_eq!(state.skip_request(), None);
        assert_eq!(state.revision, 3);
        // A claim is no remote command: only pause, seek and skip raise it.
        assert_eq!(state.remote_command_revision, 2);
    }

    #[test]
    fn a_report_from_another_device_is_refused() {
        let mut state = claimed();
        assert_eq!(
            state.report("d2", 1, playback("t1", 100, true), 2_000),
            Err(ReportError::NotActive)
        );
        // And from no device at all, when nothing is playing.
        let mut empty = StoredState::default();
        assert_eq!(
            empty.report(
                "d1",
                0,
                Playback {
                    queue_item_id: None,
                    track_id: None,
                    position_ms: 0,
                    playing: false
                },
                2_000
            ),
            Err(ReportError::NotActive)
        );
    }

    #[test]
    fn a_report_based_on_an_older_revision_changes_nothing() {
        let mut state = claimed();
        state.pause(2_000);
        let before = state.clone();
        // The heartbeat was already flying when the pause landed.
        assert!(
            !state
                .report("d1", 1, playback("t1", 4_000, true), 3_000)
                .unwrap()
        );
        assert_eq!(state, before);
    }

    #[test]
    fn a_report_that_changes_the_track_clears_the_skip_request() {
        let mut state = claimed();
        state.skip(SkipDirection::Next);
        let revision = state.revision;
        let changed = state
            .report(
                "d1",
                revision,
                Playback {
                    queue_item_id: Some("q2".to_owned()),
                    track_id: Some("t2".to_owned()),
                    position_ms: 0,
                    playing: true,
                },
                4_000,
            )
            .unwrap();
        assert!(changed);
        assert_eq!(state.skip_request(), None);
        assert_eq!(state.queue_item_id.as_deref(), Some("q2"));
        // Carrying the skip out is itself a change, so clients hear of it.
        assert_eq!(state.revision, revision + 1);
        // A report of the same Track leaves a pending skip alone: only
        // carrying it out changes the Track.
        let mut state = claimed();
        state.skip(SkipDirection::Next);
        assert!(
            !state
                .report(
                    "d1",
                    state.remote_command_revision,
                    playback("t1", 0, true),
                    4_000
                )
                .unwrap()
        );
        assert!(state.skip_request().is_some());
    }

    #[test]
    fn an_unchanging_report_keeps_the_revision_but_stays_live() {
        let mut state = claimed();
        let revision = state.revision;
        assert!(
            !state
                .report("d1", 1, playback("t1", 0, true), 6_000)
                .unwrap()
        );
        assert_eq!(state.revision, revision);
        // The heartbeat itself proves the device is alive.
        assert!(state.at(6_000 + STALE_REPORT_MS).playing);
    }

    #[test]
    fn a_playing_report_advances_and_bumps_the_revision() {
        let mut state = claimed();
        let changed = state
            .report("d1", 1, playback("t1", 1_500, true), 4_000)
            .unwrap();
        assert!(changed);
        assert_eq!((state.position_ms, state.position_at_ms), (1_500, 4_000));
        assert_eq!(state.playhead(5_000), 2_500);
        assert_eq!(state.revision, 2);
    }

    #[test]
    fn pause_stores_the_playhead_and_raises_the_remote_command_revision() {
        let mut state = claimed();
        state
            .report("d1", 1, playback("t1", 1_000, true), 2_000)
            .unwrap();
        assert!(apply(&mut state, Command::Pause, 3_500));
        assert_eq!((state.position_ms, state.position_at_ms), (2_500, 3_500));
        assert!(!state.playing);
        assert_eq!(state.revision, 3);
        assert_eq!(state.remote_command_revision, 3);
        // A second pause still raises it, so the newest command always wins.
        apply(&mut state, Command::Pause, 4_000);
        assert_eq!(state.remote_command_revision, state.revision);
    }

    #[test]
    fn pause_extrapolates_only_while_the_active_device_is_live() {
        const REPORTED_AT: i64 = 2_000;
        let reported = || {
            let mut state = claimed();
            state
                .report("d1", 1, playback("t1", 120_000, true), REPORTED_AT)
                .unwrap();
            state
        };

        // The last moment the device counts as live: still extrapolated.
        let mut live = reported();
        live.pause(REPORTED_AT + STALE_REPORT_MS);
        assert_eq!(live.position_ms, 120_000 + STALE_REPORT_MS);

        // One millisecond later it is dead: the last report stands.
        let mut just_dead = reported();
        just_dead.pause(REPORTED_AT + STALE_REPORT_MS + 1);
        assert_eq!(just_dead.position_ms, 120_000);

        // A sleep timer 30 minutes after the crash, past the Track's end.
        let mut dead = reported();
        let now_ms = REPORTED_AT + 30 * 60 * 1_000;
        dead.pause(now_ms);
        assert_eq!((dead.position_ms, dead.position_at_ms), (120_000, now_ms));
        assert!(!dead.playing);
        assert_eq!(dead.remote_command_revision, dead.revision);
        let snapshot = dead.at(now_ms);
        assert_eq!(snapshot.position_ms, 120_000);
        assert!(!snapshot.playing);
    }

    #[test]
    fn seek_stores_the_position_and_keeps_play_or_pause() {
        let mut state = claimed();
        apply(&mut state, Command::Seek { position_ms: 9_000 }, 2_000);
        assert_eq!((state.position_ms, state.position_at_ms), (9_000, 2_000));
        assert!(state.playing);
        assert_eq!(state.revision, 2);
        assert_eq!(state.remote_command_revision, 2);
    }

    #[test]
    fn skip_sets_the_request_for_the_active_device_to_carry_out_once() {
        let mut state = claimed();
        apply(
            &mut state,
            Command::Skip {
                direction: SkipDirection::Previous,
            },
            2_000,
        );
        assert_eq!(
            state.skip_request(),
            Some(SkipRequest {
                revision: 2,
                direction: SkipDirection::Previous
            })
        );
        assert_eq!(state.remote_command_revision, 2);
        // The playhead waits for the Active Device's report.
        assert_eq!((state.position_ms, state.position_at_ms), (0, 1_000));
    }

    #[test]
    fn a_silent_active_device_reads_as_paused_at_the_stored_playhead() {
        let mut state = claimed();
        state
            .report("d1", 1, playback("t1", 1_000, true), 2_000)
            .unwrap();
        assert!(state.at(2_000 + STALE_REPORT_MS).playing);
        let snapshot = state.at(2_000 + STALE_REPORT_MS + 1);
        assert!(!snapshot.playing);
        // Frozen at the last report, not advanced to now.
        assert_eq!(snapshot.position_ms, 1_000);
        assert_eq!(snapshot.active_device_id.as_deref(), Some("d1"));
    }

    #[test]
    fn an_empty_state_reads_as_revision_zero() {
        let state = StoredState::default();
        let snapshot = state.at(5_000);
        assert_eq!(snapshot.revision, 0);
        assert_eq!(snapshot.remote_command_revision, 0);
        assert_eq!(snapshot.queue_item_id, None);
        assert_eq!(snapshot.track_id, None);
        assert!(!snapshot.playing);
        assert_eq!(snapshot.skip_request, None);
        assert_eq!(snapshot.server_time_ms, 5_000);
    }

    #[test]
    fn the_snapshot_has_the_contract_field_names() {
        let mut state = StoredState::default();
        state.claim("d1", "Desktop", playback("t1", 0, false), 1_000);
        state.seek(7_000, 2_000);
        state.skip(SkipDirection::Next);
        let value = serde_json::to_value(state.at(3_000)).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "revision": 3,
                "queueItemId": "q1",
                "trackId": "t1",
                "playing": false,
                "positionMs": 7_000,
                "positionAtMs": 2_000,
                "activeDeviceId": "d1",
                "activeDeviceName": "Desktop",
                "remoteCommandRevision": 3,
                "skipRequest": { "revision": 3, "direction": "next" },
                "serverTimeMs": 3_000,
            })
        );
        // And the empty state leaves every nullable field null, as required.
        let value = serde_json::to_value(StoredState::default().at(3_000)).unwrap();
        for field in [
            "queueItemId",
            "trackId",
            "activeDeviceId",
            "activeDeviceName",
            "skipRequest",
        ] {
            assert!(value[field].is_null(), "{field}");
        }
    }
}
