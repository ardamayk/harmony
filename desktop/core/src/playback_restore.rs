use crate::connection::{
    ConnectionError, ConnectionErrorCode, HttpBridge, HttpRequest, ServerOrigin,
};
use crate::playback_lifecycle::{
    PlaybackLifecycleError, PlaybackSessionSnapshot, PlaybackSnapshotStore,
};

pub(crate) async fn load_saved_playback(
    store: &PlaybackSnapshotStore,
    bridge: &HttpBridge,
    origin: Option<&ServerOrigin>,
) -> Result<Option<PlaybackSessionSnapshot>, PlaybackLifecycleError> {
    let Some(snapshot) = store.load()? else {
        return Ok(None);
    };
    let Some(origin) = origin else {
        return Ok(Some(snapshot));
    };
    match is_saved_track_missing(&snapshot, bridge, origin).await {
        Ok(true) => {
            let cleared = PlaybackSessionSnapshot::new(
                None,
                0.0,
                snapshot.volume(),
                snapshot.is_shuffle_enabled(),
                snapshot.repeat_mode(),
            )?;
            store.save(&cleared)?;
            eprintln!(
                "Saved playback track no longer exists on the Music Server; cleared its source and playhead."
            );
            Ok(Some(cleared))
        }
        Ok(false) => Ok(Some(snapshot)),
        Err(error) => {
            eprintln!("Saved playback track validation failed; preserving the session: {error}");
            Ok(Some(snapshot))
        }
    }
}

async fn is_saved_track_missing(
    snapshot: &PlaybackSessionSnapshot,
    bridge: &HttpBridge,
    origin: &ServerOrigin,
) -> Result<bool, ConnectionError> {
    let Some(source) = snapshot.source().filter(|source| source["type"] == "track") else {
        return Ok(false);
    };
    let identifier = source
        .pointer("/track/id")
        .and_then(serde_json::Value::as_str)
        .filter(|identifier| !identifier.is_empty() && !identifier.contains(['/', '?', '#']))
        .ok_or_else(|| {
            ConnectionError::new(
                ConnectionErrorCode::InvalidRequest,
                "Saved playback track identifier is invalid.",
            )
        })?;
    let response = bridge
        .send(
            origin,
            HttpRequest {
                method: "GET".to_owned(),
                url: format!("/api/v1/library/tracks/{identifier}"),
                headers: Default::default(),
                body: None,
            },
        )
        .await?;
    if response.status == reqwest::StatusCode::OK.as_u16() {
        return Ok(false);
    }
    let error_body = serde_json::from_slice::<serde_json::Value>(&response.body).ok();
    if response.status == reqwest::StatusCode::NOT_FOUND.as_u16()
        && error_body.as_ref().and_then(|body| body["code"].as_str()) == Some("not_found")
    {
        return Ok(true);
    }
    Err(ConnectionError::new(
        ConnectionErrorCode::InvalidResponse,
        format!(
            "Music Server returned HTTP {} while checking saved track {identifier}.",
            response.status
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, http::StatusCode, routing::get};
    use serde_json::json;

    fn saved_track() -> (
        std::path::PathBuf,
        PlaybackSnapshotStore,
        PlaybackSessionSnapshot,
    ) {
        let directory =
            std::env::temp_dir().join(format!("playback-restore-{}", uuid::Uuid::new_v4()));
        let store = PlaybackSnapshotStore::new(directory.join("playback-session.json"));
        let snapshot = PlaybackSessionSnapshot::new(
            Some(json!({"type": "track", "track": {"id": "missing-track"}})),
            42.5,
            0.65,
            true,
            "loop",
        )
        .expect("valid snapshot");
        store.save(&snapshot).expect("save snapshot");
        (directory, store, snapshot)
    }

    async fn serve_track(
        status: StatusCode,
        body: &'static str,
    ) -> (ServerOrigin, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind server");
        let origin =
            ServerOrigin::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let router = Router::new().route(
            "/api/v1/library/tracks/missing-track",
            get(move || async move { (status, [("content-type", "application/json")], body) }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.expect("serve track");
        });
        (origin, server)
    }

    #[tokio::test]
    async fn missing_saved_track_is_cleared_before_restore_and_persisted() {
        let (directory, store, _) = saved_track();
        let (origin, server) = serve_track(
            StatusCode::NOT_FOUND,
            r#"{"code":"not_found","message":"track not found"}"#,
        )
        .await;
        let restored = load_saved_playback(&store, &HttpBridge::new().unwrap(), Some(&origin))
            .await
            .expect("load saved playback")
            .expect("snapshot");
        assert!(restored.source().is_none());
        assert_eq!(restored.playhead_seconds(), 0.0);
        assert_eq!(restored.volume(), 0.65);
        assert!(restored.is_shuffle_enabled());
        assert_eq!(restored.repeat_mode(), "loop");
        assert_eq!(store.load().unwrap(), Some(restored));
        server.abort();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn existing_saved_track_keeps_source_playhead_and_preferences() {
        let (directory, store, snapshot) = saved_track();
        let (origin, server) = serve_track(StatusCode::OK, r#"{"id":"missing-track"}"#).await;
        let restored = load_saved_playback(&store, &HttpBridge::new().unwrap(), Some(&origin))
            .await
            .unwrap();
        assert_eq!(restored, Some(snapshot.clone()));
        assert_eq!(store.load().unwrap(), Some(snapshot));
        server.abort();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn unconfirmed_server_errors_do_not_discard_saved_playback() {
        for (status, body) in [
            (StatusCode::SERVICE_UNAVAILABLE, r#"{"code":"unavailable"}"#),
            (StatusCode::NOT_FOUND, "<html>Not Found</html>"),
        ] {
            let (directory, store, snapshot) = saved_track();
            let (origin, server) = serve_track(status, body).await;
            let restored = load_saved_playback(&store, &HttpBridge::new().unwrap(), Some(&origin))
                .await
                .unwrap();
            assert_eq!(restored, Some(snapshot.clone()));
            assert_eq!(store.load().unwrap(), Some(snapshot));
            server.abort();
            std::fs::remove_dir_all(directory).unwrap();
        }
    }

    #[tokio::test]
    async fn unreachable_server_preserves_saved_playback() {
        let (directory, store, snapshot) = saved_track();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin =
            ServerOrigin::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        drop(listener);
        let restored = load_saved_playback(&store, &HttpBridge::new().unwrap(), Some(&origin))
            .await
            .unwrap();
        assert_eq!(restored, Some(snapshot.clone()));
        assert_eq!(store.load().unwrap(), Some(snapshot));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn no_server_or_non_track_source_preserves_saved_playback() {
        let (directory, store, snapshot) = saved_track();
        let bridge = HttpBridge::new().unwrap();
        assert_eq!(
            load_saved_playback(&store, &bridge, None).await.unwrap(),
            Some(snapshot)
        );
        let radio = PlaybackSessionSnapshot::new(
            Some(json!({"type":"radio-station", "station":{"id":"station-1"}})),
            0.0,
            0.65,
            true,
            "loop",
        )
        .unwrap();
        store.save(&radio).unwrap();
        let (origin, server) = serve_track(StatusCode::NOT_FOUND, r#"{"code":"not_found"}"#).await;
        assert_eq!(
            load_saved_playback(&store, &bridge, Some(&origin))
                .await
                .unwrap(),
            Some(radio)
        );
        server.abort();
        std::fs::remove_dir_all(directory).unwrap();
    }
}
