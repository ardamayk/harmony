use earthly_music_server::app::{self, AppState};
use earthly_music_server::config::{self, Config};
use earthly_music_server::{db, dependencies, observability};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn main() {
    let config = Config::from_env().map_err(|error| format!("configuration failed: {error}"));
    // Sentry starts before the runtime and only with SENTRY_DSN (ADR 0033).
    let sentry = config
        .as_ref()
        .ok()
        .and_then(|config| observability::init(config.sentry.as_ref(), &config.version));
    // One JSON object per line on stdout, like the Go server's slog; with
    // Sentry, the same lines also go to it.
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .flatten_event(true)
                .with_current_span(false),
        )
        .with(sentry.is_some().then(observability::tracing_layer))
        .init();
    let result = config.and_then(|config| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("runtime failed: {error}"))?
            .block_on(run(config))
    });
    if let Err(error) = result {
        tracing::error!(%error, "server failed");
        // Sends what is queued before exiting.
        drop(sentry);
        std::process::exit(1);
    }
}

async fn run(config: Config) -> Result<(), String> {
    config::validate_server_address(&config.addr)
        .map_err(|error| format!("unsafe server address: {error}"))?;
    let address = config::bind_address(&config.addr)
        .ok_or_else(|| format!("unsafe server address: {}", config.addr))?;

    // Run from server/ or from the repository root.
    let mut migrations = PathBuf::from("migrations");
    if !migrations.exists() {
        migrations = PathBuf::from("server/migrations");
    }
    let pool = db::open_and_migrate(&config.database_path, &migrations)
        .await
        .map_err(|error| format!("database init failed: {error}"))?;

    let mut report = dependencies::probe_system().await;
    if let Some(external_source) = &config.external_source {
        report
            .0
            .push(dependencies::probe_yt_dlp(&external_source.yt_dlp).await);
    }
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
    if let Err(error) = state.managed_import.start().await {
        tracing::error!(%error, "Managed Import startup cleanup failed");
    }
    let router = app::router(state.clone());
    // Go's net.Listen asks for net.core.somaxconn; tokio's bind asks for 1024 and
    // overflowed at 512 concurrent clients. The kernel caps this at somaxconn.
    let listener = {
        let socket = if address.is_ipv4() {
            tokio::net::TcpSocket::new_v4()
        } else {
            tokio::net::TcpSocket::new_v6()
        };
        socket
            .and_then(|socket| {
                socket.set_reuseaddr(true)?;
                socket.bind(address)?;
                socket.listen(4096)
            })
            .map_err(|error| format!("server error: listen tcp {address}: {error}"))?
    };
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
