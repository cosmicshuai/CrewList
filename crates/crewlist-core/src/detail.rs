//! The Mongo detail document. SPEC.md §5.1.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::task::TaskId;

/// Schema version written by this build. Readers reject anything else rather
/// than guessing at the shape. SPEC.md §5.1, AC-51.
pub const SCHEMA_VERSION: i32 = 1;

/// Free-form-*shaped* payload for a task.
///
/// Absence is normal: a task with no details has `detail_id IS NULL` and no
/// document at all. Read paths must render that as a fully-defaulted value,
/// never `null` — the skill should not need a null branch. SPEC.md §6.4, AC-25.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDetail {
    pub task_id: TaskId,
    pub schema_version: i32,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub notes: Vec<Note>,
    #[serde(default)]
    pub sources: Vec<Source>,
    #[serde(default)]
    pub contacts: Vec<Contact>,
    #[serde(default)]
    pub summary: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TaskDetail {
    /// The value a read path yields when no document exists.
    pub fn empty_for(task_id: TaskId, now: DateTime<Utc>) -> Self {
        Self {
            task_id,
            schema_version: SCHEMA_VERSION,
            description: String::new(),
            notes: Vec::new(),
            sources: Vec::new(),
            contacts: Vec::new(),
            summary: None,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub author: String,
    pub body: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    pub retrieved_at: DateTime<Utc>,
}

/// Reserved. No v1 command populates this — the phone number lives in the
/// child task's title, which is what the human actually reads. SPEC.md §9.3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub name: String,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}
