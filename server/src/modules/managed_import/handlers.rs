//! Managed Import HTTP handlers (managedimport/handlers.go,
//! artwork-handlers.go, upload_deadline.go, import_session.go).

use super::ManagedImport;
use super::errors::{Error, Issue, Result as ImportResult, Sentinel};
use super::model::*;
use crate::app::AppState;
use crate::observability;
use crate::respond::{ApiError, ErrorBody};
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use futures_util::StreamExt;
use sentry::SentryFutureExt;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const IMPORT_FILENAME_HEADER: &str = "x-import-filename";
const IMPORT_FILENAME_ENCODING_HEADER: &str = "x-import-filename-encoding";
const MAX_JOB_CREATE_BODY_BYTES: usize = 4 * 1024;
const MAX_CONFIRMATION_BODY_BYTES: usize = 4 * 1024;
const MAX_BATCH_CONFIRMATION_BODY_BYTES: usize = 256 * 1024;
const MAX_UPLOADED_ARTWORK_BYTES: usize = 20 * 1024 * 1024;
/// An upload that sends nothing for this long is interrupted.
const UPLOAD_INACTIVITY_TIMEOUT: Duration = Duration::from_secs(30);

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/import-history", get(list_history))
        .route(
            "/api/v1/import-batches/{batch_id}/albums/{album_key}/artwork",
            put(upload_artwork),
        )
        .route(
            "/api/v1/import-batches/{batch_id}/artwork/{artwork_id}",
            get(get_artwork),
        )
        .route("/api/v1/import-batches", post(create_batch))
        .route(
            "/api/v1/import-batches/{batch_id}",
            get(get_batch).delete(cancel_batch),
        )
        .route(
            "/api/v1/import-batches/{batch_id}/heartbeat",
            post(heartbeat_batch),
        )
        .route(
            "/api/v1/import-batches/{batch_id}/confirm",
            post(confirm_batch).layer(middleware::from_fn(|request, next| {
                observability::timed("import.confirm_batch.duration", request, next)
            })),
        )
        .route("/api/v1/imports", post(create_job))
        .route(
            "/api/v1/imports/{import_id}",
            get(get_job).delete(cancel_job),
        )
        .route(
            "/api/v1/imports/{import_id}/file",
            put(upload_file).layer(middleware::from_fn(|request, next| {
                observability::timed_upload(
                    "import.upload.duration",
                    "import.upload.bytes",
                    request,
                    next,
                )
            })),
        )
        .route(
            "/api/v1/imports/{import_id}/confirm",
            post(confirm).layer(middleware::from_fn(|request, next| {
                observability::timed("import.confirm.duration", request, next)
            })),
        )
        .route(
            "/api/v1/imports/{import_id}/replacement",
            post(confirm_replacement),
        )
        .route(
            "/api/v1/library/albums/{album_id}/deletion",
            get(preview_album_deletion),
        )
        .route(
            "/api/v1/library/albums/{album_id}",
            axum::routing::delete(delete_album),
        )
        .route(
            "/api/v1/library/tracks/{track_id}/deletion",
            get(preview_track_deletion),
        )
        .route(
            "/api/v1/library/tracks/{track_id}",
            axum::routing::delete(delete_track),
        )
        .route(
            "/api/v1/library/tracks/{track_id}/replacement",
            post(create_track_replacement),
        )
}

const PERMANENT_DELETE_CONFIRMATION_HEADER: &str = "x-permanent-delete";
const TRACK_REPLACEMENT_CONFIRMATION_HEADER: &str = "x-track-replacement";
const MAX_TRACK_DELETION_BODY_BYTES: usize = 1024;
const MAX_TRACK_REPLACEMENT_BODY_BYTES: usize = 4 * 1024;

#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
struct DeletionConfirmation {
    #[serde(deserialize_with = "crate::gojson::nullable")]
    confirmation_token: String,
}

fn header_is_one(headers: &HeaderMap, name: &str) -> bool {
    headers.get(name).is_some_and(|value| value == "1")
}

/// The explicit-confirmation header and the preview token shared by Track
/// and Album deletion; `Err` is the error to send.
async fn read_deletion_confirmation(
    headers: &HeaderMap,
    body: Body,
    subject: &str,
) -> Result<String, ApiError> {
    if !header_is_one(headers, PERMANENT_DELETE_CONFIRMATION_HEADER) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "permanent_deletion_forbidden",
            "Permanent Track Deletion requires explicit confirmation",
        ));
    }
    let confirmation = read_limited(body, MAX_TRACK_DELETION_BODY_BYTES)
        .await
        .and_then(|body| decode::<DeletionConfirmation>(&body).ok().flatten());
    match confirmation {
        Some(confirmation) if !confirmation.confirmation_token.is_empty() => {
            Ok(confirmation.confirmation_token)
        }
        _ => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_deletion_confirmation",
            format!("{subject} requires the preview confirmation token"),
        )),
    }
}

async fn preview_album_deletion(
    State(state): State<AppState>,
    Path(album_id): Path<String>,
) -> Response {
    respond(
        service(&state).preview_album_deletion(&album_id).await,
        StatusCode::OK,
        "/api/v1/library/albums",
    )
}

async fn delete_album(
    State(state): State<AppState>,
    Path(album_id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let token = match read_deletion_confirmation(&headers, body, "Album deletion").await {
        Ok(token) => token,
        Err(error) => return error.into_response(),
    };
    let service = service(&state);
    let result =
        detached(move |_| async move { service.delete_album(&album_id, &token).await }).await;
    respond(result, StatusCode::OK, "/api/v1/library/albums")
}

async fn preview_track_deletion(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
) -> Response {
    respond(
        service(&state).preview_track_deletion(&track_id).await,
        StatusCode::OK,
        "/api/v1/library/tracks",
    )
}

async fn delete_track(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let token = match read_deletion_confirmation(&headers, body, "Permanent Track Deletion").await {
        Ok(token) => token,
        Err(error) => return error.into_response(),
    };
    let service = service(&state);
    let result =
        detached(move |_| async move { service.delete_track(&track_id, &token).await }).await;
    respond(result, StatusCode::OK, "/api/v1/library/tracks")
}

async fn create_track_replacement(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
) -> Response {
    respond(
        service(&state).create_replacement_job(&track_id).await,
        StatusCode::CREATED,
        "/api/v1/library/tracks",
    )
}

async fn confirm_replacement(
    State(state): State<AppState>,
    Path(import_id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    if !header_is_one(&headers, TRACK_REPLACEMENT_CONFIRMATION_HEADER) {
        return ApiError::new(
            StatusCode::FORBIDDEN,
            "track_replacement_forbidden",
            "Track Replacement requires explicit confirmation",
        )
        .into_response();
    }
    let confirmation = read_limited(body, MAX_TRACK_REPLACEMENT_BODY_BYTES)
        .await
        .and_then(|body| {
            decode::<super::replacement::TrackReplacementConfirmation>(&body)
                .ok()
                .flatten()
        });
    let confirmation = match confirmation {
        Some(confirmation)
            if confirmation.revision >= 1 && !confirmation.confirmation_token.is_empty() =>
        {
            confirmation
        }
        _ => {
            return ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_replacement_confirmation",
                "Track Replacement requires a positive revision and the preview confirmation token",
            )
            .into_response();
        }
    };
    let service = service(&state);
    let result = detached(move |cancel| async move {
        service
            .confirm_replacement(&cancel, &import_id, &confirmation)
            .await
    })
    .await;
    respond(result, StatusCode::OK, "/api/v1/imports")
}

fn service(state: &AppState) -> Arc<super::service::Service> {
    state.managed_import.service.clone()
}

#[derive(Serialize)]
struct ValidationBody {
    #[serde(flatten)]
    error: ErrorBody,
    issues: Vec<Issue>,
}

fn strict_validation_message(code: &str, field: &str, reason: &str) -> String {
    if code == "missing_artwork" {
        return "Choose valid Album artwork or continue without a cover".to_owned();
    }
    if field.is_empty() {
        format!("File failed the Strict Import Profile: {reason}")
    } else {
        format!("File failed the Strict Import Profile at {field}: {reason}")
    }
}

/// `handleError`: the first matching sentinel in Go's order, then strict
/// validation failures, else 500.
pub fn error_response(error: &Error, path: &str) -> Response {
    const MAPPINGS: &[(Sentinel, StatusCode, &str, &str)] = &[
        (
            Sentinel::TrackNotFound,
            StatusCode::NOT_FOUND,
            "track_not_found",
            "Track not found",
        ),
        (
            Sentinel::AlbumNotFound,
            StatusCode::NOT_FOUND,
            "album_not_found",
            "Album not found",
        ),
        (
            Sentinel::NotManagedTrack,
            StatusCode::CONFLICT,
            "not_managed_track",
            "Only Managed Tracks can be permanently deleted or replaced",
        ),
        (
            Sentinel::DeletionConflict,
            StatusCode::CONFLICT,
            "deletion_preview_changed",
            "Track or its references changed; review Permanent Track Deletion again",
        ),
        (
            Sentinel::ReplacementConflict,
            StatusCode::CONFLICT,
            "replacement_preview_changed",
            "Track or its references changed; review Track Replacement again",
        ),
        (
            Sentinel::NotReplacementJob,
            StatusCode::CONFLICT,
            "not_track_replacement",
            "Managed Import Job does not replace a Track",
        ),
        (
            Sentinel::ReplacementRequired,
            StatusCode::CONFLICT,
            "track_replacement_required",
            "This Managed Import Job replaces a Track and must be confirmed as a Track Replacement",
        ),
        (
            Sentinel::NotFound,
            StatusCode::NOT_FOUND,
            "import_not_found",
            "This import no longer exists on the Music Server",
        ),
        (
            Sentinel::RevisionConflict,
            StatusCode::CONFLICT,
            ERROR_CODE_REVISION_CONFLICT,
            "Import Preview changed since the supplied revision",
        ),
        (
            Sentinel::ExactDuplicate,
            StatusCode::CONFLICT,
            ERROR_CODE_EXACT_DUPLICATE,
            "File bytes match an existing Track",
        ),
        (
            Sentinel::BatchTooLarge,
            StatusCode::PAYLOAD_TOO_LARGE,
            "batch_upload_too_large",
            "Managed Import batch exceeds the configured byte limit",
        ),
        (
            Sentinel::UploadInterrupted,
            StatusCode::REQUEST_TIMEOUT,
            UPLOAD_INTERRUPTED_ERROR_CODE,
            "Managed Import upload was interrupted; retry this file",
        ),
        (
            Sentinel::InvalidState,
            StatusCode::CONFLICT,
            "import_state_conflict",
            "Managed Import Job is not awaiting this operation",
        ),
        (
            Sentinel::UploadTooLarge,
            StatusCode::PAYLOAD_TOO_LARGE,
            "upload_too_large",
            "Managed Import file exceeds the configured per-file byte limit",
        ),
        (
            Sentinel::InsufficientStorage,
            StatusCode::INSUFFICIENT_STORAGE,
            "insufficient_storage",
            "Managed Storage does not have enough capacity for this import and its safety reserve",
        ),
        (
            Sentinel::UnsafeStoragePath,
            StatusCode::CONFLICT,
            "unsafe_storage_path",
            "Managed Storage path failed containment checks",
        ),
    ];
    for (sentinel, status, code, message) in MAPPINGS {
        if error.is(*sentinel) {
            return ApiError::new(*status, code, *message).into_response();
        }
    }
    if error.is(Sentinel::InvalidUpload) {
        return ApiError::new(StatusCode::BAD_REQUEST, "invalid_upload", error.message())
            .into_response();
    }
    if let Some(validation) = error.as_validation() {
        let reason = validation.strict_reason().to_owned();
        let body = ValidationBody {
            error: ErrorBody {
                error: validation.code.clone(),
                code: validation.code.clone(),
                message: strict_validation_message(&validation.code, &validation.field, &reason),
                field: validation.field.clone(),
                reason,
            },
            issues: validation.validation_issues(),
        };
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(body)).into_response();
    }
    tracing::error!(path, error = %error, "Managed Import request failed");
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "Managed Import request failed",
    )
    .into_response()
}

fn respond<T: Serialize>(result: ImportResult<T>, status: StatusCode, path: &str) -> Response {
    match result {
        Ok(value) => (status, Json(value)).into_response(),
        Err(error) => error_response(&error, path),
    }
}

fn no_content(result: ImportResult<()>, path: &str) -> Response {
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => error_response(&error, path),
    }
}

/// Reads a body up to `limit` bytes (`http.MaxBytesReader`); `None` when it
/// is longer or the read fails.
async fn read_limited(body: Body, limit: usize) -> Option<Vec<u8>> {
    let mut stream = body.into_data_stream();
    let mut data = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.ok()?;
        if data.len() + chunk.len() > limit {
            return None;
        }
        data.extend_from_slice(&chunk);
    }
    Some(data)
}

/// `json.NewDecoder(...).Decode` with `DisallowUnknownFields`: the first
/// JSON value counts, `null` is the default; `Ok(None)` for an empty body.
fn decode<T: DeserializeOwned + Default>(body: &[u8]) -> Result<Option<T>, ()> {
    let mut values = serde_json::Deserializer::from_slice(body).into_iter::<serde_json::Value>();
    match values.next() {
        None => Ok(None),
        Some(Err(error)) if error.is_eof() && body.iter().all(u8::is_ascii_whitespace) => Ok(None),
        Some(Err(_)) => Err(()),
        Some(Ok(serde_json::Value::Null)) => Ok(Some(T::default())),
        Some(Ok(value)) => serde_json::from_value(value).map(Some).map_err(|_| ()),
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
struct JobCreate {
    #[serde(deserialize_with = "crate::gojson::nullable")]
    batch_id: String,
    #[serde(deserialize_with = "crate::gojson::nullable")]
    client_file_id: String,
}

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchCreate {}

async fn create_job(State(state): State<AppState>, body: Body) -> Response {
    let creation = match read_limited(body, MAX_JOB_CREATE_BODY_BYTES)
        .await
        .map(|body| decode::<JobCreate>(&body))
    {
        Some(Ok(creation)) => creation.unwrap_or_default(),
        _ => {
            return ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_import_job",
                "Managed Import Job request is invalid",
            )
            .into_response();
        }
    };
    let result = service(&state)
        .create_job(&creation.batch_id, &creation.client_file_id)
        .await;
    respond(result, StatusCode::CREATED, "/api/v1/imports")
}

async fn create_batch(State(state): State<AppState>, body: Body) -> Response {
    let valid = read_limited(body, MAX_JOB_CREATE_BODY_BYTES)
        .await
        .map(|body| decode::<BatchCreate>(&body));
    if !matches!(valid, Some(Ok(_))) {
        return ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_import_batch",
            "Managed Import Batch request is invalid",
        )
        .into_response();
    }
    respond(
        service(&state).create_batch().await,
        StatusCode::CREATED,
        "/api/v1/import-batches",
    )
}

async fn get_batch(State(state): State<AppState>, Path(batch_id): Path<String>) -> Response {
    respond(
        service(&state).get_batch(&batch_id).await,
        StatusCode::OK,
        "/api/v1/import-batches",
    )
}

async fn list_history(State(state): State<AppState>) -> Response {
    respond(
        service(&state).list_history().await,
        StatusCode::OK,
        "/api/v1/import-history",
    )
}

async fn cancel_batch(State(state): State<AppState>, Path(batch_id): Path<String>) -> Response {
    no_content(
        service(&state).cancel_batch(&batch_id).await,
        "/api/v1/import-batches",
    )
}

async fn heartbeat_batch(State(state): State<AppState>, Path(batch_id): Path<String>) -> Response {
    no_content(
        service(&state).heartbeat_batch(&batch_id).await,
        "/api/v1/import-batches",
    )
}

/// Runs a request's work as its own task, cancelled (not aborted) when the
/// request goes away, so cleanup that Go runs "without cancel" still runs.
async fn detached<F, Fut, T>(work: F) -> T
where
    F: FnOnce(CancellationToken) -> Fut,
    Fut: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let token = CancellationToken::new();
    let _cancel_on_drop = token.clone().drop_guard();
    // The request's own hub, so its spans survive the move to a new task.
    let task = tokio::spawn(work(token).bind_hub(sentry::Hub::current()));
    task.await.expect("Managed Import task panicked")
}

async fn confirm_batch(
    State(state): State<AppState>,
    Path(batch_id): Path<String>,
    body: Body,
) -> Response {
    let confirmation = read_limited(body, MAX_BATCH_CONFIRMATION_BODY_BYTES)
        .await
        .and_then(|body| decode::<BatchConfirmation>(&body).ok().flatten());
    let confirmation = match confirmation {
        Some(confirmation)
            if confirmation.revision >= 1 && confirmation.selected_file_ids.is_some() =>
        {
            confirmation
        }
        _ => {
            return ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_batch_confirmation",
                "Managed Import Batch confirmation requires a positive revision and selected file IDs",
            )
            .into_response();
        }
    };
    let service = service(&state);
    // Unlike `detached`, this outlives the connection: once the Owner confirms,
    // losing the answer must not leave the Batch half imported.
    let result = tokio::spawn(
        async move {
            service
                .confirm_batch(&CancellationToken::new(), &batch_id, &confirmation)
                .await
        }
        .bind_hub(sentry::Hub::current()),
    )
    .await
    .expect("Managed Import task panicked");
    respond(result, StatusCode::OK, "/api/v1/import-batches")
}

async fn get_job(State(state): State<AppState>, Path(import_id): Path<String>) -> Response {
    respond(
        service(&state).get_job(&import_id).await,
        StatusCode::OK,
        "/api/v1/imports",
    )
}

async fn cancel_job(State(state): State<AppState>, Path(import_id): Path<String>) -> Response {
    no_content(
        service(&state).cancel_job(&import_id).await,
        "/api/v1/imports",
    )
}

/// Go's `url.QueryUnescape`: `+` is a space and every `%` needs two hex
/// digits.
fn query_unescape(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hex = bytes.get(index + 1..index + 3)?;
                let hex = std::str::from_utf8(hex).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                index += 3;
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

fn import_filename(headers: &HeaderMap) -> Option<String> {
    let raw = headers
        .get(IMPORT_FILENAME_HEADER)
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
        .unwrap_or_default();
    if headers
        .get(IMPORT_FILENAME_ENCODING_HEADER)
        .map(HeaderValue::as_bytes)
        != Some(b"url")
    {
        return Some(raw);
    }
    query_unescape(&raw)
}

async fn upload_file(
    State(state): State<AppState>,
    Path(import_id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let Some(filename) = import_filename(&headers) else {
        return ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_import_filename",
            "Managed Import filename encoding is invalid",
        )
        .into_response();
    };
    let content_length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(-1);
    // Each read must arrive within the inactivity timeout; a read error is
    // the client going away.
    let stream = body.into_data_stream();
    let stream = futures_util::stream::unfold(stream, |mut stream| async move {
        match tokio::time::timeout(UPLOAD_INACTIVITY_TIMEOUT, stream.next()).await {
            Ok(None) => None,
            Ok(Some(Ok(chunk))) => Some((Ok::<Bytes, Error>(chunk), stream)),
            Ok(Some(Err(error))) => Some((
                Err(Error::wrap(
                    Sentinel::UploadInterrupted,
                    format!("unexpected EOF: {error}"),
                )),
                stream,
            )),
            Err(_) => Some((
                Err(Error::wrap(Sentinel::UploadInterrupted, "i/o timeout")),
                stream,
            )),
        }
    });
    let service = service(&state);
    let result = detached(move |cancel| async move {
        service
            .upload(
                &cancel,
                &import_id,
                &filename,
                Box::pin(stream),
                content_length,
            )
            .await
    })
    .await;
    respond(result, StatusCode::OK, "/api/v1/imports")
}

async fn confirm(
    State(state): State<AppState>,
    Path(import_id): Path<String>,
    body: Body,
) -> Response {
    let confirmation = read_limited(body, MAX_CONFIRMATION_BODY_BYTES)
        .await
        .and_then(|body| decode::<Confirmation>(&body).ok().flatten());
    let confirmation = match confirmation {
        Some(confirmation) if confirmation.revision >= 1 => confirmation,
        _ => {
            return ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_confirmation",
                "Managed Import confirmation requires a positive revision",
            )
            .into_response();
        }
    };
    let service = service(&state);
    let result = detached(move |cancel| async move {
        service
            .confirm_with_decision(
                &cancel,
                &import_id,
                confirmation.revision,
                &confirmation.duplicate_decision,
            )
            .await
    })
    .await;
    respond(result, StatusCode::OK, "/api/v1/imports")
}

async fn upload_artwork(
    State(state): State<AppState>,
    Path((batch_id, album_key)): Path<(String, String)>,
    body: Body,
) -> Response {
    let Some(data) = read_limited(body, MAX_UPLOADED_ARTWORK_BYTES).await else {
        return ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_artwork",
            "Artwork must be JPEG or PNG and no larger than 20 MiB",
        )
        .into_response();
    };
    let result = service(&state)
        .upload_artwork(&batch_id, &album_key, &data)
        .await;
    respond(result, StatusCode::OK, "/api/v1/import-batches")
}

async fn get_artwork(
    State(state): State<AppState>,
    Path((batch_id, artwork_id)): Path<(String, String)>,
) -> Response {
    match service(&state).get_artwork(&batch_id, &artwork_id).await {
        Ok(artwork) => (
            [
                (header::CONTENT_TYPE, artwork.mime_type),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
                (header::CACHE_CONTROL, "no-store".to_owned()),
            ],
            artwork.data,
        )
            .into_response(),
        Err(error) => error_response(&error, "/api/v1/import-batches"),
    }
}

impl ManagedImport {
    /// Restart recovery, then the inactivity cleanup every minute.
    pub async fn start(self: &Arc<Self>) -> ImportResult<()> {
        self.service.recover_pending_track_deletions().await?;
        self.service.recover_pending_track_replacements().await?;
        self.service.cleanup_restart().await?;
        let module = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(super::service::IMPORT_CLEANUP_INTERVAL);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if let Err(error) = module
                    .service
                    .cleanup_inactive(std::time::SystemTime::now())
                    .await
                {
                    tracing::error!(%error, "inactive Managed Import cleanup failed");
                }
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescapes_filenames_like_go() {
        assert_eq!(
            query_unescape("a%20b+c.flac").as_deref(),
            Some("a b c.flac")
        );
        assert_eq!(
            query_unescape("%E2%9C%93.flac").as_deref(),
            Some("\u{2713}.flac")
        );
        assert_eq!(query_unescape("bad%2"), None);
        assert_eq!(query_unescape("bad%zz"), None);
    }

    #[test]
    fn decodes_bodies_like_go() {
        assert!(matches!(decode::<JobCreate>(b""), Ok(None)));
        assert!(matches!(decode::<JobCreate>(b"  \n"), Ok(None)));
        assert!(matches!(decode::<JobCreate>(b"null"), Ok(Some(_))));
        assert!(decode::<JobCreate>(br#"{"other":1}"#).is_err());
        assert!(decode::<JobCreate>(b"{").is_err());
        let job = decode::<JobCreate>(br#"{"batchId":"b"} trailing"#)
            .unwrap()
            .unwrap();
        assert_eq!(job.batch_id, "b");
    }
}
