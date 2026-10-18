//! The Library Search snapshot reads (librarysearch/repository.go).

use super::index::{Entity, FieldRef, Index, PreparedText};
use crate::searchdata::Text;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("{0}")]
    Database(#[from] sqlx::Error),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Clone)]
pub struct Repository {
    pool: SqlitePool,
}

impl Repository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// The current search revision, read on its own (no transaction).
    pub async fn current_revision(&self) -> Result<i64, IndexError> {
        Ok(sqlx::query_scalar!(
            r#"SELECT revision AS "revision!: i64" FROM library_search_revision WHERE singleton = 1"#
        )
        .fetch_one(&self.pool)
        .await?)
    }

    /// Reads the revision and, unless `reuse` accepts it, every field from one
    /// SQLite snapshot, so an index never mixes pre- and post-commit data.
    pub async fn load(
        &self,
        user_id: &str,
        reuse: impl FnOnce(i64) -> bool,
    ) -> Result<(Option<Index>, i64), IndexError> {
        let mut transaction = self.pool.begin().await?;
        let revision = sqlx::query_scalar!(
            r#"SELECT revision AS "revision!: i64" FROM library_search_revision WHERE singleton = 1"#
        )
        .fetch_one(&mut *transaction)
        .await?;
        if reuse(revision) {
            return Ok((None, revision));
        }
        let mut texts: HashMap<(String, String), Arc<PreparedText>> = HashMap::new();
        for row in sqlx::query!("SELECT kind, id, text_json FROM library_search_texts")
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| IndexError::Invalid(format!("read Library Search texts: {error}")))?
        {
            let text: Text = serde_json::from_str(&row.text_json).map_err(|error| {
                IndexError::Invalid(format!(
                    "decode Library Search {} {:?} text: {error}",
                    row.kind, row.id
                ))
            })?;
            texts.insert((row.kind, row.id), Arc::new(PreparedText::new(text)));
        }
        let sources = sqlx::query!(
            r#"SELECT source.kind AS "kind!: String", source.id AS "id!: String", source.field AS "field!: String",
                source.related_kind AS "related_kind!: String", source.related_id AS "related_id!: String"
            FROM library_search_field_sources source
            JOIN library_search_entities entity ON entity.kind = source.kind AND entity.id = source.id
            WHERE entity.user_id = '' OR entity.user_id = ?
            ORDER BY source.kind, source.id, CASE source.field WHEN 'primary' THEN 0 ELSE 1 END,
                source.field, source.position, source.related_id"#,
            user_id
        )
        .fetch_all(&mut *transaction)
        .await
        .map_err(|error| IndexError::Invalid(format!("read Library Search fields: {error}")))?;
        transaction.commit().await?;

        struct Building {
            kind: String,
            id: String,
            primary: Option<Arc<PreparedText>>,
            related: Vec<FieldRef>,
        }
        let mut building: Vec<Building> = Vec::new();
        for source in sources {
            let text = texts
                .get(&(source.related_kind.clone(), source.related_id.clone()))
                .cloned()
                .ok_or_else(|| {
                    IndexError::Invalid(format!(
                        "prepared Library Search data for {} {:?} is missing; rebuild derived data",
                        source.related_kind, source.related_id
                    ))
                })?;
            let is_current = building
                .last()
                .is_some_and(|current| current.kind == source.kind && current.id == source.id);
            if !is_current {
                building.push(Building {
                    kind: source.kind.clone(),
                    id: source.id.clone(),
                    primary: None,
                    related: Vec::new(),
                });
            }
            let current = building.last_mut().expect("current entity");
            if source.field == "primary" {
                current.primary = Some(text);
            } else {
                current.related.push(FieldRef {
                    name: source.field,
                    related_id: source.related_id,
                    text,
                });
            }
        }
        let mut index = Index::default();
        for entity in building {
            let primary = entity.primary.ok_or_else(|| {
                IndexError::Invalid(format!(
                    "searchable {} {:?} has no primary name",
                    entity.kind, entity.id
                ))
            })?;
            index.entities.push(Arc::new(Entity {
                kind: entity.kind,
                id: entity.id,
                primary,
                related: entity.related,
            }));
        }
        Ok((Some(index), revision))
    }
}
