//! The Radio Browser catalog client (radio/search.go).

use super::types::{CatalogOption, List, SearchResult};
use crate::gojson;
use crate::observability;
use crate::query::GoQuery;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const USER_AGENT: &str = "navidrome-replacement/0.1";

#[derive(Debug, thiserror::Error)]
pub enum BrowserError {
    #[error("radio station not found")]
    NotFound,
    #[error("{0}")]
    Failed(String),
}

pub struct RadioBrowserClient {
    base_url: String,
    client: reqwest::Client,
}

#[derive(Deserialize)]
#[serde(default)]
struct RawStation {
    #[serde(deserialize_with = "gojson::nullable")]
    stationuuid: String,
    #[serde(deserialize_with = "gojson::nullable")]
    name: String,
    #[serde(deserialize_with = "gojson::nullable")]
    url: String,
    #[serde(deserialize_with = "gojson::nullable")]
    url_resolved: String,
    #[serde(deserialize_with = "gojson::nullable")]
    homepage: String,
    #[serde(deserialize_with = "gojson::nullable")]
    favicon: String,
    #[serde(deserialize_with = "gojson::nullable")]
    country: String,
    #[serde(deserialize_with = "gojson::nullable")]
    language: String,
    #[serde(deserialize_with = "gojson::nullable")]
    tags: String,
    #[serde(deserialize_with = "gojson::nullable")]
    codec: String,
    bitrate: Value,
    votes: Value,
    lastcheckok: Value,
    #[serde(deserialize_with = "gojson::nullable")]
    lastchecktime_iso8601: String,
    #[serde(deserialize_with = "gojson::nullable")]
    lastcheckoktime_iso8601: String,
}

impl Default for RawStation {
    fn default() -> Self {
        Self {
            stationuuid: String::new(),
            name: String::new(),
            url: String::new(),
            url_resolved: String::new(),
            homepage: String::new(),
            favicon: String::new(),
            country: String::new(),
            language: String::new(),
            tags: String::new(),
            codec: String::new(),
            bitrate: Value::Null,
            votes: Value::Null,
            lastcheckok: Value::Null,
            lastchecktime_iso8601: String::new(),
            lastcheckoktime_iso8601: String::new(),
        }
    }
}

impl RawStation {
    fn into_result(self) -> SearchResult {
        let stream_url = if self.url_resolved.is_empty() {
            self.url
        } else {
            self.url_resolved
        };
        SearchResult {
            station_uuid: self.stationuuid,
            name: self.name.trim().to_owned(),
            stream_url,
            homepage_url: http_url_or_blank(self.homepage),
            favicon_url: http_url_or_blank(self.favicon),
            country: self.country,
            language: self.language,
            tags: Some(split_tags(&self.tags)),
            codec: self.codec,
            bitrate: int_value(&self.bitrate),
            votes: int_value(&self.votes),
            health_status: health_status(&self.lastcheckok).to_owned(),
            last_checked_at: non_blank(self.lastchecktime_iso8601),
            last_successful_at: non_blank(self.lastcheckoktime_iso8601),
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RawOption {
    #[serde(deserialize_with = "gojson::nullable")]
    name: String,
    #[serde(deserialize_with = "gojson::nullable")]
    iso_3166_1: String,
    stationcount: Value,
}

fn split_tags(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Go's intValue over a JSON `any`: numbers truncate, strings parse.
fn int_value(value: &Value) -> i64 {
    match value {
        Value::Number(number) => number.as_f64().map(|number| number as i64).unwrap_or(0),
        Value::String(text) => text.parse().unwrap_or(0),
        _ => 0,
    }
}

fn health_status(value: &Value) -> &'static str {
    match value {
        Value::Bool(true) => "healthy",
        Value::Bool(false) => "broken",
        Value::Number(number) => {
            if number.as_f64() == Some(1.0) {
                "healthy"
            } else {
                "broken"
            }
        }
        Value::String(text) if text == "1" || text.eq_ignore_ascii_case("true") => "healthy",
        Value::String(text) if text == "0" || text.eq_ignore_ascii_case("false") => "broken",
        _ => "unknown",
    }
}

/// Catalog entries carry free text in URL fields, such as "null".
fn is_http_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|parsed| {
        matches!(parsed.scheme(), "http" | "https")
            && parsed.host_str().is_some_and(|host| !host.is_empty())
    })
}

fn http_url_or_blank(value: String) -> String {
    if is_http_url(&value) {
        value
    } else {
        String::new()
    }
}

fn non_blank(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

/// Go's boundedQueryInt: a value that does not parse is the fallback.
fn bounded(raw: &str, fallback: i64, minimum: i64, maximum: i64) -> i64 {
    raw.parse::<i64>()
        .unwrap_or(fallback)
        .clamp(minimum, maximum)
}

/// Go's url.QueryEscape.
pub fn query_escape(text: &str) -> String {
    let mut escaped = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                escaped.push(byte as char)
            }
            b' ' => escaped.push('+'),
            other => escaped.push_str(&format!("%{other:02X}")),
        }
    }
    escaped
}

/// Go's url.PathEscape (one path segment).
fn path_escape(text: &str) -> String {
    let mut escaped = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'$'
            | b'&'
            | b'+'
            | b','
            | b':'
            | b';'
            | b'='
            | b'@' => escaped.push(byte as char),
            other => escaped.push_str(&format!("%{other:02X}")),
        }
    }
    escaped
}

/// Go's url.Values.Encode: keys sorted.
fn encode(params: &BTreeMap<&str, String>) -> String {
    params
        .iter()
        .map(|(key, value)| format!("{}={}", query_escape(key), query_escape(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Go's strings.ToLower, rune by rune (no multi-character expansions).
fn go_lower(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            'İ' => 'i',
            other => other.to_lowercase().next().unwrap_or(other),
        })
        .collect()
}

impl RadioBrowserClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            client: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .user_agent(USER_AGENT)
                .build()
                .expect("Radio Browser client builds"),
        }
    }

    pub async fn search(&self, query: &GoQuery) -> Result<List<SearchResult>, BrowserError> {
        let limit = bounded(query.get("limit"), 40, 1, 100);
        let offset = bounded(query.get("offset"), 0, 0, 100_000);
        let mut params: BTreeMap<&str, String> = BTreeMap::new();
        params.insert("limit", limit.to_string());
        params.insert("offset", offset.to_string());
        params.insert("order", "name".to_owned());
        params.insert("reverse", "false".to_owned());
        let name = query.get("q").trim();
        if !name.is_empty() {
            params.insert("name", name.to_owned());
        }
        for key in ["country", "language", "tag", "codec"] {
            if !query.get(key).is_empty() {
                params.insert(key, query.get(key).to_owned());
            }
        }
        if !query.get("minBitrate").is_empty() {
            params.insert("bitrateMin", query.get("minBitrate").to_owned());
        }
        if query.get("codec").eq_ignore_ascii_case("AAC")
            || query.get("codecGroup").eq_ignore_ascii_case("aac")
        {
            params.remove("codec");
            return self.search_aac_family(params, limit, offset).await;
        }
        self.fetch_search(&format!("/json/stations/search?{}", encode(&params)))
            .await
    }

    /// AAC and AAC+ are one codec family: both are fetched, merged by name
    /// and paged here.
    async fn search_aac_family(
        &self,
        params: BTreeMap<&str, String>,
        limit: i64,
        offset: i64,
    ) -> Result<List<SearchResult>, BrowserError> {
        let upstream_limit = offset + limit;
        let mut all = Vec::new();
        for codec in ["AAC", "AAC+"] {
            let mut next = params.clone();
            next.insert("codec", codec.to_owned());
            next.insert("limit", upstream_limit.to_string());
            next.insert("offset", "0".to_owned());
            all.extend(
                self.fetch_search(&format!("/json/stations/search?{}", encode(&next)))
                    .await?
                    .items,
            );
        }
        all.sort_by_key(|result| go_lower(&result.name));
        let offset = offset as usize;
        if offset >= all.len() {
            return Ok(List {
                items: Vec::new(),
                total: 0,
            });
        }
        let end = (offset + limit as usize).min(all.len());
        let items: Vec<SearchResult> = all.drain(offset..end).collect();
        Ok(List {
            total: items.len(),
            items,
        })
    }

    pub async fn lookup(&self, station_uuid: &str) -> Result<SearchResult, BrowserError> {
        self.fetch_search(&format!(
            "/json/stations/byuuid/{}",
            path_escape(station_uuid)
        ))
        .await?
        .items
        .into_iter()
        .next()
        .ok_or(BrowserError::NotFound)
    }

    pub async fn countries(&self) -> Result<List<CatalogOption>, BrowserError> {
        let raw: Vec<RawOption> = self
            .fetch_json("/json/countries?order=name&reverse=false")
            .await?;
        let items: Vec<CatalogOption> = raw
            .into_iter()
            .map(|option| CatalogOption {
                code: option.iso_3166_1.to_uppercase(),
                station_count: int_value(&option.stationcount),
                name: option.name,
            })
            .collect();
        Ok(List {
            total: items.len(),
            items,
        })
    }

    pub async fn tags(&self) -> Result<List<CatalogOption>, BrowserError> {
        let raw: Vec<RawOption> = self
            .fetch_json("/json/tags?order=name&reverse=false")
            .await?;
        let items: Vec<CatalogOption> = raw
            .into_iter()
            .map(|option| CatalogOption {
                code: String::new(),
                station_count: int_value(&option.stationcount),
                name: option.name,
            })
            .collect();
        Ok(List {
            total: items.len(),
            items,
        })
    }

    async fn fetch_search(&self, path: &str) -> Result<List<SearchResult>, BrowserError> {
        let raw: Vec<RawStation> = self.fetch_json(path).await?;
        // A station without a playable address has nothing to offer.
        let items: Vec<SearchResult> = raw
            .into_iter()
            .map(RawStation::into_result)
            .filter(|result| is_http_url(&result.stream_url))
            .collect();
        Ok(List {
            total: items.len(),
            items,
        })
    }

    async fn fetch_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, BrowserError> {
        let url = format!("{}{path}", self.base_url);
        let started = Instant::now();
        let result = self.get_json(&url).await;
        observability::outbound_request(&url, started, result.is_ok());
        result
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, BrowserError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| BrowserError::Failed(format!("Get \"{url}\": {error}")))?;
        if !response.status().is_success() {
            return Err(BrowserError::Failed(format!(
                "radio browser status {}",
                response.status().as_u16()
            )));
        }
        let body = response
            .bytes()
            .await
            .map_err(|error| BrowserError::Failed(error.to_string()))?;
        // Like json.Decoder, read the first value and ignore what follows.
        let mut values = serde_json::Deserializer::from_slice(&body).into_iter::<T>();
        match values.next() {
            Some(Ok(value)) => Ok(value),
            Some(Err(error)) => Err(BrowserError::Failed(error.to_string())),
            None => Err(BrowserError::Failed("EOF".to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn drops_catalog_urls_that_are_not_http() {
        let raw: RawStation = serde_json::from_value(json!({
            "stationuuid": "u1", "name": "Jazz FM", "url": "http://a.example/x",
            "favicon": "null", "homepage": "www.a.example"
        }))
        .expect("station");
        let result = raw.into_result();
        assert_eq!(result.favicon_url, "");
        assert_eq!(result.homepage_url, "");

        let kept: RawStation = serde_json::from_value(json!({
            "favicon": "https://a.example/i.png", "homepage": "http://a.example/"
        }))
        .expect("station");
        let result = kept.into_result();
        assert_eq!(result.favicon_url, "https://a.example/i.png");
        assert_eq!(result.homepage_url, "http://a.example/");
    }

    #[test]
    fn maps_catalog_stations_like_the_go_client() {
        let raw: RawStation = serde_json::from_value(json!({
            "stationuuid": "u1", "name": "  Jazz FM ", "url": "http://a.example/x",
            "url_resolved": "", "tags": "jazz, smooth ,,", "bitrate": 128.9,
            "votes": "12", "lastcheckok": 1, "lastchecktime_iso8601": " ",
            "lastcheckoktime_iso8601": "2026-01-01T00:00:00Z", "homepage": null
        }))
        .expect("station");
        let result = raw.into_result();
        assert_eq!(result.name, "Jazz FM");
        assert_eq!(result.stream_url, "http://a.example/x");
        assert_eq!(
            result.tags,
            Some(vec!["jazz".to_owned(), "smooth".to_owned()])
        );
        assert_eq!((result.bitrate, result.votes), (128, 12));
        assert_eq!(result.health_status, "healthy");
        assert_eq!(result.last_checked_at, None);
        assert_eq!(
            result.last_successful_at.as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
    }

    #[test]
    fn health_reads_every_upstream_spelling() {
        assert_eq!(health_status(&json!(false)), "broken");
        assert_eq!(health_status(&json!(0)), "broken");
        assert_eq!(health_status(&json!("TRUE")), "healthy");
        assert_eq!(health_status(&json!("maybe")), "unknown");
        assert_eq!(health_status(&Value::Null), "unknown");
    }

    #[test]
    fn escapes_like_go() {
        assert_eq!(query_escape("rock & roll/ü"), "rock+%26+roll%2F%C3%BC");
        assert_eq!(path_escape("a b/c:d"), "a%20b%2Fc:d");
        let params = BTreeMap::from([
            ("order", "name".to_owned()),
            ("limit", "40".to_owned()),
            ("name", "a b".to_owned()),
        ]);
        assert_eq!(encode(&params), "limit=40&name=a+b&order=name");
        assert_eq!(bounded("x", 40, 1, 100), 40);
        assert_eq!(bounded("500", 40, 1, 100), 100);
        assert_eq!(bounded("-3", 0, 0, 100_000), 0);
    }
}
