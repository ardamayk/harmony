//! HTTP for client metrics: only known names, so a client cannot invent
//! Sentry series, and nothing but a number and which client sent it.

use crate::app::AppState;
use crate::observability;
use crate::respond::ApiError;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;

const MAX_METRICS: usize = 32;
/// Anything longer than this is not a measurement.
const MAX_VALUE: f64 = 600_000.0;

/// The names a client may report, with `true` for a duration in milliseconds.
const KNOWN_METRICS: &[(&str, bool)] = &[
    ("client.page.ttfb", true),
    ("client.page.fcp", true),
    ("client.page.lcp", true),
    ("client.page.cls", false),
    ("client.interaction.inp", true),
    ("client.playback.start", true),
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
    Router::new().route("/api/v1/client-metrics", post(report))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_two_clients_are_labels() {
        assert_eq!(client_label("web"), Some("web"));
        assert_eq!(client_label("desktop"), Some("desktop"));
        assert_eq!(client_label("anything else"), None);
    }
}
