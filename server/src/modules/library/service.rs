//! Library reads for the HTTP handlers and for other modules.

use super::covers;
use super::repository::{AlbumCover, LibraryError, Repository};
use super::types::{Album, AlbumDetail, Artist, Page, Track};

#[derive(Clone)]
pub struct Service {
    repo: Repository,
}

impl Service {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self {
            repo: Repository::new(pool),
        }
    }

    pub async fn list_artists(
        &self,
        limit: i64,
        offset: i64,
        query: &str,
    ) -> Result<Page<Artist>, LibraryError> {
        self.repo.list_artists(limit, offset, query).await
    }

    pub async fn list_albums(
        &self,
        limit: i64,
        offset: i64,
        artist_id: &str,
        query: &str,
    ) -> Result<Page<Album>, LibraryError> {
        self.repo.list_albums(limit, offset, artist_id, query).await
    }

    pub async fn get_album(&self, album_id: &str) -> Result<AlbumDetail, LibraryError> {
        self.repo.get_album(album_id).await
    }

    pub async fn album_cover(&self, album_id: &str, size: i64) -> Result<AlbumCover, LibraryError> {
        covers::album_cover(&self.repo, album_id, size).await
    }

    pub async fn list_tracks(
        &self,
        limit: i64,
        offset: i64,
        artist_id: &str,
        query: &str,
    ) -> Result<Page<Track>, LibraryError> {
        self.repo.list_tracks(limit, offset, artist_id, query).await
    }

    pub async fn get_track(&self, track_id: &str) -> Result<Track, LibraryError> {
        self.repo.get_track(track_id).await
    }

    pub async fn get_track_lyrics(&self, track_id: &str) -> Result<String, LibraryError> {
        self.repo.get_track_lyrics(track_id).await
    }

    pub async fn get_track_file_path(&self, track_id: &str) -> Result<String, LibraryError> {
        self.repo.get_track_file_path(track_id).await
    }
}
