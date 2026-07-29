//! Errors and their stable wire codes. SPEC.md §6.5.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::status::TaskStatus;
use crate::task::TaskId;

/// Stable strings the skill branches on.
///
/// These survive changes to the wire protocol underneath, because exit codes —
/// not HTTP statuses — are the contract an agent sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotFound,
    IllegalTransition,
    Validation,
    Storage,
    Unreachable,
    Internal,
    /// Scaffold only: a route exists but its handler is not written yet.
    /// Disappears once the handlers land; it is not part of the §6.5 contract.
    Unimplemented,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::IllegalTransition => "illegal_transition",
            Self::Validation => "validation",
            Self::Storage => "storage",
            Self::Unreachable => "unreachable",
            Self::Internal => "internal",
            Self::Unimplemented => "unimplemented",
        }
    }

    /// SPEC.md §6.5. Exit codes are the agent-facing contract.
    pub fn exit_code(self) -> i32 {
        match self {
            Self::NotFound => 3,
            Self::IllegalTransition => 4,
            Self::Storage | Self::Unreachable => 5,
            Self::Validation => 6,
            Self::Internal | Self::Unimplemented => 1,
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CrewError {
    #[error("task {0} not found")]
    NotFound(TaskId),

    /// Carries both states so the message can name them. AC-4.
    #[error("task {id} is '{from}'; cannot {action} (would be '{to}')")]
    IllegalTransition {
        id: TaskId,
        from: TaskStatus,
        to: TaskStatus,
        action: &'static str,
    },

    #[error("{0}")]
    Validation(String),

    #[error("storage unavailable: {0}")]
    Storage(String),

    #[error("{0}")]
    Internal(String),

    #[error("{0} is not implemented yet")]
    Unimplemented(&'static str),
}

impl CrewError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::NotFound(_) => ErrorCode::NotFound,
            Self::IllegalTransition { .. } => ErrorCode::IllegalTransition,
            Self::Validation(_) => ErrorCode::Validation,
            Self::Storage(_) => ErrorCode::Storage,
            Self::Internal(_) => ErrorCode::Internal,
            Self::Unimplemented(_) => ErrorCode::Unimplemented,
        }
    }

    pub fn exit_code(&self) -> i32 {
        self.code().exit_code()
    }
}

/// The body every error path emits, on the wire and under `--json`.
///
/// ```json
/// { "error": { "code": "not_found", "message": "task 42 not found" } }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDetail {
    pub code: ErrorCode,
    pub message: String,
}

impl ErrorBody {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            error: ErrorDetail {
                code,
                message: message.into(),
            },
        }
    }
}

impl From<&CrewError> for ErrorBody {
    fn from(err: &CrewError) -> Self {
        Self::new(err.code(), err.to_string())
    }
}
