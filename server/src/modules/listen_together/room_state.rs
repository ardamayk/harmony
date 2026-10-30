//! The Room State and what each command does to it, free of clocks and
//! locks: the caller passes the server time in.

use serde::{Deserialize, Serialize};

/// A Room Queue holds at most this many Tracks, as the contract says.
pub const MAX_ROOM_QUEUE_TRACKS: usize = 500;

/// One full snapshot, the `ListenTogetherRoom` schema.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomState {
    pub revision: i64,
    pub track_ids: Vec<String>,
    pub current_index: Option<usize>,
    pub playing: bool,
    pub position_ms: i64,
    pub position_at_ms: i64,
    pub lyrics_open: bool,
    pub guest_count: usize,
    pub max_guests: usize,
    pub expires_at: String,
    /// Set only on a copy made for the Owner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_url: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SkipDirection {
    Next,
    Previous,
}

/// The `ListenTogetherCommand` schema.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Command {
    PlayTracks {
        track_ids: Vec<String>,
        index: usize,
    },
    Resume,
    Pause,
    Seek {
        position_ms: i64,
    },
    Skip {
        direction: SkipDirection,
        #[serde(default)]
        from_track_id: Option<String>,
    },
    SetLyricsOpen {
        open: bool,
    },
}

impl Command {
    /// What can be refused without looking at the Room or the library.
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::PlayTracks { track_ids, .. } if track_ids.is_empty() => {
                Err("trackIds cannot be empty")
            }
            Self::PlayTracks { track_ids, .. } if track_ids.len() > MAX_ROOM_QUEUE_TRACKS => {
                Err("trackIds holds too many Tracks")
            }
            Self::PlayTracks { track_ids, index } if *index >= track_ids.len() => {
                Err("index is outside trackIds")
            }
            Self::Seek { position_ms } if *position_ms < 0 => Err("positionMs cannot be negative"),
            _ => Ok(()),
        }
    }
}

impl RoomState {
    pub fn new(max_guests: usize, expires_at: String, now_ms: i64) -> Self {
        Self {
            revision: 1,
            track_ids: Vec::new(),
            current_index: None,
            playing: false,
            position_ms: 0,
            position_at_ms: now_ms,
            lyrics_open: false,
            guest_count: 0,
            max_guests,
            expires_at,
            invite_url: None,
        }
    }

    /// The playhead at `now_ms`. Paused, it stays where it was.
    fn playhead(&self, now_ms: i64) -> i64 {
        if self.playing {
            let elapsed = now_ms.saturating_sub(self.position_at_ms).max(0);
            self.position_ms.saturating_add(elapsed)
        } else {
            self.position_ms
        }
    }

    fn set_position(&mut self, position_ms: i64, now_ms: i64) {
        self.position_ms = position_ms;
        self.position_at_ms = now_ms;
    }

    /// Makes the Track at `index` current, from its start.
    fn start_track(&mut self, index: usize, now_ms: i64) {
        self.current_index = Some(index);
        self.set_position(0, now_ms);
    }

    /// Applies a validated command and raises the revision when it changed
    /// anything. Returns whether it did.
    pub fn apply(&mut self, command: Command, now_ms: i64) -> bool {
        let changed = match command {
            Command::PlayTracks { track_ids, index } => {
                self.track_ids = track_ids;
                self.start_track(index, now_ms);
                self.playing = true;
                true
            }
            Command::Resume if self.current_index.is_some() && !self.playing => {
                // The clock restarts here, so paused time does not count.
                self.position_at_ms = now_ms;
                self.playing = true;
                true
            }
            Command::Pause if self.playing => {
                self.set_position(self.playhead(now_ms), now_ms);
                self.playing = false;
                true
            }
            Command::Seek { position_ms } if self.current_index.is_some() => {
                self.set_position(position_ms, now_ms);
                true
            }
            Command::Skip {
                direction,
                from_track_id,
            } => self.skip(direction, from_track_id.as_deref(), now_ms),
            Command::SetLyricsOpen { open } if self.lyrics_open != open => {
                self.lyrics_open = open;
                true
            }
            _ => false,
        };
        if changed {
            self.revision += 1;
        }
        changed
    }

    fn skip(&mut self, direction: SkipDirection, from_track_id: Option<&str>, now_ms: i64) -> bool {
        let Some(index) = self.current_index else {
            return false;
        };
        // Every client reports the end of a Track; only the first one counts.
        if from_track_id.is_some_and(|id| self.track_ids[index] != id) {
            return false;
        }
        match direction {
            SkipDirection::Next if index + 1 < self.track_ids.len() => {
                self.start_track(index + 1, now_ms);
                true
            }
            // Past the end the last Track stays current, stopped at its
            // start; a second report finds nothing left to change.
            SkipDirection::Next => {
                let changed = self.playing || self.position_ms != 0;
                if changed {
                    self.playing = false;
                    self.set_position(0, now_ms);
                }
                changed
            }
            SkipDirection::Previous => {
                self.start_track(index.saturating_sub(1), now_ms);
                true
            }
        }
    }

    /// Records a Guest joining or leaving. It changes the snapshot, so it
    /// raises the revision like a command.
    pub fn set_guest_count(&mut self, guest_count: usize) {
        self.guest_count = guest_count;
        self.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room() -> RoomState {
        RoomState::new(8, "2026-10-31T00:00:00Z".to_owned(), 0)
    }

    fn playing(track_ids: &[&str], index: usize) -> RoomState {
        let mut room = room();
        let track_ids = track_ids.iter().map(|&id| id.to_owned()).collect();
        assert!(room.apply(Command::PlayTracks { track_ids, index }, 1_000));
        room
    }

    fn skip(direction: SkipDirection, from: Option<&str>) -> Command {
        Command::Skip {
            direction,
            from_track_id: from.map(str::to_owned),
        }
    }

    #[test]
    fn commands_decode_from_the_contract_shape() {
        let decode = |json: &str| serde_json::from_str::<Command>(json);
        assert_eq!(
            decode(r#"{"type":"playTracks","trackIds":["a"],"index":0}"#).unwrap(),
            Command::PlayTracks {
                track_ids: vec!["a".to_owned()],
                index: 0
            }
        );
        assert_eq!(decode(r#"{"type":"resume"}"#).unwrap(), Command::Resume);
        assert_eq!(decode(r#"{"type":"pause"}"#).unwrap(), Command::Pause);
        assert_eq!(
            decode(r#"{"type":"seek","positionMs":5}"#).unwrap(),
            Command::Seek { position_ms: 5 }
        );
        assert_eq!(
            decode(r#"{"type":"skip","direction":"next","fromTrackId":"a"}"#).unwrap(),
            skip(SkipDirection::Next, Some("a"))
        );
        assert_eq!(
            decode(r#"{"type":"skip","direction":"previous"}"#).unwrap(),
            skip(SkipDirection::Previous, None)
        );
        assert_eq!(
            decode(r#"{"type":"setLyricsOpen","open":true}"#).unwrap(),
            Command::SetLyricsOpen { open: true }
        );
        for invalid in [
            r#"{"type":"explode"}"#,
            r#"{"type":"playTracks","trackIds":["a"],"index":-1}"#,
            r#"{"type":"skip","direction":"sideways"}"#,
            r#"{"positionMs":5}"#,
        ] {
            assert!(decode(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn validate_refuses_what_the_contract_refuses() {
        let play = |count: usize, index: usize| Command::PlayTracks {
            track_ids: vec!["a".to_owned(); count],
            index,
        };
        assert!(play(0, 0).validate().is_err());
        assert!(play(2, 2).validate().is_err());
        assert!(play(501, 0).validate().is_err());
        assert!(play(500, 499).validate().is_ok());
        assert!(Command::Seek { position_ms: -1 }.validate().is_err());
        assert!(Command::Seek { position_ms: 0 }.validate().is_ok());
    }

    #[test]
    fn play_tracks_keeps_duplicates_and_starts_at_the_index() {
        let room = playing(&["a", "b", "a"], 1);
        assert_eq!(room.track_ids, ["a", "b", "a"]);
        assert_eq!(room.current_index, Some(1));
        assert!(room.playing);
        assert_eq!((room.position_ms, room.position_at_ms), (0, 1_000));
        assert_eq!(room.revision, 2);
    }

    #[test]
    fn pause_keeps_the_position_and_resume_restarts_the_clock() {
        let mut room = playing(&["a"], 0);
        assert!(room.apply(Command::Pause, 4_000));
        assert_eq!((room.position_ms, room.position_at_ms), (3_000, 4_000));
        assert!(!room.playing);
        assert!(room.apply(Command::Resume, 60_000));
        assert_eq!((room.position_ms, room.position_at_ms), (3_000, 60_000));
        assert_eq!(room.playhead(61_000), 4_000);
    }

    #[test]
    fn a_command_that_changes_nothing_keeps_the_revision() {
        let mut empty = room();
        for command in [
            Command::Resume,
            Command::Pause,
            Command::Seek { position_ms: 1 },
            skip(SkipDirection::Next, None),
            Command::SetLyricsOpen { open: false },
        ] {
            assert!(!empty.apply(command, 5), "empty Room");
        }
        assert_eq!(empty.revision, 1);
        let mut room = playing(&["a"], 0);
        let revision = room.revision;
        assert!(!room.apply(Command::Resume, 5));
        assert_eq!(room.revision, revision);
    }

    #[test]
    fn seek_sets_the_position_without_an_upper_bound() {
        let mut room = playing(&["a"], 0);
        assert!(room.apply(
            Command::Seek {
                position_ms: i64::MAX
            },
            2_000
        ));
        assert_eq!(room.position_ms, i64::MAX);
        assert_eq!(room.playhead(3_000), i64::MAX);
    }

    #[test]
    fn a_skip_from_a_track_that_is_no_longer_current_changes_nothing() {
        let mut room = playing(&["a", "b", "c"], 0);
        assert!(room.apply(skip(SkipDirection::Next, Some("a")), 2_000));
        assert_eq!(room.current_index, Some(1));
        let revision = room.revision;
        assert!(!room.apply(skip(SkipDirection::Next, Some("a")), 2_001));
        assert_eq!(room.current_index, Some(1));
        assert_eq!(room.revision, revision);
    }

    #[test]
    fn a_skip_past_the_end_stops_on_the_last_track() {
        let mut room = playing(&["a", "b"], 1);
        assert!(room.apply(skip(SkipDirection::Next, Some("b")), 2_000));
        assert_eq!(room.current_index, Some(1));
        assert!(!room.playing);
        assert_eq!(room.position_ms, 0);
        let revision = room.revision;
        assert!(!room.apply(skip(SkipDirection::Next, Some("b")), 2_001));
        assert_eq!(room.revision, revision);
    }

    #[test]
    fn skip_previous_goes_back_and_restarts_at_the_first_track() {
        let mut room = playing(&["a", "b"], 1);
        assert!(room.apply(skip(SkipDirection::Previous, None), 2_000));
        assert_eq!(room.current_index, Some(0));
        assert!(room.apply(Command::Seek { position_ms: 9_000 }, 2_500));
        assert!(room.apply(skip(SkipDirection::Previous, None), 3_000));
        assert_eq!(room.current_index, Some(0));
        assert_eq!((room.position_ms, room.position_at_ms), (0, 3_000));
    }

    #[test]
    fn a_skip_keeps_a_paused_room_paused() {
        let mut room = playing(&["a", "b"], 0);
        room.apply(Command::Pause, 2_000);
        assert!(room.apply(skip(SkipDirection::Next, None), 3_000));
        assert_eq!(room.current_index, Some(1));
        assert!(!room.playing);
    }

    #[test]
    fn the_snapshot_has_the_contract_field_names() {
        let value = serde_json::to_value(room()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "revision": 1,
                "trackIds": [],
                "currentIndex": null,
                "playing": false,
                "positionMs": 0,
                "positionAtMs": 0,
                "lyricsOpen": false,
                "guestCount": 0,
                "maxGuests": 8,
                "expiresAt": "2026-10-31T00:00:00Z",
            })
        );
    }
}
