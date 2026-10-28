//! Media controls on desktops without MPRIS (D-Bus). The bridge keeps the
//! Linux MPRIS bridge's interface so `lib.rs` wires both the same way; it
//! accepts updates and never dispatches actions.

use crate::playback::PlaybackSessionState;
use crate::playback_app_actions::DesktopPlaybackAction;
use std::sync::Arc;

pub(crate) type MprisDispatch = Arc<dyn Fn(DesktopPlaybackAction) + Send + Sync>;

pub(crate) struct MprisBridge;

impl MprisBridge {
    pub(crate) fn start(_dispatch: MprisDispatch, _art_base_url: String) -> Self {
        Self
    }

    pub(crate) fn update(&self, _state: PlaybackSessionState) {}
}
