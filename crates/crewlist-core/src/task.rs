//! The task record. SPEC.md §3.1.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::status::{TaskOrigin, TaskStatus};

pub type TaskId = i64;

/// Maximum title length in characters, after trimming. SPEC.md §3.1, AC-6.
pub const TITLE_MAX_CHARS: usize = 500;

/// Task metadata, mirroring the Postgres row.
///
/// The payload — description, notes, sources, contacts — lives in
/// [`crate::TaskDetail`], stored in Mongo. Nothing in this struct is
/// free-form prose except `summary`, which is denormalized so `list` need not
/// touch Mongo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub title: String,
    pub status: TaskStatus,
    pub origin: TaskOrigin,
    pub parent_id: Option<TaskId>,
    pub agent_eligible: bool,
    pub summary: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub handed_off_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl Task {
    /// Root tasks are what the human asked for; children are what an agent
    /// determined must actually be done. The hierarchy is strictly two levels.
    pub fn is_root(&self) -> bool {
        self.parent_id.is_none()
    }
}
