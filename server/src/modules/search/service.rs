//! Library Search over one cached index: matching, ranking, typo fallback
//! and Best Match.

use super::index::{self, Entity, Index};
use super::rank::{self, Match, Query};
use super::repository::{IndexError, Repository};
use crate::db::Db;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Mutex;

const PLAYLIST_KIND: &str = "playlist";
const GROUP_ORDER: [&str; 5] = ["track", "album", "artist", "genre", PLAYLIST_KIND];

struct Snapshot {
    index: Arc<Index>,
    revision: i64,
    user_id: String,
}

/// Ranks queries over one cached index, reloaded whenever the search
/// revision changes (a write to searchable data bumps it).
pub struct SearchService {
    repo: Repository,
    snapshot: Mutex<Option<Snapshot>>,
}

impl SearchService {
    pub fn new(pool: Db) -> Self {
        Self {
            repo: Repository::new(pool),
            snapshot: Mutex::new(None),
        }
    }

    async fn current_index(&self, user_id: &str) -> Result<Arc<Index>, IndexError> {
        // The common case, an unchanged library, costs one SELECT and no
        // lock held across the database.
        let revision = self.repo.current_revision().await?;
        if let Some(current) = self.snapshot.lock().await.as_ref()
            && current.revision == revision
            && current.user_id == user_id
        {
            return Ok(current.index.clone());
        }
        // A reload reads the revision again with the fields, in one
        // transaction, and only one request reloads at a time.
        let mut snapshot = self.snapshot.lock().await;
        let (index, revision) = self
            .repo
            .load(user_id, |revision| {
                snapshot.as_ref().is_some_and(|current| {
                    current.revision == revision && current.user_id == user_id
                })
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

    /// Ranks a prepared query over the index for this user.
    pub async fn search(
        &self,
        user_id: &str,
        query: &Query,
        all: bool,
        include_playlists: bool,
    ) -> Result<SearchResponse, IndexError> {
        let index = self.current_index(user_id).await?;
        let hidden_kind = (!include_playlists).then_some(PLAYLIST_KIND);
        let ranked = rank::rank(&index, query, all, hidden_kind);
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
        Ok(SearchResponse {
            albums,
            artists,
            best_match: ranked
                .best_match
                .map(|entity| to_result(&entity, Match::Direct)),
            genres,
            playlists,
            total: ranked.total,
            tracks,
        })
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
pub struct SearchResponse {
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
