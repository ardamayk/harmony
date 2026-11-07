//! HTTP for client metrics: only known names, so a client cannot invent
//! Sentry series, and nothing but a number and which client sent it. And
//! the clients' errors: their Sentry SDK's envelopes, of which only error
//! events go on, through this server's own Sentry client (ADR 0043).

use crate::app::AppState;
use crate::observability;
use crate::respond::ApiError;
use axum::body::Bytes;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use sentry::Hub;
use sentry::protocol::{Envelope, EnvelopeError, EnvelopeItem};
use serde::Deserialize;

const MAX_METRICS: usize = 32;
/// Anything longer than this is not a measurement.
const MAX_VALUE: f64 = 600_000.0;
/// A page's error with its breadcrumbs is a few KiB.
const MAX_ERROR_ENVELOPE: usize = 256 * 1024;

/// The names a client may report, with `true` for a duration in milliseconds.
const KNOWN_METRICS: &[(&str, bool)] = &[
    ("client.page.ttfb", true),
    ("client.page.fcp", true),
    ("client.page.lcp", true),
    ("client.page.cls", false),
    ("client.interaction.inp", true),
    ("client.playback.start", true),
    ("client.playback.takeover", true),
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientMetrics {
    client: String,
    metrics: Vec<ClientMetric>,
}

#[derive(Deserialize)]
struct ClientMetric {
    name: String,
    value: f64,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/client-metrics", post(report))
        .route("/api/v1/client-errors", post(report_errors))
}

fn client_label(client: &str) -> Option<&'static str> {
    match client {
        "web" => Some("web"),
        "desktop" => Some("desktop"),
        _ => None,
    }
}

async fn report(Json(body): Json<ClientMetrics>) -> Result<StatusCode, ApiError> {
    let client = client_label(&body.client).ok_or_else(|| {
        ApiError::bad_request("client must be web or desktop").with_field("client")
    })?;
    if body.metrics.len() > MAX_METRICS {
        return Err(ApiError::bad_request("too many metrics").with_field("metrics"));
    }
    for metric in &body.metrics {
        let known = KNOWN_METRICS.iter().find(|(name, _)| *name == metric.name);
        // An unknown name is a newer client, not a fault.
        let Some((name, is_duration)) = known else {
            continue;
        };
        if metric.value.is_finite() && (0.0..=MAX_VALUE).contains(&metric.value) {
            observability::client_metric(name, metric.value, *is_duration, client);
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn report_errors(body: Bytes) -> Result<StatusCode, ApiError> {
    if body.len() > MAX_ERROR_ENVELOPE {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "error report is too large",
        ));
    }
    forward_errors(&body).map_err(|_| ApiError::bad_request("body must be a Sentry envelope"))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Sends the envelope's error events with this server's client and DSN, so
/// `before_send` scrubs them like the server's own; the envelope's headers,
/// its DSN among them, are not used, nor the client's environment. Sessions, transactions, attachments and
/// anything else are dropped. Without `SENTRY_DSN` nothing is sent.
fn forward_errors(body: &[u8]) -> Result<usize, EnvelopeError> {
    let envelope = Envelope::from_slice(body)?;
    let Some(client) = Hub::current().client() else {
        return Ok(0);
    };
    let mut sent = 0;
    for item in envelope.into_items() {
        if let EnvelopeItem::Event(mut event) = item {
            // The SDK says "production"; the deployment's own name wins.
            event.environment = None;
            client.capture_event(*event, None);
            sent += 1;
        }
    }
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SentryConfig;

    #[test]
    fn only_error_events_go_on_and_are_scrubbed() {
        let config = SentryConfig {
            dsn: "https://public@sentry.example/1".parse().expect("dsn"),
            environment: "test".to_owned(),
            traces_sample_rate: 1.0,
        };
        let body = concat!(
            r#"{"event_id":"9ec79c33ec9942ab8353589fcb2e04dc","dsn":"https://key@elsewhere.example/9"}"#,
            "\n",
            r#"{"type":"event"}"#,
            "\n",
            r#"{"event_id":"9ec79c33ec9942ab8353589fcb2e04dc","platform":"javascript","environment":"production","message":"failed GET /api/v1/library/albums/abc123","request":{"url":"https://music.example/albums/abc123?q=x"},"tags":{"client":"web"}}"#,
            "\n",
            r#"{"type":"attachment","length":5,"filename":"a.txt"}"#,
            "\n",
            "hello\n",
        );
        let events = sentry::test::with_captured_events_options(
            || assert_eq!(forward_errors(body.as_bytes()).ok(), Some(1)),
            crate::observability::client_options(&config, "9.9.9"),
        );
        let [event] = events.as_slice() else {
            panic!("one event, got {}", events.len());
        };
        assert_eq!(event.platform, "javascript");
        assert_eq!(event.environment.as_deref(), Some("test"));
        assert_eq!(event.tags.get("client").map(String::as_str), Some("web"));
        assert_eq!(event.request, None);
        assert_eq!(
            event.message.as_deref(),
            Some("failed GET /api/v1/library/albums/<id>")
        );
    }

    #[test]
    fn anything_but_an_envelope_is_refused_and_nothing_goes_without_sentry() {
        assert!(forward_errors(b"not an envelope").is_err());
        let empty = br#"{"event_id":"9ec79c33ec9942ab8353589fcb2e04dc"}"#;
        assert_eq!(forward_errors(empty).ok(), Some(0));
    }

    #[test]
    fn only_the_two_clients_are_labels() {
        assert_eq!(client_label("web"), Some("web"));
        assert_eq!(client_label("desktop"), Some("desktop"));
        assert_eq!(client_label("anything else"), None);
    }
}
