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
