#[path = "../src/playback_lifecycle.rs"]
mod playback_lifecycle;
#[allow(dead_code)]
#[path = "../src/playback_tray.rs"]
mod playback_tray;

use playback_lifecycle::{
    PlaybackLifecycle, PlaybackLifecycleAction, PlaybackLifecycleState, PlaybackSessionSnapshot,
    PlaybackSnapshotStore,
};
use playback_tray::PlaybackTrayView;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

#[test]
fn relaunch_loads_the_last_playback_session_paused() {
    let directory = temporary_path("paused-restore");
    let store = PlaybackSnapshotStore::new(directory.join("playback-session.json"));
    let snapshot = PlaybackSessionSnapshot::new(
        Some(json!({
            "type": "track",
            "track": { "id": "track-1", "title": "Track 1" },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
        })),
        42.5,
        0.65,
        true,
        "loop",
    )
    .expect("valid snapshot");

    store.save(&snapshot).expect("save snapshot");
    let restored = store
        .load()
        .expect("load snapshot")
        .expect("persisted snapshot");

    assert_eq!(restored, snapshot);
    assert_eq!(
        restored
            .source()
            .and_then(|source| source.pointer("/track/id"))
            .and_then(serde_json::Value::as_str),
        Some("track-1")
    );
    assert_eq!(restored.playhead_seconds(), 42.5);
    assert_eq!(restored.volume(), 0.65);
    assert!(restored.is_shuffle_enabled());
    assert_eq!(restored.repeat_mode(), "loop");

    fs::remove_dir_all(directory).expect("remove snapshot fixture");
}

#[test]
fn relaunch_rebinds_the_source_to_the_new_private_media_proxy() {
    let snapshot = PlaybackSessionSnapshot::new(
        Some(json!({
            "type": "track",
            "track": { "id": "track-1", "title": "Track 1" },
            "playbackUrl": "http://127.0.0.1:43129/expired-token/api/v1/tracks/track-1/stream"
        })),
        12.0,
        0.8,
        false,
        "off",
    )
    .expect("valid snapshot");

    let rebound = snapshot
        .rebind_media_proxy("http://127.0.0.1:43129/new-token")
        .expect("rebind private proxy");

    assert_eq!(
        rebound
            .source()
            .and_then(|source| source.get("playbackUrl"))
            .and_then(serde_json::Value::as_str),
        Some("http://127.0.0.1:43129/new-token/api/v1/tracks/track-1/stream")
    );
}

#[test]
fn relaunch_rebinds_catalog_previews_to_the_preview_stream_route() {
    let snapshot = PlaybackSessionSnapshot::new(
        Some(json!({
            "type": "catalog-preview",
            "result": { "stationUuid": "catalog-1", "name": "Catalog 1" },
            "playbackUrl": "http://127.0.0.1:43129/expired-token/api/v1/radio/preview/catalog-1/stream"
        })),
        0.0,
        0.8,
        false,
        "off",
    )
    .expect("valid snapshot");

    let rebound = snapshot
        .rebind_media_proxy("http://127.0.0.1:43129/new-token")
        .expect("rebind private proxy");

    assert_eq!(
        rebound
            .source()
            .and_then(|source| source.get("playbackUrl"))
            .and_then(serde_json::Value::as_str),
        Some("http://127.0.0.1:43129/new-token/api/v1/radio/preview/catalog-1/stream")
    );
}

#[test]
fn renderer_reload_preserves_background_playback_and_republishes_state() {
    let mut lifecycle = PlaybackLifecycle::new();

    let closed = lifecycle.close_main_window();
    let reloaded = lifecycle.renderer_attached();

    assert_eq!(closed.state, PlaybackLifecycleState::Background);
    assert_eq!(closed.action, PlaybackLifecycleAction::KeepPlayerRunning);
    assert_eq!(reloaded.state, PlaybackLifecycleState::Background);
    assert_eq!(
        reloaded.action,
        PlaybackLifecycleAction::PublishPlaybackState
    );
}

#[test]
fn explicit_quit_persists_the_session_before_stopping_the_player() {
    let directory = temporary_path("explicit-quit");
    let store = PlaybackSnapshotStore::new(directory.join("playback-session.json"));
    let snapshot = PlaybackSessionSnapshot::new(
        Some(json!({
            "type": "radio",
            "station": { "id": "station-1", "name": "Station 1" },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/radio/stations/station-1/stream"
        })),
        18.0,
        0.8,
        false,
        "off",
    )
    .expect("valid snapshot");
    let mut lifecycle = PlaybackLifecycle::new();

    let transition = lifecycle
        .explicit_quit(&store, &snapshot)
        .expect("persist explicit Quit");

    assert_eq!(transition.state, PlaybackLifecycleState::Quitting);
    assert_eq!(
        transition.action,
        PlaybackLifecycleAction::StopPlayerAndExit
    );
    assert_eq!(store.load().expect("load snapshot"), Some(snapshot));

    fs::remove_dir_all(directory).expect("remove snapshot fixture");
}

#[test]
fn a_second_consecutive_player_failure_stops_automatic_recovery() {
    let snapshot = PlaybackSessionSnapshot::new(
        Some(json!({
            "type": "track",
            "track": { "id": "track-1", "title": "Track 1" },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
        })),
        37.25,
        0.8,
        false,
        "off",
    )
    .expect("valid snapshot");
    let mut lifecycle = PlaybackLifecycle::new();

    let first_failure = lifecycle.unexpected_player_exit(&snapshot, true);
    let second_failure = lifecycle.unexpected_player_exit(&snapshot, true);

    assert_eq!(first_failure.state, PlaybackLifecycleState::Recovering);
    assert_eq!(
        first_failure.action,
        PlaybackLifecycleAction::RestartPlayer {
            snapshot,
            should_resume: true
        }
    );
    assert_eq!(second_failure.state, PlaybackLifecycleState::Failed);
    let PlaybackLifecycleAction::SurfaceActionableError(error) = second_failure.action else {
        panic!("second failure must not restart mpv");
    };
    assert_eq!(error.code, "mpv-crash-loop");
    assert!(error.message.contains("Quit and reopen the Desktop Client"));
}

#[test]
fn stable_playback_resets_the_consecutive_failure_budget() {
    let snapshot =
        PlaybackSessionSnapshot::new(None, 0.0, 0.8, false, "off").expect("valid snapshot");
    let mut lifecycle = PlaybackLifecycle::new();

    let first_failure = lifecycle.unexpected_player_exit(&snapshot, true);
    lifecycle.player_stabilized();
    let later_failure = lifecycle.unexpected_player_exit(&snapshot, true);

    assert!(matches!(
        first_failure.action,
        PlaybackLifecycleAction::RestartPlayer { .. }
    ));
    assert!(matches!(
        later_failure.action,
        PlaybackLifecycleAction::RestartPlayer { .. }
    ));
}

#[test]
fn explicit_quit_snapshot_uses_the_public_playback_state() {
    let state = json!({
        "source": {
            "type": "track",
            "track": { "id": "track-2", "title": "Track 2" },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-2/stream"
        },
        "status": "playing",
        "currentTime": 63.75,
        "duration": 180.0,
        "volume": 0.55,
        "shuffleEnabled": true,
        "repeatMode": "once",
        "error": null
    });

    let snapshot = PlaybackSessionSnapshot::from_serializable_state(&state)
        .expect("snapshot from public playback state");

    assert_eq!(snapshot.playhead_seconds(), 63.75);
    assert_eq!(snapshot.volume(), 0.55);
    assert!(snapshot.is_shuffle_enabled());
    assert_eq!(snapshot.repeat_mode(), "once");
}

#[test]
fn tray_exposes_the_current_source_and_complete_playback_controls() {
    let source = json!({
        "type": "track",
        "track": { "id": "track-1", "title": "Track 1" }
    });

    let tray = PlaybackTrayView::from_playback(Some(&source), true);

    assert_eq!(tray.source_label, "Track 1");
    assert_eq!(tray.previous_label, "Previous");
    assert_eq!(tray.toggle_label, "Pause");
    assert_eq!(tray.next_label, "Next");
    assert_eq!(tray.open_label, "Open Earthly Audio");
    assert_eq!(tray.quit_label, "Quit Earthly Audio");
}

fn temporary_path(name: &str) -> PathBuf {
    let mut random = [0_u8; 8];
    getrandom::fill(&mut random).expect("random temporary name");
    std::env::temp_dir().join(format!(
        "earthly-audio-lifecycle-test-{}-{name}",
        u64::from_le_bytes(random)
    ))
}
