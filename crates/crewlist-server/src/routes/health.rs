//! `GET /health`. SPEC.md §6.2.
//!
//! Always 200, even when a store is down.
//!
//! The alternative — 503 on degradation — would be the one non-2xx response in
//! the API whose body is not `{"error":{…}}`, and the client would have to
//! special-case it in exactly the situation where things are already broken.
//! Keeping "non-2xx implies an error object" exceptionless is worth more than
//! expressing degradation in the status line, especially since the status line
//! cannot say *which* store failed and the body can (AC-64).
//!
//! Exit status is the client's job: `crewlist health` reads `all_ok()` and
//! exits 5 when it is false. §6.2's contract is unchanged.

use axum::extract::State;
use axum::Json;
use crewlist_core::dto::HealthResponse;

use crate::state::AppState;

pub async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(state.repo.health().await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{all_ok_health, get_req, send, FakeRepo};
    use axum::http::StatusCode;
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
    async fn health_names_a_failed_postgres() {
        let repo = FakeRepo::new().with_health(HealthResponse {
            postgres: ComponentHealth::failed("connection refused"),
            ..all_ok_health()
        });

        let (status, body) = send(repo, get_req("/health")).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["postgres"]["ok"], false);
        assert_eq!(body["postgres"]["message"], "connection refused");
        assert_eq!(body["mongo"]["ok"], true);
    }

    #[tokio::test]
    async fn health_names_a_failed_mongo() {
        let repo = FakeRepo::new().with_health(HealthResponse {
            mongo: ComponentHealth::failed("no primary"),
            ..all_ok_health()
        });

        let (status, body) = send(repo, get_req("/health")).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["mongo"]["message"], "no primary");
    }

    /// The client decides the exit code from the body, so a degraded response
    /// must stay parseable as a `HealthResponse` rather than becoming an error
    /// object. This is the whole reason the route does not return 503.
    #[tokio::test]
    async fn degraded_health_is_still_a_health_body_not_an_error() {
        let repo = FakeRepo::new().with_health(HealthResponse {
            postgres: ComponentHealth::failed("down"),
            ..all_ok_health()
        });

        let (status, body) = send(repo, get_req("/health")).await;

        assert_eq!(status, StatusCode::OK);
        assert!(body.get("error").is_none(), "must not be an error body");

        let parsed: HealthResponse = serde_json::from_value(body).expect("parses as health");
        assert!(!parsed.all_ok(), "client sees a failure and exits 5");
    }

    /// The healthy case must not carry a `message` field at all, so "is there
    /// a message" is a usable signal rather than "is the message empty".
    #[tokio::test]
    async fn health_omits_message_when_ok() {
        let (_, body) = send(FakeRepo::new(), get_req("/health")).await;
        assert!(body["postgres"].get("message").is_none());
    }
}
