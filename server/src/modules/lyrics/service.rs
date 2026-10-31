//! A Track's lyrics: the file's own first, else a stored LRCLIB result, else
//! an online lookup when there is none or it is due again (ADR 0029).

use super::lrclib::{self, Found};
use super::repository::{Outcome, Repository, RepositoryError};
use crate::modules::library::{LibraryError, LibraryService};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// LRCLIB starts fetching a missing Track in the background on the first
/// request, so nothing found, or plain lyrics only, is asked again after
/// three days, when synced ones may exist. LRCLIB is often briefly
/// overloaded, so a failure is retried after five minutes.
const MISSING_RETRY_SECONDS: i64 = 3 * 24 * 60 * 60;
const FAILED_RETRY_SECONDS: i64 = 5 * 60;
/// LRCLIB asks clients to send requests one at a time with a short gap.
const REQUEST_GAP: Duration = Duration::from_millis(300);

#[derive(Debug, thiserror::Error)]
pub enum LyricsError {
    #[error("track not found")]
    NotFound,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error(transparent)]
    Library(LibraryError),
}

impl From<LibraryError> for LyricsError {
    fn from(error: LibraryError) -> Self {
        match error {
            LibraryError::NotFound => LyricsError::NotFound,
            other => LyricsError::Library(other),
        }
    }
}

pub struct Lyrics {
    repo: Repository,
    library: LibraryService,
    client: reqwest::Client,
    base_url: String,
    /// `false` makes no outbound request; stored results are still served.
    enabled: bool,
    /// When the last LRCLIB request started; held while one runs.
    last_request: Mutex<Option<Instant>>,
}

/// `plain`: found lyrics without timestamps, worth asking again for synced.
fn is_due(outcome: Outcome, age_seconds: i64, plain: bool) -> bool {
    match outcome {
        Outcome::Found if plain => age_seconds >= MISSING_RETRY_SECONDS,
        Outcome::Found => false,
        Outcome::Missing => age_seconds >= MISSING_RETRY_SECONDS,
        Outcome::Failed => age_seconds >= FAILED_RETRY_SECONDS,
    }
}

impl Lyrics {
    pub fn new(pool: crate::db::Db, base_url: &str, enabled: bool, version: &str) -> Self {
        // LRCLIB requires clients to name themselves and a project page.
        let user_agent = format!("EarthlyAudio/{version} (https://github.com/ardamayk/harmony)");
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(user_agent)
            .build()
            .expect("HTTP client");
        Self {
            repo: Repository::new(pool.clone()),
            library: LibraryService::new(pool),
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
            enabled,
            last_request: Mutex::new(None),
        }
    }

    /// The Track's lyrics, plain or LRC; empty when there are none (yet).
    pub async fn lyrics(&self, track_id: &str) -> Result<String, LyricsError> {
        let from_file = self.library.get_track_lyrics(track_id).await?;
        if !from_file.trim().is_empty() {
            return Ok(from_file);
        }
        let stored = self.repo.get(track_id).await?;
        let due = stored.as_ref().is_none_or(|stored| {
            let plain = !stored.lyrics.trim().is_empty() && !lrclib::is_synced(&stored.lyrics);
            is_due(stored.outcome, stored.age_seconds, plain)
        });
        if !self.enabled || !due {
            return Ok(stored.map(|stored| stored.lyrics).unwrap_or_default());
        }
        let track = self.library.get_track(track_id).await?;
        // The first credited Artist: LRCLIB matches one name, not "A, B".
        let artist = track
            .artists
            .first()
            .map(|credit| credit.name.as_str())
            .unwrap_or(&track.artist_name);
        let query = lrclib::Query {
            title: &track.title,
            artist,
            album: &track.album_title,
            duration_seconds: (track.duration_ms + 500) / 1000,
        };
        let found = {
            let mut last = self.last_request.lock().await;
            if let Some(previous) = *last {
                tokio::time::sleep_until((previous + REQUEST_GAP).into()).await;
            }
            *last = Some(Instant::now());
            lrclib::find(&self.client, &self.base_url, &query).await
        };
        let (outcome, lyrics) = match found {
            Ok(Some(Found::Lyrics(text))) => (Outcome::Found, text),
            Ok(Some(Found::Instrumental)) => (Outcome::Found, String::new()),
            Ok(None) => (Outcome::Missing, String::new()),
            // Keep what was stored (plain lyrics due for a synced refresh):
            // the failed lookup brought nothing to replace it.
            Err(error) => {
                tracing::warn!(track_id, %error, "lyrics lookup failed");
                (
                    Outcome::Failed,
                    stored.map(|stored| stored.lyrics).unwrap_or_default(),
                )
            }
        };
        self.repo.put(track_id, outcome, &lyrics).await?;
        Ok(lyrics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{StatusCode, Uri};
    use axum::response::IntoResponse;
    use axum::{Json, Router};
    use serde_json::json;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn repeats_lookups_only_when_due() {
        assert!(!is_due(Outcome::Found, i64::MAX, false));
        assert!(!is_due(Outcome::Found, MISSING_RETRY_SECONDS - 1, true));
        assert!(is_due(Outcome::Found, MISSING_RETRY_SECONDS, true));
        assert!(!is_due(Outcome::Missing, MISSING_RETRY_SECONDS - 1, false));
        assert!(is_due(Outcome::Missing, MISSING_RETRY_SECONDS, false));
        assert!(!is_due(Outcome::Failed, FAILED_RETRY_SECONDS - 1, false));
        assert!(is_due(Outcome::Failed, FAILED_RETRY_SECONDS, false));
    }

    /// Stands in for LRCLIB: synced lyrics for "Hero" at 187 s; for "Lucy"
    /// plain ones from get and a synced edition 1.4 s longer from search;
    /// nothing else. Counts the requests and keeps the last get query.
    async fn fake_lrclib() -> (String, Arc<AtomicUsize>, Arc<std::sync::Mutex<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let last_query = Arc::new(std::sync::Mutex::new(String::new()));
        let (counter, seen) = (requests.clone(), last_query.clone());
        let app = Router::new().fallback(move |uri: Uri| {
            // LRCLIB answers below `/api`; local tools probe open ports at `/`.
            if uri.path() != "/" {
                counter.fetch_add(1, Ordering::SeqCst);
            }
            let query = uri.query().unwrap_or("").to_owned();
            if uri.path() == "/api/get" {
                *seen.lock().unwrap() = query.clone();
            }
            async move {
                match uri.path() {
                    "/api/get" if query.contains("track_name=Hero") && query.contains("duration=187") => {
                        Json(json!({
                            "duration": 187.0,
                            "plainLyrics": "I'm just a step away",
                            "syncedLyrics": "[00:12.00] I'm just a step away"
                        }))
                        .into_response()
                    }
                    "/api/get" if query.contains("track_name=Lucy") => Json(json!({
                        "duration": 222.0,
                        "plainLyrics": "Hey Lucy",
                        "syncedLyrics": null
                    }))
                    .into_response(),
                    "/api/search" if query.contains("track_name=Lucy") => Json(json!([
                        {"duration": 240.0, "plainLyrics": "x", "syncedLyrics": "[00:01.00] live"},
                        {"duration": 223.4, "plainLyrics": "x", "syncedLyrics": "[00:20.00] Hey Lucy"}
                    ]))
                    .into_response(),
                    "/api/search" => Json(json!([])).into_response(),
                    _ => StatusCode::NOT_FOUND.into_response(),
                }
            }
        });
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (origin, requests, last_query)
    }

    /// A library with Skillet's "Hero" (no lyrics in the file, 186.6 s),
    /// "Monster" (lyrics in the file), "Lucy" (plain on LRCLIB, synced in
    /// another edition) and "Gone" (unknown to LRCLIB).
    async fn service(base_url: &str, enabled: bool) -> (tempfile::TempDir, Lyrics) {
        let dir = tempfile::tempdir().unwrap();
        let migrations = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let pool =
            crate::db::open_and_migrate(dir.path().join("app.db").to_str().unwrap(), &migrations)
                .await
                .unwrap();
        for statement in [
            "INSERT INTO artists (id, name, name_sort) VALUES ('ar', 'Skillet', 'skillet')",
            "INSERT INTO albums (id, artist_id, title, title_sort) VALUES ('al', 'ar', 'Awake', 'awake')",
            "INSERT INTO tracks (id, album_id, title, title_sort, artist_name, format, file_path, duration_ms, lyrics)
             VALUES ('hero', 'al', 'Hero', 'hero', 'Skillet', 'flac', '/1.flac', 186600, ''),
                    ('monster', 'al', 'Monster', 'monster', 'Skillet', 'flac', '/2.flac', 178000, 'From the file'),
                    ('lucy', 'al', 'Lucy', 'lucy', 'Skillet', 'flac', '/3.flac', 222000, ''),
                    ('gone', 'al', 'Gone', 'gone', 'Skillet', 'flac', '/4.flac', 200000, '')",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        (dir, Lyrics::new(pool, base_url, enabled, "test"))
    }

    #[tokio::test]
    async fn looks_up_lrclib_once_when_the_file_has_none() {
        let (origin, requests, last_query) = fake_lrclib().await;
        let (_dir, lyrics) = service(&origin, true).await;
        assert_eq!(
            lyrics.lyrics("hero").await.unwrap(),
            "[00:12.00] I'm just a step away"
        );
        let query = last_query.lock().unwrap().clone();
        assert!(query.contains("artist_name=Skillet"), "{query}");
        assert!(query.contains("album_name=Awake"), "{query}");
        assert_eq!(
            lyrics.lyrics("hero").await.unwrap(),
            "[00:12.00] I'm just a step away"
        );
        assert_eq!(
            requests.load(Ordering::SeqCst),
            1,
            "stored lyrics make no request"
        );
    }

    #[tokio::test]
    async fn keeps_the_files_lyrics_and_remembers_nothing_found() {
        let (origin, requests, _) = fake_lrclib().await;
        let (_dir, lyrics) = service(&origin, true).await;
        assert_eq!(lyrics.lyrics("monster").await.unwrap(), "From the file");
        assert_eq!(requests.load(Ordering::SeqCst), 0);
        // Not found by get, so the search runs too; then nothing is asked.
        assert_eq!(lyrics.lyrics("gone").await.unwrap(), "");
        assert_eq!(lyrics.lyrics("gone").await.unwrap(), "");
        assert_eq!(requests.load(Ordering::SeqCst), 2);
        assert!(matches!(
            lyrics.lyrics("unknown").await,
            Err(LyricsError::NotFound)
        ));
    }

    #[tokio::test]
    async fn takes_a_synced_edition_of_the_same_length_over_plain_lyrics() {
        let (origin, requests, _) = fake_lrclib().await;
        let (_dir, lyrics) = service(&origin, true).await;
        assert_eq!(lyrics.lyrics("lucy").await.unwrap(), "[00:20.00] Hey Lucy");
        assert_eq!(requests.load(Ordering::SeqCst), 2);
        // A synced hit is the found Track itself: no search for "Hero".
        assert_eq!(
            lyrics.lyrics("hero").await.unwrap(),
            "[00:12.00] I'm just a step away"
        );
        assert_eq!(requests.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn keeps_stored_lyrics_when_a_refresh_fails() {
        // Nothing listens on port 1, so the lookup fails.
        let (dir, lyrics) = service("http://127.0.0.1:1", true).await;
        let pool =
            sqlx::SqlitePool::connect(&format!("sqlite://{}", dir.path().join("app.db").display()))
                .await
                .unwrap();
        sqlx::query(
            "INSERT INTO track_online_lyrics (track_id, outcome, lyrics, checked_at)
             VALUES ('gone', 'found', 'Plain words', datetime('now', '-4 days'))",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(lyrics.lyrics("gone").await.unwrap(), "Plain words");
        // Stored as a failed attempt: the next read waits out the backoff.
        assert_eq!(lyrics.lyrics("gone").await.unwrap(), "Plain words");
    }

    #[tokio::test]
    async fn makes_no_request_when_switched_off() {
        let (origin, requests, _) = fake_lrclib().await;
        let (_dir, lyrics) = service(&origin, false).await;
        assert_eq!(lyrics.lyrics("hero").await.unwrap(), "");
        assert_eq!(requests.load(Ordering::SeqCst), 0);
    }
}
