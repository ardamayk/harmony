//! Configuration from the environment, with the same variables, defaults and
//! validation as the Go server (server/internal/config).

use std::net::{IpAddr, SocketAddr};

pub const DEFAULT_MANAGED_STORAGE_RESERVE_BYTES: i64 = 2 * 1024 * 1024 * 1024;
pub const DEFAULT_MANAGED_IMPORT_FILE_LIMIT_BYTES: i64 = 2 * 1024 * 1024 * 1024;
pub const DEFAULT_MANAGED_IMPORT_BATCH_LIMIT_BYTES: i64 = 4 * 1024 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Config {
    pub addr: String,
    pub database_path: String,
    pub cors_origins: Vec<String>,
    pub version: String,
    pub managed_storage_path: String,
    pub managed_storage_reserve_bytes: i64,
    pub managed_import_file_limit_bytes: i64,
    pub managed_import_batch_limit_bytes: i64,
    pub radio_browser_base_url: String,
    /// Online Artist photo and biography lookups (ADR 0027).
    pub artist_info_enabled: bool,
    /// Wikipedia language for Artist biographies, English as fallback.
    pub artist_info_language: String,
    /// Online LRCLIB lookups for Tracks without lyrics (ADR 0029).
    pub lyrics_lookup_enabled: bool,
    /// The operator's own Last.fm API key, for Track play counts (ADR 0030).
    pub lastfm_api_key: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ConfigError {
    #[error("{0} must be a valid {1} supported byte count")]
    InvalidByteCount(&'static str, &'static str),
    #[error("invalid SERVER_ADDR {0:?}: {1}")]
    InvalidAddress(String, String),
    #[error("SERVER_ADDR {0:?} must bind to a loopback address while authentication is disabled")]
    NotLoopback(String),
    #[error("ARTIST_INFO_LANGUAGE {0:?} must be a Wikipedia language code such as en or tr")]
    InvalidLanguage(String),
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Reads configuration through `lookup`, so tests need not touch the
    /// process environment. Empty values count as unset, like the Go server.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let get = |key: &str, fallback: &str| {
            lookup(key)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| fallback.to_owned())
        };
        let byte_count = |key: &'static str, fallback: i64, zero_allowed: bool| {
            let Some(raw) = lookup(key).filter(|value| !value.is_empty()) else {
                return Ok(fallback);
            };
            let description = if zero_allowed {
                "non-negative"
            } else {
                "positive"
            };
            match raw.parse::<i64>() {
                Ok(value) if value >= 0 && (zero_allowed || value != 0) && value != i64::MAX => {
                    Ok(value)
                }
                _ => Err(ConfigError::InvalidByteCount(key, description)),
            }
        };
        // It becomes a Wikipedia host name, so only a language code passes.
        let artist_info_language = get("ARTIST_INFO_LANGUAGE", "en");
        if !artist_info_language
            .chars()
            .all(|c| c.is_ascii_lowercase() || c == '-')
        {
            return Err(ConfigError::InvalidLanguage(artist_info_language));
        }
        Ok(Self {
            addr: get("SERVER_ADDR", "127.0.0.1:8090"),
            database_path: get("DATABASE_PATH", "./data/app.db"),
            cors_origins: vec![
                "http://localhost:3000".to_owned(),
                "http://127.0.0.1:3000".to_owned(),
            ],
            version: get("APP_VERSION", "0.1.0"),
            managed_storage_path: get("MANAGED_STORAGE_PATH", "./data/managed"),
            managed_storage_reserve_bytes: byte_count(
                "MANAGED_STORAGE_RESERVE_BYTES",
                DEFAULT_MANAGED_STORAGE_RESERVE_BYTES,
                true,
            )?,
            managed_import_file_limit_bytes: byte_count(
                "MANAGED_IMPORT_FILE_LIMIT_BYTES",
                DEFAULT_MANAGED_IMPORT_FILE_LIMIT_BYTES,
                false,
            )?,
            managed_import_batch_limit_bytes: byte_count(
                "MANAGED_IMPORT_BATCH_LIMIT_BYTES",
                DEFAULT_MANAGED_IMPORT_BATCH_LIMIT_BYTES,
                false,
            )?,
            radio_browser_base_url: get(
                "RADIO_BROWSER_BASE_URL",
                "https://de1.api.radio-browser.info",
            ),
            artist_info_enabled: !matches!(
                get("ARTIST_INFO_ENABLED", "true")
                    .to_ascii_lowercase()
                    .as_str(),
                "false" | "0"
            ),
            artist_info_language,
            lyrics_lookup_enabled: !matches!(
                get("LYRICS_LOOKUP_ENABLED", "true")
                    .to_ascii_lowercase()
                    .as_str(),
                "false" | "0"
            ),
            lastfm_api_key: Some(get("LASTFM_API_KEY", "").trim().to_owned())
                .filter(|key| !key.is_empty()),
        })
    }
}

/// The server has no authentication yet, so it only binds to loopback.
pub fn validate_server_address(address: &str) -> Result<(), ConfigError> {
    let invalid = |reason: &str| ConfigError::InvalidAddress(address.to_owned(), reason.to_owned());
    let (host, port) = address
        .rsplit_once(':')
        .ok_or_else(|| invalid("missing port in address"))?;
    port.parse::<u16>().map_err(|_| invalid("invalid port"))?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") {
        return Ok(());
    }
    match host.parse::<IpAddr>() {
        Ok(ip) if ip.is_loopback() => Ok(()),
        _ => Err(ConfigError::NotLoopback(address.to_owned())),
    }
}

/// Resolves the bind address; `localhost` binds the IPv4 loopback.
pub fn bind_address(address: &str) -> Option<SocketAddr> {
    let (host, port) = address.rsplit_once(':')?;
    let port = port.parse().ok()?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let ip = if host.eq_ignore_ascii_case("localhost") {
        IpAddr::from([127, 0, 0, 1])
    } else {
        host.parse().ok()?
    };
    Some(SocketAddr::new(ip, port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        Config::from_lookup(|key| map.get(key).cloned())
    }

    #[test]
    fn defaults_match_the_go_server() {
        let config = config(&[]).expect("defaults");
        assert_eq!(config.addr, "127.0.0.1:8090");
        assert_eq!(config.database_path, "./data/app.db");
        assert_eq!(config.version, "0.1.0");
        assert_eq!(config.managed_storage_path, "./data/managed");
        assert_eq!(
            config.managed_import_batch_limit_bytes,
            DEFAULT_MANAGED_IMPORT_BATCH_LIMIT_BYTES
        );
        assert_eq!(
            config.radio_browser_base_url,
            "https://de1.api.radio-browser.info"
        );
        assert!(config.artist_info_enabled);
        assert_eq!(config.artist_info_language, "en");
        assert!(config.lyrics_lookup_enabled);
        assert_eq!(config.lastfm_api_key, None);
    }

    #[test]
    fn takes_a_lastfm_key_and_ignores_a_blank_one() {
        assert_eq!(
            config(&[("LASTFM_API_KEY", " abc ")])
                .expect("config")
                .lastfm_api_key,
            Some("abc".to_owned())
        );
        assert_eq!(
            config(&[("LASTFM_API_KEY", "  ")])
                .expect("config")
                .lastfm_api_key,
            None
        );
    }

    #[test]
    fn lyrics_lookup_can_be_switched_off() {
        assert!(
            !config(&[("LYRICS_LOOKUP_ENABLED", "0")])
                .expect("config")
                .lyrics_lookup_enabled
        );
    }

    #[test]
    fn artist_info_can_be_switched_off_and_takes_only_language_codes() {
        assert!(
            !config(&[("ARTIST_INFO_ENABLED", "false")])
                .expect("config")
                .artist_info_enabled
        );
        assert_eq!(
            config(&[("ARTIST_INFO_LANGUAGE", "tr")])
                .expect("config")
                .artist_info_language,
            "tr"
        );
        assert_eq!(
            config(&[("ARTIST_INFO_LANGUAGE", "evil.example/x")]).err(),
            Some(ConfigError::InvalidLanguage("evil.example/x".to_owned()))
        );
    }

    #[test]
    fn empty_values_fall_back_to_defaults() {
        assert_eq!(
            config(&[("SERVER_ADDR", "")]).expect("config").addr,
            "127.0.0.1:8090"
        );
    }

    #[test]
    fn byte_counts_reject_invalid_values() {
        assert_eq!(
            config(&[("MANAGED_STORAGE_RESERVE_BYTES", "0")])
                .expect("zero reserve is allowed")
                .managed_storage_reserve_bytes,
            0
        );
        for (key, value) in [
            ("MANAGED_IMPORT_FILE_LIMIT_BYTES", "0"),
            ("MANAGED_IMPORT_FILE_LIMIT_BYTES", "-1"),
            ("MANAGED_IMPORT_BATCH_LIMIT_BYTES", "lots"),
            ("MANAGED_STORAGE_RESERVE_BYTES", "9223372036854775807"),
        ] {
            assert!(config(&[(key, value)]).is_err(), "{key}={value}");
        }
    }

    #[test]
    fn server_address_must_be_loopback() {
        for address in [
            "127.0.0.1:8090",
            "localhost:8090",
            "[::1]:8090",
            "127.5.5.5:1",
        ] {
            assert_eq!(validate_server_address(address), Ok(()), "{address}");
        }
        for address in [
            "0.0.0.0:8090",
            "192.168.1.2:8090",
            "example.com:80",
            "nope",
            ":8090",
        ] {
            assert!(validate_server_address(address).is_err(), "{address}");
        }
        assert_eq!(
            bind_address("localhost:8090"),
            Some("127.0.0.1:8090".parse().expect("socket address"))
        );
    }
}
