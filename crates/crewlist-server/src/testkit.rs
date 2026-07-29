//! Test doubles and request helpers.
//!
//! Tests drive the *real* router — real extractors, real error mapping, real
//! serialization — with storage swapped out. That keeps them fast and
//! Docker-free while still exercising the wiring that actually ships.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use chrono::{TimeZone, Utc};
use http_body_util::BodyExt;
use tower::ServiceExt;

use crewlist_core::dto::{
    ComponentHealth, CreateTaskRequest, CreatedTask, DeletedTask, HealthResponse, ListQuery,
    TaskListResponse, TaskView,
};
use crewlist_core::{CrewError, Task, TaskDetail, TaskId, TaskOrigin, TaskStatus};

use crate::repo::TaskRepo;
use crate::state::AppState;

/// A `TaskRepo` that returns what a test tells it to and records what it saw.
///
/// A configured error is consumed by the first call that reaches the repo, so
/// one failure applies to one request — which is all any test here needs.
#[derive(Clone, Default)]
pub struct FakeRepo {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    created_id: Mutex<Option<TaskId>>,
    task_view: Mutex<Option<TaskView>>,
    tasks: Mutex<Option<Vec<Task>>>,
    deleted: Mutex<Option<DeletedTask>>,
    health: Mutex<Option<HealthResponse>>,
    error: Mutex<Option<CrewError>>,

    creates: Mutex<Vec<CreateTaskRequest>>,
    gets: Mutex<Vec<TaskId>>,
    lists: Mutex<Vec<ListQuery>>,
    deletes: Mutex<Vec<(TaskId, bool)>>,
}

impl FakeRepo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_created_id(self, id: TaskId) -> Self {
        *self.inner.created_id.lock().unwrap() = Some(id);
        self
    }

    pub fn with_task_view(self, id: TaskId) -> Self {
        *self.inner.task_view.lock().unwrap() = Some(TaskView {
            task: sample_task(id),
            detail: TaskDetail::empty_for(id, fixed_time()),
            children: Vec::new(),
        });
        self
    }

    pub fn with_tasks(self, ids: Vec<TaskId>) -> Self {
        *self.inner.tasks.lock().unwrap() = Some(ids.into_iter().map(sample_task).collect());
        self
    }

    pub fn with_deleted(self, id: TaskId, cascaded: Vec<TaskId>) -> Self {
        *self.inner.deleted.lock().unwrap() = Some(DeletedTask { id, cascaded });
        self
    }

    pub fn with_health(self, health: HealthResponse) -> Self {
        *self.inner.health.lock().unwrap() = Some(health);
        self
    }

    pub fn failing(self, error: CrewError) -> Self {
        *self.inner.error.lock().unwrap() = Some(error);
        self
    }

    pub fn last_create(&self) -> Option<CreateTaskRequest> {
        self.inner.creates.lock().unwrap().last().cloned()
    }

    pub fn last_get(&self) -> Option<TaskId> {
        self.inner.gets.lock().unwrap().last().copied()
    }

    pub fn last_list(&self) -> Option<ListQuery> {
        self.inner.lists.lock().unwrap().last().cloned()
    }

    pub fn last_delete(&self) -> Option<(TaskId, bool)> {
        self.inner.deletes.lock().unwrap().last().copied()
    }

    fn take_error(&self) -> Option<CrewError> {
        self.inner.error.lock().unwrap().take()
    }
}

#[async_trait]
impl TaskRepo for FakeRepo {
    async fn create(&self, req: CreateTaskRequest) -> Result<CreatedTask, CrewError> {
        self.inner.creates.lock().unwrap().push(req);
        if let Some(err) = self.take_error() {
            return Err(err);
        }
        let id = self.inner.created_id.lock().unwrap().unwrap_or(1);
        Ok(CreatedTask { id })
    }

    async fn get(&self, id: TaskId) -> Result<TaskView, CrewError> {
        self.inner.gets.lock().unwrap().push(id);
        if let Some(err) = self.take_error() {
            return Err(err);
        }
        self.inner
            .task_view
            .lock()
            .unwrap()
            .clone()
            .ok_or(CrewError::NotFound(id))
    }

    async fn list(&self, query: ListQuery) -> Result<TaskListResponse, CrewError> {
        self.inner.lists.lock().unwrap().push(query);
        if let Some(err) = self.take_error() {
            return Err(err);
        }
        let tasks = self.inner.tasks.lock().unwrap().clone().unwrap_or_default();
        Ok(TaskListResponse { tasks })
    }

    async fn delete(&self, id: TaskId, force: bool) -> Result<DeletedTask, CrewError> {
        self.inner.deletes.lock().unwrap().push((id, force));
        if let Some(err) = self.take_error() {
            return Err(err);
        }
        self.inner
            .deleted
            .lock()
            .unwrap()
            .clone()
            .ok_or(CrewError::NotFound(id))
    }

    async fn health(&self) -> HealthResponse {
        self.inner
            .health
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(all_ok_health)
    }
}

pub fn all_ok_health() -> HealthResponse {
    HealthResponse {
        server: ComponentHealth::ok_with_version("0.1.0"),
        postgres: ComponentHealth::ok(),
        mongo: ComponentHealth::ok(),
    }
}

/// A timestamp tests can assert against without a clock.
pub fn fixed_time() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 28, 14, 2, 11).unwrap()
}

pub fn sample_task(id: TaskId) -> Task {
    Task {
        id,
        title: format!("task {id}"),
        status: TaskStatus::Todo,
        origin: TaskOrigin::Human,
        parent_id: None,
        agent_eligible: true,
        summary: None,
        created_at: fixed_time(),
        updated_at: fixed_time(),
        handed_off_at: None,
        completed_at: None,
    }
}

// ---- request helpers ----------------------------------------------------

pub fn get_req(uri: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

pub fn put_req(uri: &str) -> Request<Body> {
    Request::builder()
        .method(Method::PUT)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

pub fn delete_req(uri: &str) -> Request<Body> {
    Request::builder()
        .method(Method::DELETE)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

pub fn post_req(uri: &str, body: serde_json::Value) -> Request<Body> {
    post_req_raw(uri, &body.to_string())
}

/// Bypasses `serde_json`, so a test can send something that is not JSON at all.
pub fn post_req_raw(uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

/// Runs a request through the real router and decodes the response.
///
/// A body that is empty or not JSON decodes to `Value::Null`, so a test can
/// assert on the status of a routing failure without unwrapping.
pub async fn send(repo: FakeRepo, request: Request<Body>) -> (StatusCode, serde_json::Value) {
    let app = crate::routes::router(AppState::new(Arc::new(repo)));
    let response = app.oneshot(request).await.expect("router is infallible");

    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("response body")
        .to_bytes();

    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}
