//! Postgres: identity, status, hierarchy.

use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

use crate::StoreError;

/// Embedded at compile time, so the binary carries its own schema and a
/// container needs no migration sidecar.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct PgStore {
    pool: PgPool,
}

impl PgStore {
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .acquire_timeout(Duration::from_secs(5))
            .connect(url)
            .await
            .map_err(|e| StoreError::Connect {
                store: "postgres",
                source: Box::new(e),
            })?;

        Ok(Self { pool })
    }

    /// Runs pending migrations. Idempotent — a restart against an initialized
    /// store is a no-op. AC-59.
    ///
    /// The server calls this before it begins listening and refuses to serve
    /// if it fails, rather than accepting traffic against a half-built schema.
    /// AC-61.
    pub async fn migrate(&self) -> Result<(), StoreError> {
        MIGRATOR
            .run(&self.pool)
            .await
            .map_err(|e| StoreError::Migrate(Box::new(e)))?;
        tracing::info!("postgres migrations applied");
        Ok(())
    }

    pub async fn ping(&self) -> Result<(), StoreError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Query {
                store: "postgres",
                source: Box::new(e),
            })?;
        Ok(())
    }

    /// Escape hatch for the query layer, which arrives with the handlers.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
