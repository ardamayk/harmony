//! One preferences row per user; theme, layout and playback are JSON
//! columns (preferences/repository.go).

use super::rules::{apply_playback_patch, normalize_layout};
use super::types::*;
use crate::db;
use sqlx::{SqliteConnection, SqlitePool};

#[derive(Clone)]
pub struct Repository {
    pool: SqlitePool,
}

#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("get preferences: {0}")]
    Get(sqlx::Error),
    #[error("decode layout: {0}")]
    Layout(serde_json::Error),
    #[error("update preferences: {0}")]
    Update(sqlx::Error),
    #[error("preferences transaction: {0}")]
    Transaction(sqlx::Error),
}

impl Repository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get(&self, user_id: &str) -> Result<UserPreferences, RepositoryError> {
        let mut connection = self.pool.acquire().await.map_err(RepositoryError::Get)?;
        get(&mut connection, user_id).await
    }

    /// Reads, applies `mutate` and writes in one IMMEDIATE transaction, so
    /// the lock is taken before the read and concurrent sparse patches merge.
    pub async fn update(
        &self,
        user_id: &str,
        mutate: impl FnOnce(&mut UserPreferences),
    ) -> Result<UserPreferences, RepositoryError> {
        let mut transaction = db::begin_write(&self.pool)
            .await
            .map_err(RepositoryError::Transaction)?;
        let mut current = get(&mut transaction, user_id).await?;
        mutate(&mut current);
        let theme = theme_to_column(&current.theme);
        let layout = crate::gojson::to_string(&current.layout);
        let playback = crate::gojson::to_string(&current.playback);
        sqlx::query!(
            "UPDATE user_preferences
             SET theme = ?, layout_json = ?, playback_json = ?, updated_at = CURRENT_TIMESTAMP
             WHERE user_id = ?",
            theme,
            layout,
            playback,
            user_id,
        )
        .execute(&mut *transaction)
        .await
        .map_err(RepositoryError::Update)?;
        transaction
            .commit()
            .await
            .map_err(RepositoryError::Transaction)?;
        Ok(current)
    }
}

async fn get(
    connection: &mut SqliteConnection,
    user_id: &str,
) -> Result<UserPreferences, RepositoryError> {
    let row = sqlx::query!(
        "SELECT theme, layout_json, playback_json FROM user_preferences WHERE user_id = ?",
        user_id
    )
    .fetch_one(&mut *connection)
    .await
    .map_err(RepositoryError::Get)?;
    let layout: LayoutInput =
        serde_json::from_str(&row.layout_json).map_err(RepositoryError::Layout)?;
    Ok(UserPreferences {
        theme: parse_theme_column(&row.theme),
        layout: normalize_layout(layout),
        playback: parse_playback_column(&row.playback_json),
    })
}

/// The theme column is JSON, or a bare mode from before presets existed.
fn parse_theme_column(raw: &str) -> ThemePreferences {
    if let Ok(mut theme) = serde_json::from_str::<ThemePreferences>(raw)
        && !theme.mode.is_empty()
    {
        if theme.preset.is_empty() {
            theme.preset = "earthly".to_owned();
        }
        return theme;
    }
    match raw {
        "light" | "dark" | "system" => ThemePreferences {
            mode: raw.to_owned(),
            preset: "earthly".to_owned(),
        },
        _ => default_theme(),
    }
}

fn theme_to_column(theme: &ThemePreferences) -> String {
    let mut theme = if theme.mode.is_empty() {
        default_theme()
    } else {
        theme.clone()
    };
    if theme.preset.is_empty() {
        theme.preset = "earthly".to_owned();
    }
    crate::gojson::to_string(&theme)
}

/// Decodes the stored JSON over the defaults, so rows written before a field
/// existed still read back complete.
fn parse_playback_column(raw: &str) -> PlaybackPreferences {
    let mut playback = default_playback();
    if let Ok(stored) = serde_json::from_str::<PlaybackPreferencesPatch>(raw) {
        apply_playback_patch(&mut playback, Some(&stored));
    }
    playback
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_column_reads_json_and_legacy_modes() {
        assert_eq!(
            parse_theme_column(r#"{"mode":"dark"}"#),
            ThemePreferences {
                mode: "dark".into(),
                preset: "earthly".into()
            }
        );
        assert_eq!(parse_theme_column("light").mode, "light");
        assert_eq!(parse_theme_column("purple"), default_theme());
        assert_eq!(
            theme_to_column(&ThemePreferences::default()),
            r#"{"mode":"system","preset":"earthly"}"#
        );
    }

    #[test]
    fn playback_column_fills_missing_fields_from_defaults() {
        let playback = parse_playback_column(r#"{"playbackRate":1.5}"#);
        assert_eq!(playback.playback_rate, 1.5);
        assert_eq!(playback.seek_step_seconds, 5);
        assert_eq!(parse_playback_column("not json"), default_playback());
    }
}
