//! Permanent Track Deletion and Album deletion (track_deletion.go,
//! album_deletion.go, repository_deletion.go, queries/deletion.sql).

use super::errors::{Error, Result, Sentinel};
use super::repository::{Repository, context};
use super::service::Service;
use super::storage::Storage;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedFile {
    pub path: String,
    pub size_bytes: i64,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct PlaylistReference {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QueueReference {
    pub user_id: String,
    pub item_count: i64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackDeletionPreview {
    pub track_id: String,
    pub track_title: String,
    pub managed_file: ManagedFile,
    pub playlist_references: Vec<PlaylistReference>,
    pub queue_references: Vec<QueueReference>,
    pub confirmation_token: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackDeletionResult {
    pub deleted_files: i64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumDeletionTrack {
    pub track_id: String,
    pub track_title: String,
    pub size_bytes: i64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumDeletionPreview {
    pub album_id: String,
    pub album_title: String,
    pub track_count: i64,
    pub total_size_bytes: i64,
    pub tracks: Vec<AlbumDeletionTrack>,
    pub playlist_references: Vec<PlaylistReference>,
    pub queue_references: Vec<QueueReference>,
    pub confirmation_token: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumDeletionFailed {
    pub track_id: String,
    pub track_title: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumDeletionResult {
    pub deleted: Vec<AlbumDeletionTrack>,
    pub stopped_at: Option<AlbumDeletionFailed>,
    pub deleted_files: i64,
}

#[derive(Clone, Debug, Default)]
pub struct TrackDeletionState {
    pub preview: TrackDeletionPreview,
    pub album_id: String,
    pub file_path: String,
    pub content_sha256: String,
    pub artwork_path: String,
    pub artwork_sha256: String,
    pub track_revision: i64,
    pub source_revision: i64,
}

/// A Queue revision advanced by a write, for Playback Clients to refetch.
pub struct QueueInvalidation {
    pub user_id: String,
    pub revision: String,
    pub sequence: String,
}

/// The SHA-256 of a value as Go's json.Marshal encodes it.
pub fn token_of<T: Serialize>(payload: &T) -> String {
    hex::encode(Sha256::digest(crate::gojson::to_string(payload).as_bytes()))
}

/// Constant-time comparison of two confirmation tokens.
pub fn tokens_equal(expected: &str, actual: &str) -> bool {
    if expected.len() != actual.len() {
        return false;
    }
    expected
        .bytes()
        .zip(actual.bytes())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

/// The token payload, field names and order as the Go struct has them.
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct DeletionTokenPayload<'a> {
    #[serde(rename = "TrackID")]
    track_id: &'a str,
    track_title: &'a str,
    #[serde(rename = "AlbumID")]
    album_id: &'a str,
    file_path: &'a str,
    #[serde(rename = "ContentSHA256")]
    content_sha256: &'a str,
    artwork_path: &'a str,
    #[serde(rename = "ArtworkSHA256")]
    artwork_sha256: &'a str,
    track_revision: i64,
    source_revision: i64,
    playlist_references: &'a [PlaylistReference],
    queue_references: &'a [QueueReference],
}

fn deletion_token(state: &TrackDeletionState) -> String {
    token_of(&DeletionTokenPayload {
        track_id: &state.preview.track_id,
        track_title: &state.preview.track_title,
        album_id: &state.album_id,
        file_path: &state.file_path,
        content_sha256: &state.content_sha256,
        artwork_path: &state.artwork_path,
        artwork_sha256: &state.artwork_sha256,
        track_revision: state.track_revision,
        source_revision: state.source_revision,
        playlist_references: &state.preview.playlist_references,
        queue_references: &state.preview.queue_references,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct AlbumTokenPayload<'a> {
    #[serde(rename = "AlbumID")]
    album_id: &'a str,
    album_revision: i64,
    track_tokens: Vec<&'a str>,
}

pub async fn list_playlists(
    conn: &mut SqliteConnection,
    track_id: &str,
) -> Result<Vec<PlaylistReference>> {
    let rows = sqlx::query!(
        r#"SELECT playlists.id AS "id!", playlists.name
        FROM playlist_tracks JOIN playlists ON playlists.id = playlist_tracks.playlist_id
        WHERE playlist_tracks.track_id = ?
        ORDER BY playlists.name, playlists.id"#,
        track_id
    )
    .fetch_all(conn)
    .await
    .map_err(context("list affected Playlists"))?;
    Ok(rows
        .into_iter()
        .map(|row| PlaylistReference {
            id: row.id,
            name: row.name,
        })
        .collect())
}

pub async fn list_queues(
    conn: &mut SqliteConnection,
    track_id: &str,
) -> Result<Vec<QueueReference>> {
    let rows = sqlx::query!(
        r#"SELECT user_id AS "user_id!", COUNT(*) AS "item_count!: i64" FROM playback_queue
        WHERE track_id = ? GROUP BY user_id ORDER BY user_id"#,
        track_id
    )
    .fetch_all(conn)
    .await
    .map_err(context("list affected Queues"))?;
    Ok(rows
        .into_iter()
        .map(|row| QueueReference {
            user_id: row.user_id,
            item_count: row.item_count,
        })
        .collect())
}

/// Bumps each affected Queue revision so Playback Clients refetch.
pub async fn advance_queues(
    conn: &mut SqliteConnection,
    queues: &[QueueReference],
) -> Result<Vec<QueueInvalidation>> {
    let mut invalidations = Vec::with_capacity(queues.len());
    for queue in queues {
        let row = sqlx::query!(
            r#"INSERT INTO playback_queue_state (user_id, revision, event_sequence) VALUES (?, 1, 1)
            ON CONFLICT(user_id) DO UPDATE SET revision = revision + 1, event_sequence = event_sequence + 1
            RETURNING CAST(revision AS TEXT) AS "revision!: String", CAST(event_sequence AS TEXT) AS "event_sequence!: String""#,
            queue.user_id
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(Error::from)?;
        invalidations.push(QueueInvalidation {
            user_id: queue.user_id.clone(),
            revision: row.revision,
            sequence: row.event_sequence,
        });
    }
    Ok(invalidations)
}

/// Hands Album Artwork to another Track, or drops it with the Album's last
/// Track.
pub async fn move_or_delete_artwork_reference(
    conn: &mut SqliteConnection,
    track_id: &str,
    album_id: &str,
) -> Result<()> {
    let other = sqlx::query_scalar!(
        r#"SELECT id AS "id!" FROM tracks WHERE album_id = ? AND id != ? ORDER BY id LIMIT 1"#,
        album_id,
        track_id
    )
    .fetch_optional(&mut *conn)
    .await;
    let result = match other {
        Ok(None) => sqlx::query!("DELETE FROM album_artwork WHERE album_id = ?", album_id)
            .execute(&mut *conn)
            .await
            .map(|_| ()),
        Ok(Some(other)) => sqlx::query!(
            "UPDATE album_artwork SET source_track_id = ?1 WHERE album_id = ?2 AND source_track_id = ?3",
            other,
            album_id,
            track_id
        )
        .execute(&mut *conn)
        .await
        .map(|_| ()),
        Err(error) => Err(error),
    };
    result.map_err(context("update Album Artwork source after Track deletion"))
}

pub async fn track_and_album_artist_ids(
    conn: &mut SqliteConnection,
    track_id: &str,
    album_id: &str,
) -> Result<Vec<String>> {
    sqlx::query_scalar!(
        r#"SELECT artist_id AS "artist_id!" FROM track_artists WHERE track_id = ?
        UNION SELECT artist_id FROM album_artists WHERE album_id = ?"#,
        track_id,
        album_id
    )
    .fetch_all(conn)
    .await
    .map_err(context("list affected Artists"))
}

pub async fn delete_album_if_empty(conn: &mut SqliteConnection, album_id: &str) -> Result<()> {
    sqlx::query!(
        "DELETE FROM albums WHERE albums.id = ?1 AND NOT EXISTS (SELECT 1 FROM tracks WHERE tracks.album_id = ?1)",
        album_id
    )
    .execute(conn)
    .await
    .map_err(Error::from)?;
    Ok(())
}

pub async fn delete_unreferenced_artists(
    conn: &mut SqliteConnection,
    artist_ids: &[String],
) -> Result<()> {
    for artist_id in artist_ids {
        sqlx::query!(
            "DELETE FROM artists WHERE artists.id = ?1
                AND NOT EXISTS (SELECT 1 FROM track_artists WHERE track_artists.artist_id = ?1)
                AND NOT EXISTS (SELECT 1 FROM album_artists WHERE album_artists.artist_id = ?1)",
            artist_id
        )
        .execute(&mut *conn)
        .await
        .map_err(Error::from)?;
    }
    Ok(())
}

async fn load_track_deletion(
    conn: &mut SqliteConnection,
    storage: &Storage,
    track_id: &str,
) -> Result<TrackDeletionState> {
    let row = sqlx::query!(
        r#"SELECT tracks.title, tracks.album_id, tracks.revision,
            track_sources.source_kind, track_sources.file_path, track_sources.content_sha256,
            track_sources.size_bytes, track_sources.revision AS source_revision
        FROM tracks JOIN track_sources ON track_sources.track_id = tracks.id
        WHERE tracks.id = ? AND tracks.missing_at IS NULL"#,
        track_id
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(context("load Managed Track deletion"))?
    .ok_or_else(|| Error::sentinel(Sentinel::TrackNotFound))?;
    if row.source_kind != "managed" {
        return Err(Error::sentinel(Sentinel::NotManagedTrack));
    }
    let mut state = TrackDeletionState {
        album_id: row.album_id,
        file_path: row.file_path,
        content_sha256: row.content_sha256.unwrap_or_default(),
        track_revision: row.revision,
        source_revision: row.source_revision,
        ..TrackDeletionState::default()
    };
    state.preview.track_title = row.title;
    let (relative, size) = storage.resolve_managed_file(&state.file_path, &state.content_sha256)?;
    if row.size_bytes != size {
        return Err(Error::wrap(
            Sentinel::DeletionConflict,
            "managed file size changed",
        ));
    }
    state.preview.track_id = track_id.to_owned();
    state.preview.managed_file = ManagedFile {
        path: relative,
        size_bytes: size,
    };
    state.preview.playlist_references = list_playlists(conn, track_id).await?;
    state.preview.queue_references = list_queues(conn, track_id).await?;
    let artwork = sqlx::query!(
        "SELECT album_artwork.file_path, album_artwork.content_sha256 FROM album_artwork
        WHERE album_artwork.album_id = ?1
            AND NOT EXISTS (SELECT 1 FROM tracks WHERE tracks.album_id = ?1 AND tracks.id != ?2)",
        state.album_id,
        track_id
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(context("load final-Album Artwork"))?;
    if let Some(artwork) = artwork {
        storage
            .resolve_managed_file(&artwork.file_path, &artwork.content_sha256)
            .map_err(|error| error.context("validate final-Album Artwork"))?;
        state.artwork_path = artwork.file_path;
        state.artwork_sha256 = artwork.content_sha256;
    }
    state.preview.confirmation_token = deletion_token(&state);
    Ok(state)
}

impl Repository {
    pub async fn load_track_deletion(
        &self,
        storage: &Storage,
        track_id: &str,
    ) -> Result<TrackDeletionState> {
        load_track_deletion(&mut *self.conn().await?, storage, track_id).await
    }

    /// Re-reads the Track, checks the token, then journals the deletion and
    /// hides the Track in one transaction.
    pub async fn prepare_track_deletion(
        &self,
        storage: &Storage,
        track_id: &str,
        token: &str,
    ) -> Result<TrackDeletionState> {
        let operation = "Permanent Track Deletion preparation";
        let mut tx = self.begin(operation).await?;
        let state = load_track_deletion(&mut tx, storage, track_id).await?;
        if !tokens_equal(&state.preview.confirmation_token, token) {
            return Err(Error::sentinel(Sentinel::DeletionConflict));
        }
        sqlx::query!(
            "INSERT INTO permanent_track_deletions (track_id, file_path, content_sha256, artwork_file_path, artwork_content_sha256)
            VALUES (?1, ?2, ?3, NULLIF(CAST(?4 AS TEXT), ''), NULLIF(CAST(?5 AS TEXT), ''))",
            track_id,
            state.file_path,
            state.content_sha256,
            state.artwork_path,
            state.artwork_sha256
        )
        .execute(&mut *tx)
        .await
        .map_err(context("journal Permanent Track Deletion"))?;
        sqlx::query!(
            "UPDATE tracks SET missing_at = CURRENT_TIMESTAMP WHERE id = ?",
            track_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("hide pending Permanent Track Deletion"))?;
        Self::commit(tx, operation).await?;
        Ok(state)
    }

    pub async fn cancel_track_deletion(&self, track_id: &str) -> Result<()> {
        let operation = "Permanent Track Deletion cancellation";
        let mut tx = self.begin(operation).await?;
        sqlx::query!("UPDATE tracks SET missing_at = NULL WHERE id = ?", track_id)
            .execute(&mut *tx)
            .await
            .map_err(context("restore Managed Track after failed deletion"))?;
        sqlx::query!(
            "DELETE FROM permanent_track_deletions WHERE track_id = ?",
            track_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("clear failed Permanent Track Deletion journal"))?;
        Self::commit(tx, operation).await
    }

    /// Removes the Track and every reference once its files are gone.
    pub async fn finalize_track_deletion(&self, track_id: &str) -> Result<Vec<QueueInvalidation>> {
        let operation = "Permanent Track Deletion finalization";
        let mut tx = self.begin(operation).await?;
        let album_id = sqlx::query_scalar!(
            r#"SELECT tracks.album_id AS "album_id!" FROM permanent_track_deletions
            JOIN tracks ON tracks.id = permanent_track_deletions.track_id WHERE tracks.id = ?"#,
            track_id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(context("load pending Permanent Track Deletion"))?
        .ok_or_else(|| Error::sentinel(Sentinel::TrackNotFound))?;
        let queues = list_queues(&mut tx, track_id).await?;
        let artist_ids = track_and_album_artist_ids(&mut tx, track_id, &album_id).await?;
        let invalidations = advance_queues(&mut tx, &queues)
            .await
            .map_err(|error| error.context("advance affected Queue revision"))?;
        sqlx::query!("DELETE FROM playback_queue WHERE track_id = ?", track_id)
            .execute(&mut *tx)
            .await
            .map_err(context("delete Queue references"))?;
        sqlx::query!("DELETE FROM playlist_tracks WHERE track_id = ?", track_id)
            .execute(&mut *tx)
            .await
            .map_err(context("delete Playlist references"))?;
        sqlx::query!(
            "DELETE FROM managed_import_jobs WHERE track_id = ?",
            track_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context(
            "archive terminal Managed Import Jobs before Track deletion",
        ))?;
        move_or_delete_artwork_reference(&mut tx, track_id, &album_id).await?;
        sqlx::query!("DELETE FROM tracks WHERE id = ?", track_id)
            .execute(&mut *tx)
            .await
            .map_err(context("delete Managed Track"))?;
        delete_album_if_empty(&mut tx, &album_id)
            .await
            .map_err(|error| error.context("delete empty Album"))?;
        delete_unreferenced_artists(&mut tx, &artist_ids)
            .await
            .map_err(|error| error.context("delete inactive Artist"))?;
        Self::commit(tx, operation).await?;
        Ok(invalidations)
    }

    pub async fn list_pending_deletions(&self) -> Result<Vec<TrackDeletionState>> {
        let rows = sqlx::query!(
            r#"SELECT track_id AS "track_id!", file_path, content_sha256,
                COALESCE(artwork_file_path, '') AS "artwork_file_path!: String",
                COALESCE(artwork_content_sha256, '') AS "artwork_content_sha256!: String"
            FROM permanent_track_deletions ORDER BY created_at, track_id"#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(context("list pending Permanent Track Deletions"))?;
        Ok(rows
            .into_iter()
            .map(|row| TrackDeletionState {
                preview: TrackDeletionPreview {
                    track_id: row.track_id,
                    ..TrackDeletionPreview::default()
                },
                file_path: row.file_path,
                content_sha256: row.content_sha256,
                artwork_path: row.artwork_file_path,
                artwork_sha256: row.artwork_content_sha256,
                ..TrackDeletionState::default()
            })
            .collect())
    }

    pub async fn load_album_deletion(
        &self,
        storage: &Storage,
        album_id: &str,
    ) -> Result<(AlbumDeletionPreview, Vec<TrackDeletionState>)> {
        let mut conn = self.conn().await?;
        let album = sqlx::query!("SELECT title, revision FROM albums WHERE id = ?", album_id)
            .fetch_optional(&mut *conn)
            .await
            .map_err(context("load Album deletion"))?
            .ok_or_else(|| Error::sentinel(Sentinel::AlbumNotFound))?;
        let track_ids = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM tracks
            WHERE album_id = ? AND missing_at IS NULL AND is_pending_commit = 0
            ORDER BY COALESCE(disc_no, 1), track_no, title_sort, id"#,
            album_id
        )
        .fetch_all(&mut *conn)
        .await
        .map_err(context("list Album Tracks for deletion"))?;
        if track_ids.is_empty() {
            return Err(Error::sentinel(Sentinel::AlbumNotFound));
        }
        let mut preview = AlbumDeletionPreview {
            album_id: album_id.to_owned(),
            album_title: album.title,
            ..AlbumDeletionPreview::default()
        };
        let mut states = Vec::with_capacity(track_ids.len());
        let mut queue_order: Vec<String> = Vec::new();
        let mut queue_items = std::collections::HashMap::<String, i64>::new();
        for track_id in &track_ids {
            let state = load_track_deletion(&mut conn, storage, track_id).await?;
            preview.tracks.push(AlbumDeletionTrack {
                track_id: track_id.clone(),
                track_title: state.preview.track_title.clone(),
                size_bytes: state.preview.managed_file.size_bytes,
            });
            preview.total_size_bytes += state.preview.managed_file.size_bytes;
            for playlist in &state.preview.playlist_references {
                if !preview
                    .playlist_references
                    .iter()
                    .any(|known| known.id == playlist.id)
                {
                    preview.playlist_references.push(playlist.clone());
                }
            }
            for queue in &state.preview.queue_references {
                if !queue_items.contains_key(&queue.user_id) {
                    queue_order.push(queue.user_id.clone());
                }
                *queue_items.entry(queue.user_id.clone()).or_default() += queue.item_count;
            }
            states.push(state);
        }
        preview.queue_references = queue_order
            .into_iter()
            .map(|user_id| QueueReference {
                item_count: queue_items[&user_id],
                user_id,
            })
            .collect();
        preview.track_count = preview.tracks.len() as i64;
        preview.confirmation_token = token_of(&AlbumTokenPayload {
            album_id,
            album_revision: album.revision,
            track_tokens: states
                .iter()
                .map(|state| state.preview.confirmation_token.as_str())
                .collect(),
        });
        Ok((preview, states))
    }
}

impl Service {
    pub(super) fn publish_queue_invalidations(&self, invalidations: &[QueueInvalidation]) {
        if let Some(events) = &self.queue_events {
            for invalidation in invalidations {
                events.publish(
                    &invalidation.user_id,
                    &invalidation.revision,
                    &invalidation.sequence,
                );
            }
        }
    }

    pub async fn preview_track_deletion(&self, track_id: &str) -> Result<TrackDeletionPreview> {
        Ok(self
            .repo
            .load_track_deletion(&self.storage, track_id)
            .await?
            .preview)
    }

    pub async fn delete_track(&self, track_id: &str, token: &str) -> Result<TrackDeletionResult> {
        let _deletion = self.deletion_lock.lock().await;
        let _commit = self.commit_lock().await;
        self.delete_track_locked(track_id, token).await
    }

    async fn delete_track_locked(
        &self,
        track_id: &str,
        token: &str,
    ) -> Result<TrackDeletionResult> {
        let state = self
            .repo
            .prepare_track_deletion(&self.storage, track_id, token)
            .await?;
        let (has_removed, all_removed, removed) = self.remove_prepared_deletion_files(&state);
        if let Some(error) = &removed
            && !has_removed
        {
            let cancelled = self.repo.cancel_track_deletion(track_id).await.err();
            return Err(Error::join([Some(error.clone()), cancelled]).unwrap());
        }
        if !all_removed {
            return Err(removed.unwrap_or_else(|| Error::new("managed file was not removed")));
        }
        let invalidations = match self.repo.finalize_track_deletion(track_id).await {
            Ok(invalidations) => invalidations,
            Err(error) => return Err(Error::join([removed, Some(error)]).unwrap()),
        };
        self.publish_queue_invalidations(&invalidations);
        if let Some(error) = removed {
            return Err(error);
        }
        Ok(TrackDeletionResult {
            deleted_files: if state.artwork_path.is_empty() { 1 } else { 2 },
        })
    }

    /// Whether any file was removed, whether all were, and what failed.
    fn remove_prepared_deletion_files(
        &self,
        state: &TrackDeletionState,
    ) -> (bool, bool, Option<Error>) {
        let never = || false;
        let audio =
            self.storage
                .remove_managed_file(&never, &state.file_path, &state.content_sha256);
        match audio {
            Ok(true) => {}
            Ok(false) => return (false, false, None),
            Err(error) => return (false, false, Some(error)),
        }
        if state.artwork_path.is_empty() {
            return (true, true, None);
        }
        match self
            .storage
            .remove_managed_file(&never, &state.artwork_path, &state.artwork_sha256)
        {
            Ok(removed) => (true, removed, None),
            Err(error) => (true, false, Some(error)),
        }
    }

    pub async fn recover_pending_track_deletions(&self) -> Result<()> {
        for deletion in self.repo.list_pending_deletions().await? {
            let track_id = &deletion.preview.track_id;
            let (_, all_removed, removed) = self.remove_prepared_deletion_files(&deletion);
            if !all_removed {
                let error = removed.unwrap_or_else(|| Error::new("managed file was not removed"));
                return Err(error.context(format!(
                    "recover pending Permanent Track Deletion {track_id:?}"
                )));
            }
            let invalidations =
                self.repo
                    .finalize_track_deletion(track_id)
                    .await
                    .map_err(|error| {
                        Error::join([removed.clone(), Some(error)])
                            .unwrap()
                            .context(format!(
                                "finalize recovered Permanent Track Deletion {track_id:?}"
                            ))
                    })?;
            self.publish_queue_invalidations(&invalidations);
        }
        Ok(())
    }

    pub async fn preview_album_deletion(&self, album_id: &str) -> Result<AlbumDeletionPreview> {
        Ok(self
            .repo
            .load_album_deletion(&self.storage, album_id)
            .await?
            .0)
    }

    /// One Permanent Track Deletion per Track in Album order after one
    /// token check; a failure stops the run and keeps what was deleted.
    pub async fn delete_album(&self, album_id: &str, token: &str) -> Result<AlbumDeletionResult> {
        let (preview, _) = self
            .repo
            .load_album_deletion(&self.storage, album_id)
            .await?;
        if !tokens_equal(&preview.confirmation_token, token) {
            return Err(Error::wrap(
                Sentinel::DeletionConflict,
                "album deletion preview changed",
            ));
        }
        let mut result = AlbumDeletionResult::default();
        for track in preview.tracks {
            let deleted = match self
                .repo
                .load_track_deletion(&self.storage, &track.track_id)
                .await
            {
                Ok(state) => {
                    self.delete_track(&track.track_id, &state.preview.confirmation_token)
                        .await
                }
                Err(error) => Err(error),
            };
            match deleted {
                Ok(deleted) => {
                    result.deleted_files += deleted.deleted_files;
                    result.deleted.push(track);
                }
                Err(error) => {
                    result.stopped_at = Some(AlbumDeletionFailed {
                        track_id: track.track_id,
                        track_title: track.track_title,
                        reason: error.to_string(),
                    });
                    break;
                }
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletion_tokens_hash_go_json() {
        let state = TrackDeletionState {
            preview: TrackDeletionPreview {
                track_id: "t".into(),
                track_title: "A <B> & C".into(),
                ..TrackDeletionPreview::default()
            },
            album_id: "a".into(),
            file_path: "/m/x.flac".into(),
            content_sha256: "h".into(),
            track_revision: 2,
            source_revision: 1,
            ..TrackDeletionState::default()
        };
        let payload = DeletionTokenPayload {
            track_id: "t",
            track_title: &state.preview.track_title,
            album_id: "a",
            file_path: "/m/x.flac",
            content_sha256: "h",
            artwork_path: "",
            artwork_sha256: "",
            track_revision: 2,
            source_revision: 1,
            playlist_references: &[],
            queue_references: &[],
        };
        assert_eq!(
            crate::gojson::to_string(&payload),
            r#"{"TrackID":"t","TrackTitle":"A \u003cB\u003e \u0026 C","AlbumID":"a","FilePath":"/m/x.flac","ContentSHA256":"h","ArtworkPath":"","ArtworkSHA256":"","TrackRevision":2,"SourceRevision":1,"PlaylistReferences":[],"QueueReferences":[]}"#
        );
        assert_eq!(deletion_token(&state), token_of(&payload));
        assert!(tokens_equal("abc", "abc"));
        assert!(!tokens_equal("abc", "abd"));
        assert!(!tokens_equal("abc", "ab"));
    }
}
