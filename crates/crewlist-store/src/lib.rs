//! Postgres and Mongo access for CrewList.
//!
//! Server-side only. If `crewlist-cli` ever gains a dependency on this crate,
//! database drivers are back in the client and AC-65 fails — that edge is the
//! architectural invariant of the whole design. SPEC.md §8.

pub mod mongo;
pub mod pg;

pub use mongo::MongoStore;
pub use pg::PgStore;

use crewlist_core::CrewError;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("cannot connect to {store}: {source}")]
    Connect {
        store: &'static str,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("cannot initialize {store}: {source}")]
    Init {
        store: &'static str,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("migration failed: {0}")]
    Migrate(Box<dyn std::error::Error + Send + Sync>),

    #[error("{store} query failed: {source}")]
    Query {
        store: &'static str,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl From<StoreError> for CrewError {
    /// Everything here is a backend problem, which the CLI surfaces as exit 5.
    /// SPEC.md §6.5.
    fn from(err: StoreError) -> Self {
        CrewError::Storage(err.to_string())
    }
}

/// Both stores, opened and ready.
#[derive(Clone)]
pub struct Stores {
    pub pg: PgStore,
    pub mongo: MongoStore,
}

impl Stores {
    pub async fn connect(postgres_url: &str, mongo_url: &str) -> Result<Self, StoreError> {
        let pg = PgStore::connect(postgres_url).await?;
        let mongo = MongoStore::connect(mongo_url).await?;
        Ok(Self { pg, mongo })
    }

    /// Migrate Postgres and install the Mongo validator. The server calls this
    /// before binding, and exits non-zero if it fails. AC-61.
    pub async fn initialize(&self) -> Result<(), StoreError> {
        self.pg.migrate().await?;
        self.mongo.initialize().await?;
        Ok(())
    }
}
