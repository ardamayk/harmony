//! LRCLIB's public API (no key): the lyrics record for a Track's title,
//! Artist, Album and duration. Its best match for that signature may hold
//! only plain lyrics while another edition of the same length (within 2 s,
//! so the timing fits) is synced; the search finds that one.

use crate::observability;
use serde::Deserialize;
use std::time::{Duration, Instant};

/// A longer Retry-After counts as a failure, retried a few minutes later.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(2);
/// LRCLIB's usual Retry-After for a 503, used when the header is missing.
const DEFAULT_RETRY_WAIT: Duration = Duration::from_secs(1);
/// LRCLIB's own tolerance between a Track's and a record's duration.
const DURATION_TOLERANCE_SECONDS: f64 = 2.0;

/// What LRCLIB knows for a Track.
#[derive(Debug, PartialEq)]
pub enum Found {
    /// Synced (LRC) lyrics when there are any, else plain ones.
    Lyrics(String),
    Instrumental,
}

pub struct Query<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub album: &'a str,
    pub duration_seconds: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    #[serde(default)]
    instrumental: bool,
    #[serde(default)]
    duration: f64,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
}

/// `None` when LRCLIB has no record within 2 s of the duration.
pub async fn find(
    client: &reqwest::Client,
    base_url: &str,
    query: &Query<'_>,
) -> Result<Option<Found>, String> {
    let duration = query.duration_seconds.to_string();
    let mut params = vec![("track_name", query.title), ("artist_name", query.artist)];
    if !query.album.is_empty() {
        params.push(("album_name", query.album));
    }
    // LRCLIB takes 1 to 3600 seconds; outside that the duration is left out.
    let has_duration = (1..=3600).contains(&query.duration_seconds);
    if has_duration {
        params.push(("duration", &duration));
    }
    let best = request::<Record>(client, base_url, "/api/get", &params)
        .await?
        .and_then(read);
    let synced = matches!(&best, Some(Found::Lyrics(text)) if is_synced(text));
    if synced || best == Some(Found::Instrumental) || !has_duration {
        return Ok(best);
    }
    // The search is a second chance only: its failure keeps what get found.
    let params = [("track_name", query.title), ("artist_name", query.artist)];
    match request::<Vec<Record>>(client, base_url, "/api/search", &params).await {
        Ok(records) => Ok(
            pick_synced(records.unwrap_or_default(), query.duration_seconds)
                .map(Found::Lyrics)
                .or(best),
        ),
        Err(error) => {
            tracing::debug!(%error, "lrclib search failed");
            Ok(best)
        }
    }
}

/// Whether the text carries LRC line timestamps (`[mm:ss.xx]`).
pub fn is_synced(text: &str) -> bool {
    text.lines().any(|line| {
        line.trim_start()
            .strip_prefix('[')
            .and_then(|rest| rest.chars().next())
            .is_some_and(|first| first.is_ascii_digit())
    })
}

/// The synced lyrics of the record closest in length, within 2 s.
fn pick_synced(records: Vec<Record>, duration_seconds: i64) -> Option<String> {
    let target = duration_seconds as f64;
    records
        .into_iter()
        .filter(|record| !record.instrumental)
        .filter(|record| (record.duration - target).abs() <= DURATION_TOLERANCE_SECONDS)
        .filter_map(|record| {
            let distance = (record.duration - target).abs();
            record
                .synced_lyrics
                .filter(|text| is_synced(text))
                .map(|text| (distance, text))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, text)| text)
}

/// The decoded body; `None` for a 404. A 503 is retried once after its
/// Retry-After, since LRCLIB is then only briefly busy.
async fn request<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    base_url: &str,
    path: &str,
    params: &[(&str, &str)],
) -> Result<Option<T>, String> {
    let url = url::Url::parse_with_params(&format!("{base_url}{path}"), params)
        .map_err(|error| format!("lrclib url: {error}"))?;
    let mut response = get(client, &url).await?;
    if response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE
        && let Some(wait) = Some(retry_after(&response).unwrap_or(DEFAULT_RETRY_WAIT))
            .filter(|wait| *wait <= MAX_RETRY_WAIT)
    {
        tokio::time::sleep(wait).await;
        response = get(client, &url).await?;
    }
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(format!("GET {url}: HTTP {status}"));
    }
    let body = response
        .bytes()
        .await
        .map_err(|error| format!("GET {url}: {error}"))?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|error| format!("GET {url}: {error}"))
}

async fn get(client: &reqwest::Client, url: &url::Url) -> Result<reqwest::Response, String> {
    let started = Instant::now();
    let result = client.get(url.clone()).send().await;
    let worked = matches!(&result, Ok(response) if !response.status().is_server_error());
    observability::outbound_request(url.as_str(), started, worked);
    result.map_err(|error| format!("GET {url}: {error}"))
}

fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    let seconds: u64 = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(seconds))
}

fn read(record: Record) -> Option<Found> {
    if record.instrumental {
        return Some(Found::Instrumental);
    }
    [record.synced_lyrics, record.plain_lyrics]
        .into_iter()
        .flatten()
        .find(|text| !text.trim().is_empty())
        .map(Found::Lyrics)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(json: &str) -> Record {
        serde_json::from_str(json).expect("record")
    }

    #[test]
    fn tells_synced_lyrics_from_plain() {
        assert!(is_synced("Intro\n[00:12.00] la"));
        assert!(!is_synced("[Chorus]\nla la"));
        assert!(!is_synced(""));
    }

    #[test]
    fn picks_the_closest_synced_record_within_two_seconds() {
        let records = vec![
            record(r#"{"duration":1441.0,"syncedLyrics":"[00:01.00] deluxe","plainLyrics":"x"}"#),
            record(r#"{"duration":1439.4,"syncedLyrics":null,"plainLyrics":"plain only"}"#),
            record(
                r#"{"duration":1439.6,"instrumental":true,"syncedLyrics":"[00:01.00] no","plainLyrics":null}"#,
            ),
            record(
                r#"{"duration":1274.0,"syncedLyrics":"[00:01.00] short edit","plainLyrics":"x"}"#,
            ),
            record(r#"{"duration":1438.2,"syncedLyrics":"[00:01.00] closest","plainLyrics":"x"}"#),
        ];
        assert_eq!(
            pick_synced(records, 1439).as_deref(),
            Some("[00:01.00] closest")
        );
        let far = vec![record(
            r#"{"duration":1450.0,"syncedLyrics":"[00:01.00] far","plainLyrics":null}"#,
        )];
        assert_eq!(pick_synced(far, 1439), None);
    }

    #[test]
    fn prefers_synced_lyrics_then_plain_and_knows_instrumentals() {
        assert_eq!(
            read(record(
                r#"{"plainLyrics":"la","syncedLyrics":"[00:01.00] la"}"#
            )),
            Some(Found::Lyrics("[00:01.00] la".to_owned()))
        );
        assert_eq!(
            read(record(r#"{"plainLyrics":"la","syncedLyrics":" "}"#)),
            Some(Found::Lyrics("la".to_owned()))
        );
        assert_eq!(
            read(record(
                r#"{"instrumental":true,"plainLyrics":null,"syncedLyrics":null}"#
            )),
            Some(Found::Instrumental)
        );
        assert_eq!(
            read(record(r#"{"plainLyrics":null,"syncedLyrics":null}"#)),
            None
        );
    }
}
