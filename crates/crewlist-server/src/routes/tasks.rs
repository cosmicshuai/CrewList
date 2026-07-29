//! Task routes.
//!
//! Handlers are thin on purpose: extract, delegate, serialize. Everything the
//! domain does lives behind [`crate::repo::TaskRepo`], which is what lets the
//! tests below drive the real router without a database.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use crewlist_core::dto::{
    CreateTaskRequest, CreatedTask, DeleteQuery, DeletedTask, ListQuery, TaskListResponse, TaskView,
};
use crewlist_core::TaskId;

use crate::error::ApiResult;
use crate::extract::{AppJson, AppPath, AppQuery};
use crate::state::AppState;

/// `POST /tasks` — create a root task (`human add`) or a child (`agent add`).
pub async fn create(
    State(state): State<AppState>,
    AppJson(req): AppJson<CreateTaskRequest>,
) -> ApiResult<(StatusCode, Json<CreatedTask>)> {
    let created = state.repo.create(req).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// `GET /tasks/{id}` — task, payload, and children.
pub async fn get(
    State(state): State<AppState>,
    AppPath(id): AppPath<TaskId>,
) -> ApiResult<Json<TaskView>> {
    Ok(Json(state.repo.get(id).await?))
}

/// `GET /tasks` — the human list, or the agent queue via `?queue=agent`.
pub async fn list(
    State(state): State<AppState>,
    AppQuery(query): AppQuery<ListQuery>,
) -> ApiResult<Json<TaskListResponse>> {
    Ok(Json(state.repo.list(query).await?))
}

/// `DELETE /tasks/{id}` — hard delete, children cascade.
pub async fn delete(
    State(state): State<AppState>,
    AppPath(id): AppPath<TaskId>,
    AppQuery(query): AppQuery<DeleteQuery>,
) -> ApiResult<Json<DeletedTask>> {
    Ok(Json(state.repo.delete(id, query.force).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{delete_req, get_req, post_req, post_req_raw, put_req, send, FakeRepo};
    use crewlist_core::dto::Queue;
    use crewlist_core::{CrewError, TaskOrigin, TaskStatus};
    use serde_json::json;

    // ---- POST /tasks -----------------------------------------------------

    #[tokio::test]
    async fn create_returns_201_and_the_new_id() {
        let repo = FakeRepo::new().with_created_id(7);

        let (status, body) = send(
            repo,
            post_req(
                "/tasks",
                json!({
                    "title": "find a reliable tree removal service",
                    "origin": "human"
                }),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["id"], 7);
    }

    #[tokio::test]
    async fn create_forwards_the_whole_request_to_the_repo() {
        let repo = FakeRepo::new().with_created_id(1);

        let (status, _) = send(
            repo.clone(),
            post_req(
                "/tasks",
                json!({
                    "title": "  Call Alex's Tree Service 617-898-0989  ",
                    "origin": "agent",
                    "parent_id": 1,
                    "agent_eligible": false,
                    "description": "quotes Tue-Thu",
                    "sources": ["https://example.test/registry"]
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let req = repo.last_create().expect("repo saw a create");
        assert_eq!(req.title, "  Call Alex's Tree Service 617-898-0989  ");
        assert_eq!(req.origin, TaskOrigin::Agent);
        assert_eq!(req.parent_id, Some(1));
        assert!(!req.agent_eligible);
        assert_eq!(req.description.as_deref(), Some("quotes Tue-Thu"));
        assert_eq!(req.sources, vec!["https://example.test/registry"]);
    }

    /// Trimming is the domain's job (AC-7); the HTTP layer must not silently
    /// pre-process the payload on its way through.
    #[tokio::test]
    async fn create_does_not_trim_before_the_repo_sees_it() {
        let repo = FakeRepo::new().with_created_id(1);
        send(
            repo.clone(),
            post_req(
                "/tasks",
                json!({ "title": "  spaced  ", "origin": "human" }),
            ),
        )
        .await;

        assert_eq!(repo.last_create().unwrap().title, "  spaced  ");
    }

    #[tokio::test]
    async fn create_defaults_agent_eligible_to_true() {
        let repo = FakeRepo::new().with_created_id(1);
        send(
            repo.clone(),
            post_req("/tasks", json!({ "title": "buy milk", "origin": "human" })),
        )
        .await;

        assert!(repo.last_create().unwrap().agent_eligible);
    }

    #[tokio::test]
    async fn create_rejects_malformed_json_as_validation() {
        let repo = FakeRepo::new();
        let (status, body) = send(repo.clone(), post_req_raw("/tasks", "{ not json")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "validation");
        assert!(repo.last_create().is_none(), "repo must not be touched");
    }

    #[tokio::test]
    async fn create_rejects_a_missing_required_field() {
        let repo = FakeRepo::new();
        let (status, body) = send(repo, post_req("/tasks", json!({ "title": "no origin" }))).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "validation");
    }

    #[tokio::test]
    async fn create_surfaces_an_unknown_parent_as_404() {
        let repo = FakeRepo::new().failing(CrewError::NotFound(99));
        let (status, body) = send(
            repo,
            post_req(
                "/tasks",
                json!({
                    "title": "orphan", "origin": "agent", "parent_id": 99
                }),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");
    }

    /// AC-31: a `--parent` pointing at a child is a validation failure, which
    /// must reach the CLI as exit 6 rather than as a conflict.
    #[tokio::test]
    async fn create_surfaces_the_depth_limit_as_400() {
        let repo =
            FakeRepo::new().failing(CrewError::Validation("task 2 is already a child".into()));
        let (status, body) = send(
            repo,
            post_req(
                "/tasks",
                json!({
                    "title": "grandchild", "origin": "agent", "parent_id": 2
                }),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "validation");
        assert_eq!(body["error"]["message"], "task 2 is already a child");
    }

    // ---- GET /tasks/{id} -------------------------------------------------

    #[tokio::test]
    async fn get_returns_task_detail_and_children() {
        let repo = FakeRepo::new().with_task_view(1);
        let (status, body) = send(repo, get_req("/tasks/1")).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["task"]["id"], 1);
        assert!(body["detail"].is_object());
        assert!(body["children"].is_array());
    }

    /// AC-25: a task with no Mongo document still yields a defaulted `detail`,
    /// never `null` — the skill should need no null branch.
    #[tokio::test]
    async fn get_never_returns_a_null_detail() {
        let repo = FakeRepo::new().with_task_view(1);
        let (_, body) = send(repo, get_req("/tasks/1")).await;

        assert!(!body["detail"].is_null());
        assert_eq!(body["detail"]["schema_version"], 1);
        assert_eq!(body["detail"]["notes"], json!([]));
        assert_eq!(body["detail"]["sources"], json!([]));
        assert_eq!(body["detail"]["contacts"], json!([]));
    }

    #[tokio::test]
    async fn get_passes_the_path_id_through() {
        let repo = FakeRepo::new().with_task_view(42);
        send(repo.clone(), get_req("/tasks/42")).await;

        assert_eq!(repo.last_get(), Some(42));
    }

    #[tokio::test]
    async fn get_unknown_id_is_404_in_the_error_shape() {
        let repo = FakeRepo::new().failing(CrewError::NotFound(42));
        let (status, body) = send(repo, get_req("/tasks/42")).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");
        assert_eq!(body["error"]["message"], "task 42 not found");
    }

    #[tokio::test]
    async fn get_non_numeric_id_is_validation_not_a_plain_text_rejection() {
        let repo = FakeRepo::new();
        let (status, body) = send(repo.clone(), get_req("/tasks/abc")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "validation");
        assert!(repo.last_get().is_none());
    }

    // ---- GET /tasks ------------------------------------------------------

    #[tokio::test]
    async fn list_returns_the_task_array() {
        let repo = FakeRepo::new().with_tasks(vec![1, 2]);
        let (status, body) = send(repo, get_req("/tasks")).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["tasks"].as_array().unwrap().len(), 2);
        assert_eq!(body["tasks"][0]["id"], 1);
    }

    #[tokio::test]
    async fn list_with_no_query_asks_for_no_filter() {
        let repo = FakeRepo::new().with_tasks(vec![]);
        send(repo.clone(), get_req("/tasks")).await;

        let query = repo.last_list().expect("repo saw a list");
        assert_eq!(query.queue, None);
        assert_eq!(query.status, None);
        assert!(!query.all);
    }

    /// AC-21 depends on this: `agent list` is the queue, and the queue is a
    /// query parameter rather than a separate route.
    #[tokio::test]
    async fn list_forwards_the_agent_queue_selector() {
        let repo = FakeRepo::new().with_tasks(vec![]);
        send(repo.clone(), get_req("/tasks?queue=agent")).await;

        assert_eq!(repo.last_list().unwrap().queue, Some(Queue::Agent));
    }

    #[tokio::test]
    async fn list_forwards_status_and_all() {
        let repo = FakeRepo::new().with_tasks(vec![]);
        send(repo.clone(), get_req("/tasks?status=handed_off&all=true")).await;

        let query = repo.last_list().unwrap();
        assert_eq!(query.status, Some(TaskStatus::HandedOff));
        assert!(query.all);
    }

    /// AC-20: an empty queue is a success, not an error.
    #[tokio::test]
    async fn list_empty_is_200_with_an_empty_array() {
        let repo = FakeRepo::new().with_tasks(vec![]);
        let (status, body) = send(repo, get_req("/tasks?queue=agent")).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["tasks"], json!([]));
    }

    #[tokio::test]
    async fn list_rejects_an_unknown_queue_value() {
        let repo = FakeRepo::new();
        let (status, body) = send(repo.clone(), get_req("/tasks?queue=bogus")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "validation");
        assert!(repo.last_list().is_none());
    }

    /// A typo must not silently answer a different question. `?queeue=agent`
    /// once returned the *human* list with a 200 — the worst outcome available
    /// to a tool whose queries are generated by an agent skill.
    #[tokio::test]
    async fn list_rejects_an_unknown_query_parameter() {
        let repo = FakeRepo::new().with_tasks(vec![]);
        let (status, body) = send(repo.clone(), get_req("/tasks?queeue=agent")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "validation");
        assert!(repo.last_list().is_none(), "repo must not be touched");
    }

    #[tokio::test]
    async fn list_surfaces_a_store_outage_as_503() {
        let repo = FakeRepo::new().failing(CrewError::Storage("postgres is down".into()));
        let (status, body) = send(repo, get_req("/tasks")).await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "storage");
    }

    // ---- DELETE /tasks/{id} ---------------------------------------------

    #[tokio::test]
    async fn delete_returns_the_id_and_the_cascade() {
        let repo = FakeRepo::new().with_deleted(1, vec![2, 3]);
        let (status, body) = send(repo, delete_req("/tasks/1?force=true")).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["id"], 1);
        assert_eq!(body["cascaded"], json!([2, 3]));
    }

    #[tokio::test]
    async fn delete_defaults_force_to_false() {
        let repo = FakeRepo::new().with_deleted(1, vec![]);
        send(repo.clone(), delete_req("/tasks/1")).await;

        assert_eq!(repo.last_delete(), Some((1, false)));
    }

    #[tokio::test]
    async fn delete_forwards_force() {
        let repo = FakeRepo::new().with_deleted(1, vec![]);
        send(repo.clone(), delete_req("/tasks/1?force=true")).await;

        assert_eq!(repo.last_delete(), Some((1, true)));
    }

    /// AC-46: deleting a parent without `--force` must fail and delete nothing.
    #[tokio::test]
    async fn delete_parent_without_force_is_a_validation_error() {
        let repo = FakeRepo::new().failing(CrewError::Validation(
            "task 1 has 2 children; pass --force".into(),
        ));
        let (status, body) = send(repo, delete_req("/tasks/1")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "validation");
    }

    #[tokio::test]
    async fn delete_rejects_an_unknown_query_parameter() {
        let repo = FakeRepo::new().with_deleted(1, vec![]);
        let (status, body) = send(repo.clone(), delete_req("/tasks/1?forse=true")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "validation");
        assert!(
            repo.last_delete().is_none(),
            "a misspelled force must never delete"
        );
    }

    #[tokio::test]
    async fn delete_unknown_id_is_404() {
        let repo = FakeRepo::new().failing(CrewError::NotFound(42));
        let (status, body) = send(repo, delete_req("/tasks/42")).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");
    }

    // ---- routing ---------------------------------------------------------

    #[tokio::test]
    async fn unknown_path_is_404() {
        let (status, _) = send(FakeRepo::new(), get_req("/nope")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn wrong_method_is_405_not_404() {
        let (status, _) = send(FakeRepo::new(), put_req("/tasks")).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn delete_is_not_routed_at_the_collection() {
        let (status, _) = send(FakeRepo::new(), delete_req("/tasks")).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    }
}
