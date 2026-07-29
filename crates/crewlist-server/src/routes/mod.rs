//! Route table.
//!
//! Internal and unversioned — SPEC.md §6.6. These paths may change in any
//! release; nothing outside this repository should depend on them, and the
//! skill document must never teach them. The stable contracts are the CLI's
//! arguments, its `--json` output, and its exit codes.

pub mod health;
pub mod tasks;

use axum::routing::{get, post};
use axum::Router;
use tower_http::trace::TraceLayer;

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health))
        .route("/tasks", post(tasks::create).get(tasks::list))
        .route("/tasks/{id}", get(tasks::get).delete(tasks::delete))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
