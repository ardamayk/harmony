//! Cloudflare Access in front of the Owner's listener (ADR 0038). Access
//! decides who gets in; this checks the token it signs, so a mistake in the
//! Access policy does not open the Owner's routes to the internet.

use crate::config::OwnerAccessConfig;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Cloudflare rotates its keys, so a token may name one not fetched yet. An
/// unknown key cannot ask for the certs more often than this.
const MIN_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
struct Certs {
    keys: Vec<Cert>,
}

#[derive(Deserialize)]
struct Cert {
    kid: String,
    kty: String,
    n: Option<String>,
    e: Option<String>,
}

pub struct OwnerAccess {
    settings: OwnerAccessConfig,
    certs_url: String,
    validation: Validation,
    http: reqwest::Client,
    keys: RwLock<HashMap<String, DecodingKey>>,
    last_refresh: Mutex<Option<Instant>>,
}

impl OwnerAccess {
    pub fn new(settings: OwnerAccessConfig) -> Self {
        let certs_url = format!("https://{}/cdn-cgi/access/certs", settings.team_domain);
        Self::with_certs_url(settings, certs_url)
    }

    /// For tests: where the signing keys come from.
    pub fn with_certs_url(settings: OwnerAccessConfig, certs_url: String) -> Self {
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[&settings.audience]);
        validation.set_issuer(&[format!("https://{}", settings.team_domain)]);
        validation.set_required_spec_claims(&["exp", "iss", "aud"]);
        Self {
            settings,
            certs_url,
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

    /// Whether Access signed this token for this application. Without the
    /// keys it refuses: a Cloudflare outage must not open the Owner's routes.
    pub async fn verify(&self, token: &str) -> bool {
        let Some(key_id) = decode_header(token).ok().and_then(|header| header.kid) else {
            return false;
        };
        if !self.has_key(&key_id) {
            self.refresh().await;
        }
        let keys = self.keys.read().expect("Access keys lock");
        keys.get(&key_id).is_some_and(|key| {
            decode::<serde_json::Value>(token.as_bytes(), key, &self.validation).is_ok()
        })
    }

    fn has_key(&self, key_id: &str) -> bool {
        self.keys
            .read()
            .expect("Access keys lock")
            .contains_key(key_id)
    }

    async fn refresh(&self) {
        let mut last_refresh = self.last_refresh.lock().await;
        if last_refresh.is_some_and(|last| last.elapsed() < MIN_REFRESH_INTERVAL) {
            return;
        }
        *last_refresh = Some(Instant::now());
        match self.fetch().await {
            Ok(keys) => *self.keys.write().expect("Access keys lock") = keys,
            Err(error) => tracing::error!(%error, "Cloudflare Access keys unavailable"),
        }
    }

    async fn fetch(&self) -> Result<HashMap<String, DecodingKey>, reqwest::Error> {
        let certs: Certs = self
            .http
            .get(&self.certs_url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(certs
            .keys
            .into_iter()
            .filter(|cert| cert.kty == "RSA")
            .filter_map(|cert| {
                let key = DecodingKey::from_rsa_components(cert.n.as_deref()?, cert.e.as_deref()?)
                    .ok()?;
                Some((cert.kid, key))
            })
            .collect())
    }
}
