//! Outbound GETs shared by the Deezer and Wikipedia lookups. Errors are text:
//! the caller only records that the lookup failed.

use crate::observability;
use serde::de::DeserializeOwned;
use std::time::Instant;

/// A failed GET; the HTTP status, when there was one, lets a caller retry.
#[derive(Debug)]
pub struct JsonError {
    pub status: Option<u16>,
    message: String,
}

impl From<String> for JsonError {
    fn from(message: String) -> Self {
        Self {
            status: None,
            message,
        }
    }
}

impl From<JsonError> for String {
    fn from(error: JsonError) -> Self {
        error.message
    }
}

pub async fn json<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    query: &[(&str, &str)],
) -> Result<T, JsonError> {
    let started = Instant::now();
    let result = get_json(client, url, query).await;
    observability::outbound_request(url, started, result.is_ok());
    result
}

async fn get_json<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    query: &[(&str, &str)],
) -> Result<T, JsonError> {
    let url = if query.is_empty() {
        url::Url::parse(url)
    } else {
        url::Url::parse_with_params(url, query)
    }
    .map_err(|error| format!("GET {url}: {error}"))?;
    let response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|error| format!("GET {url}: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(JsonError {
            status: Some(status.as_u16()),
            message: format!("GET {url}: HTTP {status}"),
        });
    }
    let body = response
        .bytes()
        .await
        .map_err(|error| format!("GET {url}: {error}"))?;
    Ok(serde_json::from_slice(&body).map_err(|error| format!("GET {url}: {error}"))?)
}

/// The body and its content type, for an image.
pub async fn bytes(client: &reqwest::Client, url: &str) -> Result<(String, Vec<u8>), String> {
    let started = Instant::now();
    let result = get_bytes(client, url).await;
    observability::outbound_request(url, started, result.is_ok());
    result
}

async fn get_bytes(client: &reqwest::Client, url: &str) -> Result<(String, Vec<u8>), String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("GET {url}: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("GET {url}: HTTP {status}"));
    }
    let media_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    if !media_type.starts_with("image/") {
        return Err(format!("GET {url}: not an image ({media_type:?})"));
    }
    let data = response
        .bytes()
        .await
        .map_err(|error| format!("GET {url}: {error}"))?;
    Ok((media_type, data.to_vec()))
}
