//! CrewList domain types and wire DTOs.
//!
//! This crate performs no I/O. It is the only crate shared by both sides of
//! the client/server split, which is what keeps the status machine and the
//! wire types from drifting apart. See SPEC.md §8.
//!
//! Behavior is deliberately absent at this stage — the status machine
//! (SPEC.md §3.2) and validation rules (§7.2) are driven in by tests.

pub mod detail;
pub mod dto;
pub mod error;
pub mod status;
pub mod task;

pub use detail::{Contact, Note, Source, TaskDetail, SCHEMA_VERSION};
pub use error::{CrewError, ErrorCode};
pub use status::{TaskOrigin, TaskStatus};
pub use task::{Task, TaskId};
