//! A visibility-filtered snapshot of every searchable entity
//! (librarysearch/index.go, repository.go). Candidate collection covers the
//! whole eligible library, so ranking and group limits apply globally.

use crate::searchdata::{Text, is_letter_or_digit};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;

/// One stored representation with its word lists computed once per load;
/// queries never normalize library names again.
#[derive(Debug)]
pub struct PreparedText {
    pub text: Text,
    pub faithful_words: Vec<String>,
    pub folded_words: Vec<String>,
    pub compact_words: Vec<String>,
}

impl PreparedText {
    fn new(text: Text) -> Self {
        Self {
            faithful_words: split_faithful(&text.faithful),
            folded_words: fields(&text.folded),
            compact_words: fields(&text.compact),
            text,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FieldRef {
    pub name: String,
    pub related_id: String,
    pub text: Arc<PreparedText>,
}

#[derive(Debug)]
pub struct Entity {
    pub kind: String,
    pub id: String,
    pub primary: Arc<PreparedText>,
    pub related: Vec<FieldRef>,
}

impl Entity {
    pub fn fields<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a FieldRef> + 'a {
        self.related.iter().filter(move |field| field.name == name)
    }
}

#[derive(Debug, Default)]
pub struct Index {
    pub entities: Vec<Arc<Entity>>,
}

pub fn fields(value: &str) -> Vec<String> {
    value.split_whitespace().map(str::to_owned).collect()
}

/// Keeps accents and inner punctuation (don't, ac/dc) as spelling evidence
/// while ignoring enclosing punctuation such as a version section's
/// parentheses.
pub fn split_faithful(faithful: &str) -> Vec<String> {
    faithful
        .split_whitespace()
        .map(|word| word.trim_matches(|character: char| !is_letter_or_digit(character)))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("{0}")]
    Database(#[from] sqlx::Error),
    #[error("{0}")]
    Invalid(String),
}

/// Reads the revision and, unless `reuse` accepts it, every field from one
/// SQLite snapshot, so an index never mixes pre- and post-commit data.
pub async fn load(
    pool: &SqlitePool,
    user_id: &str,
    reuse: impl FnOnce(i64) -> bool,
) -> Result<(Option<Index>, i64), IndexError> {
    let mut transaction = pool.begin().await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faithful_words_drop_enclosing_punctuation_only() {
        assert_eq!(
            split_faithful("don't (live) ac/dc ... «x»"),
            ["don't", "live", "ac/dc", "x"]
        );
    }
}
