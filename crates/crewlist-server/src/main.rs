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

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}
