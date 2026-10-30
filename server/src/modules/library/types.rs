//! Library API shapes (library/types.go). Field order and `omitempty`
//! follow the Go structs: zero numbers, empty strings and empty lists marked
//! omitempty there are left out here too.

use crate::gojson;
use serde::Serialize;

fn is_zero_i64(value: &i64) -> bool {
    *value == 0
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artist {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub album_count: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ArtistCredit {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Genre {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReleaseIdentifier {
    pub scheme: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumArtworkMetadata {
    #[serde(rename = "sourceTrackId", skip_serializing_if = "String::is_empty")]
    pub source_track_id: String,
    #[serde(rename = "contentSha256")]
    pub content_sha256: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub media_type: String,
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub width: i64,
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub height: i64,
    pub size_bytes: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub id: String,
    pub title: String,
    pub artist_id: String,
    pub artist_name: String,
    pub album_artists: Vec<ArtistCredit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    pub release_identifiers: Vec<ReleaseIdentifier>,
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub track_count: i64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub genres: Vec<String>,
    pub genre_items: Vec<Genre>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork: Option<AlbumArtworkMetadata>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayGainMetadata {
    #[serde(rename = "trackGainDb", serialize_with = "gojson::optional_float")]
    pub track_gain_db: Option<f64>,
    #[serde(serialize_with = "gojson::optional_float")]
    pub track_peak: Option<f64>,
    #[serde(rename = "albumGainDb", serialize_with = "gojson::optional_float")]
    pub album_gain_db: Option<f64>,
    #[serde(serialize_with = "gojson::optional_float")]
    pub album_peak: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist_name: String,
    pub artists: Vec<ArtistCredit>,
    pub album_artists: Vec<ArtistCredit>,
    pub album_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub album_title: String,
    pub disc_no: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_no: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_total: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disc_total: Option<i64>,
    pub duration_ms: i64,
    pub format: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub codec: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub container: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub sample_format: String,
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub size_bytes: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub genre: String,
    pub genres: Vec<Genre>,
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub sample_rate_hz: i64,
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub bit_depth: i64,
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub channel_count: i64,
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub bitrate_bps: i64,
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub bitrate_kbps: i64,
    pub replay_gain: ReplayGainMetadata,
    pub title_sort: String,
    pub file_path: String,
    pub file_mtime: i64,
    #[serde(rename = "contentSha256", skip_serializing_if = "String::is_empty")]
    pub content_sha256: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub identity_key: String,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
    pub explicit: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AlbumDetail {
    #[serde(flatten)]
    pub album: Album,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackWaveform {
    pub track_id: String,
    pub peak_count: usize,
    pub peaks: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackWaveformPending {
    pub status: &'static str,
    pub retry_after_seconds: u64,
}
