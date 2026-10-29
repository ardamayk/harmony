//! HTTP for Soulseek Acquisition: parse the request, call the service,
//! respond. Every route answers 404 `soulseek_acquisition_disabled` while
//! slskd is not configured.

use super::acquisitions::{Acquisition, AcquisitionState, FileKind};
use super::service::{Sharing, SoulseekError, SoulseekSearch, Status};
use super::versions::{Quality, Scope, Source, Version};
use crate::app::AppState;
use crate::gojson;
use crate::respond::{ApiError, ApiResult};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/soulseek/status", get(status))
        .route("/api/v1/soulseek/searches", post(start_search))
        .route("/api/v1/soulseek/searches/{search_id}", get(search))
        .route(
            "/api/v1/soulseek/acquisitions",
            get(list_acquisitions).post(start_acquisition),
        )
        .route(
            "/api/v1/soulseek/acquisitions/{acquisition_id}",
            get(get_acquisition).delete(cancel_acquisition),
        )
}

impl From<SoulseekError> for ApiError {
    fn from(error: SoulseekError) -> Self {
        let (status, code) = match &error {
            SoulseekError::Disabled => (StatusCode::NOT_FOUND, "soulseek_acquisition_disabled"),
            SoulseekError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            SoulseekError::AcquisitionNotFound => (StatusCode::NOT_FOUND, "acquisition_not_found"),
            SoulseekError::Conflict(_) => (StatusCode::CONFLICT, "acquisition_state_conflict"),
            SoulseekError::Internal(cause) => return ApiError::internal(cause),
            SoulseekError::SlskdUnreachable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, "slskd_unreachable")
            }
            SoulseekError::NotLoggedIn => {
                (StatusCode::SERVICE_UNAVAILABLE, "soulseek_not_logged_in")
            }
            SoulseekError::SearchNotFound => (StatusCode::NOT_FOUND, "search_not_found"),
            SoulseekError::VersionNotFound => (StatusCode::NOT_FOUND, "version_not_found"),
        };
        ApiError::new(status, code, error.to_string())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    slskd: SlskdStatus,
    soulseek: SoulseekLogin,
    sharing: SharingStatus,
}

#[derive(Serialize)]
struct SlskdStatus {
    reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SoulseekLogin {
    connected: bool,
    logged_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SharingStatus {
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared_file_count: Option<u64>,
}

async fn status(State(state): State<AppState>) -> ApiResult<Json<StatusResponse>> {
    let Status { slskd, sharing } = state.soulseek.status().await?;
    let (state, shared_file_count) = match sharing {
        Sharing::Shared { file_count } => ("shared", file_count),
        Sharing::NotShared => ("not_shared", None),
        Sharing::Unknown => ("unknown", None),
    };
    let reachable = slskd.is_some();
    let (version, soulseek) = match slskd {
        Some(application) => (
            application.version,
            SoulseekLogin {
                connected: application.connected,
                logged_in: application.logged_in,
                username: application.username.filter(|_| application.logged_in),
            },
        ),
        None => (
            None,
            SoulseekLogin {
                connected: false,
                logged_in: false,
                username: None,
            },
        ),
    };
    Ok(Json(StatusResponse {
        slskd: SlskdStatus { reachable, version },
        soulseek,
        sharing: SharingStatus {
            state,
            shared_file_count,
        },
    }))
}

#[derive(Default, Deserialize)]
struct SearchCreate {
    #[serde(default, deserialize_with = "gojson::nullable")]
    query: String,
    #[serde(default, deserialize_with = "gojson::nullable")]
    scope: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchStarted {
    search_id: String,
}

async fn start_search(
    State(state): State<AppState>,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<SearchStarted>)> {
    state.soulseek.ensure_enabled()?;
    let input: SearchCreate = gojson::decode_body(&body, "invalid JSON body")?;
    let scope = Scope::parse(&input.scope)
        .ok_or_else(|| SoulseekError::BadRequest("scope must be album or track".to_owned()))?;
    let search_id = state.soulseek.start_search(&input.query, scope).await?;
    Ok((
        StatusCode::CREATED,
        Json(SearchStarted {
            search_id: search_id.to_string(),
        }),
    ))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResponse {
    search_id: String,
    query: String,
    scope: &'static str,
    state: &'static str,
    versions: Vec<VersionResponse>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VersionResponse {
    version_key: String,
    quality: QualityResponse,
    file_count: usize,
    total_bytes: u64,
    source_count: usize,
    complete_source_count: usize,
    sources: Vec<SourceResponse>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QualityResponse {
    format: String,
    lossless: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    bit_depth: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample_rate_hz: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bitrate_kbps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    variable_bitrate: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceResponse {
    source_id: String,
    username: String,
    folder: String,
    file_count: usize,
    total_bytes: u64,
    complete: bool,
    has_free_upload_slot: bool,
    queue_length: u64,
    upload_speed_bytes_per_second: u64,
}

async fn search(
    State(state): State<AppState>,
    Path(search_id): Path<String>,
) -> ApiResult<Json<SearchResponse>> {
    let SoulseekSearch {
        id,
        query,
        scope,
        complete,
        versions,
    } = state.soulseek.search(&search_id).await?;
    Ok(Json(SearchResponse {
        search_id: id.to_string(),
        query,
        scope: scope.name(),
        state: if complete { "complete" } else { "searching" },
        versions: versions.into_iter().map(version_response).collect(),
    }))
}

fn quality_response(quality: Quality) -> QualityResponse {
    let Quality {
        format,
        lossless,
        bit_depth,
        sample_rate_hz,
        bitrate_kbps,
        variable_bitrate,
    } = quality;
    QualityResponse {
        format,
        lossless,
        bit_depth,
        sample_rate_hz,
        bitrate_kbps,
        variable_bitrate,
    }
}

fn version_response(version: Version) -> VersionResponse {
    VersionResponse {
        version_key: version.key,
        quality: quality_response(version.quality),
        file_count: version.file_count,
        total_bytes: version.total_bytes,
        source_count: version.sources.len(),
        complete_source_count: version.complete_source_count,
        sources: version.sources.into_iter().map(source_response).collect(),
    }
}

fn source_response(source: Source) -> SourceResponse {
    SourceResponse {
        source_id: source.id,
        username: source.username,
        folder: source.folder,
        file_count: source.files.len(),
        total_bytes: source.total_bytes,
        complete: source.complete,
        has_free_upload_slot: source.has_free_upload_slot,
        queue_length: source.queue_length,
        upload_speed_bytes_per_second: source.upload_speed,
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcquisitionCreate {
    #[serde(default, deserialize_with = "gojson::nullable")]
    search_id: String,
    #[serde(default, deserialize_with = "gojson::nullable")]
    version_key: String,
    #[serde(default, deserialize_with = "gojson::nullable")]
    source_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AcquisitionResponse {
    acquisition_id: String,
    query: String,
    scope: &'static str,
    version_key: String,
    quality: QualityResponse,
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    batch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_source: Option<SourceReference>,
    sources_tried: usize,
    source_count: usize,
    files: Vec<AcquisitionFileResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_reason: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceReference {
    source_id: String,
    username: String,
    folder: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AcquisitionFileResponse {
    filename: String,
    state: &'static str,
    bytes_transferred: u64,
    total_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    job_id: Option<String>,
}

#[derive(Serialize)]
struct AcquisitionList {
    items: Vec<AcquisitionResponse>,
}

fn acquisition_response(acquisition: Acquisition) -> AcquisitionResponse {
    let current_source = acquisition
        .state
        .eq(&AcquisitionState::Downloading)
        .then(|| acquisition.current_source())
        .flatten()
        .map(|source| SourceReference {
            source_id: source.id.clone(),
            username: source.username.clone(),
            folder: source.folder.clone(),
        });
    let sources_tried = acquisition.sources_tried();
    // The batch shows once Managed Import has taken a file into it.
    let importing = acquisition.files.iter().any(|file| file.job_id.is_some());
    AcquisitionResponse {
        acquisition_id: acquisition.id,
        query: acquisition.query,
        scope: acquisition.scope.name(),
        version_key: acquisition.version_key,
        quality: quality_response(acquisition.quality),
        state: acquisition.state.name(),
        batch_id: acquisition.batch_id.filter(|_| importing),
        current_source,
        sources_tried,
        source_count: acquisition.sources.len(),
        files: acquisition
            .files
            .into_iter()
            .filter(|file| file.kind == FileKind::Audio)
            .map(|file| AcquisitionFileResponse {
                filename: file.name,
                state: file.state.name(),
                bytes_transferred: file.bytes_transferred.min(file.size),
                total_bytes: file.size,
                job_id: file.job_id,
            })
            .collect(),
        failure_reason: acquisition.failure_reason,
        created_at: gojson::time(&acquisition.created_at),
        updated_at: gojson::time(&acquisition.updated_at),
    }
}

async fn start_acquisition(
    State(state): State<AppState>,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<AcquisitionResponse>)> {
    state.soulseek.ensure_enabled()?;
    let input: AcquisitionCreate = gojson::decode_body(&body, "invalid JSON body")?;
    if input.search_id.is_empty() || input.version_key.is_empty() {
        return Err(
            SoulseekError::BadRequest("searchId and versionKey are required".to_owned()).into(),
        );
    }
    let source_id = input.source_id.filter(|id| !id.is_empty());
    let (acquisition, created) = state
        .soulseek
        .start_acquisition(&input.search_id, &input.version_key, source_id.as_deref())
        .await?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(acquisition_response(acquisition))))
}

async fn list_acquisitions(State(state): State<AppState>) -> ApiResult<Json<AcquisitionList>> {
    let acquisitions = state.soulseek.list_acquisitions().await?;
    Ok(Json(AcquisitionList {
        items: acquisitions.into_iter().map(acquisition_response).collect(),
    }))
}

async fn get_acquisition(
    State(state): State<AppState>,
    Path(acquisition_id): Path<String>,
) -> ApiResult<Json<AcquisitionResponse>> {
    let acquisition = state.soulseek.get_acquisition(&acquisition_id).await?;
    Ok(Json(acquisition_response(acquisition)))
}

async fn cancel_acquisition(
    State(state): State<AppState>,
    Path(acquisition_id): Path<String>,
) -> ApiResult<StatusCode> {
    state.soulseek.cancel_acquisition(&acquisition_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
