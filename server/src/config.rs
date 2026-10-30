//! Configuration from the environment, with the same variables, defaults and
//! validation as the Go server (server/internal/config).

use std::net::{IpAddr, SocketAddr};

pub const DEFAULT_MANAGED_STORAGE_RESERVE_BYTES: i64 = 2 * 1024 * 1024 * 1024;
pub const DEFAULT_MANAGED_IMPORT_FILE_LIMIT_BYTES: i64 = 2 * 1024 * 1024 * 1024;
pub const DEFAULT_MANAGED_IMPORT_BATCH_LIMIT_BYTES: i64 = 4 * 1024 * 1024 * 1024;
/// A Room admits at most this many Guests (ADR 0037).
pub const MAX_LISTEN_TOGETHER_GUESTS: usize = 8;

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
    /// The operator's yt-dlp search, off when unset (ADR 0032).
    pub external_source: Option<ExternalSourceConfig>,
    /// The owner's own Sentry, off when `SENTRY_DSN` is unset (ADR 0033).
    pub sentry: Option<SentryConfig>,
    /// Where Guests reach this server, off unless `GUEST_ADDR` and
    /// `PUBLIC_URL` are both set (ADR 0037).
    pub listen_together: Option<ListenTogetherConfig>,
    /// `LISTEN_TOGETHER_MAX_GUESTS`: at most this many Guests in a Room.
    pub listen_together_max_guests: usize,
}

/// The Guest Listener and the public address that reaches it.
#[derive(Clone, Debug, PartialEq)]
pub struct ListenTogetherConfig {
    /// The Guest Listener's loopback address.
    pub guest_addr: String,
    /// `PUBLIC_URL`'s origin, such as `https://music.example.com`.
    pub public_origin: String,
    /// Whether Guests arrive over HTTPS, which decides the cookie's name
    /// and `Secure` attribute.
    pub secure: bool,
    /// The Host values that mean `PUBLIC_URL`: its authority as written and
    /// with the scheme's default port spelled out.
    hosts: [String; 2],
}

impl ListenTogetherConfig {
    /// Whether a Host header names `PUBLIC_URL`.
    pub fn accepts_host(&self, host: &str) -> bool {
        self.hosts
            .iter()
            .any(|accepted| accepted.eq_ignore_ascii_case(host))
    }
}

/// Where and how the Music Server reports to the owner's Sentry.
#[derive(Clone, PartialEq)]
pub struct SentryConfig {
    pub dsn: sentry::types::Dsn,
    /// `SENTRY_ENVIRONMENT`, else "development" for a debug build and
    /// "production" for a release build.
    pub environment: String,
    /// `SENTRY_TRACES_SAMPLE_RATE`: the share of requests sent as
    /// transactions, 1.0 unless set.
    pub traces_sample_rate: f32,
}

/// Leaves the DSN's key out of debug output.
impl std::fmt::Debug for SentryConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SentryConfig")
            .field("dsn", &"<set>")
            .field("environment", &self.environment)
            .field("traces_sample_rate", &self.traces_sample_rate)
            .finish()
    }
}

/// The environment a build reports when `SENTRY_ENVIRONMENT` is unset.
pub fn default_sentry_environment() -> &'static str {
    if cfg!(debug_assertions) {
        "development"
    } else {
        "production"
    }
}

/// The External Source: which yt-dlp search runs, and with which binary.
#[derive(Clone, Debug, PartialEq)]
pub struct ExternalSourceConfig {
    /// A yt-dlp search prefix such as `scsearch`.
    pub search_prefix: String,
    /// The yt-dlp binary: a name looked up on PATH, or a path.
    pub yt_dlp: String,
    /// yt-dlp extractor arguments such as `youtube:skip=hls,dash`, each
    /// passed after its own `--extractor-args`.
    pub extractor_args: Vec<String>,
    /// The site's name for the clients: `EXTERNAL_SOURCE_NAME`, else the
    /// name of a known search prefix, else "External".
    pub name: String,
}

/// yt-dlp's search prefixes the clients can name without
/// `EXTERNAL_SOURCE_NAME`. None is a default: the operator still chooses
/// the prefix (ADR 0032).
const SEARCH_PREFIX_NAMES: &[(&str, &str)] = &[
    ("ytsearch", "YouTube"),
    ("scsearch", "SoundCloud"),
    ("bilisearch", "BiliBili"),
    ("nicosearch", "Niconico"),
    ("gvsearch", "Google Video"),
    ("yvsearch", "Yahoo Video"),
    ("rkfnsearch", "Rokfin"),
];

/// The name shown for a search prefix nobody named.
const FALLBACK_EXTERNAL_SOURCE_NAME: &str = "External";
const MAX_EXTERNAL_SOURCE_NAME_CHARS: usize = 40;

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
    #[error("EXTERNAL_SOURCE_SEARCH {0:?} must be a yt-dlp search prefix such as scsearch")]
    InvalidExternalSourceSearch(String),
    #[error(
        "EXTERNAL_SOURCE_EXTRACTOR_ARGS item {0:?} must be yt-dlp extractor arguments such as youtube:skip=hls,dash"
    )]
    InvalidExternalSourceExtractorArgs(String),
    #[error("EXTERNAL_SOURCE_NAME {0:?} must be 1 to 40 printable characters such as SoundCloud")]
    InvalidExternalSourceName(String),
    // The DSN holds the project's key, so the message does not repeat it.
    #[error(
        "SENTRY_DSN must be a Sentry DSN such as https://<public-key>@<your-sentry-host>/<project-id>"
    )]
    InvalidSentryDsn,
    #[error(
        "SENTRY_ENVIRONMENT {0:?} must be 1 to 64 letters, digits, '.', '_' or '-' such as production"
    )]
    InvalidSentryEnvironment(String),
    #[error("SENTRY_TRACES_SAMPLE_RATE {0:?} must be a number from 0 to 1 such as 0.25")]
    InvalidSentryTracesSampleRate(String),
    #[error("GUEST_ADDR and PUBLIC_URL must be set together: {0} is missing")]
    IncompleteListenTogether(&'static str),
    #[error("invalid GUEST_ADDR {0:?}: {1}")]
    InvalidGuestAddress(String, String),
    #[error(
        "GUEST_ADDR {0:?} must bind to a loopback address; a reverse proxy or tunnel carries Guests to it"
    )]
    GuestAddressNotLoopback(String),
    #[error("GUEST_ADDR {0:?} must differ from SERVER_ADDR")]
    GuestAddressIsServerAddress(String),
    // A URL with credentials would put them in the log, so the message does
    // not repeat the value.
    #[error("PUBLIC_URL must be an origin such as https://music.example.com: {0}")]
    InvalidPublicUrl(&'static str),
    #[error("LISTEN_TOGETHER_MAX_GUESTS {0:?} must be a whole number from 1 to 8")]
    InvalidMaxGuests(String),
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
        // The prefix becomes part of yt-dlp's argument, so only a search
        // prefix's shape passes.
        let external_source_search = get("EXTERNAL_SOURCE_SEARCH", "").trim().to_owned();
        // Each item becomes one yt-dlp argument, so only the shape of
        // `IE_KEY:ARG=VAL;ARG=VAL` passes.
        let extractor_args = get("EXTERNAL_SOURCE_EXTRACTOR_ARGS", "")
            .split_whitespace()
            .map(|item| {
                if is_extractor_args(item) {
                    Ok(item.to_owned())
                } else {
                    Err(ConfigError::InvalidExternalSourceExtractorArgs(
                        item.to_owned(),
                    ))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        // Shown in the clients, so it stays one short line of text.
        let external_source_name = get("EXTERNAL_SOURCE_NAME", "").trim().to_owned();
        if external_source_name.chars().count() > MAX_EXTERNAL_SOURCE_NAME_CHARS
            || external_source_name.chars().any(char::is_control)
        {
            return Err(ConfigError::InvalidExternalSourceName(external_source_name));
        }
        let external_source = if external_source_search.is_empty() {
            None
        } else if is_search_prefix(&external_source_search) {
            Some(ExternalSourceConfig {
                name: Some(external_source_name)
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| search_prefix_name(&external_source_search).to_owned()),
                search_prefix: external_source_search,
                yt_dlp: Some(get("YT_DLP", "").trim().to_owned())
                    .filter(|binary| !binary.is_empty())
                    .unwrap_or_else(|| "yt-dlp".to_owned()),
                extractor_args,
            })
        } else {
            return Err(ConfigError::InvalidExternalSourceSearch(
                external_source_search,
            ));
        };
        let sentry = sentry_config(&lookup)?;
        let addr = get("SERVER_ADDR", "127.0.0.1:8090");
        let listen_together = listen_together_config(&lookup, &addr)?;
        let listen_together_max_guests = max_guests(&lookup)?;
        Ok(Self {
            addr,
            database_path: get("DATABASE_PATH", "./data/app.db"),
            // The Web Client's dev server (3000) and `vp preview` (4173)
            // proxy the API but forward the browser's Origin.
            cors_origins: vec![
                "http://localhost:3000".to_owned(),
                "http://127.0.0.1:3000".to_owned(),
                "http://localhost:4173".to_owned(),
                "http://127.0.0.1:4173".to_owned(),
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
            external_source,
            sentry,
            listen_together,
            listen_together_max_guests,
        })
    }
}

/// Listen Together needs both a Guest Listener and the public address that
/// reaches it; one without the other would produce a link nobody can open.
fn listen_together_config(
    lookup: &impl Fn(&str) -> Option<String>,
    server_addr: &str,
) -> Result<Option<ListenTogetherConfig>, ConfigError> {
    let value = |key: &str| {
        lookup(key)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    let (guest_addr, public_url) = match (value("GUEST_ADDR"), value("PUBLIC_URL")) {
        (None, None) => return Ok(None),
        (Some(_), None) => return Err(ConfigError::IncompleteListenTogether("PUBLIC_URL")),
        (None, Some(_)) => return Err(ConfigError::IncompleteListenTogether("GUEST_ADDR")),
        (Some(guest_addr), Some(public_url)) => (guest_addr, public_url),
    };
    check_loopback(&guest_addr).map_err(|problem| match problem {
        AddressProblem::Invalid(reason) => {
            ConfigError::InvalidGuestAddress(guest_addr.clone(), reason)
        }
        AddressProblem::NotLoopback => ConfigError::GuestAddressNotLoopback(guest_addr.clone()),
    })?;
    if guest_addr.eq_ignore_ascii_case(server_addr)
        || bind_address(&guest_addr).is_some_and(|guest| bind_address(server_addr) == Some(guest))
    {
        return Err(ConfigError::GuestAddressIsServerAddress(guest_addr));
    }
    let invalid = ConfigError::InvalidPublicUrl;
    let url = url::Url::parse(&public_url).map_err(|_| invalid("it is not an absolute URL"))?;
    let secure = match url.scheme() {
        "https" => true,
        "http" => false,
        _ => return Err(invalid("the scheme must be https")),
    };
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid("it cannot hold a user name or password"));
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(invalid("it cannot have a path, query or fragment"));
    }
    let is_loopback = match url.host() {
        None => return Err(invalid("it needs a host")),
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
    };
    if !secure && !is_loopback {
        return Err(invalid(
            "the scheme must be https unless the host is loopback",
        ));
    }
    let host = url.host_str().unwrap_or_default();
    let port = url.port_or_known_default().unwrap_or_default();
    let authority = url
        .port()
        .map_or_else(|| host.to_owned(), |port| format!("{host}:{port}"));
    Ok(Some(ListenTogetherConfig {
        guest_addr,
        public_origin: url.origin().ascii_serialization(),
        secure,
        hosts: [authority, format!("{host}:{port}")],
    }))
}

/// Checked even while Listen Together is off, like the Sentry settings.
fn max_guests(lookup: &impl Fn(&str) -> Option<String>) -> Result<usize, ConfigError> {
    let Some(raw) = lookup("LISTEN_TOGETHER_MAX_GUESTS")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Ok(MAX_LISTEN_TOGETHER_GUESTS);
    };
    match raw.parse::<usize>() {
        Ok(limit) if (1..=MAX_LISTEN_TOGETHER_GUESTS).contains(&limit) => Ok(limit),
        _ => Err(ConfigError::InvalidMaxGuests(raw)),
    }
}

/// Sentry is on only with `SENTRY_DSN`; there is no default DSN. The
/// environment and sample rate are checked even while it is off, like the
/// External Source's extractor arguments.
fn sentry_config(
    lookup: &impl Fn(&str) -> Option<String>,
) -> Result<Option<SentryConfig>, ConfigError> {
    let value = |key: &str| {
        lookup(key)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    let environment = value("SENTRY_ENVIRONMENT");
    if let Some(environment) = &environment
        && !is_sentry_environment(environment)
    {
        return Err(ConfigError::InvalidSentryEnvironment(environment.clone()));
    }
    let traces_sample_rate = match value("SENTRY_TRACES_SAMPLE_RATE") {
        None => 1.0,
        Some(raw) => match raw.parse::<f32>() {
            Ok(rate) if (0.0..=1.0).contains(&rate) => rate,
            _ => return Err(ConfigError::InvalidSentryTracesSampleRate(raw)),
        },
    };
    let Some(dsn) = value("SENTRY_DSN") else {
        return Ok(None);
    };
    let dsn = dsn
        .parse::<sentry::types::Dsn>()
        .map_err(|_| ConfigError::InvalidSentryDsn)?;
    Ok(Some(SentryConfig {
        dsn,
        environment: environment.unwrap_or_else(|| default_sentry_environment().to_owned()),
        traces_sample_rate,
    }))
}

/// Sentry's environment names: one short word without spaces or slashes.
fn is_sentry_environment(value: &str) -> bool {
    value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// One yt-dlp `--extractor-args` value: an extractor key, a colon, then
/// arguments such as `skip=hls,dash;player_client=web`. It cannot start
/// with `-` or hold spaces.
fn is_extractor_args(item: &str) -> bool {
    let Some((key, arguments)) = item.split_once(':') else {
        return false;
    };
    key.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        && !arguments.is_empty()
        && arguments
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_=,;.:-".contains(c))
}

/// The site a known yt-dlp search prefix searches, else "External".
fn search_prefix_name(prefix: &str) -> &'static str {
    SEARCH_PREFIX_NAMES
        .iter()
        .find(|(known, _)| *known == prefix)
        .map_or(FALLBACK_EXTERNAL_SOURCE_NAME, |(_, name)| name)
}

/// `^[a-z][a-z0-9]{0,30}search$`, the shape of yt-dlp's search prefixes.
fn is_search_prefix(value: &str) -> bool {
    let Some(name) = value.strip_suffix("search") else {
        return false;
    };
    let mut characters = name.chars();
    name.len() <= 31
        && characters
            .next()
            .is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// The server has no authentication yet, so it only binds to loopback.
pub fn validate_server_address(address: &str) -> Result<(), ConfigError> {
    check_loopback(address).map_err(|problem| match problem {
        AddressProblem::Invalid(reason) => ConfigError::InvalidAddress(address.to_owned(), reason),
        AddressProblem::NotLoopback => ConfigError::NotLoopback(address.to_owned()),
    })
}

enum AddressProblem {
    Invalid(String),
    NotLoopback,
}

fn check_loopback(address: &str) -> Result<(), AddressProblem> {
    let invalid = |reason: &str| AddressProblem::Invalid(reason.to_owned());
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
        _ => Err(AddressProblem::NotLoopback),
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
        assert_eq!(config.external_source, None);
        assert_eq!(config.sentry, None);
        // Writes through the dev and preview proxies keep the browser's
        // Origin, which the origin check accepts only from this list.
        for origin in ["http://localhost:3000", "http://localhost:4173"] {
            assert!(config.cors_origins.iter().any(|o| o == origin), "{origin}");
        }
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
    fn external_source_is_on_only_with_a_search_prefix() {
        assert_eq!(
            config(&[("EXTERNAL_SOURCE_SEARCH", " scsearch ")])
                .expect("config")
                .external_source,
            Some(ExternalSourceConfig {
                search_prefix: "scsearch".to_owned(),
                yt_dlp: "yt-dlp".to_owned(),
                extractor_args: Vec::new(),
                name: "SoundCloud".to_owned(),
            })
        );
        assert_eq!(
            config(&[
                ("EXTERNAL_SOURCE_SEARCH", "ytsearch"),
                ("YT_DLP", "/opt/yt-dlp/bin/yt-dlp")
            ])
            .expect("config")
            .external_source
            .map(|source| source.yt_dlp),
            Some("/opt/yt-dlp/bin/yt-dlp".to_owned())
        );
        assert_eq!(
            config(&[("EXTERNAL_SOURCE_SEARCH", "  "), ("YT_DLP", "yt-dlp")])
                .expect("config")
                .external_source,
            None
        );
        for invalid in [
            "search",
            "Scsearch",
            "scsearch:",
            "sc search",
            "1search",
            "--execsearch",
            "scsearch10",
            &format!("a{}search", "b".repeat(31)),
        ] {
            assert_eq!(
                config(&[("EXTERNAL_SOURCE_SEARCH", invalid)]).err(),
                Some(ConfigError::InvalidExternalSourceSearch(invalid.to_owned())),
                "{invalid}"
            );
        }
        assert!(
            config(&[(
                "EXTERNAL_SOURCE_SEARCH",
                &format!("a{}search", "b".repeat(30))
            )])
            .is_ok()
        );
    }

    #[test]
    fn external_source_name_comes_from_the_prefix_unless_the_operator_names_it() {
        let name = |pairs: &[(&str, &str)]| {
            config(pairs).map(|config| config.external_source.expect("on").name)
        };
        for (prefix, expected) in [
            ("ytsearch", "YouTube"),
            ("scsearch", "SoundCloud"),
            ("bilisearch", "BiliBili"),
            ("nicosearch", "Niconico"),
            ("gvsearch", "Google Video"),
            ("yvsearch", "Yahoo Video"),
            ("rkfnsearch", "Rokfin"),
            ("iasearch", "External"),
            ("ytmsearch", "External"),
        ] {
            assert_eq!(
                name(&[("EXTERNAL_SOURCE_SEARCH", prefix)]),
                Ok(expected.to_owned()),
                "{prefix}"
            );
        }
        assert_eq!(
            name(&[
                ("EXTERNAL_SOURCE_SEARCH", "ytsearch"),
                ("EXTERNAL_SOURCE_NAME", "  My Archive  ")
            ]),
            Ok("My Archive".to_owned())
        );
        assert_eq!(
            name(&[
                ("EXTERNAL_SOURCE_SEARCH", "iasearch"),
                ("EXTERNAL_SOURCE_NAME", "  ")
            ]),
            Ok("External".to_owned())
        );
        let longest = "é".repeat(40);
        assert_eq!(
            name(&[
                ("EXTERNAL_SOURCE_SEARCH", "scsearch"),
                ("EXTERNAL_SOURCE_NAME", &longest)
            ]),
            Ok(longest.clone())
        );
        for invalid in [
            format!("{longest}x"),
            "Sound\nCloud".to_owned(),
            "a\u{7}".to_owned(),
        ] {
            assert_eq!(
                config(&[
                    ("EXTERNAL_SOURCE_SEARCH", "scsearch"),
                    ("EXTERNAL_SOURCE_NAME", &invalid)
                ])
                .err(),
                Some(ConfigError::InvalidExternalSourceName(invalid.clone())),
                "{invalid:?}"
            );
        }
    }

    #[test]
    fn extractor_args_are_split_on_whitespace_and_checked() {
        let extractor_args = |value: &str| {
            config(&[
                ("EXTERNAL_SOURCE_SEARCH", "ytsearch"),
                ("EXTERNAL_SOURCE_EXTRACTOR_ARGS", value),
            ])
            .map(|config| config.external_source.expect("on").extractor_args)
        };
        assert_eq!(extractor_args("  "), Ok(Vec::new()));
        assert_eq!(
            extractor_args(" youtube:skip=hls,dash "),
            Ok(vec!["youtube:skip=hls,dash".to_owned()])
        );
        assert_eq!(
            extractor_args("youtube:skip=hls,dash;player_client=web\tsoundcloud:formats=http_mp3"),
            Ok(vec![
                "youtube:skip=hls,dash;player_client=web".to_owned(),
                "soundcloud:formats=http_mp3".to_owned(),
            ])
        );
        for invalid in [
            "skip=hls",
            "youtube:",
            ":skip=hls",
            "-youtube:skip=hls",
            "--exec:rm",
            "YouTube:skip=hls",
            "youtube:skip=$(id)",
            "youtube:skip=a/b",
        ] {
            assert_eq!(
                extractor_args(invalid),
                Err(ConfigError::InvalidExternalSourceExtractorArgs(
                    invalid.to_owned()
                )),
                "{invalid}"
            );
        }
        // Checked at startup even while the External Source is off.
        assert_eq!(
            config(&[("EXTERNAL_SOURCE_EXTRACTOR_ARGS", "-x")]).err(),
            Some(ConfigError::InvalidExternalSourceExtractorArgs(
                "-x".to_owned()
            ))
        );
    }

    #[test]
    fn sentry_is_off_without_a_dsn_and_never_has_a_default_one() {
        let off = config(&[]).expect("defaults");
        assert_eq!(off.sentry, None);
        assert_eq!(
            config(&[("SENTRY_DSN", "  "), ("SENTRY_ENVIRONMENT", "staging")])
                .expect("config")
                .sentry,
            None
        );
        let on = config(&[("SENTRY_DSN", " https://public@sentry.example/42 ")])
            .expect("config")
            .sentry
            .expect("on");
        assert_eq!(on.dsn.project_id().to_string(), "42");
        assert_eq!(on.environment, default_sentry_environment());
        assert_eq!(on.traces_sample_rate, 1.0);
        // The key stays out of debug output.
        assert!(!format!("{on:?}").contains("public"));
        let tuned = config(&[
            ("SENTRY_DSN", "https://public@sentry.example/42"),
            ("SENTRY_ENVIRONMENT", "production"),
            ("SENTRY_TRACES_SAMPLE_RATE", "0.25"),
        ])
        .expect("config")
        .sentry
        .expect("on");
        assert_eq!(tuned.environment, "production");
        assert_eq!(tuned.traces_sample_rate, 0.25);
    }

    #[test]
    fn sentry_settings_are_checked_even_while_it_is_off() {
        for invalid in ["not a dsn", "https://sentry.example/42", "sentry.example"] {
            let error = config(&[("SENTRY_DSN", invalid)]).err();
            assert_eq!(error, Some(ConfigError::InvalidSentryDsn), "{invalid}");
            // The message never repeats the value.
            assert!(!error.expect("error").to_string().contains(invalid));
        }
        for invalid in ["prod uction", "a/b", "é", &"a".repeat(65)] {
            assert_eq!(
                config(&[("SENTRY_ENVIRONMENT", invalid)]).err(),
                Some(ConfigError::InvalidSentryEnvironment(invalid.to_owned())),
                "{invalid}"
            );
        }
        for invalid in ["1.5", "-0.1", "half", "NaN", "inf"] {
            assert_eq!(
                config(&[("SENTRY_TRACES_SAMPLE_RATE", invalid)]).err(),
                Some(ConfigError::InvalidSentryTracesSampleRate(
                    invalid.to_owned()
                )),
                "{invalid}"
            );
        }
        for valid in ["0", "1", "0.05"] {
            assert!(config(&[("SENTRY_TRACES_SAMPLE_RATE", valid)]).is_ok());
        }
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

    fn guest_config(
        guest_addr: &str,
        public_url: &str,
    ) -> Result<Option<ListenTogetherConfig>, ConfigError> {
        config(&[("GUEST_ADDR", guest_addr), ("PUBLIC_URL", public_url)])
            .map(|config| config.listen_together)
    }

    #[test]
    fn listen_together_is_off_without_both_settings() {
        let off = config(&[("GUEST_ADDR", " "), ("PUBLIC_URL", "")]).expect("config");
        assert_eq!(off.listen_together, None);
        assert_eq!(off.listen_together_max_guests, 8);
        assert_eq!(
            config(&[("GUEST_ADDR", "127.0.0.1:8091")]).err(),
            Some(ConfigError::IncompleteListenTogether("PUBLIC_URL"))
        );
        assert_eq!(
            config(&[("PUBLIC_URL", "https://music.example.com")]).err(),
            Some(ConfigError::IncompleteListenTogether("GUEST_ADDR"))
        );
    }

    #[test]
    fn guest_address_must_be_loopback_and_not_the_server_address() {
        let url = "https://music.example.com";
        for address in ["127.0.0.1:8091", "localhost:8091", "[::1]:8090"] {
            assert!(guest_config(address, url).expect("valid").is_some());
        }
        for address in ["0.0.0.0:8091", "192.168.1.2:8091", "example.com:80"] {
            assert_eq!(
                guest_config(address, url),
                Err(ConfigError::GuestAddressNotLoopback(address.to_owned()))
            );
        }
        for address in ["nope", "127.0.0.1:port"] {
            assert!(matches!(
                guest_config(address, url),
                Err(ConfigError::InvalidGuestAddress(..))
            ));
        }
        for address in ["127.0.0.1:8090", "localhost:8090"] {
            assert_eq!(
                guest_config(address, url),
                Err(ConfigError::GuestAddressIsServerAddress(address.to_owned()))
            );
        }
    }

    #[test]
    fn public_url_is_an_origin_over_https_unless_on_loopback() {
        let origin = |url: &str| {
            guest_config("127.0.0.1:8091", url)
                .map(|config| config.expect("on").public_origin)
                .map_err(|_| ())
        };
        for (url, expected) in [
            ("https://music.example.com", "https://music.example.com"),
            ("https://music.example.com/", "https://music.example.com"),
            (
                "HTTPS://Music.Example.com:8443",
                "https://music.example.com:8443",
            ),
            ("http://localhost:8091", "http://localhost:8091"),
            ("http://127.0.0.1:8091", "http://127.0.0.1:8091"),
            ("http://[::1]:8091", "http://[::1]:8091"),
        ] {
            assert_eq!(origin(url), Ok(expected.to_owned()), "{url}");
        }
        for url in [
            "music.example.com",
            "/join",
            "ftp://music.example.com",
            "http://music.example.com",
            "http://192.168.1.2:8091",
            "https://music.example.com/music",
            "https://music.example.com/?a=1",
            "https://music.example.com/#a",
            "https://user:secret@music.example.com",
        ] {
            assert_eq!(origin(url), Err(()), "{url}");
        }
    }

    #[test]
    fn a_public_url_error_never_repeats_credentials() {
        let error = guest_config("127.0.0.1:8091", "https://user:secret@music.example.com")
            .expect_err("refused");
        assert!(!error.to_string().contains("secret"));
    }

    #[test]
    fn accepts_only_the_public_host() {
        let config = guest_config("127.0.0.1:8091", "https://music.example.com")
            .expect("valid")
            .expect("on");
        assert!(config.secure);
        for host in [
            "music.example.com",
            "MUSIC.example.com",
            "music.example.com:443",
        ] {
            assert!(config.accepts_host(host), "{host}");
        }
        for host in [
            "music.example.com:8443",
            "evil.example",
            "music.example.com.evil.example",
            "127.0.0.1:8091",
            "",
        ] {
            assert!(!config.accepts_host(host), "{host}");
        }
        let local = guest_config("127.0.0.1:8091", "http://localhost:8091")
            .expect("valid")
            .expect("on");
        assert!(!local.secure);
        assert!(local.accepts_host("localhost:8091"));
        assert!(!local.accepts_host("localhost"));
    }

    #[test]
    fn max_guests_is_one_to_eight() {
        let limit = |value: &str| {
            config(&[("LISTEN_TOGETHER_MAX_GUESTS", value)])
                .map(|config| config.listen_together_max_guests)
        };
        assert_eq!(limit("1"), Ok(1));
        assert_eq!(limit(" 8 "), Ok(8));
        assert_eq!(limit(""), Ok(8));
        for invalid in ["0", "9", "-1", "many", "2.5"] {
            assert_eq!(
                limit(invalid),
                Err(ConfigError::InvalidMaxGuests(invalid.to_owned())),
                "{invalid}"
            );
        }
    }
}
