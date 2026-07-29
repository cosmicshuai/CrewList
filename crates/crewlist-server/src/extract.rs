//! Extractors that fail in the project's error shape.
//!
//! Axum's built-in rejections return `text/plain`. SPEC.md §6.5 requires that
//! *every* error path emit `{"error":{"code":…,"message":…}}`, so malformed
//! JSON and an unparseable `{id}` have to look like every other failure —
//! otherwise the CLI has one branch that can't map to an exit code (AC-56).

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;

use crewlist_core::CrewError;

use crate::error::ApiError;

/// `Json`, but a parse failure becomes a `validation` error (400 → exit 6).
pub struct AppJson<T>(pub T);

impl<S, T> FromRequest<S> for AppJson<T>
where
    axum::Json<T>: FromRequest<S, Rejection = JsonRejection>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(ApiError(CrewError::Validation(rejection.body_text()))),
        }
    }
}

/// `Path`, but a non-numeric id becomes a `validation` error, not a 400 with
/// an axum-shaped body.
pub struct AppPath<T>(pub T);

impl<S, T> FromRequestParts<S> for AppPath<T>
where
    axum::extract::Path<T>: FromRequestParts<S, Rejection = PathRejection>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Path::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Path(value)) => Ok(Self(value)),
            Err(rejection) => Err(ApiError(CrewError::Validation(rejection.body_text()))),
        }
    }
}

/// `Query`, same treatment — an unknown `?queue=` value is a validation error.
pub struct AppQuery<T>(pub T);

impl<S, T> FromRequestParts<S> for AppQuery<T>
where
    axum::extract::Query<T>: FromRequestParts<S, Rejection = QueryRejection>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Query::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Query(value)) => Ok(Self(value)),
            Err(rejection) => Err(ApiError(CrewError::Validation(rejection.body_text()))),
        }
    }
}
