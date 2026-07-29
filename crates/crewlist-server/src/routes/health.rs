//! `GET /health`. SPEC.md §6.2.
//!
//! Real, not stubbed: it is the one thing worth having before any handler
//! works, because "is the backend up" is the first question anything asks.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use crewlist_core::dto::{ComponentHealth, HealthResponse};

use crate::state::AppState;

pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    let postgres = match state.stores.pg.ping().await {
        Ok(()) => ComponentHealth::ok(),
        Err(e) => ComponentHealth::failed(e.to_string()),
    };

    let mongo = match state.stores.mongo.ping().await {
        Ok(()) => ComponentHealth::ok(),
        Err(e) => ComponentHealth::failed(e.to_string()),
    };

    let body = HealthResponse {
        server: ComponentHealth::ok_with_version(env!("CARGO_PKG_VERSION")),
        postgres,
        mongo,
    };

    // 503 when a store is down, so the CLI's status mapping alone gets it to
    // exit 5 without special-casing this route. AC-64.
    let status = if body.all_ok() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status, Json(body))
}
