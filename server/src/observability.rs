//! Opt-in reporting to the owner's own Sentry (ADR 0033). Nothing here
//! starts without `SENTRY_DSN`: no client, no transport, no tracing layer,
//! and every call below is then a no-op.
//!
//! What is sent: errors and panics, `info` and `warn` lines as Sentry logs,
//! one transaction per request named by its route template, child spans
//! and metrics for the External Source's timings. What is never sent:
//! request URLs, query strings, search queries, ids in paths, headers,
//! client addresses, and URLs or file paths in free text, which
//! [`scrub_text`] replaces before anything leaves the process.

use crate::config::SentryConfig;
use axum::extract::{MatchedPath, Request};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use sentry::protocol::{Context, Event, Log, LogAttribute, SpanId, SpanStatus, Unit, Value};
use sentry::{Hub, SentryFutureExt, TransactionContext, TransactionOrSpan};
use std::borrow::Cow;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// The name every event carries instead of the host's name.
const SERVER_NAME: &str = "music-server";

/// What replaces a URL or a file path in text sent to Sentry.
const URL: &str = "<url>";
const PATH: &str = "<path>";
const REDACTED: &str = "<redacted>";

/// The client options for `config`; tests swap in a test transport.
pub fn client_options(config: &SentryConfig, version: &str) -> sentry::ClientOptions {
    let mut options = sentry::ClientOptions::new()
        .release(format!("earthly-music-server@{version}"))
        .environment(config.environment.clone())
        .traces_sample_rate(config.traces_sample_rate)
        .send_default_pii(false)
        .server_name(SERVER_NAME)
        .max_request_body_size(sentry::MaxRequestBodySize::None)
        .before_send(|event| Some(scrub_event(event)))
        .before_send_log(|log| Some(scrub_log(log)));
    options.dsn = Some(config.dsn.clone());
    options
}

/// Starts the client when Sentry is configured. Keep the guard until exit:
/// dropping it sends what is still queued.
pub fn init(config: Option<&SentryConfig>, version: &str) -> Option<sentry::ClientInitGuard> {
    config.map(|config| sentry::init(client_options(config, version)))
}

/// The tracing layer that turns `error` lines into Sentry events and
/// `info` and `warn` lines into Sentry logs and breadcrumbs. Only this
/// crate's own spans become Sentry spans; a dependency's span fields could
/// hold URLs.
pub fn tracing_layer<S>() -> sentry::integrations::tracing::SentryLayer<S>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    sentry::integrations::tracing::layer()
        .span_filter(|metadata| metadata.target().starts_with("earthly_music_server"))
}

/// One transaction per request, named `<method> <route template>`, with
/// the request's own hub so concurrent requests keep apart. It holds the
/// method, the route template and the status: never the URL, its query or
/// path ids, headers or the client's address. A stream's transaction ends
/// when its response starts.
pub async fn http_transaction(request: Request, next: Next) -> Response {
    let current = Hub::current();
    if current.client().is_none() {
        return next.run(request).await;
    }
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("fallback", MatchedPath::as_str)
        .to_owned();
    let method = request.method().as_str().to_owned();
    let hub = Arc::new(Hub::new_from_top(current));
    let transaction = hub.start_transaction(TransactionContext::new(
        &format!("{method} {route}"),
        "http.server",
    ));
    transaction.set_origin("auto.http.earthly");
    transaction.set_data("http.request.method", method.into());
    transaction.set_data("http.route", route.clone().into());
    let transaction = TransactionOrSpan::from(transaction);
    hub.configure_scope(|scope| {
        scope.set_span(Some(transaction.clone()));
        scope.set_tag("http.route", &route);
    });
    let mut finish = Finish(Some(transaction));
    let response = next.run(request).bind_hub(hub).await;
    finish.respond(response.status());
    response
}

/// Finishes the request's transaction, as aborted when the client went away
/// before the response.
struct Finish(Option<TransactionOrSpan>);

impl Finish {
    fn respond(&mut self, status: StatusCode) {
        if let Some(transaction) = self.0.take() {
            transaction.set_data("http.response.status_code", status.as_u16().into());
            transaction.set_status(span_status(status));
            transaction.finish();
        }
    }
}

impl Drop for Finish {
    fn drop(&mut self) {
        if let Some(transaction) = self.0.take() {
            transaction.set_status(SpanStatus::Aborted);
            transaction.finish();
        }
    }
}

fn span_status(status: StatusCode) -> SpanStatus {
    match status {
        StatusCode::NOT_FOUND => SpanStatus::NotFound,
        StatusCode::FORBIDDEN => SpanStatus::PermissionDenied,
        StatusCode::TOO_MANY_REQUESTS => SpanStatus::ResourceExhausted,
        StatusCode::GATEWAY_TIMEOUT => SpanStatus::DeadlineExceeded,
        StatusCode::SERVICE_UNAVAILABLE => SpanStatus::Unavailable,
        status if status.is_client_error() => SpanStatus::InvalidArgument,
        status if status.is_server_error() => SpanStatus::InternalError,
        _ => SpanStatus::Ok,
    }
}

/// One step of timed work, from `offset` after its parent started.
pub struct Step {
    pub op: &'static str,
    pub offset: Duration,
    pub duration: Duration,
}

/// Steps that ran one after the other, skipping those not reached.
pub fn sequential(steps: &[(&'static str, Option<Duration>)]) -> Vec<Step> {
    let mut offset = Duration::ZERO;
    steps
        .iter()
        .filter_map(|(op, duration)| {
            let duration = (*duration)?;
            let step = Step {
                op,
                offset,
                duration,
            };
            offset += duration;
            Some(step)
        })
        .collect()
}

/// Adds work timed elsewhere (`started`, `total`) to the current request's
/// transaction as a span `op` with `steps` as its children. Nothing happens
/// outside a sampled request.
pub fn record_span(
    started: Instant,
    op: &'static str,
    total: Duration,
    data: &[(&'static str, Value)],
    steps: &[Step],
) {
    let Some(parent) = sentry::configure_scope(|scope| scope.get_span()) else {
        return;
    };
    let start = SystemTime::now()
        .checked_sub(started.elapsed())
        .unwrap_or_else(SystemTime::now);
    let span =
        TransactionOrSpan::from(parent.start_child_with_details(op, op, SpanId::default(), start));
    for (key, value) in data {
        span.set_data(key, value.clone());
    }
    for step in steps {
        let step_start = start + step.offset;
        span.start_child_with_details(step.op, step.op, SpanId::default(), step_start)
            .finish_with_timestamp(step_start + step.duration);
    }
    span.finish_with_timestamp(start + total);
}

/// A duration metric in milliseconds, tied to the current trace.
pub fn distribution_ms(
    name: &'static str,
    duration: Duration,
    attributes: &[(&'static str, &str)],
) {
    distribution(
        name,
        duration.as_secs_f64() * 1000.0,
        Unit::Millisecond,
        attributes,
    );
}

pub fn distribution(
    name: &'static str,
    value: f64,
    unit: Unit,
    attributes: &[(&'static str, &str)],
) {
    if Hub::current().client().is_none() {
        return;
    }
    let mut metric = sentry::metrics::distribution(name, value).unit(unit);
    for (key, value) in attributes {
        metric = metric.attribute(*key, *value);
    }
    metric.capture();
}

pub fn count(name: &'static str, attributes: &[(&'static str, &str)]) {
    if Hub::current().client().is_none() {
        return;
    }
    let mut metric = sentry::metrics::counter(name, 1);
    for (key, value) in attributes {
        metric = metric.attribute(*key, *value);
    }
    metric.capture();
}

/// A log line or event from the External Source, whose `error` text can
/// repeat yt-dlp's output and with it a search query or a media id.
fn is_external_source(module: Option<&str>) -> bool {
    module.is_some_and(|module| module.contains("::external_source"))
}

fn scrub_event(mut event: Event<'static>) -> Event<'static> {
    event.request = None;
    event.user = None;
    event.server_name = Some(Cow::Borrowed(SERVER_NAME));
    let external = is_external_source(event.logger.as_deref());
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
        let external = is_external_source(breadcrumb.category.as_deref());
        scrub_fields(breadcrumb.data.iter_mut(), external);
    }
    scrub_fields(event.extra.iter_mut(), external);
    for value in event.tags.values_mut() {
        scrub_string(value);
    }
    for context in event.contexts.values_mut() {
        if let Context::Other(fields) = context {
            scrub_fields(fields.iter_mut(), external);
        }
    }
    event
}

fn scrub_log(mut log: Log) -> Log {
    scrub_string(&mut log.body);
    let external = is_external_source(
        log.attributes
            .get("code.module.name")
            .and_then(|LogAttribute(value)| value.as_str()),
    );
    for (key, LogAttribute(value)) in &mut log.attributes {
        scrub_field(key, value, external);
    }
    log
}

fn scrub_fields<'a>(fields: impl Iterator<Item = (&'a String, &'a mut Value)>, external: bool) {
    for (key, value) in fields {
        scrub_field(key, value, external);
    }
}

fn scrub_field(key: &str, value: &mut Value, external: bool) {
    if external && key == "error" {
        *value = Value::from(REDACTED);
    } else {
        scrub_value(value);
    }
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

/// `text` with each URL replaced by `<url>` and each file path (any word
/// holding a `/` or `\`) by `<path>`. An API path keeps its route but
/// loses its query.
pub fn scrub_text(text: &str) -> Cow<'_, str> {
    if !text.contains(['/', '\\']) {
        return Cow::Borrowed(text);
    }
    let mut scrubbed = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let word_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let (word, after) = rest.split_at(word_end);
        scrubbed.push_str(&scrub_word(word));
        let space_end = after
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(after.len());
        scrubbed.push_str(&after[..space_end]);
        rest = &after[space_end..];
    }
    if scrubbed == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(scrubbed)
    }
}

fn scrub_word(word: &str) -> Cow<'_, str> {
    if !word.contains(['/', '\\']) {
        return Cow::Borrowed(word);
    }
    const OPENING: &[char] = &['(', '[', '{', '<', '"', '\'', '`'];
    const CLOSING: &[char] = &[')', ']', '}', '>', '"', '\'', '`', ',', '.', ';', ':', '!'];
    let core = word.trim_start_matches(OPENING);
    let prefix = &word[..word.len() - core.len()];
    let core = core.trim_end_matches(CLOSING);
    let suffix = &word[prefix.len() + core.len()..];
    if let Some(scheme) = core.find("://") {
        let start = core[..scheme]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')))
            .map_or(0, |index| index + 1);
        return Cow::Owned(format!("{prefix}{}{URL}{suffix}", &core[..start]));
    }
    let path_start = core.find(['/', '\\', '~']).unwrap_or(0);
    let (lead, path) = core.split_at(path_start);
    // `name=/x/y` or `C:\x`: what precedes a path stays only if it is a key.
    let lead = if lead.ends_with('=') { lead } else { "" };
    if let Some(api) = path.strip_prefix("/api/") {
        let route = api.split(['?', '#']).next().unwrap_or_default();
        if !route.contains('\\') {
            return Cow::Owned(format!("{prefix}{lead}/api/{route}{suffix}"));
        }
    }
    Cow::Owned(format!("{prefix}{lead}{PATH}{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::middleware;
    use axum::routing::get;
    use sentry::protocol::EnvelopeItem;
    use tower::ServiceExt;
    use tracing_subscriber::layer::SubscriberExt;

    fn options() -> sentry::ClientOptions {
        let config = SentryConfig {
            dsn: "https://public@sentry.example/1".parse().expect("dsn"),
            environment: "test".to_owned(),
            traces_sample_rate: 1.0,
        };
        client_options(&config, "9.9.9")
    }

    #[test]
    fn nothing_starts_without_a_dsn() {
        assert!(init(None, "0.1.0").is_none());
        assert!(Hub::current().client().is_none());
        // Without a client these are no-ops.
        count("test.count", &[]);
        record_span(Instant::now(), "test", Duration::ZERO, &[], &[]);
    }

    #[test]
    fn scrubs_urls_paths_and_queries_from_text() {
        assert_eq!(scrub_text("no paths here"), Cow::Borrowed("no paths here"));
        assert_eq!(
            scrub_text("error sending request for url (https://cdn.example/a.mp3?sig=1)"),
            "error sending request for url (<url>)"
        );
        assert_eq!(
            scrub_text("open /home/me/Music/Artist/a.flac: denied"),
            "open <path>: denied"
        );
        assert_eq!(scrub_text("path=/srv/music/a.flac."), "path=<path>.");
        assert_eq!(scrub_text(r"C:\Users\me\Music\a.flac"), "<path>");
        assert_eq!(scrub_text("~/Music/a b/c.flac"), "<path> <path>");
        assert_eq!(
            scrub_text("GET /api/v1/library/tracks/7/stream?token=x failed"),
            "GET /api/v1/library/tracks/7/stream failed"
        );
        assert_eq!(scrub_text("\"hls+https://a.example/x\""), "\"<url>\"");
        assert_eq!(scrub_text("url=https://a.example/x"), "url=<url>");
    }

    #[test]
    fn external_source_errors_are_redacted_and_other_fields_scrubbed() {
        let mut log = Log {
            level: sentry::protocol::LogLevel::Warn,
            body: "External Track not resolved".to_owned(),
            trace_id: None,
            timestamp: SystemTime::now(),
            severity_number: None,
            attributes: Default::default(),
        };
        log.attributes.insert(
            "code.module.name".to_owned(),
            "earthly_music_server::modules::external_source::service".into(),
        );
        log.attributes.insert(
            "error".to_owned(),
            "ERROR: [soundcloud:search] daft punk: not found".into(),
        );
        log.attributes
            .insert("file".to_owned(), "/srv/music/a.flac".into());
        let log = scrub_log(log);
        assert_eq!(log.attributes["error"].0, REDACTED);
        assert_eq!(log.attributes["file"].0, PATH);
    }

    /// A request through the transaction middleware and the tracing layer,
    /// as the server wires them, that logs an error.
    #[test]
    fn a_request_sends_its_route_template_and_a_scrubbed_error_only() {
        let router = Router::new()
            .route(
                "/api/v1/external/tracks/{external_track_id}/stream",
                get(|| async {
                    tracing::error!(
                        error = "upstream https://cdn.example/a.mp3?sig=secret-signature failed for /home/me/Music/a.flac",
                        "External Track stream failed"
                    );
                    StatusCode::BAD_GATEWAY
                }),
            )
            .layer(middleware::from_fn(http_transaction));
        let subscriber = tracing_subscriber::registry().with(tracing_layer());
        let envelopes = sentry::test::with_captured_envelopes_options(
            || {
                tracing::subscriber::with_default(subscriber, || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .build()
                        .expect("runtime");
                    let response = runtime.block_on(
                        router.oneshot(
                            axum::http::Request::get(
                                "/api/v1/external/tracks/secret-track-id/stream?q=secret-query",
                            )
                            .header("host", "127.0.0.1:8090")
                            .header("x-forwarded-for", "203.0.113.9")
                            .body(Body::empty())
                            .expect("request"),
                        ),
                    );
                    assert_eq!(
                        response.expect("response").status(),
                        StatusCode::BAD_GATEWAY
                    );
                });
            },
            options(),
        );

        let mut sent = String::new();
        let mut events = Vec::new();
        let mut transactions = Vec::new();
        for envelope in &envelopes {
            let mut bytes = Vec::new();
            envelope.to_writer(&mut bytes).expect("serialize");
            sent.push_str(&String::from_utf8_lossy(&bytes));
            for item in envelope.items() {
                match item {
                    EnvelopeItem::Event(event) => events.push(event.clone()),
                    EnvelopeItem::Transaction(transaction) => {
                        transactions.push(transaction.clone())
                    }
                    _ => {}
                }
            }
        }
        for secret in [
            "secret-track-id",
            "secret-query",
            "secret-signature",
            "cdn.example",
            "/home/me",
            "203.0.113.9",
            "127.0.0.1",
        ] {
            assert!(!sent.contains(secret), "{secret} was sent");
        }

        let [event] = events.as_slice() else {
            panic!("one event, got {}", events.len());
        };
        assert_eq!(
            event.message.as_deref(),
            Some("External Track stream failed")
        );
        assert_eq!(event.release.as_deref(), Some("earthly-music-server@9.9.9"));
        assert_eq!(event.environment.as_deref(), Some("test"));
        assert_eq!(event.server_name.as_deref(), Some(SERVER_NAME));
        assert!(event.request.is_none() && event.user.is_none());
        assert_eq!(
            event.tags.get("http.route").map(String::as_str),
            Some("/api/v1/external/tracks/{external_track_id}/stream")
        );
        assert!(sent.contains("upstream <url> failed for <path>"), "{sent}");

        let [transaction] = transactions.as_slice() else {
            panic!("one transaction, got {}", transactions.len());
        };
        assert_eq!(
            transaction.name.as_deref(),
            Some("GET /api/v1/external/tracks/{external_track_id}/stream")
        );
        assert!(transaction.request.is_none());
        let Some(Context::Trace(trace)) = transaction.contexts.get("trace") else {
            panic!("trace context");
        };
        assert_eq!(trace.op.as_deref(), Some("http.server"));
        assert_eq!(trace.status, Some(SpanStatus::InternalError));
        assert_eq!(
            trace.data.get("http.response.status_code"),
            Some(&Value::from(502))
        );
        // The error belongs to the request's trace.
        let Some(Context::Trace(event_trace)) = event.contexts.get("trace") else {
            panic!("event trace context");
        };
        assert_eq!(event_trace.trace_id, trace.trace_id);
    }

    #[test]
    fn spans_timed_elsewhere_join_the_request() {
        let envelopes = sentry::test::with_captured_envelopes_options(
            || {
                let transaction = sentry::start_transaction(TransactionContext::new(
                    "GET /api/v1/external/search",
                    "http.server",
                ));
                sentry::configure_scope(|scope| {
                    scope.set_span(Some(transaction.clone().into()));
                });
                let started = Instant::now();
                record_span(
                    started,
                    "external_source.search",
                    Duration::from_millis(30),
                    &[("outcome", "ok".into())],
                    &sequential(&[
                        ("queue", Some(Duration::from_millis(5))),
                        ("yt_dlp", Some(Duration::from_millis(20))),
                        ("parse", None),
                    ]),
                );
                transaction.finish();
            },
            options(),
        );
        let transaction = envelopes
            .iter()
            .flat_map(|envelope| envelope.items())
            .find_map(|item| match item {
                EnvelopeItem::Transaction(transaction) => Some(transaction.clone()),
                _ => None,
            })
            .expect("transaction");
        let ops: Vec<_> = transaction
            .spans
            .iter()
            .map(|span| span.op.clone().unwrap_or_default())
            .collect();
        assert_eq!(ops.len(), 3, "{ops:?}");
        for op in ["external_source.search", "queue", "yt_dlp"] {
            assert!(ops.iter().any(|candidate| candidate == op), "{ops:?}");
        }
        let parent = transaction
            .spans
            .iter()
            .find(|span| span.op.as_deref() == Some("external_source.search"))
            .expect("parent");
        assert_eq!(parent.data.get("outcome"), Some(&Value::from("ok")));
        let yt_dlp = transaction
            .spans
            .iter()
            .find(|span| span.op.as_deref() == Some("yt_dlp"))
            .expect("yt_dlp");
        assert_eq!(yt_dlp.parent_span_id, Some(parent.span_id));
    }
}
