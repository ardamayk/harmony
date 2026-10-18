use crate::playback::{PlaybackCommandError, PlaybackController};
use crate::playback_lifecycle::{
    PlaybackLifecycle, PlaybackSessionSnapshot, PlaybackSnapshotStore,
};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DesktopPlaybackAction {
    OpenMainWindow,
    CloseMainWindow,
    TogglePlay,
    Play,
    Pause,
    Stop,
    Previous,
    Next,
    /// Absolute position in seconds, from MPRIS SetPosition or a relative Seek.
    SeekTo(f64),
    /// Software volume in 0..=1, from the MPRIS Volume property.
    SetVolume(f64),
    SetRate(f64),
    ToggleMiniPlayer,
    Quit,
}

pub(crate) trait DesktopPlaybackShell {
    fn show_main_window(&self) -> Result<(), String>;
    fn hide_main_window(&self) -> Result<(), String>;
    /// Volume lives in the Processing Profile, which the shell owns; the
    /// playback controller alone cannot persist it.
    fn set_software_volume(&self, _volume: f64) -> Result<(), String> {
        Err("Volume control is unavailable from this shell.".to_owned())
    }
    fn toggle_mini_window(&self) -> Result<(), String> {
        Err("The mini player is unavailable from this shell.".to_owned())
    }
    fn exit(&self);
}

pub(crate) fn dispatch_desktop_playback_action(
    action: DesktopPlaybackAction,
    playback: &PlaybackController,
    lifecycle: &Arc<Mutex<PlaybackLifecycle>>,
    snapshot_store: &PlaybackSnapshotStore,
    shell: &dyn DesktopPlaybackShell,
) -> Result<(), PlaybackCommandError> {
    match action {
        DesktopPlaybackAction::OpenMainWindow => {
            shell.show_main_window().map_err(PlaybackCommandError::new)
        }
        DesktopPlaybackAction::CloseMainWindow => close_main_window(lifecycle, shell),
        DesktopPlaybackAction::TogglePlay => playback.toggle_play().map(|_| ()),
        DesktopPlaybackAction::Play => playback.play(None).map(|_| ()),
        DesktopPlaybackAction::Pause => playback.pause().map(|_| ()),
        DesktopPlaybackAction::Stop => playback.stop().map(|_| ()),
        DesktopPlaybackAction::Previous => playback.previous().map(|_| ()),
        DesktopPlaybackAction::Next => playback.next().map(|_| ()),
        DesktopPlaybackAction::SeekTo(seconds) => playback.seek(seconds.max(0.0)).map(|_| ()),
        DesktopPlaybackAction::SetVolume(volume) => shell
            .set_software_volume(volume.clamp(0.0, 1.0))
            .map_err(PlaybackCommandError::new),
        DesktopPlaybackAction::SetRate(rate) => playback.set_playback_rate(rate).map(|_| ()),
        DesktopPlaybackAction::ToggleMiniPlayer => shell
            .toggle_mini_window()
            .map_err(PlaybackCommandError::new),
        DesktopPlaybackAction::Quit => quit(playback, lifecycle, snapshot_store, shell),
    }
}

fn close_main_window(
    lifecycle: &Arc<Mutex<PlaybackLifecycle>>,
    shell: &dyn DesktopPlaybackShell,
) -> Result<(), PlaybackCommandError> {
    shell
        .hide_main_window()
        .map_err(PlaybackCommandError::new)?;
    lifecycle
        .lock()
        .map_err(|_| PlaybackCommandError::new("Playback lifecycle state is unavailable."))?
        .close_main_window();
    Ok(())
}

fn quit(
    playback: &PlaybackController,
    lifecycle: &Arc<Mutex<PlaybackLifecycle>>,
    snapshot_store: &PlaybackSnapshotStore,
    shell: &dyn DesktopPlaybackShell,
) -> Result<(), PlaybackCommandError> {
    let playback_state = playback.state()?;
    let snapshot = PlaybackSessionSnapshot::from_serializable_state(&playback_state)
        .map_err(|error| PlaybackCommandError::new(error.to_string()))?;
    playback.shutdown()?;
    lifecycle
        .lock()
        .map_err(|_| PlaybackCommandError::new("Playback lifecycle state is unavailable."))?
        .explicit_quit(snapshot_store, &snapshot)
        .map_err(|error| PlaybackCommandError::new(error.to_string()))?;
    shell.exit();
    Ok(())
}
