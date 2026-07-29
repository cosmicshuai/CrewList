//! `GET /health`. SPEC.md §6.2.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use crewlist_core::dto::HealthResponse;

use crate::state::AppState;

pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    let body = state.repo.health().await;

    // 503 when a store is down, so the CLI's generic status mapping alone
    // reaches exit 5 without special-casing this route. AC-64.
    let status = if body.all_ok() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status, Json(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{all_ok_health, get_req, send, FakeRepo};
    use crewlist_core::dto::ComponentHealth;

    #[tokio::test]
    async fn health_is_200_when_everything_is_up() {
        let (status, body) = send(FakeRepo::new(), get_req("/health")).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["server"]["ok"], true);
        assert_eq!(body["postgres"]["ok"], true);
        assert_eq!(body["mongo"]["ok"], true);
    }

    #[tokio::test]
    async fn health_reports_the_server_version() {
        let (_, body) = send(FakeRepo::new(), get_req("/health")).await;
        assert!(body["server"]["version"].is_string());
    }

    /// AC-64: the response has to say *which* store failed, or the operator
    /// learns only that something is wrong.
    #[tokio::test]
    async fn health_is_503_and_names_a_failed_postgres() {
        let repo = FakeRepo::new().with_health(HealthResponse {
            postgres: ComponentHealth::failed("connection refused"),
            ..all_ok_health()
        });

        let (status, body) = send(repo, get_req("/health")).await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["postgres"]["ok"], false);
        assert_eq!(body["postgres"]["message"], "connection refused");
        assert_eq!(body["mongo"]["ok"], true);
    }

    #[tokio::test]
    async fn health_is_503_and_names_a_failed_mongo() {
        let repo = FakeRepo::new().with_health(HealthResponse {
            mongo: ComponentHealth::failed("no primary"),
            ..all_ok_health()
        });

        let (status, body) = send(repo, get_req("/health")).await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["mongo"]["message"], "no primary");
    }

    /// The healthy case must not carry a `message` field at all, so "is there
    /// a message" is a usable signal rather than "is the message empty".
    #[tokio::test]
    async fn health_omits_message_when_ok() {
        let (_, body) = send(FakeRepo::new(), get_req("/health")).await;
        assert!(body["postgres"].get("message").is_none());
    }
}
