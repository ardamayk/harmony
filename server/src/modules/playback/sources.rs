//! Where a queue item came from, recorded at insertion
//! (playback/queue-source.go).

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::collections::HashMap;

const MAX_SOURCE_BYTES: usize = 16 * 1024;
const MAX_STRING_LENGTH: usize = 1024;
const MAX_BASED_ON: usize = 20;
const INVALID: &str = "invalid queue source";

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QueueItemSource {
    #[serde(deserialize_with = "crate::gojson::nullable")]
    pub kind: String,
    #[serde(
        skip_serializing_if = "String::is_empty",
        deserialize_with = "crate::gojson::nullable"
    )]
    pub album_id: String,
    #[serde(
        skip_serializing_if = "String::is_empty",
        deserialize_with = "crate::gojson::nullable"
    )]
    pub album_title: String,
    #[serde(
        skip_serializing_if = "String::is_empty",
        deserialize_with = "crate::gojson::nullable"
    )]
    pub artist_name: String,
    #[serde(
        skip_serializing_if = "String::is_empty",
        deserialize_with = "crate::gojson::nullable"
    )]
    pub playlist_id: String,
    #[serde(
        skip_serializing_if = "String::is_empty",
        deserialize_with = "crate::gojson::nullable"
    )]
    pub name: String,
    /// Always written for a suggestion, otherwise only when non-empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub based_on: Option<Vec<String>>,
}

impl QueueItemSource {
    pub fn user() -> Self {
        Self {
            kind: "user".to_owned(),
            ..Self::default()
        }
    }

    /// Go's custom MarshalJSON: basedOn is omitted when empty except for
    /// suggestions.
    fn for_encoding(&self) -> Self {
        let mut source = self.clone();
        if source.kind != "suggestion" && source.based_on.as_ref().is_some_and(Vec::is_empty) {
            source.based_on = None;
        }
        if source.kind == "suggestion" && source.based_on.is_none() {
            source.based_on = Some(Vec::new());
        }
        source
    }
}

/// A request's `source`: absent is a user insertion; anything else must be
/// an object whose fields match its kind exactly.
pub fn parse(raw: Option<&RawValue>) -> Result<QueueItemSource, String> {
    let Some(raw) = raw else {
        return Ok(QueueItemSource::user());
    };
    parse_json(raw.get())
}

pub fn parse_json(data: &str) -> Result<QueueItemSource, String> {
    if data.is_empty() {
        return Ok(QueueItemSource::user());
    }
    if data.len() > MAX_SOURCE_BYTES {
        return Err(format!(
            "{INVALID}: source exceeds {MAX_SOURCE_BYTES} bytes"
        ));
    }
    let fields: HashMap<String, Box<RawValue>> =
        match serde_json::from_str::<Option<HashMap<String, Box<RawValue>>>>(data) {
            Ok(Some(fields)) => fields,
            _ => return Err(format!("{INVALID}: source must be an object")),
        };
    let source: QueueItemSource = serde_json::from_str(data)
        .map_err(|_| format!("{INVALID}: source fields have invalid types"))?;
    let required: &[&str] = match source.kind.as_str() {
        "album" => &["kind", "albumId", "albumTitle", "artistName"],
        "playlist" => &["kind", "playlistId", "name"],
        "user" => &["kind"],
        "suggestion" => &["kind", "basedOn"],
        _ => return Err(format!("{INVALID}: unknown kind")),
    };
    if fields.len() != required.len() {
        return Err(format!(
            "{INVALID}: fields do not match source kind {:?}",
            source.kind
        ));
    }
    for field in required {
        validate_field(field, fields.get(*field).map(|value| value.get()))?;
    }
    Ok(source)
}

fn validate_field(field: &str, value: Option<&str>) -> Result<(), String> {
    let value = value.unwrap_or_default();
    if field == "basedOn" {
        let track_ids = match serde_json::from_str::<Option<Vec<String>>>(value) {
            Ok(Some(track_ids)) => track_ids,
            _ => return Err(format!("{INVALID}: basedOn must be an array of track IDs")),
        };
        if track_ids.len() > MAX_BASED_ON {
            return Err(format!(
                "{INVALID}: basedOn exceeds {MAX_BASED_ON} track IDs"
            ));
        }
        if track_ids.iter().any(|track_id| {
            track_id.trim().is_empty() || track_id.chars().count() > MAX_STRING_LENGTH
        }) {
            return Err(format!(
                "{INVALID}: basedOn track IDs must be nonblank and at most {MAX_STRING_LENGTH} characters"
            ));
        }
        return Ok(());
    }
    match serde_json::from_str::<String>(value) {
        Ok(text) if !text.trim().is_empty() => {
            if text.chars().count() > MAX_STRING_LENGTH {
                Err(format!(
                    "{INVALID}: {field} exceeds {MAX_STRING_LENGTH} characters"
                ))
            } else {
                Ok(())
            }
        }
        _ => Err(format!("{INVALID}: {field} must be a nonblank string")),
    }
}

/// The stored JSON of one source, validated again as Go does.
pub fn encode(source: &QueueItemSource) -> Result<String, String> {
    let data = crate::gojson::to_string(&source.for_encoding());
    parse_json(&data)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(json: &str) -> Box<RawValue> {
        RawValue::from_string(json.to_owned()).expect("raw")
    }

    #[test]
    fn sources_must_match_their_kind_exactly() {
        assert_eq!(parse(None).expect("absent").kind, "user");
        assert_eq!(
            parse(Some(&raw("null"))),
            Err(format!("{INVALID}: source must be an object"))
        );
        assert_eq!(
            parse(Some(&raw("[1]"))),
            Err(format!("{INVALID}: source must be an object"))
        );
        assert_eq!(
            parse(Some(&raw(r#"{"kind":"radio"}"#))),
            Err(format!("{INVALID}: unknown kind"))
        );
        assert_eq!(
            parse(Some(&raw(r#"{"kind":"user","name":"x"}"#))),
            Err(format!(
                "{INVALID}: fields do not match source kind \"user\""
            ))
        );
        assert_eq!(
            parse(Some(&raw(
                r#"{"kind":"album","albumId":"a","albumTitle":" ","artistName":"x"}"#
            ))),
            Err(format!("{INVALID}: albumTitle must be a nonblank string"))
        );
        assert_eq!(
            parse(Some(&raw(
                r#"{"kind":"album","albumId":3,"albumTitle":"t","artistName":"x"}"#
            ))),
            Err(format!("{INVALID}: source fields have invalid types"))
        );
        assert_eq!(
            parse(Some(&raw(
                r#"{"kind":"album","albumId":null,"albumTitle":"t","artistName":"x"}"#
            ))),
            Err(format!("{INVALID}: albumId must be a nonblank string"))
        );
        assert!(parse(Some(&raw(r#"{"kind":"suggestion","basedOn":[]}"#))).is_ok());
        assert_eq!(
            parse(Some(&raw(r#"{"kind":"suggestion","basedOn":null}"#))),
            Err(format!("{INVALID}: basedOn must be an array of track IDs"))
        );
    }

    #[test]
    fn encodes_like_go_marshal() {
        let album = parse(Some(&raw(
            r#"{"kind":"album","albumId":"a1","albumTitle":"Tom & Jerry","artistName":"X"}"#,
        )))
        .expect("album");
        assert_eq!(
            encode(&album),
            Ok(r#"{"kind":"album","albumId":"a1","albumTitle":"Tom \u0026 Jerry","artistName":"X"}"#.to_owned())
        );
        let suggestion =
            parse(Some(&raw(r#"{"kind":"suggestion","basedOn":[]}"#))).expect("suggestion");
        assert_eq!(
            encode(&suggestion),
            Ok(r#"{"kind":"suggestion","basedOn":[]}"#.to_owned())
        );
        assert_eq!(
            encode(&QueueItemSource::user()),
            Ok(r#"{"kind":"user"}"#.to_owned())
        );
    }
}
