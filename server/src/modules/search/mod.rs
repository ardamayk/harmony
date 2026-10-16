//! Library Search (server/internal/modules/librarysearch): matching,
//! ranking, typo fallback and Best Match over the prepared texts of
//! crate::searchdata, behind `GET /api/v1/library/search`.

mod index;
mod rank;

use crate::app::AppState;
use crate::auth;
use crate::query::GoQuery;
use crate::respond::{ApiError, ApiResult};
use crate::searchdata;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use index::{Entity, Index, IndexError};
use rank::{Match, Query};
use serde::Serialize;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::Mutex;

const MAX_QUERY_CHARACTERS: usize = 200;
const GROUP_ORDER: [&str; 5] = ["track", "album", "artist", "genre", "playlist"];

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/library/search", get(search))
}

struct Snapshot {
    index: Arc<Index>,
    revision: i64,
    user_id: String,
}

/// Ranks queries over one cached index, reloaded whenever the search
/// revision changes (a write to searchable data bumps it).
pub struct SearchService {
    pool: SqlitePool,
    snapshot: Mutex<Option<Snapshot>>,
}

impl SearchService {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            snapshot: Mutex::new(None),
        }
    }

    async fn current_index(&self, user_id: &str) -> Result<Arc<Index>, IndexError> {
        // The common case, an unchanged library, costs one SELECT and no
        // lock held across the database.
        let revision = index::current_revision(&self.pool).await?;
        if let Some(current) = self.snapshot.lock().await.as_ref()
            && current.revision == revision
            && current.user_id == user_id
        {
            return Ok(current.index.clone());
        }
        // A reload reads the revision again with the fields, in one
        // transaction, and only one request reloads at a time.
        let mut snapshot = self.snapshot.lock().await;
        let (index, revision) = index::load(&self.pool, user_id, |revision| {
            snapshot
                .as_ref()
                .is_some_and(|current| current.revision == revision && current.user_id == user_id)
        })
        .await?;
        match index {
            None => Ok(snapshot.as_ref().expect("reused snapshot").index.clone()),
            Some(index) => {
                let index = Arc::new(index);
                *snapshot = Some(Snapshot {
                    index: index.clone(),
                    revision,
                    user_id: user_id.to_owned(),
                });
                Ok(index)
            }
        }
    }
}

#[derive(Serialize)]
struct Reference {
    id: String,
    name: String,
}

#[derive(Serialize)]
struct SearchResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    album: Option<Reference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    artists: Option<Vec<Reference>>,
    id: String,
    #[serde(rename = "match")]
    matched: &'static str,
    name: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResponse {
    albums: Vec<SearchResult>,
    artists: Vec<SearchResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    best_match: Option<SearchResult>,
    genres: Vec<SearchResult>,
    playlists: Vec<SearchResult>,
    total: usize,
    tracks: Vec<SearchResult>,
}

fn to_result(entity: &Entity, matched: Match) -> SearchResult {
    let credits = match entity.kind.as_str() {
        "track" => Some("track_artist"),
        "album" => Some("album_artist"),
        _ => None,
    };
    let reference = |field: &index::FieldRef| Reference {
        id: field.related_id.clone(),
        name: field.text.text.original.clone(),
    };
    let albums: Vec<_> = entity.fields("album").collect();
    SearchResult {
        album: (albums.len() == 1).then(|| reference(albums[0])),
        artists: credits.map(|credits| entity.fields(credits).map(reference).collect()),
        id: entity.id.clone(),
        matched: matched.as_str(),
        name: entity.primary.text.original.clone(),
        kind: entity.kind.clone(),
    }
}

async fn search(State(state): State<AppState>, query: GoQuery) -> ApiResult<Json<SearchResponse>> {
    let raw = query.get("q");
    if raw.trim().is_empty() {
        return Err(ApiError::bad_request("query parameter q is required"));
    }
    if raw.chars().count() > MAX_QUERY_CHARACTERS {
        return Err(ApiError::bad_request(format!(
            "query is longer than {MAX_QUERY_CHARACTERS} characters"
        )));
    }
    let prepared = Query::new(&searchdata::prepare_query(raw));
    if prepared.is_empty() {
        return Err(ApiError::bad_request(
            "query has no searchable letters or digits",
        ));
    }
    let all = match query.values("all").as_deref() {
        None => false,
        Some(["true"]) => true,
        Some(["false"]) => false,
        Some(_) => {
            return Err(ApiError::bad_request(
                "query parameter all must be true or false",
            ));
        }
    };
    let index = state
        .search
        .current_index(auth::current_user().id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Library Search failed");
            ApiError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Library Search is unavailable",
            )
        })?;
    let ranked = rank::rank(&index, &prepared, all);
    let mut groups = ranked.groups;
    let mut group = |kind: &str| -> Vec<SearchResult> {
        groups
            .remove(kind)
            .unwrap_or_default()
            .iter()
            .map(|entity| to_result(entity, ranked.matched))
            .collect()
    };
    let [tracks, albums, artists, genres, playlists] = GROUP_ORDER.map(&mut group);
    Ok(Json(SearchResponse {
        albums,
        artists,
        best_match: ranked
            .best_match
            .map(|entity| to_result(&entity, Match::Direct)),
        genres,
        playlists,
        total: ranked.total,
        tracks,
    }))
}
