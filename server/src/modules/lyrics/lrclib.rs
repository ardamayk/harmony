//! LRCLIB's public API (no key): the lyrics record for a Track's title,
//! Artist, Album and duration.

use serde::Deserialize;
use std::time::Duration;

/// A longer Retry-After counts as a failure, retried an hour later.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(2);

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
    if (1..=3600).contains(&query.duration_seconds) {
        params.push(("duration", &duration));
    }
    let url = url::Url::parse_with_params(&format!("{base_url}/api/get"), &params)
        .map_err(|error| format!("lrclib url: {error}"))?;
    let mut response = get(client, &url).await?;
    // 503 means LRCLIB is briefly busy; one retry after its Retry-After.
    if response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE
        && let Some(wait) = retry_after(&response).filter(|wait| *wait <= MAX_RETRY_WAIT)
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
    let record: Record =
        serde_json::from_slice(&body).map_err(|error| format!("GET {url}: {error}"))?;
    Ok(read(record))
}

async fn get(client: &reqwest::Client, url: &url::Url) -> Result<reqwest::Response, String> {
    client
        .get(url.clone())
        .send()
        .await
        .map_err(|error| format!("GET {url}: {error}"))
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
