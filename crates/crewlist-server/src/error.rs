//! Domain error → HTTP status. The inverse mapping lives in the CLI.
//!
//! SPEC.md §6.5 fixes the round trip: a `CrewError` becomes a status here,
//! and the client turns that status back into the same error code and its
//! exit code. AC-58 pins every row of that table.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use crewlist_core::error::{CrewError, ErrorBody, ErrorCode};

pub struct ApiError(pub CrewError);

impl From<CrewError> for ApiError {
    fn from(err: CrewError) -> Self {
        Self(err)
    }
}

impl From<crewlist_store::StoreError> for ApiError {
    fn from(err: crewlist_store::StoreError) -> Self {
        Self(err.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = status_for(self.0.code());
        let body = ErrorBody::from(&self.0);

        if status.is_server_error() {
            tracing::error!(error = %self.0, "request failed");
        } else {
            tracing::debug!(error = %self.0, "request rejected");
        }

        (status, Json(body)).into_response()
    }
}

pub fn status_for(code: ErrorCode) -> StatusCode {
    match code {
        ErrorCode::Validation => StatusCode::BAD_REQUEST,
        ErrorCode::NotFound => StatusCode::NOT_FOUND,
        ErrorCode::IllegalTransition => StatusCode::CONFLICT,
        ErrorCode::Storage => StatusCode::SERVICE_UNAVAILABLE,
        ErrorCode::Unimplemented => StatusCode::NOT_IMPLEMENTED,
        // `Unreachable` is a client-side condition — the server never emits it.
        ErrorCode::Internal | ErrorCode::Unreachable => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crewlist_core::TaskStatus;

    /// The server half of AC-58. The CLI owns the inverse, and the two tables
    /// have to agree or an agent gets the wrong exit code.
    #[test]
    fn every_error_code_maps_to_its_specified_status() {
        let cases = [
            (ErrorCode::Validation, StatusCode::BAD_REQUEST),
            (ErrorCode::NotFound, StatusCode::NOT_FOUND),
            (ErrorCode::IllegalTransition, StatusCode::CONFLICT),
            (ErrorCode::Storage, StatusCode::SERVICE_UNAVAILABLE),
            (ErrorCode::Internal, StatusCode::INTERNAL_SERVER_ERROR),
            (ErrorCode::Unimplemented, StatusCode::NOT_IMPLEMENTED),
        ];

        for (code, expected) in cases {
            assert_eq!(status_for(code), expected, "{code} mapped wrongly");
        }
    }

    /// SPEC.md §6.5 fixes these strings because the skill branches on them.
    #[test]
    fn error_code_strings_are_stable() {
        assert_eq!(ErrorCode::NotFound.as_str(), "not_found");
        assert_eq!(ErrorCode::IllegalTransition.as_str(), "illegal_transition");
        assert_eq!(ErrorCode::Validation.as_str(), "validation");
        assert_eq!(ErrorCode::Storage.as_str(), "storage");
        assert_eq!(ErrorCode::Unreachable.as_str(), "unreachable");
        assert_eq!(ErrorCode::Internal.as_str(), "internal");
    }

    #[test]
    fn error_codes_carry_the_specified_exit_codes() {
        assert_eq!(ErrorCode::NotFound.exit_code(), 3);
        assert_eq!(ErrorCode::IllegalTransition.exit_code(), 4);
        assert_eq!(ErrorCode::Storage.exit_code(), 5);
        assert_eq!(ErrorCode::Unreachable.exit_code(), 5);
        assert_eq!(ErrorCode::Validation.exit_code(), 6);
        assert_eq!(ErrorCode::Internal.exit_code(), 1);
    }

    /// AC-4: a rejected transition has to name both states, or the operator
    /// cannot tell why it was rejected.
    #[test]
    fn illegal_transition_message_names_both_states() {
        let err = CrewError::IllegalTransition {
            id: 1,
            from: TaskStatus::Done,
            to: TaskStatus::HandedOff,
            action: "hand off",
        };

        let message = err.to_string();
        assert!(message.contains("done"), "{message}");
        assert!(message.contains("handed_off"), "{message}");
        assert_eq!(err.code(), ErrorCode::IllegalTransition);
    }

    /// A store outage must surface as `storage` (exit 5), never as `internal`.
    #[test]
    fn store_errors_become_storage_not_internal() {
        let err: CrewError = crewlist_store::StoreError::Migrate("boom".into()).into();
        assert_eq!(err.code(), ErrorCode::Storage);
        assert_eq!(err.exit_code(), 5);
    }
}
