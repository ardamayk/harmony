//! Radio (server/internal/modules/radio): saved stations, the Radio Browser
//! catalog, catalog previews, the stream proxy and now-playing metadata.

mod browser;
mod handlers;
#[cfg(test)]
mod hls_browser_test;
mod repository;
mod service;
mod stream;
mod types;

use crate::gojson;
use browser::RadioBrowserClient;
use repository::Repository;
use service::Service;
use std::sync::Arc;
use std::time::SystemTime;
use stream::{NowPlayingCache, NowPlayingSink, StreamProxy};
use types::NowPlaying;

// The External Source proxies its streams with the same SSRF guards and HLS
// rewriting.
pub(crate) use stream::{
    HLS_CONTENT_TYPE, HLS_RESOURCE_TOKEN_TTL_SECONDS, MAX_HLS_PLAYLIST_BYTES, ResourceSigner,
    RewriteError, SafeUpstream, Upstream, UpstreamResponse, copy_stream_headers, is_hls_content,
    rewrite_playlist_with, validate_stream_url,
};

pub const RADIO_BROWSER_SOURCE: &str = "radio-browser";

/// Everything radio keeps across requests.
pub struct Radio {
    service: Service,
    browser: RadioBrowserClient,
    proxy: StreamProxy,
    cache: Arc<NowPlayingCache>,
}

/// Now playing from a stream goes to the cache and, in the background, to
/// the station row.
struct Recorder {
    cache: Arc<NowPlayingCache>,
    repo: Repository,
}

impl NowPlayingSink for Recorder {
    fn now_playing(&self, station_id: &str, now: NowPlaying, updated: Option<SystemTime>) {
        self.cache.set(station_id, now.clone(), updated);
        let repo = self.repo.clone();
        let station_id = station_id.to_owned();
        tokio::spawn(async move {
            let updated_at = updated.map(gojson::sqlite_time);
            if let Err(error) = repo.update_now_playing(&station_id, &now, updated_at).await {
                tracing::warn!(stationId = %station_id, %error, "now playing was not saved");
            }
        });
    }
}

impl Radio {
    pub fn new(pool: crate::db::Db, radio_browser_base_url: &str) -> Self {
        let repo = Repository::new(pool);
        let cache = Arc::new(NowPlayingCache::default());
        let recorder = Arc::new(Recorder {
            cache: cache.clone(),
            repo: repo.clone(),
        });
        Self {
            service: Service { repo },
            browser: RadioBrowserClient::new(radio_browser_base_url),
            proxy: StreamProxy::new(Arc::new(SafeUpstream::new()), recorder),
            cache,
        }
    }
}

pub use handlers::routes;
