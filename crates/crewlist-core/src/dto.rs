//! Wire types.
//!
//! Internal and unversioned — SPEC.md §6.6. The stable contracts are CLI
//! arguments, `--json` output, and exit codes. Nothing outside this repository
//! should depend on these shapes.

use serde::{Deserialize, Serialize};

use crate::detail::TaskDetail;
use crate::status::{TaskOrigin, TaskStatus};
use crate::task::{Task, TaskId};

/// `POST /tasks`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateTaskRequest {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub parent_id: Option<TaskId>,
    /// `human add` sets this; `agent add` always sends `Agent`.
    pub origin: TaskOrigin,
    /// `human add --self` clears this to keep the task out of the agent queue.
    #[serde(default = "default_true")]
    pub agent_eligible: bool,
    #[serde(default)]
    pub sources: Vec<String>,
}

fn default_true() -> bool {
    true
}

/// `GET /tasks/{id}` — the task, its payload, and its children.
///
/// `detail` is always present. A task with no Mongo document yields a
/// fully-defaulted value, never `null`. AC-25.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskView {
    pub task: Task,
    pub detail: TaskDetail,
    pub children: Vec<Task>,
}

/// `GET /tasks` filters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListQuery {
    /// `agent` selects the agent queue: todo + human-origin + eligible + root.
    /// SPEC.md §3.3.
    #[serde(default)]
    pub queue: Option<Queue>,
    #[serde(default)]
    pub status: Option<TaskStatus>,
    /// Include `done` and `cancelled`, and bypass the queue filter.
    #[serde(default)]
    pub all: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Queue {
    Agent,
    Human,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskListResponse {
    pub tasks: Vec<Task>,
}

/// `POST /tasks` response. The CLI prints just the id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedTask {
    pub id: TaskId,
}

/// `DELETE /tasks/{id}`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeleteQuery {
    /// Required when the task has children, which cascade. AC-46, AC-47.
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletedTask {
    pub id: TaskId,
    /// Child ids removed by the cascade.
    pub cascaded: Vec<TaskId>,
}

/// `GET /health` — SPEC.md §6.2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub server: ComponentHealth,
    pub postgres: ComponentHealth,
    pub mongo: ComponentHealth,
}

impl HealthResponse {
    pub fn all_ok(&self) -> bool {
        self.server.ok && self.postgres.ok && self.mongo.ok
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Present only on failure, and it names what broke. AC-64.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl ComponentHealth {
    pub fn ok() -> Self {
        Self {
            ok: true,
            version: None,
            message: None,
        }
    }

    pub fn ok_with_version(version: impl Into<String>) -> Self {
        Self {
            ok: true,
            version: Some(version.into()),
            message: None,
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            version: None,
            message: Some(message.into()),
        }
    }
}
