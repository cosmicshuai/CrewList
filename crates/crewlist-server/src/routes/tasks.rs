//! Task routes.
//!
//! Handlers are stubs: the wiring, extractors, and response types are real,
//! the behavior is not. Each `todo!`-equivalent names the acceptance criteria
//! that will drive it in, so the red tests have somewhere obvious to land.

use axum::extract::{Path, Query, State};
use axum::Json;
use crewlist_core::dto::{
    CreateTaskRequest, CreatedTask, DeleteQuery, DeletedTask, ListQuery, TaskListResponse, TaskView,
};
use crewlist_core::{CrewError, TaskId};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// `POST /tasks` — create a root task (`human add`) or a child (`agent add`).
///
/// Drives: AC-5 … AC-15 (validation, defaults, detail linkage),
/// AC-30 … AC-34 (child creation, two-level limit).
pub async fn create(
    State(_state): State<AppState>,
    Json(_req): Json<CreateTaskRequest>,
) -> ApiResult<Json<CreatedTask>> {
    Err(ApiError(CrewError::Unimplemented("create task")))
}

/// `GET /tasks/{id}` — task, payload, and children.
///
/// Drives: AC-25 (defaulted detail, never null), AC-44 (missing Mongo document
/// is not an error).
pub async fn get(
    State(_state): State<AppState>,
    Path(_id): Path<TaskId>,
) -> ApiResult<Json<TaskView>> {
    Err(ApiError(CrewError::Unimplemented("get task")))
}

/// `GET /tasks` — the human list, or the agent queue via `?queue=agent`.
///
/// Drives: AC-16 … AC-21 (queue filter), AC-41 … AC-43 (human list ordering
/// and the done-parent-with-open-children case).
pub async fn list(
    State(_state): State<AppState>,
    Query(_query): Query<ListQuery>,
) -> ApiResult<Json<TaskListResponse>> {
    Err(ApiError(CrewError::Unimplemented("list tasks")))
}

/// `DELETE /tasks/{id}` — hard delete, children cascade.
///
/// Drives: AC-46 (parent needs `force`), AC-47 (cascade), and the delete half
/// of §5.3: Postgres first, then best-effort Mongo cleanup.
pub async fn delete(
    State(_state): State<AppState>,
    Path(_id): Path<TaskId>,
    Query(_query): Query<DeleteQuery>,
) -> ApiResult<Json<DeletedTask>> {
    Err(ApiError(CrewError::Unimplemented("delete task")))
}
