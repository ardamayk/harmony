//! Layout clamping, playback bounds and sparse patching
//! (preferences/layout.go, playback.go, service.go).

use super::repository::{Repository, RepositoryError};
use super::types::*;
use crate::db::Db;

/// Preferences rules over the repository.
pub struct Service {
    repo: Repository,
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl Service {
    pub fn new(pool: Db) -> Self {
        Self {
            repo: Repository::new(pool),
        }
    }

    pub async fn get(&self, user_id: &str) -> Result<UserPreferences, RepositoryError> {
        self.repo.get(user_id).await
    }

    pub async fn update(
        &self,
        user_id: &str,
        patch: &UserPreferencesPatch,
    ) -> Result<UserPreferences, ServiceError> {
        validate_playback_patch(patch.playback.as_ref()).map_err(ServiceError::Invalid)?;
        Ok(self
            .repo
            .update(user_id, |current| apply_patch(current, patch))
            .await?)
    }
}

pub const MIN_SEEK_STEP_SECONDS: i64 = 1;
pub const MAX_SEEK_STEP_SECONDS: i64 = 120;
pub const MIN_PLAYBACK_RATE: f64 = 0.5;
pub const MAX_PLAYBACK_RATE: f64 = 2.0;
pub const MIN_TRANSITION_FADE_MS: i64 = 0;
pub const MAX_TRANSITION_FADE_MS: i64 = 2000;
pub const MIN_AUTO_SKIP_ON_ERROR_SECONDS: i64 = 0;
pub const MAX_AUTO_SKIP_ON_ERROR_SECONDS: i64 = 60;

fn clamp_panel_sizes(
    sizes: &[f64],
    collapsed: &std::collections::BTreeMap<String, bool>,
) -> Vec<f64> {
    const MIN_LEFT: f64 = 15.0;
    const MAX_LEFT: f64 = 45.0;
    const MIN_RIGHT: f64 = 18.0;
    const MAX_RIGHT: f64 = 50.0;
    const MIN_MINI: f64 = 4.0;
    const MAX_MINI: f64 = 12.0;
    const MIN_MAIN: f64 = 25.0;
    let is_collapsed = |side: &str| collapsed.get(side).copied().unwrap_or(false);
    // Not f64::clamp: the Go code applies the lower bound first, which
    // differs for NaN.
    let bound = |value: f64, minimum: f64, maximum: f64| {
        let value = if value < minimum { minimum } else { value };
        if value > maximum { maximum } else { value }
    };
    let mut left = if is_collapsed("left") {
        bound(sizes[0], MIN_MINI, MAX_MINI)
    } else {
        bound(sizes[0], MIN_LEFT, MAX_LEFT)
    };
    let mut right = if is_collapsed("right") {
        bound(sizes[2], MIN_MINI, MAX_MINI)
    } else {
        bound(sizes[2], MIN_RIGHT, MAX_RIGHT)
    };
    let mut main = 100.0 - left - right;
    if main < MIN_MAIN {
        let deficit = MIN_MAIN - main;
        let side_sum = left + right;
        left -= (left / side_sum) * deficit;
        right -= (right / side_sum) * deficit;
        main = MIN_MAIN;
    }
    vec![left, main, right]
}

/// Fills unset parts from the defaults and clamps the panel sizes.
pub fn normalize_layout(layout: LayoutInput) -> LayoutPreferences {
    let defaults = default_layout();
    let collapsed = layout.collapsed.unwrap_or(defaults.collapsed);
    let sizes = if layout.sizes.len() == 3 {
        clamp_panel_sizes(&layout.sizes, &collapsed)
    } else {
        defaults.sizes
    };
    LayoutPreferences {
        sidebar_position: if layout.sidebar_position.is_empty() {
            "left".to_owned()
        } else {
            layout.sidebar_position
        },
        panels: match layout.panels {
            Some(panels) => panels
                .into_iter()
                .map(|(side, panels)| (side, panels.unwrap_or_default()))
                .collect(),
            None => defaults.panels,
        },
        collapsed,
        sizes,
    }
}

/// The stored layout as the input form, so it normalizes the same way.
fn as_input(layout: &LayoutPreferences) -> LayoutInput {
    LayoutInput {
        sidebar_position: layout.sidebar_position.clone(),
        panels: Some(
            layout
                .panels
                .iter()
                .map(|(side, panels)| (side.clone(), Some(panels.clone())))
                .collect(),
        ),
        collapsed: Some(layout.collapsed.clone()),
        sizes: layout.sizes.clone(),
    }
}

pub fn apply_patch(current: &mut UserPreferences, patch: &UserPreferencesPatch) {
    if !patch.theme.mode.is_empty() {
        current.theme.mode = patch.theme.mode.clone();
    }
    if !patch.theme.preset.is_empty() {
        current.theme.preset = patch.theme.preset.clone();
    }
    let mut layout = as_input(&current.layout);
    if !patch.layout.sidebar_position.is_empty() {
        layout.sidebar_position = patch.layout.sidebar_position.clone();
    }
    for side in ["left", "right"] {
        if let Some(Some(panels)) = patch.layout.panels.as_ref().and_then(|map| map.get(side)) {
            layout
                .panels
                .get_or_insert_default()
                .insert(side.to_owned(), Some(panels.clone()));
        }
        if let Some(&collapsed) = patch
            .layout
            .collapsed
            .as_ref()
            .and_then(|map| map.get(side))
        {
            layout
                .collapsed
                .get_or_insert_default()
                .insert(side.to_owned(), collapsed);
        }
    }
    if patch.layout.sizes.len() == 3 {
        layout.sizes = patch.layout.sizes.clone();
    }
    current.layout = normalize_layout(layout);
    apply_playback_patch(&mut current.playback, patch.playback.as_ref());
}

pub fn apply_playback_patch(
    current: &mut PlaybackPreferences,
    patch: Option<&PlaybackPreferencesPatch>,
) {
    let Some(patch) = patch else { return };
    if let Some(value) = patch.seek_step_seconds {
        current.seek_step_seconds = value;
    }
    if let Some(value) = patch.seek_step_large_seconds {
        current.seek_step_large_seconds = value;
    }
    if let Some(value) = patch.playback_rate {
        current.playback_rate = value;
    }
    if let Some(value) = patch.transition_fade_ms {
        current.transition_fade_ms = value;
    }
    if let Some(value) = patch.show_waveform {
        current.show_waveform = value;
    }
    if let Some(value) = patch.accent_from_cover {
        current.accent_from_cover = value;
    }
    if let Some(value) = patch.show_up_next {
        current.show_up_next = value;
    }
    if let Some(value) = patch.auto_skip_on_error_seconds {
        current.auto_skip_on_error_seconds = value;
    }
    if let Some(value) = patch.gapless_playback {
        current.gapless_playback = value;
    }
}

/// The first out-of-range field, with the Go server's message.
pub fn validate_playback_patch(patch: Option<&PlaybackPreferencesPatch>) -> Result<(), String> {
    let Some(patch) = patch else { return Ok(()) };
    let check = |field: &str, value: Option<i64>, minimum: i64, maximum: i64| match value {
        Some(value) if value < minimum || value > maximum => {
            Err(format!("{field} must be between {minimum} and {maximum}"))
        }
        _ => Ok(()),
    };
    check(
        "seekStepSeconds",
        patch.seek_step_seconds,
        MIN_SEEK_STEP_SECONDS,
        MAX_SEEK_STEP_SECONDS,
    )?;
    check(
        "seekStepLargeSeconds",
        patch.seek_step_large_seconds,
        MIN_SEEK_STEP_SECONDS,
        MAX_SEEK_STEP_SECONDS,
    )?;
    if let Some(rate) = patch.playback_rate
        && !(MIN_PLAYBACK_RATE..=MAX_PLAYBACK_RATE).contains(&rate)
    {
        return Err(format!(
            "playbackRate must be between {MIN_PLAYBACK_RATE} and {MAX_PLAYBACK_RATE}"
        ));
    }
    check(
        "transitionFadeMs",
        patch.transition_fade_ms,
        MIN_TRANSITION_FADE_MS,
        MAX_TRANSITION_FADE_MS,
    )?;
    check(
        "autoSkipOnErrorSeconds",
        patch.auto_skip_on_error_seconds,
        MIN_AUTO_SKIP_ON_ERROR_SECONDS,
        MAX_AUTO_SKIP_ON_ERROR_SECONDS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn preferences() -> UserPreferences {
        UserPreferences {
            theme: default_theme(),
            layout: default_layout(),
            playback: default_playback(),
        }
    }

    #[test]
    fn sizes_are_clamped_per_side_and_main_keeps_its_minimum() {
        let layout = normalize_layout(LayoutInput {
            sizes: vec![60.0, 0.0, 70.0],
            ..LayoutInput::default()
        });
        // Left clamps to 45, right to 50, then both shrink so main is 25.
        assert_eq!(layout.sizes[1], 25.0);
        assert!((layout.sizes[0] - 45.0 * 75.0 / 95.0).abs() < 1e-9);
        assert!((layout.sizes[2] - 50.0 * 75.0 / 95.0).abs() < 1e-9);

        let collapsed = normalize_layout(LayoutInput {
            collapsed: Some(BTreeMap::from([("left".to_owned(), true)])),
            sizes: vec![30.0, 50.0, 20.0],
            ..LayoutInput::default()
        });
        assert_eq!(collapsed.sizes, [12.0, 68.0, 20.0]);
    }

    #[test]
    fn layout_defaults_fill_unset_parts_and_wrong_sized_lists() {
        let layout = normalize_layout(LayoutInput {
            sizes: vec![1.0, 2.0],
            ..LayoutInput::default()
        });
        assert_eq!(layout, default_layout());
    }

    #[test]
    fn patches_change_only_what_they_carry() {
        let mut current = preferences();
        let patch: UserPreferencesPatch = serde_json::from_str(
            r#"{"theme":{"mode":"dark"},"layout":{"panels":{"left":[],"right":null},"collapsed":{"right":null}},"playback":{"showWaveform":true}}"#,
        )
        .expect("patch");

        apply_patch(&mut current, &patch);

        assert_eq!(current.theme.mode, "dark");
        assert_eq!(current.theme.preset, "noir-red");
        assert_eq!(current.layout.panels["left"], Vec::<String>::new());
        assert_eq!(current.layout.panels["right"], ["discover"]);
        assert!(!current.layout.collapsed["right"]);
        assert!(current.playback.show_waveform);
        assert!(current.playback.show_up_next);
    }

    #[test]
    fn playback_bounds_use_the_go_messages() {
        let patch =
            |json: &str| -> PlaybackPreferencesPatch { serde_json::from_str(json).expect("patch") };
        assert_eq!(
            validate_playback_patch(Some(&patch(r#"{"playbackRate":3}"#))),
            Err("playbackRate must be between 0.5 and 2".to_owned())
        );
        assert_eq!(
            validate_playback_patch(Some(&patch(r#"{"seekStepSeconds":0}"#))),
            Err("seekStepSeconds must be between 1 and 120".to_owned())
        );
        assert_eq!(
            validate_playback_patch(Some(&patch(r#"{"transitionFadeMs":2000}"#))),
            Ok(())
        );
        assert_eq!(validate_playback_patch(None), Ok(()));
    }
}
