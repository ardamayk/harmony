//! The operator's slskd over its REST API (`/api/v0`, `X-API-Key`). This is
//! the only place that knows slskd's JSON shapes; the rest of the module
//! sees the plain types below. Every call is bounded by a timeout, and
//! every failure is one [`SlskdError`] whose message the clients can show.

use crate::config::SoulseekConfig;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// slskd runs next to the Music Server, so a connection that takes longer
/// than this means it is down; the settings should say so quickly.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SlskdError {
    /// slskd did not answer, refused the API key or answered something
    /// unexpected; the reason says which.
    #[error("slskd is unavailable: {0}")]
    Unreachable(String),
    /// slskd answered but is not logged in to the Soulseek server.
    #[error("slskd is not logged in to Soulseek")]
    NotLoggedIn,
    /// slskd does not know the search.
    #[error("slskd does not know this search")]
    NotFound,
}

/// A search as slskd keeps it, with the responses received so far.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Search {
    pub search_text: String,
    pub is_complete: bool,
    pub responses: Vec<SearchResponse>,
}

/// One user's answer to a search. Locked files, which the user does not
/// let this account download, are left out.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SearchResponse {
    pub username: String,
    pub has_free_upload_slot: bool,
    pub queue_length: u64,
    /// Bytes per second.
    pub upload_speed: u64,
    pub files: Vec<SearchFile>,
}

/// One file as the user's client reports it; any attribute may be missing.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SearchFile {
    /// The full remote path, usually with backslashes.
    pub filename: String,
    pub size: u64,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<u32>,
    /// Kilobits per second.
    pub bit_rate: Option<u32>,
    pub is_variable_bit_rate: Option<bool>,
}

/// slskd's own state and its connection to the Soulseek server.
#[derive(Clone, Debug, PartialEq)]
pub struct Application {
    pub version: Option<String>,
    pub connected: bool,
    pub logged_in: bool,
    pub username: Option<String>,
}

/// One directory slskd shares (or excludes), as slskd sees the filesystem.
#[derive(Clone, Debug, PartialEq)]
pub struct Share {
    pub local_path: String,
    pub excluded: bool,
    /// Files found by slskd's last scan, when it has scanned.
    pub files: Option<u64>,
}

pub struct Slskd {
    client: reqwest::Client,
    /// slskd's base address ending in `/`, so relative joins keep any base
    /// path the operator serves slskd under.
    base: url::Url,
    api_key: String,
}

impl Slskd {
    pub fn new(config: &SoulseekConfig) -> Self {
        let mut base = config.slskd_url.clone();
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        Self {
            client: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .connect_timeout(CONNECT_TIMEOUT)
                .build()
                .expect("HTTP client"),
            base,
            api_key: config.api_key.clone(),
        }
    }

    /// `GET /api/v0/application`: version, server connection and login.
    pub async fn application(&self) -> Result<Application, SlskdError> {
        let state: ApplicationState = self.get("api/v0/application").await?;
        Ok(Application {
            version: state.version.current.filter(|version| !version.is_empty()),
            connected: state.server.is_connected,
            logged_in: state.server.is_logged_in,
            username: state.user.username.filter(|username| !username.is_empty()),
        })
    }

    /// `GET /api/v0/shares`: every host's shares, flattened.
    pub async fn shares(&self) -> Result<Vec<Share>, SlskdError> {
        let hosts: std::collections::BTreeMap<String, Vec<ShareState>> =
            self.get("api/v0/shares").await?;
        Ok(hosts
            .into_values()
            .flatten()
            .map(|share| Share {
                local_path: share.local_path,
                excluded: share.is_excluded,
                files: share.files,
            })
            .collect())
    }

    /// `POST /api/v0/searches` under the Music Server's own id. slskd answers
    /// 409 when it cannot search, which is when it is not logged in.
    pub async fn start_search(&self, id: uuid::Uuid, text: &str) -> Result<(), SlskdError> {
        let body = serde_json::json!({ "id": id, "searchText": text });
        let request = self
            .client
            .post(self.url("api/v0/searches")?)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string());
        let response = self.send_raw(request).await?;
        if response.status() == reqwest::StatusCode::CONFLICT {
            return Err(SlskdError::NotLoggedIn);
        }
        check(response).map(drop)
    }

    /// `GET /api/v0/searches/{id}` with its responses.
    pub async fn search(&self, id: uuid::Uuid) -> Result<Search, SlskdError> {
        let mut url = self.url(&format!("api/v0/searches/{id}"))?;
        url.set_query(Some("includeResponses=true"));
        let response = self.send_raw(self.client.get(url)).await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(SlskdError::NotFound);
        }
        read_json(check(response)?).await
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, SlskdError> {
        let response = self.send(self.client.get(self.url(path)?)).await?;
        read_json(response).await
    }

    fn url(&self, path: &str) -> Result<url::Url, SlskdError> {
        self.base
            .join(path)
            .map_err(|error| SlskdError::Unreachable(format!("invalid address: {error}")))
    }

    async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, SlskdError> {
        check(self.send_raw(request).await?)
    }

    /// Sends with the API key; only a failed exchange is an error here.
    async fn send_raw(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, SlskdError> {
        request
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .map_err(|error| SlskdError::Unreachable(describe(&error)))
    }
}

fn check(response: reqwest::Response) -> Result<reqwest::Response, SlskdError> {
    match response.status() {
        status if status.is_success() => Ok(response),
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
            Err(SlskdError::Unreachable("it refused the API key".to_owned()))
        }
        reqwest::StatusCode::TOO_MANY_REQUESTS => Err(SlskdError::Unreachable(
            "it is busy starting another search; try again".to_owned(),
        )),
        status => Err(SlskdError::Unreachable(format!(
            "it answered HTTP {}",
            status.as_u16()
        ))),
    }
}

async fn read_json<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, SlskdError> {
    let body = response
        .bytes()
        .await
        .map_err(|error| SlskdError::Unreachable(describe(&error)))?;
    serde_json::from_slice(&body)
        .map_err(|error| SlskdError::Unreachable(format!("unexpected answer: {error}")))
}

fn describe(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        format!("no answer within {} seconds", REQUEST_TIMEOUT.as_secs())
    } else if error.is_connect() {
        "could not connect".to_owned()
    } else {
        error.to_string()
    }
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ApplicationState {
    version: VersionState,
    server: ServerState,
    user: UserState,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct VersionState {
    current: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ServerState {
    is_connected: bool,
    is_logged_in: bool,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct UserState {
    username: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ShareState {
    local_path: String,
    is_excluded: bool,
    files: Option<u64>,
}
