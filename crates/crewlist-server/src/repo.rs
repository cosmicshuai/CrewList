//! The port between routes and storage.
//!
//! Handlers depend on this trait, not on `crewlist-store`. That is what makes
//! the HTTP layer testable without a database: tests substitute an in-memory
//! fake and drive the real router.
//!
//! [`StoreRepo`] is the production adapter. Its task operations are
//! deliberately unimplemented — the domain behavior in SPEC.md §3 is driven in
//! by tests, not guessed at here. Only `health` is real, because "is the
//! backend up" is the first question anything asks.

use std::sync::Arc;

use async_trait::async_trait;
use crewlist_core::dto::{
    ComponentHealth, CreateTaskRequest, CreatedTask, DeletedTask, HealthResponse, ListQuery,
    TaskListResponse, TaskView,
};
use crewlist_core::{CrewError, TaskId};
use crewlist_store::Stores;

/// Handlers hold this, so a test can swap the implementation wholesale.
pub type SharedRepo = Arc<dyn TaskRepo>;

#[async_trait]
pub trait TaskRepo: Send + Sync + 'static {
    async fn create(&self, req: CreateTaskRequest) -> Result<CreatedTask, CrewError>;
    async fn get(&self, id: TaskId) -> Result<TaskView, CrewError>;
    async fn list(&self, query: ListQuery) -> Result<TaskListResponse, CrewError>;
    async fn delete(&self, id: TaskId, force: bool) -> Result<DeletedTask, CrewError>;
    async fn health(&self) -> HealthResponse;
}

/// Postgres + Mongo, per SPEC.md §5.3.
#[derive(Clone)]
pub struct StoreRepo {
    stores: Stores,
}

impl StoreRepo {
    pub fn new(stores: Stores) -> Self {
        Self { stores }
    }
}

#[async_trait]
impl TaskRepo for StoreRepo {
    /// Drives: AC-5 … AC-15 (validation, defaults, detail linkage),
    /// AC-30 … AC-34 (child creation, two-level limit).
    async fn create(&self, _req: CreateTaskRequest) -> Result<CreatedTask, CrewError> {
        Err(CrewError::Unimplemented("create task"))
    }

    /// Drives: AC-25 (defaulted detail, never null), AC-44 (a missing Mongo
    /// document is not an error).
    async fn get(&self, _id: TaskId) -> Result<TaskView, CrewError> {
        Err(CrewError::Unimplemented("get task"))
    }

    /// Drives: AC-16 … AC-21 (agent queue filter), AC-41 … AC-43 (human list
    /// ordering and the done-parent-with-open-children case).
    async fn list(&self, _query: ListQuery) -> Result<TaskListResponse, CrewError> {
        Err(CrewError::Unimplemented("list tasks"))
    }

    /// Drives: AC-46 (parent needs force), AC-47 (cascade), and the delete half
    /// of §5.3 — Postgres first, then best-effort Mongo cleanup.
    async fn delete(&self, _id: TaskId, _force: bool) -> Result<DeletedTask, CrewError> {
        Err(CrewError::Unimplemented("delete task"))
    }

    async fn health(&self) -> HealthResponse {
        let postgres = match self.stores.pg.ping().await {
            Ok(()) => ComponentHealth::ok(),
            Err(e) => ComponentHealth::failed(e.to_string()),
        };

        let mongo = match self.stores.mongo.ping().await {
            Ok(()) => ComponentHealth::ok(),
            Err(e) => ComponentHealth::failed(e.to_string()),
        };

        HealthResponse {
            server: ComponentHealth::ok_with_version(env!("CARGO_PKG_VERSION")),
            postgres,
            mongo,
        }
    }
}
