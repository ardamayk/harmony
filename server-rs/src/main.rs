use earthly_music_server::app::{self, AppState};
use earthly_music_server::config::{self, Config};
use earthly_music_server::{db, dependencies};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // One JSON object per line on stdout, like the Go server's slog.
    tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_current_span(false)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    if let Err(error) = run().await {
        tracing::error!(%error, "server failed");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let config = Config::from_env().map_err(|error| format!("configuration failed: {error}"))?;
    config::validate_server_address(&config.addr)
        .map_err(|error| format!("unsafe server address: {error}"))?;
    let address = config::bind_address(&config.addr)
        .ok_or_else(|| format!("unsafe server address: {}", config.addr))?;

    // Run from server/ (like the Go server) or from the repository root.
    let mut migrations = PathBuf::from("migrations");
    if !migrations.exists() {
        migrations = PathBuf::from("server/migrations");
    }
    let pool = db::open_and_migrate(&config.database_path, &migrations)
        .await
        .map_err(|error| format!("database init failed: {error}"))?;

    let report = dependencies::probe_system().await;
    for dependency in &report.0 {
        if dependency.available {
            tracing::info!(name = %dependency.name, required = dependency.required, version = %dependency.version, "server dependency found");
        } else if dependency.required {
            tracing::error!(name = %dependency.name, required = dependency.required, "required server dependency missing");
        } else {
            tracing::warn!(name = %dependency.name, "optional server dependency missing; dependent features stay disabled");
        }
    }

    let state = AppState::new(config, report, pool.clone());
    let router = app::router(state.clone());
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|error| format!("server error: listen tcp {address}: {error}"))?;
    tracing::info!(addr = %state.config.addr, "server listening");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|error| format!("server error: {error}"))?;
    pool.close().await;
    Ok(())
}

async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut terminate = signal(SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}
