//! Sign-in rules: passwords, sessions and attempt limits (ADR 0042).

use super::limits::Attempts;
use super::repository::Repository;
use crate::auth::DEFAULT_USER_ID;
use crate::config::{AuthConfig, IpNet};
use crate::db::Db;
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const SESSION_LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// A session in use gets a new expiry at most this often, so a request is
/// rarely a write.
const RENEW_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
/// Argon2 per request would slow every Track stream, so a Basic header's
/// answer is reused for this long. A wrong one is remembered too: a client
/// repeating a stale header (say, after a password change) is not guessing,
/// and must not use up its address's attempts.
const BASIC_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const BASIC_CACHE_MAX: usize = 256;

/// Checked when the username is unknown, so the answer takes as long as for
/// a known one and the timing does not tell which usernames exist.
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
    let mut password = [0u8; 32];
    getrandom::fill(&mut password).expect("system random numbers");
    hash_password(&hex::encode(password)).expect("dummy password hash")
});

/// An argon2id PHC string with the crate's recommended cost.
pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("too many attempts; retry in {0} s")]
    TooManyAttempts(u64),
    #[error("wrong username or password")]
    InvalidCredentials,
    #[error("auth storage: {0}")]
    Storage(#[from] sqlx::Error),
}

pub struct User {
    pub id: String,
    pub username: String,
}

/// A session cookie that still holds, and whether its expiry moved.
pub struct Session {
    pub user_id: String,
    pub renewed: bool,
}

type HeaderKey = [u8; 32];

pub struct Auth {
    repository: Repository,
    config: AuthConfig,
    attempts: Mutex<Attempts>,
    /// Basic headers already checked, by SHA-256 of the header value: the
    /// user they belong to, or `None` for wrong credentials.
    basic_checked: Mutex<HashMap<HeaderKey, (Option<String>, Instant)>>,
    /// One check per Basic header at a time: a client's first burst of
    /// requests waits for the first answer instead of each counting as an
    /// attempt.
    basic_checks: Mutex<HashMap<HeaderKey, Arc<tokio::sync::Mutex<()>>>>,
}

impl Auth {
    pub fn new(db: Db, config: AuthConfig) -> Self {
        Self {
            repository: Repository::new(db),
            config,
            attempts: Mutex::default(),
            basic_checked: Mutex::default(),
            basic_checks: Mutex::default(),
        }
    }

    pub fn trusted_proxies(&self) -> &[IpNet] {
        &self.config.trusted_proxies
    }

    /// Brings the stored user in line with the configuration.
    pub async fn start(&self) -> Result<(), sqlx::Error> {
        self.repository
            .sync_user(
                DEFAULT_USER_ID,
                &self.config.username,
                &self.config.password_hash,
                unix_now(),
            )
            .await
    }

    /// The user's id when the password is theirs, under the attempt limits.
    async fn check_password(
        &self,
        client: IpAddr,
        username: &str,
        password: &str,
    ) -> Result<String, AuthError> {
        if let Some(wait) = self
            .attempts
            .lock()
            .expect("attempts lock")
            .begin(client, Instant::now())
        {
            return Err(AuthError::TooManyAttempts(whole_seconds(wait)));
        }
        let user = self.repository.user_by_username(username).await?;
        let hash = user.as_ref().map(|user| user.password_hash.clone());
        let password = password.to_owned();
        let matches = tokio::task::spawn_blocking(move || {
            let hash = hash.as_deref().unwrap_or(&DUMMY_HASH);
            Argon2::default()
                .verify_password(password.as_bytes(), hash)
                .is_ok()
        })
        .await
        .unwrap_or(false);
        match user.filter(|_| matches) {
            Some(user) => {
                self.attempts.lock().expect("attempts lock").succeed(client);
                Ok(user.id)
            }
            None => {
                self.attempts
                    .lock()
                    .expect("attempts lock")
                    .fail(client, Instant::now());
                Err(AuthError::InvalidCredentials)
            }
        }
    }

    /// Signs in and returns the new session's token for the cookie.
    pub async fn login(
        &self,
        client: IpAddr,
        username: &str,
        password: &str,
    ) -> Result<String, AuthError> {
        let user_id = self.check_password(client, username, password).await?;
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).expect("system random numbers");
        let token = URL_SAFE_NO_PAD.encode(bytes);
        let now = unix_now();
        self.repository
            .insert_session(
                &token_hash(&token),
                &user_id,
                now,
                now + SESSION_LIFETIME.as_secs() as i64,
            )
            .await?;
        Ok(token)
    }

    /// The user behind an `Authorization: Basic` header, whose decoded
    /// credentials are `username` and `password`.
    pub async fn basic(
        &self,
        client: IpAddr,
        header: &str,
        username: &str,
        password: &str,
    ) -> Result<String, AuthError> {
        let key: HeaderKey = Sha256::digest(header.as_bytes()).into();
        if let Some(known) = self.basic_cached(&key) {
            return known.ok_or(AuthError::InvalidCredentials);
        }
        let check = self
            .basic_checks
            .lock()
            .expect("basic checks lock")
            .entry(key)
            .or_default()
            .clone();
        let result = {
            let _turn = check.lock().await;
            match self.basic_cached(&key) {
                Some(known) => known.ok_or(AuthError::InvalidCredentials),
                None => self.check_password(client, username, password).await,
            }
        };
        {
            let mut checks = self.basic_checks.lock().expect("basic checks lock");
            if checks
                .get(&key)
                .is_some_and(|stored| Arc::ptr_eq(stored, &check))
            {
                checks.remove(&key);
            }
        }
        let known = match &result {
            Ok(user_id) => Some(Some(user_id.clone())),
            Err(AuthError::InvalidCredentials) => Some(None),
            // A wait or a database error says nothing about the header.
            Err(_) => None,
        };
        if let Some(known) = known {
            let mut checked = self.basic_checked.lock().expect("basic cache lock");
            if checked.len() >= BASIC_CACHE_MAX {
                checked.retain(|_, (_, at)| at.elapsed() < BASIC_CACHE_TTL);
            }
            if checked.len() >= BASIC_CACHE_MAX {
                checked.clear();
            }
            checked.insert(key, (known, Instant::now()));
        }
        result
    }

    fn basic_cached(&self, key: &HeaderKey) -> Option<Option<String>> {
        self.basic_checked
            .lock()
            .expect("basic cache lock")
            .get(key)
            .filter(|(_, at)| at.elapsed() < BASIC_CACHE_TTL)
            .map(|(user_id, _)| user_id.clone())
    }

    /// The session behind a cookie's token, renewed when it is due. An
    /// expired session is deleted.
    pub async fn session(&self, token: &str) -> Result<Option<Session>, sqlx::Error> {
        let hash = token_hash(token);
        let Some(session) = self.repository.session(&hash).await? else {
            return Ok(None);
        };
        let now = unix_now();
        if session.expires_at <= now {
            self.repository.delete_session(&hash).await?;
            return Ok(None);
        }
        let renewed = now - session.last_seen_at >= RENEW_AFTER.as_secs() as i64;
        if renewed {
            self.repository
                .renew_session(&hash, now, now + SESSION_LIFETIME.as_secs() as i64)
                .await?;
        }
        Ok(Some(Session {
            user_id: session.user_id,
            renewed,
        }))
    }

    pub async fn logout(&self, token: &str) -> Result<(), sqlx::Error> {
        self.repository.delete_session(&token_hash(token)).await
    }

    pub async fn user(&self, id: &str) -> Result<Option<User>, sqlx::Error> {
        Ok(self.repository.user(id).await?.map(|user| User {
            id: user.id,
            username: user.username,
        }))
    }
}

/// The database keeps only this, so a leaked copy holds no usable token.
fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

/// Retry-After in whole seconds, never 0 while a wait remains.
fn whole_seconds(wait: Duration) -> u64 {
    wait.as_secs() + u64::from(wait.subsec_nanos() > 0)
}
