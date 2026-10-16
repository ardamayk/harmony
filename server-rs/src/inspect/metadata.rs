//! Tag normalization shared by every inspector (media_inspector.go
//! normalizeMediaMetadata, metadata.go ReplayGain, genres.go).

use super::errors::{Code, Failure, Result, error};
use serde::Serialize;
use std::collections::HashMap;
use unicode_normalization::UnicodeNormalization;

pub const MAX_IDENTITY_VALUE_BYTES: usize = 200;
pub const MAX_MEDIA_POSITION: i64 = 9999;

/// Raw tag values by upper-case key, in file order.
pub type Tags = HashMap<String, Vec<String>>;

/// Marks a value that was not valid UTF-8 in the file. Go keeps such bytes
/// as they are and rejects them where a value must be valid; the marker is
/// a noncharacter, so the same checks reject it here.
const INVALID_UTF8_MARKER: char = '\u{FFFE}';

/// A tag value read from raw bytes.
pub fn tag_string(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(value) => value.to_owned(),
        Err(_) => format!("{INVALID_UTF8_MARKER}{}", String::from_utf8_lossy(bytes)),
    }
}

fn without_marker(value: &str) -> &str {
    value.strip_prefix(INVALID_UTF8_MARKER).unwrap_or(value)
}

/// Go's `strings.ToUpper`: per-rune simple upper case.
pub fn go_upper(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(single), None) => single,
                _ => c,
            }
        })
        .collect()
}

/// The key of a raw tag name: trimmed and upper-cased.
pub fn tag_key(raw: &str) -> String {
    go_upper(raw.trim())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct ReplayGain {
    #[serde(rename = "trackGainDb")]
    pub track_gain_db: Option<f64>,
    #[serde(rename = "trackPeak")]
    pub track_peak: Option<f64>,
    #[serde(rename = "albumGainDb")]
    pub album_gain_db: Option<f64>,
    #[serde(rename = "albumPeak")]
    pub album_peak: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MediaPosition {
    pub number: i64,
    pub total: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NormalizedMetadata {
    pub title: String,
    pub artists: Vec<String>,
    pub album_artists: Vec<String>,
    pub album: String,
    pub track_position: MediaPosition,
    pub disc_position: MediaPosition,
    pub has_disc_number: bool,
    pub genres: Vec<String>,
    pub year: i64,
    pub replay_gain: ReplayGain,
    /// Unsynchronised lyrics (Vorbis LYRICS / UNSYNCEDLYRICS, ID3 USLT, MP4
    /// lyrics); empty when absent.
    pub lyrics: String,
}

fn go_parse_float(value: &str) -> Option<f64> {
    // strconv.ParseFloat accepts what Rust does plus hex floats and
    // underscores; neither appears in ReplayGain tags.
    let lower = value.to_ascii_lowercase();
    let parsed: f64 = match lower.trim_start_matches(['+', '-']) {
        "inf" | "infinity" | "nan" => return None,
        _ => value.parse().ok()?,
    };
    parsed.is_finite().then_some(parsed)
}

pub fn parse_replay_gain_value(value: &str) -> Option<f64> {
    let value = value.trim().to_lowercase();
    let value = value.strip_suffix("db").unwrap_or(&value).trim();
    go_parse_float(value)
}

pub fn parse_replay_gain_peak(value: &str) -> Option<f64> {
    parse_replay_gain_value(value).filter(|peak| *peak >= 0.0)
}

/// `readReplayGainStringMetadata`: keys are matched case-insensitively.
pub fn replay_gain_from_strings<'a, I>(raw: I) -> ReplayGain
where
    I: IntoIterator<Item = (&'a String, &'a String)>,
{
    let values: HashMap<String, &String> = raw
        .into_iter()
        .map(|(key, value)| (go_upper(key), value))
        .collect();
    let get = |key: &str| values.get(key).map_or("", |value| value.as_str());
    ReplayGain {
        track_gain_db: parse_replay_gain_value(get("REPLAYGAIN_TRACK_GAIN")),
        track_peak: parse_replay_gain_peak(get("REPLAYGAIN_TRACK_PEAK")),
        album_gain_db: parse_replay_gain_value(get("REPLAYGAIN_ALBUM_GAIN")),
        album_peak: parse_replay_gain_peak(get("REPLAYGAIN_ALBUM_PEAK")),
    }
}

/// `replayGainFromTags`: the first value of each tag.
pub fn replay_gain_from_tags(tags: &Tags) -> ReplayGain {
    let first: Vec<(&String, &String)> = tags
        .iter()
        .filter_map(|(key, values)| values.first().map(|value| (key, value)))
        .collect();
    replay_gain_from_strings(first)
}

/// `splitGenreTagValues`: every value split on the genre delimiters, first
/// spellings kept, duplicates dropped case-insensitively.
pub fn split_genre_tag_values<S: AsRef<str>>(values: &[S]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for value in values {
        for genre in crate::modules::library::genres::split_genres(without_marker(value.as_ref())) {
            if seen.insert(genre.to_lowercase()) {
                out.push(genre);
            }
        }
    }
    out
}

pub fn normalize(tags: &Tags, replay_gain: ReplayGain) -> Result<NormalizedMetadata> {
    let title = required_single_tag(tags, "TITLE");
    let artists = required_artist_credits(tags, "ARTIST");
    let album_artists = required_artist_credits(tags, "ALBUMARTIST");
    let album = required_single_tag(tags, "ALBUM");
    let genres = split_genre_tag_values(tags.get("GENRE").map_or(&[][..], Vec::as_slice));
    let track = track_position(tags);
    let disc = disc_position(tags);
    let year = optional_year(tags);
    let failure = Failure::join([
        title.as_ref().err().cloned(),
        artists.as_ref().err().cloned(),
        album_artists.as_ref().err().cloned(),
        album.as_ref().err().cloned(),
        track.as_ref().err().cloned(),
        disc.as_ref().err().cloned(),
        year.as_ref().err().cloned(),
    ]);
    if !failure.is_empty() {
        return Err(failure);
    }
    Ok(NormalizedMetadata {
        title: title?,
        artists: artists?,
        album_artists: album_artists?,
        album: album?,
        track_position: track?,
        disc_position: disc?,
        has_disc_number: has_values(tags, "DISCNUMBER"),
        genres,
        year: year?,
        replay_gain,
        lyrics: optional_lyrics(tags),
    })
}

fn has_values(tags: &Tags, key: &str) -> bool {
    tags.get(key).is_some_and(|values| !values.is_empty())
}

/// The first non-empty lyrics tag with normalized line endings.
fn optional_lyrics(tags: &Tags) -> String {
    for key in ["LYRICS", "UNSYNCEDLYRICS", "UNSYNCED LYRICS"] {
        for value in tags.get(key).into_iter().flatten() {
            let normalized = without_marker(value)
                .replace("\r\n", "\n")
                .replace('\r', "\n");
            let normalized = normalized.trim();
            if !normalized.is_empty() {
                return normalized.to_owned();
            }
        }
    }
    String::new()
}

fn required_artist_credits(tags: &Tags, key: &str) -> Result<Vec<String>> {
    let plural = format!("{key}S");
    if tags.contains_key(&plural) {
        return required_tags(tags, &plural);
    }
    required_tags(tags, key)
}

fn required_single_tag(tags: &Tags, key: &str) -> Result<String> {
    let mut values = required_tags(tags, key)?;
    if values.len() != 1 {
        return Err(error(
            Code::InvalidMetadata,
            key,
            "expected exactly one value",
        ));
    }
    Ok(values.remove(0))
}

fn required_tags(tags: &Tags, key: &str) -> Result<Vec<String>> {
    let raw = tags.get(key).map_or(&[][..], Vec::as_slice);
    if raw.is_empty() {
        return Err(error(Code::InvalidMetadata, key, "required tag is missing"));
    }
    raw.iter()
        .map(|value| {
            normalize_value(value).ok_or_else(|| {
                error(
                    Code::InvalidMetadata,
                    key,
                    "tag is empty, too long, or contains unsafe characters",
                )
            })
        })
        .collect()
}

fn is_unsafe_identity_char(c: char) -> bool {
    let code = u32::from(c);
    c.is_control()
        || matches!(code, 0x061C | 0x200E | 0x200F | 0x202A..=0x202E | 0x2066..=0x2069)
        || (0xFDD0..=0xFDEF).contains(&code)
        || code & 0xFFFE == 0xFFFE
}

pub fn normalize_value(value: &str) -> Option<String> {
    if value.chars().any(is_unsafe_identity_char) {
        return None;
    }
    let value: String = value.trim().nfc().collect();
    if value.len() > MAX_IDENTITY_VALUE_BYTES || value.chars().any(is_unsafe_identity_char) {
        return None;
    }
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!value.is_empty()).then_some(value)
}

/// Go's `strconv.Atoi`.
pub fn atoi(value: &str) -> Option<i64> {
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn track_position(tags: &Tags) -> Result<MediaPosition> {
    let position = required_position(tags, "TRACKNUMBER")?;
    merge_position_total(tags, position, "TOTALTRACKS")
}

fn disc_position(tags: &Tags) -> Result<MediaPosition> {
    let has_disc_number = has_values(tags, "DISCNUMBER");
    let mut position = MediaPosition {
        number: 1,
        total: 0,
    };
    if has_disc_number {
        position = required_position(tags, "DISCNUMBER")?;
    }
    let position = merge_position_total(tags, position, "TOTALDISCS")?;
    if !has_disc_number && position.total > 1 {
        return Err(error(
            Code::InvalidMetadata,
            "DISCNUMBER",
            "required for a multi-disc Album",
        ));
    }
    Ok(position)
}

fn required_position(tags: &Tags, key: &str) -> Result<MediaPosition> {
    let value = required_single_tag(tags, key)?;
    parse_position(&value).map_err(|cause| {
        error(
            Code::InvalidMetadata,
            key,
            format!(
                "position must be between 1 and {MAX_MEDIA_POSITION} with an optional total: {cause}"
            ),
        )
    })
}

fn atoi_error(value: &str) -> String {
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        format!(
            "strconv.Atoi: parsing {}: value out of range",
            go_quote(value)
        )
    } else {
        format!("strconv.Atoi: parsing {}: invalid syntax", go_quote(value))
    }
}

/// `strconv.Quote` for the values that reach error messages.
fn go_quote(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn parse_position(value: &str) -> std::result::Result<MediaPosition, String> {
    let parts: Vec<&str> = value.split('/').collect();
    if parts.len() > 2 {
        return Err("too many position parts".to_owned());
    }
    let first = parts[0].trim();
    let number = atoi(first).ok_or_else(|| atoi_error(first))?;
    if number <= 0 || number > MAX_MEDIA_POSITION {
        return Err("position is outside the supported range".to_owned());
    }
    if parts.len() == 1 || parts[1].trim().is_empty() {
        return Ok(MediaPosition { number, total: 0 });
    }
    match atoi(parts[1].trim()) {
        Some(total) if total >= number && total <= MAX_MEDIA_POSITION => {
            Ok(MediaPosition { number, total })
        }
        _ => Err("position total is invalid".to_owned()),
    }
}

fn merge_position_total(
    tags: &Tags,
    mut position: MediaPosition,
    total_key: &str,
) -> Result<MediaPosition> {
    let Some(total) = optional_total(tags, total_key)? else {
        return Ok(position);
    };
    if position.total > 0 && position.total != total {
        return Err(error(
            Code::InvalidMetadata,
            total_key,
            "total conflicts with the position tag",
        ));
    }
    if total < position.number {
        return Err(error(
            Code::InvalidMetadata,
            total_key,
            "total must not be less than the position",
        ));
    }
    position.total = total;
    Ok(position)
}

fn optional_total(tags: &Tags, key: &str) -> Result<Option<i64>> {
    if !has_values(tags, key) {
        return Ok(None);
    }
    let value = required_single_tag(tags, key)?;
    match atoi(&value) {
        Some(total) if total > 0 && total <= MAX_MEDIA_POSITION => Ok(Some(total)),
        _ => Err(error(
            Code::InvalidMetadata,
            key,
            format!("total must be between 1 and {MAX_MEDIA_POSITION}"),
        )),
    }
}

fn optional_year(tags: &Tags) -> Result<i64> {
    if !has_values(tags, "DATE") {
        return Ok(0);
    }
    let value = required_single_tag(tags, "DATE")?;
    let invalid = || {
        error(
            Code::InvalidMetadata,
            "DATE",
            "date must begin with a four-digit year",
        )
    };
    if value.len() < 4 {
        return Err(invalid());
    }
    let prefix = std::str::from_utf8(&value.as_bytes()[..4]).map_err(|_| invalid())?;
    match atoi(prefix) {
        Some(year) if year > 0 => Ok(year),
        _ => Err(invalid()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[(&str, &str)]) -> Tags {
        let mut tags = Tags::new();
        for (key, value) in pairs {
            tags.entry((*key).to_owned())
                .or_default()
                .push((*value).to_owned());
        }
        tags
    }

    fn complete() -> Vec<(&'static str, &'static str)> {
        vec![
            ("TITLE", " Song  Title "),
            ("ARTIST", "Artist"),
            ("ALBUMARTIST", "Album Artist"),
            ("ALBUM", "Album"),
            ("TRACKNUMBER", "3/12"),
            ("GENRE", "Pop, Rock"),
            ("GENRE", "rock; Jazz"),
            ("DATE", "2021-04-01"),
        ]
    }

    #[test]
    fn normalizes_complete_tags() {
        let metadata = normalize(&tags(&complete()), ReplayGain::default()).unwrap();
        assert_eq!(metadata.title, "Song Title");
        assert_eq!(
            metadata.track_position,
            MediaPosition {
                number: 3,
                total: 12
            }
        );
        assert_eq!(
            metadata.disc_position,
            MediaPosition {
                number: 1,
                total: 0
            }
        );
        assert!(!metadata.has_disc_number);
        assert_eq!(metadata.genres, ["Pop", "Rock", "Jazz"]);
        assert_eq!(metadata.year, 2021);
    }

    #[test]
    fn reports_every_failed_field_in_check_order() {
        let failure = normalize(
            &tags(&[("TITLE", "a"), ("TITLE", "b"), ("TRACKNUMBER", "0")]),
            ReplayGain::default(),
        )
        .unwrap_err();
        let fields: Vec<_> = failure
            .issues()
            .into_iter()
            .map(|issue| (issue.field, issue.reason))
            .collect();
        assert_eq!(
            fields,
            [
                ("TITLE".to_owned(), "expected exactly one value".to_owned()),
                ("ARTIST".to_owned(), "required tag is missing".to_owned()),
                (
                    "ALBUMARTIST".to_owned(),
                    "required tag is missing".to_owned()
                ),
                ("ALBUM".to_owned(), "required tag is missing".to_owned()),
                (
                    "TRACKNUMBER".to_owned(),
                    "position must be between 1 and 9999 with an optional total".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn plural_credits_win_and_disc_totals_need_a_disc_number() {
        let mut pairs = complete();
        pairs.push(("ARTISTS", "One"));
        pairs.push(("ARTISTS", "Two"));
        pairs.push(("TOTALDISCS", "2"));
        let failure = normalize(&tags(&pairs), ReplayGain::default()).unwrap_err();
        assert_eq!(failure.issues()[0].field, "DISCNUMBER");
        pairs.pop();
        let metadata = normalize(&tags(&pairs), ReplayGain::default()).unwrap();
        assert_eq!(metadata.artists, ["One", "Two"]);
    }

    #[test]
    fn rejects_unsafe_values() {
        assert_eq!(normalize_value("a\u{202E}b"), None);
        assert_eq!(normalize_value("a\u{0007}"), None);
        assert_eq!(normalize_value(&tag_string(b"\xffab")), None);
        assert_eq!(normalize_value("   "), None);
        assert_eq!(normalize_value(&"x".repeat(201)), None);
        assert_eq!(normalize_value("e\u{0301}  x").as_deref(), Some("\u{e9} x"));
    }

    #[test]
    fn parses_replay_gain_like_go() {
        assert_eq!(parse_replay_gain_value(" -6.5 dB "), Some(-6.5));
        assert_eq!(parse_replay_gain_value("NaN"), None);
        assert_eq!(parse_replay_gain_peak("-1"), None);
        assert_eq!(parse_replay_gain_peak("0.98"), Some(0.98));
    }

    #[test]
    fn position_errors_carry_go_wording() {
        assert_eq!(
            parse_position("x").unwrap_err(),
            "strconv.Atoi: parsing \"x\": invalid syntax"
        );
        assert_eq!(
            parse_position("2/1").unwrap_err(),
            "position total is invalid"
        );
        assert_eq!(
            parse_position("1/2/3").unwrap_err(),
            "too many position parts"
        );
    }
}
