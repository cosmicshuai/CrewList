//! Task status and origin. SPEC.md §3.2, §3.3.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Where a task sits in its lifecycle.
///
/// The legal transition table lives in SPEC.md §3.2. It is intentionally not
/// implemented here yet — AC-1 through AC-4 drive it in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    HandedOff,
    Done,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::HandedOff => "handed_off",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who created a task.
///
/// This is what keeps the agent queue from eating itself: `agent list` filters
/// on `Human`, so agent-created action items never re-enter it. SPEC.md §3.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOrigin {
    Human,
    Agent,
}

impl TaskOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Agent => "agent",
        }
    }
}

impl fmt::Display for TaskOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
