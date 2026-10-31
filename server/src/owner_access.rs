//! The Owner's public address (ADR 0038). The server has no sign-in of its
//! own, so a proxy in front of it decides who gets in; this checks the token
//! that proxy signs, so a mistake in the proxy does not open the Owner's
//! routes to the internet.

use crate::config::OwnerAccessConfig;
use axum::http::{HeaderMap, HeaderName};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Proxies rotate their keys, so a token may name one not fetched yet. An
/// unknown key cannot ask for the key set more often than this.
const MIN_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
struct KeySet {
    keys: Vec<Jwk>,
}

#[derive(Deserialize)]
struct Jwk {
    kid: String,
    kty: String,
    crv: Option<String>,
    n: Option<String>,
    e: Option<String>,
    x: Option<String>,
    y: Option<String>,
}

impl Jwk {
    /// Each key allows one algorithm, so a token cannot choose how it is
    /// checked. Any other kind of key is left out.
    fn decoding_key(&self) -> Option<(DecodingKey, Algorithm)> {
        match (self.kty.as_str(), self.crv.as_deref()) {
            ("RSA", _) => Some((
                DecodingKey::from_rsa_components(self.n.as_deref()?, self.e.as_deref()?).ok()?,
                Algorithm::RS256,
            )),
            ("EC", Some("P-256")) => Some((
                DecodingKey::from_ec_components(self.x.as_deref()?, self.y.as_deref()?).ok()?,
                Algorithm::ES256,
            )),
            _ => None,
        }
    }
}

pub struct OwnerAccess {
    settings: OwnerAccessConfig,
    validation: Validation,
    http: reqwest::Client,
    keys: RwLock<HashMap<String, (DecodingKey, Algorithm)>>,
    last_refresh: Mutex<Option<Instant>>,
}

impl OwnerAccess {
    pub fn new(settings: OwnerAccessConfig) -> Self {
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[&settings.audience]);
        validation.set_issuer(&[&settings.issuer]);
        validation.set_required_spec_claims(&["exp", "iss", "aud"]);
        Self {
            settings,
            validation,
            http: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("HTTP client"),
            keys: RwLock::default(),
            last_refresh: Mutex::default(),
        }
    }

    pub fn accepts_host(&self, host: &str) -> bool {
        self.settings.accepts_host(host)
    }

    fn token<'a>(&self, headers: &'a HeaderMap) -> Option<&'a str> {
        let name: &HeaderName = &self.settings.token_header;
        headers.get(name).and_then(|value| value.to_str().ok())
    }

    /// Whether the request carries a token the proxy signed for this server.
    /// Without the keys it refuses: an outage must not open the Owner's routes.
    pub async fn allows(&self, headers: &HeaderMap) -> bool {
        match self.token(headers) {
            Some(token) => self.verify(token).await,
            None => false,
        }
    }

    async fn verify(&self, token: &str) -> bool {
        let Some(key_id) = decode_header(token).ok().and_then(|header| header.kid) else {
            return false;
        };
        if !self.has_key(&key_id) {
            self.refresh().await;
        }
        let keys = self.keys.read().expect("owner keys lock");
        keys.get(&key_id).is_some_and(|(key, algorithm)| {
            let mut validation = self.validation.clone();
            validation.algorithms = vec![*algorithm];
            decode::<serde_json::Value>(token.as_bytes(), key, &validation).is_ok()
        })
    }

    fn has_key(&self, key_id: &str) -> bool {
        self.keys
            .read()
            .expect("owner keys lock")
            .contains_key(key_id)
    }

    async fn refresh(&self) {
        let mut last_refresh = self.last_refresh.lock().await;
        if last_refresh.is_some_and(|last| last.elapsed() < MIN_REFRESH_INTERVAL) {
            return;
        }
        let started = std::time::Instant::now();
        *last_refresh = Some(Instant::now());
        let keys = self.fetch().await;
        crate::observability::outbound_request(&self.settings.jwks_url, started, keys.is_ok());
        match keys {
            Ok(keys) => *self.keys.write().expect("owner keys lock") = keys,
            Err(error) => tracing::error!(%error, "Owner JWT keys unavailable"),
        }
    }

    async fn fetch(&self) -> Result<HashMap<String, (DecodingKey, Algorithm)>, reqwest::Error> {
        let key_set: KeySet = self
            .http
            .get(&self.settings.jwks_url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(key_set
            .keys
            .into_iter()
            .filter_map(|jwk| Some((jwk.kid.clone(), jwk.decoding_key()?)))
            .collect())
    }
}
