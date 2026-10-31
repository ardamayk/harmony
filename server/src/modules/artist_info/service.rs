//! Artist photos, biographies and popular Tracks, and genres for Albums
//! whose tags name none: stored results first, an online lookup when there
//! is none or it is due again (ADR 0027).

use super::repository::{Biography, Outcome, Photo, Repository, RepositoryError, Stored};
use super::{deezer, lastfm, popular, wikipedia};
use crate::modules::library::{LibraryError, LibraryService, Track};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Semaphore, SemaphorePermit};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// A cover at least this wide fills the Album page well enough to keep.
const SHARP_COVER_EDGE: u32 = 1000;
/// How far from square a cover and Deezer's may be and still be one picture.
const MAX_COVER_ASPECT_DIFFERENCE: f64 = 0.05;
/// A lookup that found nothing is repeated after 30 days. One that failed,
/// mostly a burst (the Artists page asking for every photo at once) over a
/// rate limit, after five minutes.
const MISSING_RETRY_SECONDS: i64 = 30 * 24 * 60 * 60;
const FAILED_RETRY_SECONDS: i64 = 5 * 60;
/// Popularity moves, so a found top-Track list is refreshed weekly.
const TOP_TITLES_REFRESH_SECONDS: i64 = 7 * 24 * 60 * 60;
/// Play counts move as slowly as popularity: refreshed weekly too.
const PLAY_COUNTS_REFRESH_SECONDS: i64 = TOP_TITLES_REFRESH_SECONDS;
/// Lookup time one play-count request may spend, well inside the 60s
/// request timeout.
const PLAY_COUNTS_LOOKUP_BUDGET: Duration = Duration::from_secs(30);
/// The Artist's library Tracks that the top titles are matched against.
const TRACKS_FOR_MATCHING: i64 = 2000;
const DEEZER_LOOKUPS_AT_ONCE: usize = 3;
/// The Artist's Albums the lookups may use to tell homonyms apart.
const ALBUMS_FOR_MATCHING: i64 = 3;
/// How often Albums without an explicit lookup are looked for, new ones included.
const EXPLICIT_SWEEP_EVERY: Duration = Duration::from_secs(10 * 60);
/// A lookup is two requests, so this stays well inside Deezer's rate limit.
const EXPLICIT_LOOKUP_PAUSE: Duration = Duration::from_millis(500);
const EXPLICIT_SWEEP_PAGE: i64 = 200;

#[derive(Debug, thiserror::Error)]
pub enum ArtistInfoError {
    /// The Artist or Album asked about is not in the library.
    #[error("not found")]
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
    pub lastfm: String,
}

impl Sources {
    pub fn public() -> Self {
        Self {
            deezer: "https://api.deezer.com".to_owned(),
            musicbrainz: "https://musicbrainz.org".to_owned(),
            wikidata: "https://www.wikidata.org".to_owned(),
            wikipedia: "https://{lang}.wikipedia.org".to_owned(),
            lastfm: "https://ws.audioscrobbler.com".to_owned(),
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
    /// The operator's Last.fm API key; without one no play counts.
    lastfm_key: Option<String>,
    /// One Last.fm lookup at a time, well inside its rate limit.
    lastfm_slot: Semaphore,
}

/// A slot of a rate limit, whose queue can outlast the lookup it holds up.
async fn take_slot(slots: &Semaphore) -> SemaphorePermit<'_> {
    let started = Instant::now();
    let slot = slots.acquire().await.expect("semaphore open");
    crate::observability::record_span(started, "artist_info.wait", started.elapsed(), &[], &[]);
    slot
}

/// A library Track's Last.fm play count.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayCount {
    pub track_id: String,
    pub play_count: i64,
}

/// `found_refresh`: seconds after which a found result is looked up again;
/// `None` keeps it for good.
fn is_due(outcome: Outcome, age_seconds: i64, found_refresh: Option<i64>) -> bool {
    match outcome {
        Outcome::Found => found_refresh.is_some_and(|refresh| age_seconds >= refresh),
        Outcome::Missing => age_seconds >= MISSING_RETRY_SECONDS,
        Outcome::Failed => age_seconds >= FAILED_RETRY_SECONDS,
    }
}

impl ArtistInfo {
    pub fn new(
        pool: crate::db::Db,
        sources: Sources,
        enabled: bool,
        language: &str,
        version: &str,
        lastfm_key: Option<String>,
    ) -> Self {
        // MusicBrainz asks every client to name itself and how to reach it.
        let user_agent = format!("EarthlyAudio/{version} ( https://github.com/ardamayk/harmony )");
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
            lastfm_key,
            lastfm_slot: Semaphore::new(1),
        }
    }

    /// Last.fm play counts for these library Tracks, by each Track's first
    /// credited Artist; Tracks without a count (or unknown) are left out,
    /// and there are none without a Last.fm key.
    pub async fn play_counts(
        &self,
        track_ids: &[String],
    ) -> Result<Vec<PlayCount>, ArtistInfoError> {
        let Some(key) = self.lastfm_key.as_deref() else {
            return Ok(Vec::new());
        };
        let mut by_artist: HashMap<String, HashMap<String, i64>> = HashMap::new();
        let mut found = Vec::new();
        // Lookups run one at a time (rate limit), so past the budget the
        // rest come from what is stored: partial counts, not a timed-out request.
        let deadline = tokio::time::Instant::now() + PLAY_COUNTS_LOOKUP_BUDGET;
        for track_id in track_ids {
            let track = match self.library.get_track(track_id).await {
                Ok(track) => track,
                Err(LibraryError::NotFound) => continue,
                Err(error) => return Err(error.into()),
            };
            // A credit without an id is only a name, nothing to store counts under.
            let Some(artist) = track
                .artists
                .iter()
                .chain(&track.album_artists)
                .find(|credit| !credit.id.is_empty())
            else {
                continue;
            };
            if !by_artist.contains_key(&artist.id) {
                let lookup = self.artist_play_counts(key, &artist.id, &artist.name);
                let counts = match tokio::time::timeout_at(deadline, lookup).await {
                    Ok(counts) => counts?,
                    Err(_) => self
                        .repo
                        .play_counts(&artist.id)
                        .await?
                        .and_then(|stored| stored.value)
                        .unwrap_or_default(),
                };
                by_artist.insert(artist.id.clone(), counts);
            }
            if let Some(play_count) = lastfm::count_for(&by_artist[&artist.id], &track.title) {
                found.push(PlayCount {
                    track_id: track.id,
                    play_count,
                });
            }
        }
        Ok(found)
    }

    async fn artist_play_counts(
        &self,
        key: &str,
        artist_id: &str,
        name: &str,
    ) -> Result<HashMap<String, i64>, ArtistInfoError> {
        let stored = self.repo.play_counts(artist_id).await?;
        // The key is its own opt-in (ADR 0030): `enabled` covers only the
        // keyless lookups.
        let due = stored.as_ref().is_none_or(|stored| {
            is_due(
                stored.outcome,
                stored.age_seconds,
                Some(PLAY_COUNTS_REFRESH_SECONDS),
            )
        });
        if !due {
            return Ok(stored.and_then(|stored| stored.value).unwrap_or_default());
        }
        let _slot = take_slot(&self.lastfm_slot).await;
        let found = lastfm::play_counts(&self.client, &self.sources.lastfm, key, name)
            .await
            .map(|counts| (!counts.is_empty()).then_some(counts));
        let (outcome, mut counts) = settle(artist_id, found);
        // A failed refresh keeps last week's counts rather than showing none,
        // stored as failed so the retry waits out the failure backoff.
        if outcome == Outcome::Failed {
            counts = stored.and_then(|stored| stored.value).or(counts);
        }
        self.repo
            .put_play_counts(artist_id, outcome, counts.as_ref())
            .await?;
        Ok(counts.unwrap_or_default())
    }

    /// The Artist's photo, `None` when there is none (yet).
    pub async fn photo(&self, artist_id: &str) -> Result<Option<Photo>, ArtistInfoError> {
        let stored = self.repo.photo(artist_id).await?;
        if !self.should_look_up(stored.as_ref(), None) {
            return Ok(stored.and_then(|stored| stored.value));
        }
        let (name, albums) = self.artist(artist_id).await?;
        let _slot = take_slot(&self.deezer_slots).await;
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
        if !self.should_look_up(stored.as_ref(), None) {
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

    /// The Artist's library Tracks among their most popular on Deezer, best
    /// first, at most `limit`; empty when there is no list (yet).
    pub async fn popular_tracks(
        &self,
        artist_id: &str,
        limit: usize,
    ) -> Result<Vec<Track>, ArtistInfoError> {
        let titles = self.top_titles(artist_id).await?;
        if titles.is_empty() {
            return Ok(Vec::new());
        }
        let tracks = self
            .library
            .list_tracks(TRACKS_FOR_MATCHING, 0, artist_id, "")
            .await?
            .items;
        Ok(popular::rank(&titles, tracks, |track| &track.title, limit))
    }

    async fn top_titles(&self, artist_id: &str) -> Result<Vec<String>, ArtistInfoError> {
        // An unknown Artist is a 404 even when nothing is stored or looked up.
        self.library.get_artist(artist_id).await?;
        let stored = self.repo.top_titles(artist_id).await?;
        if !self.should_look_up(stored.as_ref(), Some(TOP_TITLES_REFRESH_SECONDS)) {
            return Ok(stored.and_then(|stored| stored.value).unwrap_or_default());
        }
        let (name, albums) = self.artist(artist_id).await?;
        let _slot = take_slot(&self.deezer_slots).await;
        let found = deezer::find_top_titles(&self.client, &self.sources.deezer, &name, &albums)
            .await
            .map(|titles| (!titles.is_empty()).then_some(titles));
        let (outcome, mut titles) = settle(artist_id, found);
        // A failed refresh keeps last week's list rather than showing none,
        // stored as failed so the retry waits out the failure backoff.
        if outcome == Outcome::Failed {
            titles = stored.and_then(|stored| stored.value).or(titles);
        }
        self.repo
            .put_top_titles(artist_id, outcome, titles.as_ref())
            .await?;
        Ok(titles.unwrap_or_default())
    }

    /// Deezer's genres for an Album whose tags name none; empty when its
    /// tags name some, or Deezer has none (yet).
    pub async fn album_genres(&self, album_id: &str) -> Result<Vec<String>, ArtistInfoError> {
        let album = self.library.get_album(album_id).await?.album;
        if !album.genres.is_empty() || !album.genre_items.is_empty() {
            return Ok(Vec::new());
        }
        let stored = self.repo.album_genres(album_id).await?;
        if !self.should_look_up(stored.as_ref(), None) {
            return Ok(stored.and_then(|stored| stored.value).unwrap_or_default());
        }
        let _slot = take_slot(&self.deezer_slots).await;
        let found = deezer::find_album_genres(
            &self.client,
            &self.sources.deezer,
            &album.artist_name,
            &album.title,
        )
        .await
        .map(|genres| (!genres.is_empty()).then_some(genres));
        let (outcome, genres) = settle(album_id, found);
        self.repo
            .put_album_genres(album_id, outcome, genres.as_ref())
            .await?;
        Ok(genres.unwrap_or_default())
    }

    /// A sharper copy of the Album's cover from Deezer, `None` when its own
    /// is sharp enough or Deezer has nothing better (yet). Scaled covers of
    /// the Album keep using its own, so a list never looks anything up.
    pub async fn sharper_cover(&self, album_id: &str) -> Result<Option<Photo>, ArtistInfoError> {
        let own = self.library.album_cover(album_id, 0).await?;
        let stored = self
            .repo
            .album_cover(album_id)
            .await?
            .filter(|(source_sha256, _)| *source_sha256 == own.source_sha256)
            .map(|(_, stored)| stored);
        if !self.should_look_up(stored.as_ref(), None) {
            return Ok(stored.and_then(|stored| stored.value));
        }
        let found = match dimensions(&own.data) {
            Some((width, height)) if width.max(height) < SHARP_COVER_EDGE => {
                self.find_sharper_cover(album_id, (width, height)).await
            }
            _ => Ok(None),
        };
        let (outcome, cover) = settle(album_id, found);
        self.repo
            .put_album_cover(album_id, &own.source_sha256, outcome, cover.as_ref())
            .await?;
        Ok(cover)
    }

    async fn find_sharper_cover(
        &self,
        album_id: &str,
        own: (u32, u32),
    ) -> Result<Option<Photo>, String> {
        let album = self
            .library
            .get_album(album_id)
            .await
            .map_err(|error| error.to_string())?
            .album;
        let _slot = take_slot(&self.deezer_slots).await;
        let Some(url) = deezer::find_album_cover(
            &self.client,
            &self.sources.deezer,
            &album.artist_name,
            &album.title,
        )
        .await?
        else {
            return Ok(None);
        };
        let (media_type, data) = super::fetch::bytes(&self.client, &url).await?;
        Ok(is_sharper_copy(own, dimensions(&data)).then_some(Photo { media_type, data }))
    }

    /// Keeps looking up Albums in the background for as long as the server runs.
    pub fn spawn_explicit_sweep(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut sweep = tokio::time::interval(EXPLICIT_SWEEP_EVERY);
            loop {
                sweep.tick().await;
                if let Err(error) = self.mark_explicit_tracks().await {
                    tracing::warn!(%error, "explicit Track sweep failed");
                }
            }
        });
    }

    /// Looks up the Albums that are due, so every Track list can show which
    /// Tracks are explicit without waiting on Deezer.
    pub async fn mark_explicit_tracks(&self) -> Result<(), ArtistInfoError> {
        if !self.enabled {
            return Ok(());
        }
        let checks = self.repo.album_explicit_checks().await?;
        let mut offset = 0;
        loop {
            let page = self
                .library
                .list_albums(EXPLICIT_SWEEP_PAGE, offset, "", "")
                .await?;
            let count = page.items.len() as i64;
            for album in page.items {
                if self.should_look_up(checks.get(&album.id), None) {
                    self.mark_album_explicit_tracks(&album.id).await?;
                    tokio::time::sleep(EXPLICIT_LOOKUP_PAUSE).await;
                }
            }
            if count < EXPLICIT_SWEEP_PAGE {
                return Ok(());
            }
            offset += count;
        }
    }

    async fn mark_album_explicit_tracks(&self, album_id: &str) -> Result<(), ArtistInfoError> {
        let detail = self.library.get_album(album_id).await?;
        let _slot = take_slot(&self.deezer_slots).await;
        let found = deezer::find_explicit_titles(
            &self.client,
            &self.sources.deezer,
            &detail.album.artist_name,
            &detail.album.title,
        )
        .await;
        let (outcome, titles) = settle(album_id, found);
        if let Some(titles) = titles {
            let titles: HashSet<String> = titles.into_iter().collect();
            let track_ids: Vec<String> = detail
                .tracks
                .into_iter()
                .filter(|track| titles.contains(&popular::base_title(&track.title)))
                .map(|track| track.id)
                .collect();
            self.library
                .set_explicit_tracks(album_id, &track_ids)
                .await?;
        }
        self.repo
            .put_album_explicit_check(album_id, outcome)
            .await?;
        Ok(())
    }

    fn should_look_up<T>(&self, stored: Option<&Stored<T>>, found_refresh: Option<i64>) -> bool {
        self.enabled
            && stored.is_none_or(|stored| is_due(stored.outcome, stored.age_seconds, found_refresh))
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

fn dimensions(image: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(image))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// Whether Deezer's image is larger than the cover and the same shape.
fn is_sharper_copy(own: (u32, u32), found: Option<(u32, u32)>) -> bool {
    let aspect = |(width, height): (u32, u32)| f64::from(width) / f64::from(height.max(1));
    found.is_some_and(|found| {
        found.0.max(found.1) > own.0.max(own.1)
            && (aspect(found) - aspect(own)).abs() <= MAX_COVER_ASPECT_DIFFERENCE
    })
}

/// How the lookup ended, to store, and what it found.
fn settle<T>(id: &str, found: Result<Option<T>, String>) -> (Outcome, Option<T>) {
    match found {
        Ok(Some(value)) => (Outcome::Found, Some(value)),
        Ok(None) => (Outcome::Missing, None),
        Err(error) => {
            tracing::warn!(id, %error, "artist info lookup failed");
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
        assert!(!is_due(Outcome::Found, i64::MAX, None));
        assert!(!is_due(Outcome::Found, 59, Some(60)));
        assert!(is_due(Outcome::Found, 60, Some(60)));
        assert!(!is_due(Outcome::Missing, MISSING_RETRY_SECONDS - 1, None));
        assert!(is_due(Outcome::Missing, MISSING_RETRY_SECONDS, None));
        assert!(!is_due(Outcome::Failed, FAILED_RETRY_SECONDS - 1, None));
        assert!(is_due(Outcome::Failed, FAILED_RETRY_SECONDS, None));
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
        let cover_url = format!("{origin}/img/cover.png");
        let app = Router::new().fallback(move |uri: Uri| {
            // No source lives at `/`; local tools probe open ports there.
            if uri.path() != "/" {
                counter.fetch_add(1, Ordering::SeqCst);
            }
            let photo_url = photo_url.clone();
            let cover_url = cover_url.clone();
            let musicbrainz_busy = musicbrainz_busy.clone();
            async move {
                let query = uri.query().unwrap_or("");
                let body = match uri.path() {
                    "/search/artist" if query.contains("Skillet") => json!({"data": [
                        {"id": 7, "name": "Skillet", "picture_big": photo_url, "nb_fan": 5}
                    ]}),
                    "/artist/7/top" => json!({"data": [
                        {"title": "Monster (Live)", "title_short": "Monster"},
                        {"title": "Not In Library", "title_short": ""},
                        {"title": "Hero", "title_short": "Hero"}
                    ]}),
                    "/search/album" if query.to_lowercase().contains("awake") && !query.contains("Edition") => json!({"data": [
                        {"id": 9, "title": "Awake", "cover_xl": null, "artist": {"id": 7, "name": "Skillet"}},
                        {"id": 9, "title": "Awake (Deluxe)", "cover_xl": cover_url, "artist": {"id": 7, "name": "Skillet"}}
                    ]}),
                    "/album/9" => json!({"id": 9, "genres": {"data": [{"id": 152, "name": "Rock"}]}}),
                    "/album/9/tracks" => json!({"data": [
                        {"title": "HERO (Remastered)", "explicit_lyrics": true},
                        {"title": "Monster", "explicit_lyrics": false}
                    ]}),
                    "/search/artist" | "/search/album" => json!({"data": []}),
                    "/2.0/" if query.contains("artist=Skillet") && query.contains("api_key=test-key") => {
                        json!({"toptracks": {"track": [
                            {"name": "Monster", "playcount": "11940672"},
                            {"name": "Hero (Live)", "playcount": "900"},
                            {"name": "Hero", "playcount": "8000000"}
                        ]}})
                    }
                    "/img/cover.png" => {
                        return ([(header::CONTENT_TYPE, "image/png")], png(1000, 1000)).into_response();
                    }
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
            lastfm: origin.clone(),
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
        let key = Some("test-key".to_owned());
        (
            dir,
            ArtistInfo::new(pool, sources, enabled, "en", "test", key),
        )
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

    /// One Album by Skillet holding Tracks with these titles.
    async fn seed_tracks(pool: &crate::db::Db, titles: &[&str]) {
        sqlx::query("INSERT INTO albums (id, artist_id, title, title_sort) VALUES ('al', 'ar', 'Awake', 'awake')")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO album_artists (album_id, artist_id, position) VALUES ('al', 'ar', 0)",
        )
        .execute(pool)
        .await
        .unwrap();
        for (index, title) in titles.iter().enumerate() {
            sqlx::query(
                "INSERT INTO tracks (id, album_id, title, title_sort, artist_name, format, file_path)
                VALUES (?1, 'al', ?2, ?2, 'Skillet', 'flac', ?3)",
            )
            .bind(format!("t{index}"))
            .bind(title)
            .bind(format!("/{index}.flac"))
            .execute(pool)
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn ranks_library_tracks_by_deezer_popularity_and_stores_the_list() {
        let (sources, requests) = fake_sources().await;
        let (dir, info) = service(sources, true).await;
        // Through the server's opener, which adds the SQL functions the
        // Track triggers call.
        let migrations = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let pool =
            crate::db::open_and_migrate(dir.path().join("app.db").to_str().unwrap(), &migrations)
                .await
                .unwrap();
        seed_tracks(&pool, &["Hero", "Monster", "Lucy"]).await;
        let titles = |tracks: Vec<Track>| -> Vec<String> {
            tracks.into_iter().map(|track| track.title).collect()
        };
        assert_eq!(
            titles(info.popular_tracks("ar", 10).await.unwrap()),
            ["Monster", "Hero"]
        );
        let made = requests.load(Ordering::SeqCst);
        assert_eq!(
            titles(info.popular_tracks("ar", 1).await.unwrap()),
            ["Monster"]
        );
        assert_eq!(
            requests.load(Ordering::SeqCst),
            made,
            "stored list makes no request"
        );
        assert!(matches!(
            info.popular_tracks("unknown", 5).await,
            Err(ArtistInfoError::NotFound)
        ));
    }

    #[tokio::test]
    async fn gives_last_fm_play_counts_by_each_tracks_artist_and_stores_them() {
        let (sources, requests) = fake_sources().await;
        let (dir, info) = service(sources, true).await;
        let migrations = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let pool =
            crate::db::open_and_migrate(dir.path().join("app.db").to_str().unwrap(), &migrations)
                .await
                .unwrap();
        seed_tracks(&pool, &["Hero", "Monster", "Lucy"]).await;
        let ids: Vec<String> = ["t0", "t1", "t2", "unknown"].map(str::to_owned).to_vec();
        let counts = |found: Vec<PlayCount>| -> Vec<(String, i64)> {
            found
                .into_iter()
                .map(|count| (count.track_id, count.play_count))
                .collect()
        };
        let expected = vec![("t0".to_owned(), 8_000_000), ("t1".to_owned(), 11_940_672)];
        assert_eq!(counts(info.play_counts(&ids).await.unwrap()), expected);
        let made = requests.load(Ordering::SeqCst);
        assert_eq!(counts(info.play_counts(&ids).await.unwrap()), expected);
        assert_eq!(
            requests.load(Ordering::SeqCst),
            made,
            "stored counts make no request"
        );
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbImage::from_pixel(width, height, image::Rgb([200, 40, 10]));
        let mut data = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut data),
                image::ImageFormat::Png,
            )
            .unwrap();
        data
    }

    async fn set_cover(pool: &crate::db::Db, cover: Vec<u8>) {
        sqlx::query("UPDATE albums SET cover_mime = 'image/png', cover_data = ? WHERE id = 'al'")
            .bind(cover)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn album_pool(dir: &tempfile::TempDir) -> crate::db::Db {
        let migrations = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let pool =
            crate::db::open_and_migrate(dir.path().join("app.db").to_str().unwrap(), &migrations)
                .await
                .unwrap();
        seed_tracks(&pool, &["Hero"]).await;
        pool
    }

    #[tokio::test]
    async fn fetches_a_sharper_cover_once_and_again_for_a_new_own_cover() {
        let (sources, requests) = fake_sources().await;
        let (dir, info) = service(sources, true).await;
        let pool = album_pool(&dir).await;
        set_cover(&pool, png(500, 500)).await;

        let cover = info.sharper_cover("al").await.unwrap().expect("cover");
        assert_eq!(cover.media_type, "image/png");
        assert_eq!(dimensions(&cover.data), Some((1000, 1000)));
        let made = requests.load(Ordering::SeqCst);
        assert!(info.sharper_cover("al").await.unwrap().is_some());
        assert_eq!(requests.load(Ordering::SeqCst), made, "a stored cover");

        set_cover(&pool, png(600, 600)).await;
        assert!(info.sharper_cover("al").await.unwrap().is_some());
        assert!(requests.load(Ordering::SeqCst) > made, "a new own cover");
    }

    #[tokio::test]
    async fn searches_for_a_cover_without_the_edition_in_the_title() {
        let (sources, _) = fake_sources().await;
        let (dir, info) = service(sources, true).await;
        let pool = album_pool(&dir).await;
        set_cover(&pool, png(500, 500)).await;
        sqlx::query("UPDATE albums SET title = 'Awake (Deluxe Edition)' WHERE id = 'al'")
            .execute(&pool)
            .await
            .unwrap();

        assert!(info.sharper_cover("al").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn keeps_a_sharp_cover_and_asks_nobody() {
        let (sources, requests) = fake_sources().await;
        let (dir, info) = service(sources, true).await;
        let pool = album_pool(&dir).await;
        set_cover(&pool, png(1000, 1000)).await;

        assert_eq!(info.sharper_cover("al").await.unwrap(), None);
        assert_eq!(requests.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn looks_nothing_up_while_lookups_are_off() {
        let (sources, requests) = fake_sources().await;
        let (dir, info) = service(sources, false).await;
        let pool = album_pool(&dir).await;
        set_cover(&pool, png(500, 500)).await;

        assert_eq!(info.sharper_cover("al").await.unwrap(), None);
        assert_eq!(requests.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_sharper_copy_is_larger_and_the_same_shape() {
        assert!(is_sharper_copy((500, 500), Some((1000, 1000))));
        assert!(!is_sharper_copy((500, 500), Some((500, 500))));
        assert!(!is_sharper_copy((800, 500), Some((1000, 1000))));
        assert!(!is_sharper_copy((500, 500), None));
    }

    #[tokio::test]
    async fn gives_deezer_genres_only_to_albums_whose_tags_name_none() {
        let (sources, requests) = fake_sources().await;
        let (dir, info) = service(sources, true).await;
        let migrations = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let pool =
            crate::db::open_and_migrate(dir.path().join("app.db").to_str().unwrap(), &migrations)
                .await
                .unwrap();
        seed_tracks(&pool, &["Hero"]).await;
        assert_eq!(info.album_genres("al").await.unwrap(), ["Rock"]);
        let made = requests.load(Ordering::SeqCst);
        assert_eq!(info.album_genres("al").await.unwrap(), ["Rock"]);
        assert_eq!(
            requests.load(Ordering::SeqCst),
            made,
            "stored genres make no request"
        );
        sqlx::query("UPDATE albums SET genres = '[\"Metal\"]' WHERE id = 'al'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(info.album_genres("al").await.unwrap().is_empty());
        assert!(matches!(
            info.album_genres("unknown").await,
            Err(ArtistInfoError::NotFound)
        ));
    }

    #[tokio::test]
    async fn marks_the_tracks_deezer_calls_explicit_once() {
        let (sources, requests) = fake_sources().await;
        let (dir, info) = service(sources, true).await;
        let migrations = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let pool =
            crate::db::open_and_migrate(dir.path().join("app.db").to_str().unwrap(), &migrations)
                .await
                .unwrap();
        seed_tracks(&pool, &["Hero", "Monster"]).await;
        info.mark_explicit_tracks().await.unwrap();
        let tracks = info.library.get_album("al").await.unwrap().tracks;
        let explicit: Vec<&str> = tracks
            .iter()
            .filter(|track| track.explicit)
            .map(|track| track.title.as_str())
            .collect();
        assert_eq!(explicit, ["Hero"]);
        let made = requests.load(Ordering::SeqCst);
        info.mark_explicit_tracks().await.unwrap();
        assert_eq!(
            requests.load(Ordering::SeqCst),
            made,
            "a found Album is not looked up again"
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
    async fn gives_play_counts_with_artist_lookups_switched_off() {
        let (sources, _) = fake_sources().await;
        let (dir, info) = service(sources, false).await;
        let migrations = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let pool =
            crate::db::open_and_migrate(dir.path().join("app.db").to_str().unwrap(), &migrations)
                .await
                .unwrap();
        seed_tracks(&pool, &["Hero"]).await;
        let found = info.play_counts(&["t0".to_owned()]).await.unwrap();
        assert_eq!(found.len(), 1, "the Last.fm key is its own opt-in");
    }

    #[tokio::test]
    async fn keeps_stale_counts_and_backs_off_when_a_refresh_fails() {
        // Nothing listens on port 1, so every lookup fails.
        let down = "http://127.0.0.1:1".to_owned();
        let sources = Sources {
            deezer: down.clone(),
            musicbrainz: down.clone(),
            wikidata: down.clone(),
            wikipedia: down.clone(),
            lastfm: down,
        };
        let (dir, info) = service(sources, true).await;
        let migrations = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let pool =
            crate::db::open_and_migrate(dir.path().join("app.db").to_str().unwrap(), &migrations)
                .await
                .unwrap();
        seed_tracks(&pool, &["Hero"]).await;
        for statement in [
            "INSERT INTO artist_play_counts (artist_id, outcome, counts, checked_at)
             VALUES ('ar', 'found', '{\"hero\": 5}', datetime('now', '-8 days'))",
            "INSERT INTO artist_top_tracks (artist_id, outcome, titles, checked_at)
             VALUES ('ar', 'found', '[\"Hero\"]', datetime('now', '-8 days'))",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        let found = info.play_counts(&["t0".to_owned()]).await.unwrap();
        assert_eq!(found[0].play_count, 5, "last week's counts stay");
        assert_eq!(info.top_titles("ar").await.unwrap(), ["Hero"]);
        for query in [
            "SELECT outcome, CAST((julianday('now') - julianday(checked_at)) * 86400 AS INTEGER)
             FROM artist_play_counts WHERE artist_id = 'ar'",
            "SELECT outcome, CAST((julianday('now') - julianday(checked_at)) * 86400 AS INTEGER)
             FROM artist_top_tracks WHERE artist_id = 'ar'",
        ] {
            let (outcome, age): (String, i64) =
                sqlx::query_as(query).fetch_one(&pool).await.unwrap();
            assert_eq!(outcome, "failed", "{query}");
            assert!(
                age < 60,
                "the failure is timestamped for the backoff: {query}"
            );
        }
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
