//! Offline Artist credit repair (artist_credit_repair.go, queries/credits.sql,
//! cmd/repair-artist-credits): re-reads the credits of every managed file
//! and rewrites the Track and Album credits that disagree, after a backup.
//! Run only while the Music Server is stopped.

use super::commit::{insert_album_artist_credits, insert_track_artist_credits, upsert_artists};
use super::deletion::delete_unreferenced_artists;
use super::identity::{album_key_of, normalize_identity};
use super::storage::{Limits, Storage};
use crate::db::Db;
use crate::inspect::{self, NormalizedMetadata};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

type Result<T> = std::result::Result<T, String>;

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// An in-memory preview; `apply` revalidates its private snapshot.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistCreditRepairReport {
    pub applied: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub backup_path: String,
    pub changes: Vec<Entry>,
    pub skipped: Vec<Entry>,
    pub conflicts: Vec<Entry>,
    #[serde(skip)]
    database_path: PathBuf,
    #[serde(skip)]
    managed_root: PathBuf,
    #[serde(skip)]
    snapshot: Snapshot,
    #[serde(skip)]
    tracks: HashMap<String, Vec<String>>,
    #[serde(skip)]
    albums: HashMap<String, Vec<String>>,
    #[serde(skip)]
    album_keys: HashMap<String, String>,
    #[serde(skip)]
    ready: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Entry {
    pub kind: String,
    pub id: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct Credit {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Position")]
    position: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Track {
    id: String,
    album_id: String,
    artist_name: String,
    path: String,
    source_kind: String,
    source_path: String,
    hash: String,
    revision: i64,
    source_revision: i64,
    size: i64,
    source_size: i64,
    hidden: bool,
    credits: Vec<Credit>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Album {
    id: String,
    title: String,
    artist_id: String,
    key: String,
    revision: i64,
    credits: Vec<Credit>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Snapshot {
    tracks: Vec<Track>,
    albums: Vec<Album>,
}

fn names(credits: &[Credit]) -> Vec<String> {
    credits.iter().map(|credit| credit.name.clone()).collect()
}

fn same_credits(credits: &[Credit], names: &[String]) -> bool {
    credits.len() == names.len()
        && credits.iter().enumerate().all(|(index, credit)| {
            credit.position == index as i64
                && normalize_identity(&credit.name) == normalize_identity(&names[index])
        })
}

async fn open(path: &Path, writable: bool) -> Result<Db> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("open existing repair database: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err("repair database must be an existing nonempty regular file".to_owned());
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(!writable)
        .create_if_missing(false)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    SqlitePoolOptions::new()
        .max_connections(1)
        .after_connect(|connection, _| Box::pin(crate::searchdata::register_on(connection)))
        .connect_with(options)
        .await
        .map(Db::from)
        .map_err(message)
}

async fn reject_pending(conn: &mut SqliteConnection) -> Result<()> {
    let pending = sqlx::query_scalar!(
        r#"SELECT (EXISTS(SELECT 1 FROM managed_import_commit_journal WHERE phase NOT IN ('completed', 'rolled_back'))
            OR EXISTS(SELECT 1 FROM managed_track_replacements WHERE phase NOT IN ('completed', 'rolled_back'))
            OR EXISTS(SELECT 1 FROM permanent_track_deletions)) AS "pending!: bool""#
    )
    .fetch_one(conn)
    .await
    .map_err(|error| format!("check pending file operations: {error}"))?;
    if pending {
        return Err(
            "pending file operation journal; recover with the Music Server before repair"
                .to_owned(),
        );
    }
    Ok(())
}

async fn read_snapshot(conn: &mut SqliteConnection) -> Result<Snapshot> {
    let track_rows = sqlx::query!(
        r#"SELECT t.id AS "id!", t.album_id, t.artist_name, t.file_path, t.revision, t.size_bytes,
            CAST(t.missing_at IS NOT NULL OR t.is_pending_commit = 1 AS INTEGER) AS "hidden!: i64",
            COALESCE(s.source_kind, '') AS "source_kind!: String",
            COALESCE(s.file_path, '') AS "source_path!: String", COALESCE(s.content_sha256, '') AS "source_hash!: String",
            COALESCE(s.revision, 0) AS "source_revision!: i64", COALESCE(s.size_bytes, 0) AS "source_size!: i64",
            CAST((SELECT json_group_array(json_object('ID', id, 'Name', name, 'Position', position)) FROM (
                SELECT a.id, a.name, c.position FROM track_artists c JOIN artists a ON a.id = c.artist_id
                WHERE c.track_id = t.id ORDER BY c.position
            )) AS TEXT) AS "credits!: String"
        FROM tracks t LEFT JOIN track_sources s ON s.track_id = t.id
        ORDER BY t.id"#
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(message)?;
    let mut snapshot = Snapshot::default();
    for row in track_rows {
        snapshot.tracks.push(Track {
            id: row.id,
            album_id: row.album_id,
            artist_name: row.artist_name,
            path: row.file_path,
            revision: row.revision,
            size: row.size_bytes,
            hidden: row.hidden != 0,
            source_kind: row.source_kind,
            source_path: row.source_path,
            hash: row.source_hash,
            source_revision: row.source_revision,
            source_size: row.source_size,
            credits: serde_json::from_str(&row.credits).map_err(message)?,
        });
    }
    let album_rows = sqlx::query!(
        r#"SELECT b.id AS "id!", b.title, b.artist_id, COALESCE(b.identity_key, '') AS "identity_key!: String", b.revision,
            CAST((SELECT json_group_array(json_object('ID', id, 'Name', name, 'Position', position)) FROM (
                SELECT a.id, a.name, c.position FROM album_artists c JOIN artists a ON a.id = c.artist_id
                WHERE c.album_id = b.id ORDER BY c.position
            )) AS TEXT) AS "credits!: String"
        FROM albums b
        ORDER BY b.id"#
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(message)?;
    for row in album_rows {
        snapshot.albums.push(Album {
            id: row.id,
            title: row.title,
            artist_id: row.artist_id,
            key: row.identity_key,
            revision: row.revision,
            credits: serde_json::from_str(&row.credits).map_err(message)?,
        });
    }
    Ok(snapshot)
}

fn repair_storage(root: &Path) -> Storage {
    Storage::new(
        root.to_str().unwrap_or_default(),
        Limits {
            reserve_bytes: 0,
            file_bytes: 1,
            batch_bytes: 1,
        },
    )
}

fn inspect_track(storage: &Storage, track: &Track) -> Result<NormalizedMetadata> {
    if track.hidden {
        return Err("track is not visible".to_owned());
    }
    if track.source_kind != "managed"
        || track.path != track.source_path
        || track.size != track.source_size
    {
        return Err("authoritative managed source differs from Track".to_owned());
    }
    let (_, size) = storage
        .resolve_managed_file(&track.source_path, &track.hash)
        .map_err(message)?;
    if size != track.source_size {
        return Err("managed source size changed".to_owned());
    }
    let never = || false;
    let mut ctx = inspect::Context::new(&never, None);
    let inspection = inspect::inspect(Path::new(&track.source_path), &mut ctx).map_err(message)?;
    if inspection.file_sha256 != track.hash {
        return Err("managed source changed during inspection".to_owned());
    }
    let (_, size) = storage
        .resolve_managed_file(&track.source_path, &track.hash)
        .map_err(message)?;
    if size != track.source_size {
        return Err("managed source size changed during inspection".to_owned());
    }
    Ok(inspection.metadata)
}

fn consensus(
    album: &Album,
    tracks: &[Track],
    inspected: &HashMap<String, NormalizedMetadata>,
) -> std::result::Result<Vec<String>, &'static str> {
    let mut names: Option<&Vec<String>> = None;
    for track in tracks.iter().filter(|track| track.album_id == album.id) {
        let Some(metadata) = inspected.get(&track.id) else {
            return Err("not all Album members were safely inspected");
        };
        match names {
            None => names = Some(&metadata.album_artists),
            Some(names) if *names != metadata.album_artists => {
                return Err("Album members disagree on Album Artist credits");
            }
            Some(_) => {}
        }
    }
    match names {
        Some(names) if !names.is_empty() => Ok(names.clone()),
        _ => Err("Album has no inspected members"),
    }
}

fn album_identity(album: &Album, names: &[String]) -> Result<String> {
    let old_key = album_key_of(&self::names(&album.credits), &album.title);
    let suffix = album.key.strip_prefix(&old_key).unwrap_or(&album.key);
    if album.key != old_key {
        let edition = suffix.strip_prefix("\u{1f}managed-import-edition:");
        let valid = album.key.starts_with(&old_key)
            && edition.is_some_and(|edition| uuid::Uuid::parse_str(edition).is_ok());
        if !valid {
            return Err("unrecognized old Album identity; manual review required".to_owned());
        }
    }
    Ok(format!("{}{suffix}", album_key_of(names, &album.title)))
}

fn absolute(path: &str) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| PathBuf::from(path))
}

/// Reads only authoritative managed files; never migrates or writes.
pub async fn preview(database_path: &str, managed_root: &str) -> Result<ArtistCreditRepairReport> {
    let mut report = ArtistCreditRepairReport {
        database_path: absolute(database_path),
        managed_root: absolute(managed_root),
        ..ArtistCreditRepairReport::default()
    };
    if managed_root.trim().is_empty() {
        return Err("managed root is required".to_owned());
    }
    let root = std::fs::symlink_metadata(&report.managed_root)
        .map_err(|error| format!("existing managed root required: {error}"))?;
    if !root.is_dir() {
        return Err("managed root must be a directory, not a symlink".to_owned());
    }
    let pool = open(&report.database_path, false).await?;
    {
        let mut conn = pool.acquire().await.map_err(message)?;
        reject_pending(&mut conn).await?;
        report.snapshot = read_snapshot(&mut conn).await?;
    }
    pool.close().await;
    let storage = repair_storage(&report.managed_root);
    let mut inspected = HashMap::new();
    for track in &report.snapshot.tracks {
        match inspect_track(&storage, track) {
            Err(reason) => report.skipped.push(Entry {
                kind: "track".into(),
                id: track.id.clone(),
                reason,
                ..Entry::default()
            }),
            Ok(metadata) => {
                if !same_credits(&track.credits, &metadata.artists)
                    || track.artist_name != metadata.artists.join(", ")
                {
                    report
                        .tracks
                        .insert(track.id.clone(), metadata.artists.clone());
                    report.changes.push(Entry {
                        kind: "track".into(),
                        id: track.id.clone(),
                        before: names(&track.credits),
                        after: metadata.artists.clone(),
                        ..Entry::default()
                    });
                }
                inspected.insert(track.id.clone(), metadata);
            }
        }
    }
    let occupied: HashMap<&str, &str> = report
        .snapshot
        .albums
        .iter()
        .filter(|album| !album.key.is_empty())
        .map(|album| (album.key.as_str(), album.id.as_str()))
        .collect();
    let mut proposed: HashMap<String, Vec<String>> = HashMap::new();
    let mut planned = Vec::new();
    for album in &report.snapshot.albums {
        let names = match consensus(album, &report.snapshot.tracks, &inspected) {
            Ok(names) => names,
            Err(reason) => {
                report.skipped.push(Entry {
                    kind: "album".into(),
                    id: album.id.clone(),
                    reason: reason.to_owned(),
                    ..Entry::default()
                });
                continue;
            }
        };
        if same_credits(&album.credits, &names)
            && album
                .credits
                .first()
                .is_some_and(|first| first.id == album.artist_id)
        {
            continue;
        }
        match album_identity(album, &names) {
            Err(reason) => report.conflicts.push(Entry {
                kind: "album".into(),
                id: album.id.clone(),
                reason,
                ..Entry::default()
            }),
            Ok(key) => {
                proposed
                    .entry(key.clone())
                    .or_default()
                    .push(album.id.clone());
                planned.push((album.id.clone(), names, key));
            }
        }
    }
    for album in &report.snapshot.albums {
        let Some((_, names, key)) = planned.iter().find(|(id, _, _)| *id == album.id) else {
            continue;
        };
        let owner = occupied.get(key.as_str()).copied().unwrap_or_default();
        if (!owner.is_empty() && owner != album.id) || proposed[key].len() > 1 {
            report.conflicts.push(Entry {
                kind: "album".into(),
                id: album.id.clone(),
                reason: "Album identity collision; no merge or split".into(),
                ..Entry::default()
            });
            continue;
        }
        report.albums.insert(album.id.clone(), names.clone());
        report.album_keys.insert(album.id.clone(), key.clone());
        report.changes.push(Entry {
            kind: "album".into(),
            id: album.id.clone(),
            before: self::names(&album.credits),
            after: names.clone(),
            ..Entry::default()
        });
    }
    report.ready = true;
    Ok(report)
}

/// Reserves a new backup file, then `VACUUM INTO` it and checks it.
async fn backup(pool: &Db, path: &Path) -> Result<()> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(message)?;
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(message)?;
    let target = path.to_string_lossy().into_owned();
    sqlx::query("VACUUM INTO ?")
        .bind(&target)
        .execute(pool)
        .await
        .map_err(message)?;
    file.sync_all().map_err(message)?;
    let copy = open(path, false).await?;
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&copy)
        .await
        .map_err(message)?;
    if integrity != "ok" {
        return Err(format!("backup integrity: {integrity}"));
    }
    let broken = sqlx::query("PRAGMA foreign_key_check")
        .fetch_optional(&copy)
        .await
        .map_err(message)?;
    copy.close().await;
    if broken.is_some() {
        return Err("backup foreign key check failed".to_owned());
    }
    Ok(())
}

impl ArtistCreditRepairReport {
    /// Backs up first, then applies only this preview's plan in one
    /// transaction; a changed database or file aborts everything.
    pub async fn apply(&mut self, backup_path: &str) -> Result<()> {
        if !self.ready || self.applied {
            return Err("fresh artist credit preview required".to_owned());
        }
        if self.tracks.is_empty() && self.albums.is_empty() {
            return Ok(());
        }
        let pool = open(&self.database_path, true).await?;
        let result = self.apply_with(&pool, backup_path).await;
        pool.close().await;
        result
    }

    async fn apply_with(&mut self, pool: &Db, backup_path: &str) -> Result<()> {
        reject_pending(&mut *pool.acquire().await.map_err(message)?).await?;
        if backup_path.trim().is_empty() {
            return Err("backup path is required".to_owned());
        }
        let backup_path = absolute(backup_path);
        backup(pool, &backup_path)
            .await
            .map_err(|error| format!("backup failed; no repair applied: {error}"))?;
        self.backup_path = backup_path.to_string_lossy().into_owned();
        let storage = repair_storage(&self.managed_root);
        for track in &self.snapshot.tracks {
            if !self.tracks.contains_key(&track.id) && !self.albums.contains_key(&track.album_id) {
                continue;
            }
            let (_, size) = storage
                .resolve_managed_file(&track.source_path, &track.hash)
                .map_err(|error| format!("stale repair source {}: {error}", track.id))?;
            if size != track.source_size {
                return Err(format!("stale repair source {}: size changed", track.id));
            }
        }
        let mut tx = crate::db::begin_write(pool).await.map_err(message)?;
        reject_pending(&mut tx).await?;
        if read_snapshot(&mut tx).await? != self.snapshot {
            return Err("stale artist credit preview; database changed".to_owned());
        }
        let mut old_artists = Vec::new();
        for track in &self.snapshot.tracks {
            let Some(names) = self.tracks.get(&track.id) else {
                continue;
            };
            let ids = upsert_artists(&mut tx, names, &[], "")
                .await
                .map_err(message)?;
            sqlx::query!("DELETE FROM track_artists WHERE track_id = ?", track.id)
                .execute(&mut *tx)
                .await
                .map_err(message)?;
            insert_track_artist_credits(&mut tx, &track.id, names, &ids)
                .await
                .map_err(message)?;
            let artist_name = names.join(", ");
            sqlx::query!(
                "UPDATE tracks SET artist_name = ?, revision = revision + 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                artist_name,
                track.id
            )
            .execute(&mut *tx)
            .await
            .map_err(message)?;
            old_artists.extend(track.credits.iter().map(|credit| credit.id.clone()));
        }
        for album in &self.snapshot.albums {
            let Some(names) = self.albums.get(&album.id) else {
                continue;
            };
            let ids = upsert_artists(&mut tx, names, &[], "")
                .await
                .map_err(message)?;
            sqlx::query!("DELETE FROM album_artists WHERE album_id = ?", album.id)
                .execute(&mut *tx)
                .await
                .map_err(message)?;
            insert_album_artist_credits(&mut tx, &album.id, names, &ids)
                .await
                .map_err(message)?;
            let key = &self.album_keys[&album.id];
            let artist_id = &ids[&normalize_identity(&names[0])];
            sqlx::query!(
                "UPDATE albums SET artist_id = ?, identity_key = ?, revision = revision + 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                artist_id,
                key,
                album.id
            )
            .execute(&mut *tx)
            .await
            .map_err(message)?;
            old_artists.push(album.artist_id.clone());
            old_artists.extend(album.credits.iter().map(|credit| credit.id.clone()));
        }
        // Relationship cleanup alone would miss the legacy primary-artist FK.
        for id in old_artists {
            let primary = sqlx::query_scalar!(
                r#"SELECT EXISTS(SELECT 1 FROM albums WHERE artist_id = ?) AS "primary!: bool""#,
                id
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(message)?;
            if !primary {
                delete_unreferenced_artists(&mut tx, std::slice::from_ref(&id))
                    .await
                    .map_err(|error| {
                        format!("delete unreferenced Artist after Track Replacement: {error}")
                    })?;
            }
        }
        tx.commit().await.map_err(message)?;
        self.applied = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rekeys_albums_and_keeps_edition_suffixes() {
        let album = Album {
            title: "Al".into(),
            credits: vec![Credit {
                name: "Old".into(),
                ..Credit::default()
            }],
            key: album_key_of(&["Old".into()], "Al"),
            ..Album::default()
        };
        assert_eq!(
            album_identity(&album, &["New".into()]).unwrap(),
            album_key_of(&["New".into()], "Al")
        );
        let edition = format!(
            "{}\u{1f}managed-import-edition:{}",
            album.key,
            uuid::Uuid::new_v4()
        );
        let separate = Album {
            key: edition.clone(),
            ..album.clone()
        };
        assert!(
            album_identity(&separate, &["New".into()])
                .unwrap()
                .contains("managed-import-edition")
        );
        let odd = Album {
            key: "something else".into(),
            ..album
        };
        assert!(album_identity(&odd, &["New".into()]).is_err());
    }
}
