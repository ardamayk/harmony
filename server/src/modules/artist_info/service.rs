//! Artist photos and biographies: stored results first, an online lookup
//! when there is none or it is due again (ADR 0027).

use super::repository::{Biography, Outcome, Photo, Repository, RepositoryError, Stored};
use super::{deezer, wikipedia};
use crate::modules::library::{LibraryError, LibraryService};
use std::time::Duration;
use tokio::sync::Semaphore;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// A lookup that found nothing is repeated after 30 days, one that failed
/// after an hour.
const MISSING_RETRY_SECONDS: i64 = 30 * 24 * 60 * 60;
const FAILED_RETRY_SECONDS: i64 = 60 * 60;
const DEEZER_LOOKUPS_AT_ONCE: usize = 3;
/// The Artist's Albums the lookups may use to tell homonyms apart.
const ALBUMS_FOR_MATCHING: i64 = 3;

#[derive(Debug, thiserror::Error)]
pub enum ArtistInfoError {
    #[error("artist not found")]
    NotFound,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error(transparent)]
    Library(LibraryError),
}

impl From<LibraryError> for ArtistInfoError {
    fn from(error: LibraryError) -> Self {
        match error {
            LibraryError::NotFound => ArtistInfoError::NotFound,
            other => ArtistInfoError::Library(other),
        }
    }
}

/// Where the lookups go; tests point them at a local server.
#[derive(Clone, Debug)]
pub struct Sources {
    pub deezer: String,
    pub musicbrainz: String,
    pub wikidata: String,
    /// With `{lang}` where the language subdomain goes.
    pub wikipedia: String,
}

impl Sources {
    pub fn public() -> Self {
        Self {
            deezer: "https://api.deezer.com".to_owned(),
            musicbrainz: "https://musicbrainz.org".to_owned(),
            wikidata: "https://www.wikidata.org".to_owned(),
            wikipedia: "https://{lang}.wikipedia.org".to_owned(),
        }
    }
}

pub struct ArtistInfo {
    repo: Repository,
    library: LibraryService,
    client: reqwest::Client,
    sources: Sources,
    /// `false` makes no outbound request; stored results are still served.
    enabled: bool,
    language: String,
    deezer_slots: Semaphore,
    musicbrainz_pacer: wikipedia::Pacer,
}

fn is_due(outcome: Outcome, age_seconds: i64) -> bool {
    match outcome {
        Outcome::Found => false,
        Outcome::Missing => age_seconds >= MISSING_RETRY_SECONDS,
        Outcome::Failed => age_seconds >= FAILED_RETRY_SECONDS,
    }
}

impl ArtistInfo {
    pub fn new(
        pool: sqlx::SqlitePool,
        sources: Sources,
        enabled: bool,
        language: &str,
        version: &str,
    ) -> Self {
        // MusicBrainz asks every client to name itself and how to reach it.
        let user_agent =
            format!("EarthlyAudio/{version} ( https://github.com/ardamayk/test-project-with-ai )");
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(user_agent)
            .build()
            .expect("HTTP client");
        Self {
            repo: Repository::new(pool.clone()),
            library: LibraryService::new(pool),
            client,
            sources,
            enabled,
            language: language.to_owned(),
            deezer_slots: Semaphore::new(DEEZER_LOOKUPS_AT_ONCE),
            musicbrainz_pacer: wikipedia::Pacer::default(),
        }
    }

    /// The Artist's photo, `None` when there is none (yet).
    pub async fn photo(&self, artist_id: &str) -> Result<Option<Photo>, ArtistInfoError> {
        let stored = self.repo.photo(artist_id).await?;
        if !self.should_look_up(stored.as_ref()) {
            return Ok(stored.and_then(|stored| stored.value));
        }
        let (name, albums) = self.artist(artist_id).await?;
        let _slot = self.deezer_slots.acquire().await.expect("semaphore open");
        let found =
            match deezer::find_picture(&self.client, &self.sources.deezer, &name, &albums).await {
                Ok(Some(url)) => super::fetch::bytes(&self.client, &url)
                    .await
                    .map(|(media_type, data)| Some(Photo { media_type, data })),
                Ok(None) => Ok(None),
                Err(error) => Err(error),
            };
        let (outcome, photo) = settle(artist_id, found);
        self.repo
            .put_photo(artist_id, outcome, photo.as_ref())
            .await?;
        Ok(photo)
    }

    /// The Artist's Wikipedia summary, `None` when there is none (yet).
    pub async fn biography(&self, artist_id: &str) -> Result<Option<Biography>, ArtistInfoError> {
        let stored = self.repo.biography(artist_id).await?;
        if !self.should_look_up(stored.as_ref()) {
            return Ok(stored.and_then(|stored| stored.value));
        }
        let (name, albums) = self.artist(artist_id).await?;
        let endpoints = wikipedia::Endpoints {
            musicbrainz: &self.sources.musicbrainz,
            wikidata: &self.sources.wikidata,
            wikipedia: &self.sources.wikipedia,
        };
        let found = wikipedia::find_biography(
            &self.client,
            &endpoints,
            &self.musicbrainz_pacer,
            &name,
            &albums,
            &self.language,
        )
        .await;
        let (outcome, biography) = settle(artist_id, found);
        self.repo
            .put_biography(artist_id, outcome, biography.as_ref())
            .await?;
        Ok(biography)
    }

    fn should_look_up<T>(&self, stored: Option<&Stored<T>>) -> bool {
        self.enabled && stored.is_none_or(|stored| is_due(stored.outcome, stored.age_seconds))
    }

    async fn artist(&self, artist_id: &str) -> Result<(String, Vec<String>), ArtistInfoError> {
        let artist = self.library.get_artist(artist_id).await?;
        let albums = self
            .library
            .list_albums(ALBUMS_FOR_MATCHING, 0, artist_id, "")
            .await?;
        Ok((
            artist.name,
            albums.items.into_iter().map(|album| album.title).collect(),
        ))
    }
}

/// How the lookup ended, to store, and what it found.
fn settle<T>(artist_id: &str, found: Result<Option<T>, String>) -> (Outcome, Option<T>) {
    match found {
        Ok(Some(value)) => (Outcome::Found, Some(value)),
        Ok(None) => (Outcome::Missing, None),
        Err(error) => {
            tracing::warn!(artist_id, %error, "artist info lookup failed");
            (Outcome::Failed, None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{StatusCode, Uri, header};
    use axum::response::IntoResponse;
    use axum::{Json, Router};
    use serde_json::json;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn repeats_lookups_only_when_due() {
        assert!(!is_due(Outcome::Found, i64::MAX));
        assert!(!is_due(Outcome::Missing, MISSING_RETRY_SECONDS - 1));
        assert!(is_due(Outcome::Missing, MISSING_RETRY_SECONDS));
        assert!(!is_due(Outcome::Failed, FAILED_RETRY_SECONDS - 1));
        assert!(is_due(Outcome::Failed, FAILED_RETRY_SECONDS));
    }

    /// Stands in for Deezer, MusicBrainz, Wikidata and Wikipedia, counting
    /// the requests it answers.
    async fn fake_sources() -> (Sources, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = requests.clone();
        // MusicBrainz's first answer is a 503, which is retried.
        let musicbrainz_busy = Arc::new(AtomicBool::new(true));
        let photo_url = format!("{origin}/img/skillet.jpg");
        let app = Router::new().fallback(move |uri: Uri| {
            counter.fetch_add(1, Ordering::SeqCst);
            let photo_url = photo_url.clone();
            let musicbrainz_busy = musicbrainz_busy.clone();
            async move {
                let query = uri.query().unwrap_or("");
                let body = match uri.path() {
                    "/search/artist" if query.contains("Skillet") => json!({"data": [
                        {"name": "Skillet", "picture_big": photo_url, "nb_fan": 5}
                    ]}),
                    "/search/artist" => json!({"data": []}),
                    "/img/skillet.jpg" => {
                        return ([(header::CONTENT_TYPE, "image/jpeg")], b"jpeg".to_vec()).into_response();
                    }
                    "/ws/2/artist" if musicbrainz_busy.swap(false, Ordering::SeqCst) => {
                        return StatusCode::SERVICE_UNAVAILABLE.into_response();
                    }
                    "/ws/2/artist" => json!({"artists": [{"id": "mbid-1", "name": "Skillet", "score": 100}]}),
                    "/ws/2/artist/mbid-1" => json!({"relations": [
                        {"type": "wikidata", "url": {"resource": "https://www.wikidata.org/wiki/Q1"}}
                    ]}),
                    "/wiki/Special:EntityData/Q1.json" => {
                        json!({"entities": {"Q1": {"sitelinks": {"enwiki": {"title": "Skillet (band)"}}}}})
                    }
                    path if path.starts_with("/en/api/rest_v1/page/summary/Skillet_") => json!({
                        "type": "standard",
                        "extract": "Skillet is a rock band.",
                        "content_urls": {"desktop": {"page": "https://en.wikipedia.org/wiki/Skillet_(band)"}}
                    }),
                    _ => return StatusCode::NOT_FOUND.into_response(),
                };
                Json(body).into_response()
            }
        });
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let sources = Sources {
            deezer: origin.clone(),
            musicbrainz: origin.clone(),
            wikidata: origin.clone(),
            wikipedia: format!("{origin}/{{lang}}"),
        };
        (sources, requests)
    }

    async fn service(sources: Sources, enabled: bool) -> (tempfile::TempDir, ArtistInfo) {
        let dir = tempfile::tempdir().unwrap();
        let migrations = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let pool =
            crate::db::open_and_migrate(dir.path().join("app.db").to_str().unwrap(), &migrations)
                .await
                .unwrap();
        sqlx::query("INSERT INTO artists (id, name, name_sort) VALUES ('ar', 'Skillet', 'skillet'), ('no', 'Nobody', 'nobody')")
            .execute(&pool)
            .await
            .unwrap();
        (dir, ArtistInfo::new(pool, sources, enabled, "en", "test"))
    }

    #[tokio::test]
    async fn looks_up_once_and_then_serves_what_it_stored() {
        let (sources, requests) = fake_sources().await;
        let (_dir, info) = service(sources, true).await;
        let photo = info.photo("ar").await.unwrap().expect("photo");
        assert_eq!(
            (photo.media_type.as_str(), photo.data.as_slice()),
            ("image/jpeg", &b"jpeg"[..])
        );
        let biography = info.biography("ar").await.unwrap().expect("biography");
        assert_eq!(biography.text, "Skillet is a rock band.");
        assert_eq!(
            biography.source_url,
            "https://en.wikipedia.org/wiki/Skillet_(band)"
        );
        let made = requests.load(Ordering::SeqCst);
        assert_eq!(info.photo("ar").await.unwrap(), Some(photo));
        assert_eq!(info.biography("ar").await.unwrap(), Some(biography));
        assert_eq!(
            requests.load(Ordering::SeqCst),
            made,
            "stored results make no request"
        );
    }

    #[tokio::test]
    async fn remembers_when_nothing_was_found() {
        let (sources, requests) = fake_sources().await;
        let (_dir, info) = service(sources, true).await;
        assert_eq!(info.photo("no").await.unwrap(), None);
        let made = requests.load(Ordering::SeqCst);
        assert_eq!(info.photo("no").await.unwrap(), None);
        assert_eq!(requests.load(Ordering::SeqCst), made);
        assert!(matches!(
            info.photo("unknown").await,
            Err(ArtistInfoError::NotFound)
        ));
    }

    #[tokio::test]
    async fn makes_no_request_when_switched_off() {
        let (sources, requests) = fake_sources().await;
        let (_dir, info) = service(sources, false).await;
        assert_eq!(info.photo("ar").await.unwrap(), None);
        assert_eq!(info.biography("ar").await.unwrap(), None);
        assert_eq!(requests.load(Ordering::SeqCst), 0);
    }
}
