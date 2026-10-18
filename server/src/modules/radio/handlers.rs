//! HTTP for radio: parse the request, call the service, respond.

use super::browser::BrowserError;
use super::repository::RadioError;
use super::stream::StreamRequest;
use super::types;
use super::types::{NowPlaying, SearchResult, Station, StationCreate, StationPatch};
use super::{RADIO_BROWSER_SOURCE, Radio};
use crate::app::AppState;
use crate::auth;
use crate::gojson;
use crate::query::GoQuery;
use crate::respond::{ApiError, ApiResult};
use axum::body::Bytes;
use axum::extract::{OriginalUri, Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/radio/stations",
            get(list_stations).post(create_station),
        )
        .route(
            "/api/v1/radio/stations/{station_id}",
            get(get_station).patch(patch_station).delete(delete_station),
        )
        .route("/api/v1/radio/search", get(search_stations))
        .route("/api/v1/radio/catalog/countries", get(list_countries))
        .route("/api/v1/radio/catalog/tags", get(list_tags))
        .route("/api/v1/radio/import", post(import_station))
        .route("/api/v1/radio/preview", post(preview_station_stream))
        .route(
            "/api/v1/radio/preview/{station_uuid}/stream",
            get(stream_preview),
        )
        .route(
            "/api/v1/radio/stations/{station_id}/stream",
            get(stream_station),
        )
        .route(
            "/api/v1/radio/stations/{station_id}/now-playing",
            get(get_now_playing),
        )
}

fn not_found_or(status: StatusCode, code: &'static str) -> impl FnOnce(RadioError) -> ApiError {
    move |error| match error {
        RadioError::NotFound => ApiError::not_found("radio station not found"),
        other => ApiError::new(status, code, other.to_string()),
    }
}

fn bad_gateway(error: BrowserError) -> ApiError {
    ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", error.to_string())
}

async fn list_stations(State(state): State<AppState>) -> ApiResult<Json<types::List<Station>>> {
    let list = state
        .radio
        .service
        .list(auth::current_user().id)
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                error.to_string(),
            )
        })?;
    Ok(Json(list))
}

async fn create_station(
    State(state): State<AppState>,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<Station>)> {
    let input: StationCreate = gojson::decode_body(&body, "invalid JSON body")?;
    let station = state
        .radio
        .service
        .create(auth::current_user().id, input)
        .await
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok((StatusCode::CREATED, Json(station)))
}

async fn get_station(
    State(state): State<AppState>,
    Path(station_id): Path<String>,
) -> ApiResult<Json<Station>> {
    let station = state
        .radio
        .service
        .get(auth::current_user().id, &station_id)
        .await
        .map_err(not_found_or(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
        ))?;
    Ok(Json(station))
}

async fn patch_station(
    State(state): State<AppState>,
    Path(station_id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<Station>> {
    let patch: StationPatch = gojson::decode_body(&body, "invalid JSON body")?;
    let station = state
        .radio
        .service
        .update(auth::current_user().id, &station_id, patch)
        .await
        .map_err(not_found_or(StatusCode::BAD_REQUEST, "bad_request"))?;
    Ok(Json(station))
}

async fn delete_station(
    State(state): State<AppState>,
    Path(station_id): Path<String>,
) -> ApiResult<StatusCode> {
    state
        .radio
        .service
        .delete(auth::current_user().id, &station_id)
        .await
        .map_err(not_found_or(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
        ))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn search_stations(
    State(state): State<AppState>,
    query: GoQuery,
) -> ApiResult<Json<types::List<SearchResult>>> {
    Ok(Json(
        state
            .radio
            .browser
            .search(&query)
            .await
            .map_err(bad_gateway)?,
    ))
}

async fn list_countries(
    State(state): State<AppState>,
) -> ApiResult<Json<types::List<types::CatalogOption>>> {
    Ok(Json(
        state.radio.browser.countries().await.map_err(bad_gateway)?,
    ))
}

async fn list_tags(
    State(state): State<AppState>,
) -> ApiResult<Json<types::List<types::CatalogOption>>> {
    Ok(Json(state.radio.browser.tags().await.map_err(bad_gateway)?))
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ImportRequest {
    #[serde(rename = "stationUuid", deserialize_with = "gojson::nullable")]
    station_uuid: String,
    result: Option<SearchResult>,
}

/// The catalog's current record when it answers, else the result the
/// client sent along.
async fn import_result(radio: &Radio, body: ImportRequest) -> Result<SearchResult, String> {
    if !body.station_uuid.is_empty() {
        match radio.browser.lookup(&body.station_uuid).await {
            Ok(result) => return Ok(result),
            Err(error) if body.result.is_none() => return Err(error.to_string()),
            Err(_) => {}
        }
    }
    body.result
        .ok_or_else(|| "stationUuid or result is required".to_owned())
}

fn preview_station(result: SearchResult) -> Station {
    Station {
        id: format!("preview:{}", result.station_uuid),
        name: result.name,
        stream_url: result.stream_url,
        homepage_url: result.homepage_url,
        favicon_url: result.favicon_url,
        country: result.country,
        language: result.language,
        tags: result.tags.unwrap_or_default(),
        codec: result.codec,
        bitrate: result.bitrate,
        source: RADIO_BROWSER_SOURCE.to_owned(),
        external_id: result.station_uuid,
        ..Station::default()
    }
}

async fn import_station(
    State(state): State<AppState>,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<Station>)> {
    let body: ImportRequest = gojson::decode_body(&body, "invalid JSON body")?;
    let result = import_result(&state.radio, body)
        .await
        .map_err(ApiError::bad_request)?;
    let station = state
        .radio
        .service
        .import(auth::current_user().id, result)
        .await
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok((StatusCode::CREATED, Json(station)))
}

fn stream_request<'a>(
    uri: &'a OriginalUri,
    query: &'a GoQuery,
    headers: &HeaderMap,
) -> StreamRequest<'a> {
    StreamRequest {
        path: uri.path(),
        query,
        range: headers.get(header::RANGE).cloned(),
    }
}

async fn preview_station_stream(
    State(state): State<AppState>,
    uri: OriginalUri,
    query: GoQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let body: ImportRequest = match gojson::decode_body(&body, "invalid JSON body") {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    let result = match import_result(&state.radio, body).await {
        Ok(result) => result,
        Err(error) => return ApiError::bad_request(error).into_response(),
    };
    state
        .radio
        .proxy
        .stream(
            stream_request(&uri, &query, &headers),
            &preview_station(result),
        )
        .await
}

async fn stream_preview(
    State(state): State<AppState>,
    Path(station_uuid): Path<String>,
    uri: OriginalUri,
    query: GoQuery,
    headers: HeaderMap,
) -> Response {
    let result = match state.radio.browser.lookup(&station_uuid).await {
        Ok(result) => result,
        Err(error) => return ApiError::bad_request(error.to_string()).into_response(),
    };
    state
        .radio
        .proxy
        .stream(
            stream_request(&uri, &query, &headers),
            &preview_station(result),
        )
        .await
}

async fn stream_station(
    State(state): State<AppState>,
    Path(station_id): Path<String>,
    uri: OriginalUri,
    query: GoQuery,
    headers: HeaderMap,
) -> Response {
    let station = match state
        .radio
        .service
        .get(auth::current_user().id, &station_id)
        .await
    {
        Ok(station) => station,
        Err(error) => {
            return not_found_or(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")(error)
                .into_response();
        }
    };
    state
        .radio
        .proxy
        .stream(stream_request(&uri, &query, &headers), &station)
        .await
}

async fn get_now_playing(
    State(state): State<AppState>,
    Path(station_id): Path<String>,
) -> ApiResult<Json<NowPlaying>> {
    if station_id.starts_with("preview:") {
        return Ok(Json(state.radio.cache.get(&station_id).unwrap_or_default()));
    }
    let station = state
        .radio
        .service
        .get(auth::current_user().id, &station_id)
        .await
        .map_err(not_found_or(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
        ))?;
    if let Some(now) = state.radio.cache.get(&station_id) {
        return Ok(Json(now));
    }
    Ok(Json(station.last_now_playing.unwrap_or_default()))
}
