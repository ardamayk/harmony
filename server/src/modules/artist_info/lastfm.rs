//! Last.fm's play counts for an Artist's most played Tracks
//! (`artist.getTopTracks`), with the operator's own API key (ADR 0030).

use super::fetch;
use super::popular::{base_title, folded};
use serde::Deserialize;
use std::collections::HashMap;

/// One request covers an Artist's 1000 most played Tracks, Last.fm's most.
const TOP_TRACKS: &str = "1000";
/// Last.fm's "The artist you supplied could not be found".
const ARTIST_NOT_FOUND: i64 = 6;

#[derive(Deserialize)]
struct Answer {
    toptracks: Option<TopTracks>,
    error: Option<i64>,
    message: Option<String>,
}

#[derive(Deserialize)]
struct TopTracks {
    #[serde(default)]
    track: Vec<TopTrack>,
}

#[derive(Deserialize)]
struct TopTrack {
    name: String,
    /// Last.fm sends counts as strings.
    playcount: String,
}

/// Play counts by folded title; empty when Last.fm does not know the Artist.
pub async fn play_counts(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    artist: &str,
) -> Result<HashMap<String, i64>, String> {
    let url = format!("{base_url}/2.0/");
    let answer: Answer = fetch::json(
        client,
        &url,
        &[
            ("method", "artist.getTopTracks"),
            ("artist", artist),
            ("autocorrect", "1"),
            ("limit", TOP_TRACKS),
            ("format", "json"),
            ("api_key", api_key),
        ],
    )
    .await
    // The URL carries the key, so errors name the method instead.
    .map_err(|error| format!("Last.fm artist.getTopTracks: HTTP {:?}", error.status))?;
    match answer.error {
        Some(ARTIST_NOT_FOUND) => return Ok(HashMap::new()),
        Some(code) => {
            let message = answer.message.unwrap_or_default();
            return Err(format!(
                "Last.fm artist.getTopTracks: error {code} {message}"
            ));
        }
        None => {}
    }
    Ok(by_title(
        answer.toptracks.map(|top| top.track).unwrap_or_default(),
    ))
}

/// Counts by folded title, versions kept: "Red (Taylor's Version)" is not
/// the 2012 "Red".
fn by_title(tracks: Vec<TopTrack>) -> HashMap<String, i64> {
    let mut counts = HashMap::new();
    for track in tracks {
        let Ok(count) = track.playcount.trim().parse::<i64>() else {
            continue;
        };
        let title = folded(&track.name);
        if title.is_empty() {
            continue;
        }
        let best = counts.entry(title).or_insert(count);
        *best = (*best).max(count);
    }
    counts
}

/// A library Track's count: its own title first; when Last.fm lists only
/// other editions ("- Remastered", "(Live)"), the most played of them.
pub fn count_for(counts: &HashMap<String, i64>, title: &str) -> Option<i64> {
    if let Some(&count) = counts.get(&folded(title)) {
        return Some(count);
    }
    let base = base_title(title);
    counts
        .iter()
        .filter(|(listed, _)| base_title(listed) == base)
        .map(|(_, &count)| count)
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_exact_edition_first_then_the_most_played_other_one() {
        let answer: Answer = serde_json::from_str(
            r#"{"toptracks":{"track":[
                {"name":"Red","playcount":"20000000"},
                {"name":"Red (Taylor's Version)","playcount":"14955446"},
                {"name":"All Too Well (10 Minute Version) (Taylor's Version) (From The Vault)","playcount":"30047454"},
                {"name":"All Too Well","playcount":"9000000"},
                {"name":"My Walden - Remastered","playcount":"26086"},
                {"name":"Broken","playcount":"n/a"}
            ]}}"#,
        )
        .expect("answer");
        let counts = by_title(answer.toptracks.expect("tracks").track);
        assert_eq!(counts.len(), 5);
        assert_eq!(
            count_for(&counts, "RED (Taylor's Version)"),
            Some(14_955_446)
        );
        assert_eq!(count_for(&counts, "Red"), Some(20_000_000));
        assert_eq!(count_for(&counts, "All Too Well"), Some(9_000_000));
        // Only another edition is listed: that one stands in.
        assert_eq!(count_for(&counts, "My Walden"), Some(26_086));
        assert_eq!(count_for(&counts, "Lucy"), None);
    }

    #[test]
    fn reads_last_fm_errors() {
        let answer: Answer = serde_json::from_str(
            r#"{"error":6,"message":"The artist you supplied could not be found"}"#,
        )
        .expect("answer");
        assert_eq!(answer.error, Some(ARTIST_NOT_FOUND));
        assert!(answer.toptracks.is_none());
    }
}
