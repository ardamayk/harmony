use crate::gojson;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ThemePreferences {
    #[serde(default, deserialize_with = "gojson::nullable")]
    pub mode: String,
    #[serde(default, deserialize_with = "gojson::nullable")]
    pub preset: String,
}

/// Maps are ordered like Go writes them: by key.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutPreferences {
    pub sidebar_position: String,
    pub panels: BTreeMap<String, Vec<String>>,
    pub collapsed: BTreeMap<String, bool>,
    #[serde(
        serialize_with = "gojson::floats",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub sizes: Vec<f64>,
}

/// Every field always carries a value once read from the store.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackPreferences {
    pub seek_step_seconds: i64,
    pub seek_step_large_seconds: i64,
    #[serde(serialize_with = "gojson::float")]
    pub playback_rate: f64,
    pub transition_fade_ms: i64,
    pub show_waveform: bool,
    pub accent_from_cover: bool,
    pub show_up_next: bool,
    pub auto_skip_on_error_seconds: i64,
    pub gapless_playback: bool,
}

/// The sparse form: only the fields the client sent, so a false boolean is
/// distinguishable from an absent one.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackPreferencesPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seek_step_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seek_step_large_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition_fade_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_waveform: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent_from_cover: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_up_next: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_skip_on_error_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gapless_playback: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UserPreferences {
    pub theme: ThemePreferences,
    pub layout: LayoutPreferences,
    pub playback: PlaybackPreferences,
}

/// Layout as stored and as patched. Like the Go struct, a missing or null
/// map stays unset (`None`), and a null element inside a map reads as the
/// element's zero value while still counting as present.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayoutInput {
    #[serde(default, deserialize_with = "gojson::nullable")]
    pub sidebar_position: String,
    #[serde(default, deserialize_with = "panels")]
    pub panels: Option<PanelsInput>,
    #[serde(default, deserialize_with = "collapsed")]
    pub collapsed: Option<BTreeMap<String, bool>>,
    #[serde(default, deserialize_with = "gojson::nullable")]
    pub sizes: Vec<f64>,
}

/// Panel ids per side; a null side is unset.
pub type PanelsInput = BTreeMap<String, Option<Vec<String>>>;

fn panels<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<PanelsInput>, D::Error> {
    Option::deserialize(deserializer)
}

fn collapsed<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<BTreeMap<String, bool>>, D::Error> {
    let map: Option<BTreeMap<String, Option<bool>>> = Option::deserialize(deserializer)?;
    Ok(map.map(|map| {
        map.into_iter()
            .map(|(key, value)| (key, value.unwrap_or(false)))
            .collect()
    }))
}

/// The PATCH body. Theme and Layout keep the zero-value-means-absent
/// convention; Playback is sparse.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct UserPreferencesPatch {
    #[serde(default, deserialize_with = "gojson::nullable")]
    pub theme: ThemePreferences,
    #[serde(default, deserialize_with = "gojson::nullable")]
    pub layout: LayoutInput,
    #[serde(default)]
    pub playback: Option<PlaybackPreferencesPatch>,
}

pub fn default_theme() -> ThemePreferences {
    ThemePreferences {
        mode: "system".to_owned(),
        preset: "noir-red".to_owned(),
    }
}

pub fn default_layout() -> LayoutPreferences {
    LayoutPreferences {
        sidebar_position: "left".to_owned(),
        panels: BTreeMap::from([
            ("left".to_owned(), vec!["now-playing".to_owned()]),
            ("right".to_owned(), vec!["discover".to_owned()]),
        ]),
        collapsed: BTreeMap::from([("left".to_owned(), false), ("right".to_owned(), false)]),
        sizes: vec![22.0, 50.0, 28.0],
    }
}

pub fn default_playback() -> PlaybackPreferences {
    PlaybackPreferences {
        seek_step_seconds: 5,
        seek_step_large_seconds: 30,
        playback_rate: 1.0,
        transition_fade_ms: 0,
        show_waveform: false,
        accent_from_cover: false,
        show_up_next: true,
        auto_skip_on_error_seconds: 5,
        gapless_playback: true,
    }
}
