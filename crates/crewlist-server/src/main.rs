//! CrewList server.
//!
//! Owns Postgres and Mongo so the CLI does not have to. Migrates on boot and
//! refuses to listen if that fails, rather than accepting traffic against a
//! half-built schema. SPEC.md §2.1, AC-61.

mod config;
mod error;
mod extract;
mod repo;
mod routes;
mod state;
#[cfg(test)]
mod testkit;

use std::sync::Arc;

use anyhow::Context;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crewlist_store::Stores;

use crate::config::Config;
use crate::repo::StoreRepo;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Before tracing, so RUST_LOG can come from `.env` too. Absence is normal:
    // under Docker the environment is supplied by Compose.
    let _ = dotenvy::dotenv();

    init_tracing();

    let config = Config::from_env()?;

    let stores = Stores::connect(&config.postgres_url, &config.mongo_url)
        .await
        .context("connecting to stores")?;

    // Before binding, deliberately. A server that cannot migrate is worse than
    // a server that is down, because it looks healthy.
    stores
        .initialize()
        .await
        .context("initializing schema; refusing to serve")?;

    let app = routes::router(AppState::new(Arc::new(StoreRepo::new(stores))));

    let listener = TcpListener::bind(config.bind)
        .await
        .with_context(|| format!("binding {}", config.bind))?;

    tracing::info!(addr = %config.bind, "crewlist-server listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    Ok(())
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "crewlist_server=info,crewlist_store=info,tower_http=info".into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}

/// Resolves on SIGINT or SIGTERM.
///
/// SIGTERM is the one that matters: it is what `docker compose down` and
/// `restart` send. Listening for SIGINT alone means graceful shutdown never
/// runs under Docker — the container sits until the 10s timeout and is then
/// SIGKILLed, dropping in-flight requests on every restart.
async fn shutdown_signal() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(e) => {
                // Never resolve, rather than shutting down spuriously.
                tracing::error!(error = %e, "cannot listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = interrupt => tracing::info!("received SIGINT, shutting down"),
        _ = terminate => tracing::info!("received SIGTERM, shutting down"),
    }
}

#[cfg(all(test, unix))]
mod shutdown_tests {
    use super::*;
    use std::time::Duration;

    /// Sends a real SIGTERM to this process, because the bug being fixed was
    /// precisely that the signal was never observed — asserting on anything
    /// less would not have caught it.
    ///
    /// If the handler fails to install, the signal's default disposition kills
    /// the test binary. That is a loud failure, not a silent pass.
    #[tokio::test]
    async fn shutdown_signal_resolves_on_sigterm() {
        let waiting = tokio::spawn(shutdown_signal());

        // The handler is installed inside the spawned task; raising before it
        // is registered would terminate the process.
        tokio::time::sleep(Duration::from_millis(250)).await;

        let status = std::process::Command::new("kill")
            .args(["-TERM", &std::process::id().to_string()])
            .status()
            .expect("send SIGTERM");
        assert!(status.success());

        tokio::time::timeout(Duration::from_secs(5), waiting)
            .await
            .expect("shutdown_signal ignored SIGTERM")
            .expect("shutdown task panicked");
    }
}
