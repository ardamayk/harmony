//! Saved radio stations (radio/repository.go, queries.sql).

use super::types::{NowPlaying, Station, StationCreate};
use crate::db::Db;

#[derive(Debug, thiserror::Error)]
pub enum RadioError {
    #[error("radio station not found")]
    NotFound,
    #[error("{}", crate::gojson::sqlite_error(.0))]
    Database(#[from] sqlx::Error),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Clone)]
pub struct Repository {
    pool: Db,
}

struct Row {
    id: String,
    name: String,
    stream_url: String,
    homepage_url: String,
    favicon_url: String,
    country: String,
    language: String,
    tags: String,
    codec: String,
    bitrate: i64,
    source: String,
    external_id: String,
    is_favorite: i64,
    position: i64,
    last_now_playing_title: String,
    last_now_playing_artist: String,
    last_now_playing_raw: String,
    last_now_playing_updated_at: Option<String>,
}

macro_rules! select_stations {
    ($tail:literal $(, $argument:expr)* $(,)?) => {
        sqlx::query_as!(
            Row,
            r#"SELECT id AS "id!", name, stream_url, homepage_url, favicon_url, country, language, tags,
                codec, bitrate, source, external_id, is_favorite, position,
                last_now_playing_title, last_now_playing_artist, last_now_playing_raw,
                last_now_playing_updated_at AS "last_now_playing_updated_at?: String"
            FROM radio_stations "# + $tail
            $(, $argument)*
        )
    };
}

fn to_station(row: Row) -> Station {
    let mut station = Station {
        id: row.id,
        name: row.name,
        stream_url: row.stream_url,
        homepage_url: row.homepage_url,
        favicon_url: row.favicon_url,
        country: row.country,
        language: row.language,
        tags: serde_json::from_str(&row.tags).unwrap_or_default(),
        codec: row.codec,
        bitrate: row.bitrate,
        source: row.source,
        external_id: row.external_id,
        is_favorite: row.is_favorite != 0,
        position: row.position,
        last_now_playing: None,
    };
    if !row.last_now_playing_title.is_empty()
        || !row.last_now_playing_artist.is_empty()
        || !row.last_now_playing_raw.is_empty()
        || row.last_now_playing_updated_at.is_some()
    {
        station.last_now_playing = Some(NowPlaying {
            title: row.last_now_playing_title,
            artist: row.last_now_playing_artist,
            raw: row.last_now_playing_raw,
            updated_at: row
                .last_now_playing_updated_at
                .as_deref()
                .map(crate::gojson::time),
            stale: false,
        });
    }
    station
}

fn encode_tags(tags: &[String]) -> String {
    crate::gojson::to_string(tags)
}

impl Repository {
    pub fn new(pool: Db) -> Self {
        Self { pool }
    }

    pub async fn list(&self, user_id: &str) -> Result<Vec<Station>, RadioError> {
        let rows = select_stations!(
            "WHERE user_id = ? ORDER BY is_favorite DESC, position ASC, name COLLATE NOCASE",
            user_id
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(to_station).collect())
    }

    pub async fn get(&self, user_id: &str, station_id: &str) -> Result<Station, RadioError> {
        select_stations!("WHERE user_id = ? AND id = ?", user_id, station_id)
            .fetch_optional(&self.pool)
            .await?
            .map(to_station)
            .ok_or(RadioError::NotFound)
    }

    pub async fn get_by_external_id(
        &self,
        user_id: &str,
        source: &str,
        external_id: &str,
    ) -> Result<Station, RadioError> {
        if external_id.is_empty() {
            return Err(RadioError::NotFound);
        }
        select_stations!(
            "WHERE user_id = ? AND source = ? AND external_id = ?",
            user_id,
            source,
            external_id
        )
        .fetch_optional(&self.pool)
        .await?
        .map(to_station)
        .ok_or(RadioError::NotFound)
    }

    /// Appends the station after the user's existing ones.
    pub async fn insert(&self, user_id: &str, input: &StationCreate) -> Result<String, RadioError> {
        let position = sqlx::query_scalar!(
            r#"SELECT CAST(COALESCE(MAX(position) + 1, 0) AS INTEGER) AS "next!: i64"
            FROM radio_stations WHERE user_id = ?"#,
            user_id
        )
        .fetch_one(&self.pool)
        .await?;
        let id = uuid::Uuid::new_v4().to_string();
        let tags = encode_tags(&input.tags);
        let is_favorite = i64::from(input.is_favorite);
        sqlx::query!(
            "INSERT INTO radio_stations (
                id, user_id, name, stream_url, homepage_url, favicon_url, country, language, tags, codec, bitrate,
                source, external_id, is_favorite, position
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            id,
            user_id,
            input.name,
            input.stream_url,
            input.homepage_url,
            input.favicon_url,
            input.country,
            input.language,
            tags,
            input.codec,
            input.bitrate,
            input.source,
            input.external_id,
            is_favorite,
            position
        )
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn update(&self, user_id: &str, station: &Station) -> Result<(), RadioError> {
        let tags = encode_tags(&station.tags);
        let is_favorite = i64::from(station.is_favorite);
        sqlx::query!(
            "UPDATE radio_stations
            SET name = ?, stream_url = ?, homepage_url = ?, favicon_url = ?, country = ?, language = ?,
                tags = ?, codec = ?, bitrate = ?, is_favorite = ?, position = ?, updated_at = CURRENT_TIMESTAMP
            WHERE user_id = ? AND id = ?",
            station.name,
            station.stream_url,
            station.homepage_url,
            station.favicon_url,
            station.country,
            station.language,
            tags,
            station.codec,
            station.bitrate,
            is_favorite,
            station.position,
            user_id,
            station.id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete(&self, user_id: &str, station_id: &str) -> Result<(), RadioError> {
        let result = sqlx::query!(
            "DELETE FROM radio_stations WHERE user_id = ? AND id = ?",
            user_id,
            station_id
        )
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(RadioError::NotFound);
        }
        Ok(())
    }

    /// `updated_at` is stored the way the Go driver writes a time.
    pub async fn update_now_playing(
        &self,
        station_id: &str,
        now: &NowPlaying,
        updated_at: Option<String>,
    ) -> Result<(), RadioError> {
        sqlx::query!(
            "UPDATE radio_stations
            SET last_now_playing_title = ?, last_now_playing_artist = ?, last_now_playing_raw = ?,
                last_now_playing_updated_at = ?, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?",
            now.title,
            now.artist,
            now.raw,
            updated_at,
            station_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
