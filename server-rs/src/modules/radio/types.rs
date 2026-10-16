//! Radio API shapes (radio/types.go, search.go), with the Go omitempty rules.

use crate::gojson;
use serde::{Deserialize, Serialize};

fn is_zero(value: &i64) -> bool {
    *value == 0
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub artist: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub raw: String,
    /// RFC 3339, as Go marshals the time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub stale: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Station {
    pub id: String,
    pub name: String,
    pub stream_url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub homepage_url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub favicon_url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub country: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub language: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub codec: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub bitrate: i64,
    pub source: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub external_id: String,
    pub is_favorite: bool,
    pub position: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_now_playing: Option<NowPlaying>,
}

#[derive(Clone, Debug, Serialize)]
pub struct List<T> {
    pub items: Vec<T>,
    pub total: usize,
}

/// The POST body. Absent and null fields read as empty, like Go.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StationCreate {
    #[serde(deserialize_with = "gojson::nullable")]
    pub name: String,
    #[serde(deserialize_with = "gojson::nullable")]
    pub stream_url: String,
    #[serde(deserialize_with = "gojson::nullable")]
    pub homepage_url: String,
    #[serde(deserialize_with = "gojson::nullable")]
    pub favicon_url: String,
    #[serde(deserialize_with = "gojson::nullable")]
    pub country: String,
    #[serde(deserialize_with = "gojson::nullable")]
    pub language: String,
    #[serde(deserialize_with = "gojson::nullable")]
    pub tags: Vec<String>,
    #[serde(deserialize_with = "gojson::nullable")]
    pub codec: String,
    #[serde(deserialize_with = "gojson::nullable")]
    pub bitrate: i64,
    #[serde(deserialize_with = "gojson::nullable")]
    pub source: String,
    #[serde(deserialize_with = "gojson::nullable")]
    pub external_id: String,
    #[serde(deserialize_with = "gojson::nullable")]
    pub is_favorite: bool,
}

/// The PATCH body: only what the client sent.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StationPatch {
    pub name: Option<String>,
    pub stream_url: Option<String>,
    pub homepage_url: Option<String>,
    pub favicon_url: Option<String>,
    pub country: Option<String>,
    pub language: Option<String>,
    pub tags: Option<Vec<String>>,
    pub codec: Option<String>,
    pub bitrate: Option<i64>,
    pub is_favorite: Option<bool>,
    pub position: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchResult {
    #[serde(rename = "stationUuid", deserialize_with = "gojson::nullable")]
    pub station_uuid: String,
    #[serde(deserialize_with = "gojson::nullable")]
    pub name: String,
    #[serde(deserialize_with = "gojson::nullable")]
    pub stream_url: String,
    #[serde(
        skip_serializing_if = "String::is_empty",
        deserialize_with = "gojson::nullable"
    )]
    pub homepage_url: String,
    #[serde(
        skip_serializing_if = "String::is_empty",
        deserialize_with = "gojson::nullable"
    )]
    pub favicon_url: String,
    #[serde(
        skip_serializing_if = "String::is_empty",
        deserialize_with = "gojson::nullable"
    )]
    pub country: String,
    #[serde(
        skip_serializing_if = "String::is_empty",
        deserialize_with = "gojson::nullable"
    )]
    pub language: String,
    /// `None` is Go's nil slice: absent from a client's result, and left
    /// alone when an import updates a station.
    pub tags: Option<Vec<String>>,
    #[serde(
        skip_serializing_if = "String::is_empty",
        deserialize_with = "gojson::nullable"
    )]
    pub codec: String,
    #[serde(skip_serializing_if = "is_zero", deserialize_with = "gojson::nullable")]
    pub bitrate: i64,
    #[serde(skip_serializing_if = "is_zero", deserialize_with = "gojson::nullable")]
    pub votes: i64,
    #[serde(
        skip_serializing_if = "String::is_empty",
        deserialize_with = "gojson::nullable"
    )]
    pub health_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_successful_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogOption {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub code: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub station_count: i64,
}
