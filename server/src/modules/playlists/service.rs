//! Playlist rules: every user has a default Favorites playlist, and a user
//! playlist disappears when its last Track is removed.

use super::repository::{Playlist, PlaylistError, Repository};
use crate::app::AppState;
use crate::gojson;
use crate::modules::library::{LibraryService, Track};
use serde::Serialize;

pub const DEFAULT_FAVORITES_NAME: &str = "Favorites";

#[derive(Serialize)]
pub struct PlaylistList {
    items: Vec<Playlist>,
    total: usize,
}

#[derive(Clone, Serialize)]
pub struct PlaylistDetail {
    #[serde(flatten)]
    pub playlist: Playlist,
    pub tracks: Vec<Track>,
}

/// Playlist rules over the repository and the library.
pub struct Service {
    repo: Repository,
    tracks: LibraryService,
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("playlist not found")]
    PlaylistNotFound,
    #[error("track not found")]
    TrackNotFound,
    #[error("playlist name is required")]
    NameRequired,
    /// A database error the create handler words like the Go driver.
    #[error("{}", gojson::sqlite_error(.0))]
    Create(sqlx::Error),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl From<PlaylistError> for ServiceError {
    fn from(error: PlaylistError) -> Self {
        match error {
            PlaylistError::NotFound => ServiceError::PlaylistNotFound,
            PlaylistError::Database(error) => ServiceError::Database(error),
        }
    }
}

impl Service {
    pub fn new(state: &AppState) -> Self {
        Self {
            repo: Repository::new(state.db.clone()),
            tracks: LibraryService::new(state.db.clone()),
        }
    }

    pub async fn list(&self, user_id: &str) -> Result<PlaylistList, ServiceError> {
        self.repo.ensure_default(user_id).await?;
        let items = self.repo.list(user_id).await?;
        Ok(PlaylistList {
            total: items.len(),
            items,
        })
    }

    pub async fn create(&self, user_id: &str, name: &str) -> Result<Playlist, ServiceError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ServiceError::NameRequired);
        }
        self.repo.ensure_default(user_id).await?;
        let id = self
            .repo
            .create(user_id, name)
            .await
            .map_err(ServiceError::Create)?;
        Ok(self.repo.get(user_id, &id).await?)
    }

    pub async fn get(
        &self,
        user_id: &str,
        playlist_id: &str,
    ) -> Result<PlaylistDetail, ServiceError> {
        self.repo.ensure_default(user_id).await?;
        let playlist = self.repo.get(user_id, playlist_id).await?;
        let mut tracks = Vec::new();
        for track_id in self.repo.track_ids(playlist_id).await? {
            // Hidden or removed Tracks are skipped, not errors.
            if let Ok(track) = self.tracks.get_track(&track_id).await {
                tracks.push(track);
            }
        }
        Ok(PlaylistDetail { playlist, tracks })
    }

    pub async fn add_track(
        &self,
        user_id: &str,
        playlist_id: &str,
        track_id: &str,
    ) -> Result<PlaylistDetail, ServiceError> {
        self.get(user_id, playlist_id).await?;
        // Any failure to read the Track counts as not found, as in Go.
        if self.tracks.get_track(track_id).await.is_err() {
            return Err(ServiceError::TrackNotFound);
        }
        self.repo.append_track(playlist_id, track_id).await?;
        self.get(user_id, playlist_id).await
    }

    pub async fn remove_track(
        &self,
        user_id: &str,
        playlist_id: &str,
        track_id: &str,
    ) -> Result<PlaylistDetail, ServiceError> {
        let mut detail = self.get(user_id, playlist_id).await?;
        if !self
            .repo
            .remove_track(user_id, playlist_id, track_id)
            .await?
        {
            return self.get(user_id, playlist_id).await;
        }
        detail.playlist.track_count = 0;
        detail.tracks = Vec::new();
        Ok(detail)
    }
}
