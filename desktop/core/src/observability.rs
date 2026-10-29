//! Opt-in reporting to the owner's own Sentry (ADR 0033). The core reads
//! `SENTRY_DSN`, `SENTRY_ENVIRONMENT` and `SENTRY_TRACES_SAMPLE_RATE` from
//! the environment the Electron shell starts it with; without `SENTRY_DSN`
//! nothing starts and every call below is a no-op.
//!
//! What is sent: panics, the core failing, the mpv process failing, and
//! each Playback Source load as a transaction and metrics named by its
//! source type and what started it. What is never sent: media URLs (the
//! media proxy's carry a token), file paths, Track titles or the host's
//! name; [`scrub_text`] replaces URLs and paths in free text. The scrubbing
//! follows the Music Server's (server/src/observability.rs).

use crate::playback_load_timing::{LoadOutcome, LoadReport};
use sentry::protocol::{Context, Event, SpanId, SpanStatus, Unit, Value};
use sentry::{Hub, Level, TransactionContext};
use std::borrow::Cow;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// The name every event carries instead of the computer's name.
const SERVER_NAME: &str = "desktop-core";
const URL: &str = "<url>";
const PATH: &str = "<path>";

/// Where and how the core reports to the owner's Sentry.
#[derive(Clone, PartialEq)]
pub struct SentryConfig {
    pub dsn: sentry::types::Dsn,
    pub environment: String,
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

/// Reads the settings through `lookup`. Sentry is on only with a DSN; the
/// environment defaults to "development" for a debug build and
/// "production" for a release build, the sample rate to 1.0.
pub fn config_from_lookup(
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Option<SentryConfig>, String> {
    let value = |key: &str| {
        lookup(key)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    let environment = value("SENTRY_ENVIRONMENT");
    if let Some(environment) = &environment
        && !(environment.len() <= 64
            && environment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
    {
        return Err(format!(
            "SENTRY_ENVIRONMENT {environment:?} must be 1 to 64 letters, digits, '.', '_' or '-'"
        ));
    }
    let traces_sample_rate = match value("SENTRY_TRACES_SAMPLE_RATE") {
        None => 1.0,
        Some(raw) => match raw.parse::<f32>() {
            Ok(rate) if (0.0..=1.0).contains(&rate) => rate,
            _ => {
                return Err(format!(
                    "SENTRY_TRACES_SAMPLE_RATE {raw:?} must be a number from 0 to 1"
                ));
            }
        },
    };
    let Some(dsn) = value("SENTRY_DSN") else {
        return Ok(None);
    };
    // The DSN holds the project's key, so the message does not repeat it.
    let dsn = dsn.parse().map_err(|_| {
        "SENTRY_DSN must be a Sentry DSN such as https://<public-key>@<your-sentry-host>/<project-id>"
            .to_owned()
    })?;
    let default_environment = if cfg!(debug_assertions) {
        "development"
    } else {
        "production"
    };
    Ok(Some(SentryConfig {
        dsn,
        environment: environment.unwrap_or_else(|| default_environment.to_owned()),
        traces_sample_rate,
    }))
}

pub fn client_options(config: &SentryConfig) -> sentry::ClientOptions {
    let mut options = sentry::ClientOptions::new()
        .release(concat!("earthly-audio-core@", env!("CARGO_PKG_VERSION")))
        .environment(config.environment.clone())
        .traces_sample_rate(config.traces_sample_rate)
        .send_default_pii(false)
        .server_name(SERVER_NAME)
        .before_send(|event| Some(scrub_event(event)));
    options.dsn = Some(config.dsn.clone());
    options
}

/// Starts the client from the process environment when `SENTRY_DSN` is
/// set. Keep the guard until exit: dropping it sends what is still queued.
pub fn init_from_env() -> Result<Option<sentry::ClientInitGuard>, String> {
    Ok(config_from_lookup(|key| std::env::var(key).ok())?
        .map(|config| sentry::init(client_options(&config))))
}

fn enabled() -> bool {
    Hub::current().client().is_some()
}

/// The core stopped with `error`. Paths under `config_directory`, such as
/// the Processing Profile settings, are replaced before the text is
/// scrubbed: the directory's name can hold the user's name with spaces.
pub fn core_failed(error: &str, config_directory: &Path) {
    if enabled() {
        sentry::capture_message(&core_failure(error, config_directory), Level::Error);
    }
}

fn core_failure(error: &str, config_directory: &Path) -> String {
    let directory = config_directory.display().to_string();
    // A bare separator in its place, so the scrubber takes the rest of the
    // path with it.
    let error = if directory.is_empty() {
        Cow::Borrowed(error)
    } else {
        Cow::Owned(error.replace(&directory, "/"))
    };
    format!("Earthly Audio core failed: {}", scrub_text(&error))
}

/// The mpv process failed or could not be recovered; one issue however
/// its detail reads.
pub(crate) fn playback_process_failed(detail: &str) {
    if !enabled() {
        return;
    }
    sentry::with_scope(
        |scope| scope.set_fingerprint(Some(&["native-playback-process-failed"])),
        || {
            sentry::capture_message(
                &format!("Native playback process failed: {}", scrub_text(detail)),
                Level::Error,
            )
        },
    );
}

/// One Playback Source load: a transaction from `loadfile` to its outcome
/// with `file-loaded` as a child span, duration metrics, and an error event
/// when it failed. It names the source type and trigger, never the source.
pub(crate) fn playback_load(report: &LoadReport) {
    if !enabled() {
        return;
    }
    let source_type = report.source_type;
    let trigger = report.trigger.name();
    let outcome = report.outcome.name();
    let start = SystemTime::now()
        .checked_sub(report.elapsed)
        .unwrap_or_else(SystemTime::now);
    let transaction = sentry::start_transaction_with_timestamp(
        TransactionContext::new(&format!("playback.load {source_type}"), "playback.load"),
        start,
    );
    transaction.set_tag("source_type", source_type);
    transaction.set_tag("start", trigger);
    transaction.set_tag("outcome", outcome);
    transaction.set_data("load_ms", milliseconds(report.elapsed).into());
    if let Some(file_loaded) = report.file_loaded {
        transaction
            .start_child_with_details(
                "playback.file_loaded",
                "loadfile to file-loaded",
                SpanId::default(),
                start,
            )
            .finish_with_timestamp(start + file_loaded);
    }
    transaction.set_status(match report.outcome {
        LoadOutcome::Started => SpanStatus::Ok,
        LoadOutcome::Failed => SpanStatus::InternalError,
        LoadOutcome::Replaced => SpanStatus::Cancelled,
    });
    transaction.finish_with_timestamp(start + report.elapsed);

    let attributes = [
        ("source_type", source_type),
        ("start", trigger),
        ("outcome", outcome),
    ];
    distribution_ms("playback.load.duration", report.elapsed, &attributes);
    if let Some(file_loaded) = report.file_loaded {
        distribution_ms("playback.load.file_loaded", file_loaded, &attributes);
    }
    if report.outcome == LoadOutcome::Failed {
        sentry::with_scope(
            |scope| {
                scope.set_tag("source_type", source_type);
                scope.set_tag("start", trigger);
            },
            || sentry::capture_message("Playback Source failed to start", Level::Error),
        );
    }
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn distribution_ms(name: &'static str, duration: Duration, attributes: &[(&'static str, &str)]) {
    let mut metric =
        sentry::metrics::distribution(name, milliseconds(duration)).unit(Unit::Millisecond);
    for (key, value) in attributes {
        metric = metric.attribute(*key, *value);
    }
    metric.capture();
}

fn scrub_event(mut event: Event<'static>) -> Event<'static> {
    event.request = None;
    event.user = None;
    event.server_name = Some(Cow::Borrowed(SERVER_NAME));
    scrub_option(&mut event.message);
    if let Some(entry) = &mut event.logentry {
        scrub_string(&mut entry.message);
        entry.params.iter_mut().for_each(scrub_value);
    }
    for exception in &mut event.exception.values {
        scrub_option(&mut exception.value);
    }
    for breadcrumb in &mut event.breadcrumbs.values {
        scrub_option(&mut breadcrumb.message);
        breadcrumb.data.values_mut().for_each(scrub_value);
    }
    event.extra.values_mut().for_each(scrub_value);
    for value in event.tags.values_mut() {
        scrub_string(value);
    }
    for context in event.contexts.values_mut() {
        if let Context::Other(fields) = context {
            fields.values_mut().for_each(scrub_value);
        }
    }
    event
}

fn scrub_value(value: &mut Value) {
    match value {
        Value::String(text) => scrub_string(text),
        Value::Array(items) => items.iter_mut().for_each(scrub_value),
        Value::Object(fields) => fields.values_mut().for_each(scrub_value),
        _ => {}
    }
}

fn scrub_option(text: &mut Option<String>) {
    if let Some(text) = text {
        scrub_string(text);
    }
}

fn scrub_string(text: &mut String) {
    if let Cow::Owned(scrubbed) = scrub_text(text) {
        *text = scrubbed;
    }
}

/// `text` with each URL replaced by `<url>` and each file path by
/// `<path>`. A file path is any word holding a `/` or `\`, together with
/// the words after it up to the last such word, since a path can hold
/// spaces (`C:\Users\Jane Mary Smith\...`), and then the words after that
/// up to the end of the clause, since a file name can too
/// (`Jane Smith - Song.flac`). Prose after a path may be lost with it.
pub fn scrub_text(text: &str) -> Cow<'_, str> {
    if !text.contains(['/', '\\']) {
        return Cow::Borrowed(text);
    }
    let rest = text.trim_start();
    let mut scrubbed = String::with_capacity(text.len());
    scrubbed.push_str(&text[..text.len() - rest.len()]);
    let words = words(rest);
    let mut index = 0;
    while index < words.len() {
        let (word, space) = words[index];
        let kind = Word::of(word);
        let Word::Path { prefix, lead } = kind else {
            scrubbed.push_str(&kind.scrubbed(word));
            scrubbed.push_str(space);
            index += 1;
            continue;
        };
        // The path runs on to the last word holding a separator before a
        // URL, which is not part of it.
        let mut last = index;
        for (next, (word, _)) in words.iter().enumerate().skip(index + 1) {
            match Word::of(word) {
                Word::Path { .. } => last = next,
                Word::Url { .. } => break,
                Word::Plain => {}
            }
        }
        // A file name after the last separator can hold spaces as well, so
        // the path also takes the plain words up to the end of the clause: a
        // word ending in `:`, `;`, `,` or a closing bracket or quote, or the
        // end of the text.
        if !ends_clause(words[last].0) {
            for (next, (word, _)) in words.iter().enumerate().skip(last + 1) {
                if !matches!(Word::of(word), Word::Plain) {
                    break;
                }
                last = next;
                if ends_clause(word) {
                    break;
                }
            }
        }
        let (word, space) = words[last];
        scrubbed.push_str(&format!("{prefix}{lead}{PATH}{}", suffix(word)));
        scrubbed.push_str(space);
        index = last + 1;
    }
    if scrubbed == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(scrubbed)
    }
}

/// `text`'s words, each with the whitespace after it.
fn words(text: &str) -> Vec<(&str, &str)> {
    let mut words = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let word_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let (word, after) = rest.split_at(word_end);
        let space_end = after
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(after.len());
        words.push((word, &after[..space_end]));
        rest = &after[space_end..];
    }
    words
}

/// A word's opening punctuation and what follows it, without its closing
/// punctuation.
fn trim(word: &str) -> (&str, &str) {
    const OPENING: &[char] = &['(', '[', '{', '<', '"', '\'', '`'];
    const CLOSING: &[char] = &[')', ']', '}', '>', '"', '\'', '`', ',', '.', ';', ':', '!'];
    let core = word.trim_start_matches(OPENING);
    let prefix = &word[..word.len() - core.len()];
    (prefix, core.trim_end_matches(CLOSING))
}

/// Whether `word` closes a clause, ending a path that runs through it.
fn ends_clause(word: &str) -> bool {
    word.ends_with([
        ':', ';', ',', ')', ']', '}', '"', '\'', '\u{2019}', '\u{201d}',
    ])
}

/// A word's closing punctuation.
fn suffix(word: &str) -> &str {
    let (prefix, core) = trim(word);
    &word[prefix.len() + core.len()..]
}

/// What one word of free text is.
enum Word<'a> {
    Plain,
    /// `before` is what precedes the URL's scheme in the word.
    Url {
        prefix: &'a str,
        before: &'a str,
    },
    /// `lead` is a `key=` before the path, which stays.
    Path {
        prefix: &'a str,
        lead: &'a str,
    },
}

impl<'a> Word<'a> {
    fn of(word: &'a str) -> Self {
        if !word.contains(['/', '\\']) {
            return Self::Plain;
        }
        let (prefix, core) = trim(word);
        if let Some(scheme) = core.find("://") {
            // The scheme's own letters go with the URL; `char_indices`
            // keeps the cut on a character boundary.
            let start = core[..scheme]
                .char_indices()
                .rev()
                .find(|(_, c)| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')))
                .map_or(0, |(index, c)| index + c.len_utf8());
            return Self::Url {
                prefix,
                before: &core[..start],
            };
        }
        let path_start = core.find(['/', '\\', '~']).unwrap_or(0);
        let lead = &core[..path_start];
        // `name=/x/y` or `C:\x`: what precedes a path stays only if it is a key.
        let lead = if lead.ends_with('=') { lead } else { "" };
        Self::Path { prefix, lead }
    }

    /// `word` scrubbed, when it stands alone.
    fn scrubbed(&self, word: &'a str) -> Cow<'a, str> {
        let suffix = suffix(word);
        match self {
            Self::Plain => Cow::Borrowed(word),
            Self::Url { prefix, before } => Cow::Owned(format!("{prefix}{before}{URL}{suffix}")),
            Self::Path { prefix, lead } => Cow::Owned(format!("{prefix}{lead}{PATH}{suffix}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback_load_timing::LoadTrigger;
    use sentry::protocol::EnvelopeItem;
    use std::collections::HashMap;

    fn config(pairs: &[(&str, &str)]) -> Result<Option<SentryConfig>, String> {
        let map: HashMap<&str, &str> = pairs.iter().copied().collect();
        config_from_lookup(|key| map.get(key).map(|value| (*value).to_owned()))
    }

    fn options() -> sentry::ClientOptions {
        client_options(
            &config(&[
                ("SENTRY_DSN", "https://public@sentry.example/7"),
                ("SENTRY_ENVIRONMENT", "test"),
            ])
            .expect("config")
            .expect("on"),
        )
    }

    #[test]
    fn nothing_starts_without_a_dsn() {
        assert_eq!(config(&[]), Ok(None));
        assert_eq!(config(&[("SENTRY_DSN", " ")]), Ok(None));
        assert!(Hub::current().client().is_none());
        // Without a client these are no-ops.
        core_failed("x", Path::new("/x"));
        playback_process_failed("x");
        let on = config(&[("SENTRY_DSN", "https://public@sentry.example/7")])
            .expect("config")
            .expect("on");
        assert_eq!(on.traces_sample_rate, 1.0);
        assert!(!format!("{on:?}").contains("public"));
    }

    #[test]
    fn settings_are_checked() {
        let error = config(&[("SENTRY_DSN", "not a dsn")]).expect_err("invalid");
        assert!(!error.contains("not a dsn"));
        assert!(config(&[("SENTRY_ENVIRONMENT", "a b")]).is_err());
        assert!(config(&[("SENTRY_TRACES_SAMPLE_RATE", "2")]).is_err());
        assert!(config(&[("SENTRY_TRACES_SAMPLE_RATE", "0.5")]).is_ok());
    }

    #[test]
    fn scrubs_urls_and_paths() {
        assert_eq!(
            scrub_text("mpv could not open http://127.0.0.1:41234/media/abc?token=t"),
            "mpv could not open <url>"
        );
        assert_eq!(
            scrub_text(r"spawn C:\Users\me\AppData\mpv.exe failed: denied"),
            "spawn <path>: denied"
        );
        assert_eq!(scrub_text("no paths"), Cow::Borrowed("no paths"));
    }

    #[test]
    fn non_ascii_before_a_url_does_not_panic() {
        for (text, scrubbed) in [
            ("‘https://cdn.example/a.mp3’", "‘<url>"),
            ("url：https://cdn.example/a.mp3", "url：<url>"),
            ("取得https://cdn.example/a.mp3", "取得<url>"),
            ("échec«https://cdn.example/a.mp3»", "échec«<url>"),
            ("Ünïcödé https://cdn.example/a.mp3 ok", "Ünïcödé <url> ok"),
        ] {
            assert_eq!(scrub_text(text), scrubbed, "{text}");
        }
    }

    #[test]
    fn scrubbing_any_text_does_not_panic() {
        let pieces = [
            "‘", "’", "：", "取得", "é", "🎵", "\u{200b}", "\u{301}", "://", "/", "\\", "~", "=",
            "C:", "x", " ", "\t", "(", ")", ".", "<", ">", "?", "#",
        ];
        // Every run of three pieces, then each with a URL after it.
        for a in pieces {
            for b in pieces {
                for c in pieces {
                    let text = format!("{a}{b}{c}");
                    scrub_text(&text);
                    let text = format!("{text}https://cdn.example/a.mp3");
                    assert!(!scrub_text(&text).contains("cdn.example"), "{text}");
                }
            }
        }
    }

    #[test]
    fn paths_with_spaces_are_redacted_whole() {
        assert_eq!(
            scrub_text(r"spawn C:\Program Files\Jane Mary Smith\mpv.exe failed: denied"),
            "spawn <path>: denied"
        );
        assert_eq!(
            scrub_text("read /home/jane/Music/Jane Mary Smith/a.flac failed"),
            "read <path>"
        );
        // A file name with spaces after the last separator goes with it, up
        // to the end of the clause.
        assert_eq!(
            scrub_text("load /music/Jane Smith - Song Title.flac failed"),
            "load <path>"
        );
        assert_eq!(
            scrub_text("open /music/Jane Smith - Song Title.flac: denied, retrying"),
            "open <path>: denied, retrying"
        );
        // Prose with a slash: over-redacting it is acceptable.
        assert_eq!(scrub_text("the a/b ratio is high"), "the <path>");
        assert_eq!(scrub_text("a/b and c/d"), "<path>");
        assert_eq!(
            scrub_text("open /srv/a b/c.flac for https://a.example/x"),
            "open <path> <url>"
        );
    }

    #[test]
    fn a_core_failure_names_no_part_of_the_config_directory() {
        let directory = Path::new(r"C:\Users\Jane Mary Smith\AppData\Roaming\Earthly Audio");
        let error = format!(
            "failed to read {}\\processing-profile.json: denied",
            directory.display()
        );
        assert_eq!(
            core_failure(&error, directory),
            "Earthly Audio core failed: failed to read <path>: denied"
        );
        // A trailing name part with no separator after it is still caught.
        let error = format!("cannot create {}", directory.display());
        assert_eq!(
            core_failure(&error, directory),
            "Earthly Audio core failed: cannot create <path>"
        );
    }

    #[test]
    fn a_failed_load_sends_a_transaction_and_an_event_without_its_source() {
        let envelopes = sentry::test::with_captured_envelopes_options(
            || {
                playback_load(&LoadReport {
                    outcome: LoadOutcome::Failed,
                    source_type: "external-track",
                    trigger: LoadTrigger::Play,
                    file_loaded: Some(Duration::from_millis(40)),
                    elapsed: Duration::from_millis(120),
                });
                playback_process_failed(
                    "mpv exited reading http://127.0.0.1:41234/media/secret-token from /home/me/Music/a.flac",
                );
            },
            options(),
        );
        let items: Vec<_> = envelopes
            .iter()
            .flat_map(|envelope| envelope.items().cloned().collect::<Vec<_>>())
            .collect();
        let mut sent = String::new();
        for envelope in &envelopes {
            let mut bytes = Vec::new();
            envelope.to_writer(&mut bytes).expect("serialize");
            sent.push_str(&String::from_utf8_lossy(&bytes));
        }
        for secret in ["secret-token", "127.0.0.1", "/home/me"] {
            assert!(!sent.contains(secret), "{secret} was sent");
        }

        let transaction = items
            .iter()
            .find_map(|item| match item {
                EnvelopeItem::Transaction(transaction) => Some(transaction),
                _ => None,
            })
            .expect("transaction");
        assert_eq!(
            transaction.name.as_deref(),
            Some("playback.load external-track")
        );
        assert_eq!(
            transaction.release.as_deref(),
            Some(concat!("earthly-audio-core@", env!("CARGO_PKG_VERSION")))
        );
        assert_eq!(
            transaction.tags.get("start").map(String::as_str),
            Some("play")
        );
        assert_eq!(transaction.spans.len(), 1);
        assert_eq!(
            transaction.spans[0].op.as_deref(),
            Some("playback.file_loaded")
        );

        let events: Vec<_> = items
            .iter()
            .filter_map(|item| match item {
                EnvelopeItem::Event(event) => Some(event),
                _ => None,
            })
            .collect();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].message.as_deref(),
            Some("Playback Source failed to start")
        );
        assert_eq!(
            events[0].tags.get("source_type").map(String::as_str),
            Some("external-track")
        );
        assert_eq!(
            events[1].message.as_deref(),
            Some("Native playback process failed: mpv exited reading <url> from <path>")
        );
        assert_eq!(events[1].server_name.as_deref(), Some(SERVER_NAME));
        assert_eq!(
            events[1].fingerprint,
            vec![Cow::Borrowed("native-playback-process-failed")]
        );
    }
}
