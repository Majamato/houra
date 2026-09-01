use thiserror::Error;

use crate::{
    TrackerState,
    id::{EntryId, ProjectId, TaskId},
};

/// A rejected domain operation. Infrastructure errors belong in the app crate.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("the tracker is not in the required state (current state: {0:?})")]
    InvalidState(TrackerState),

    #[error("end time {end_ms} must be after start time {start_ms}")]
    InvalidInterval { start_ms: i64, end_ms: i64 },

    #[error(
        "idle start {idle_start_ms} is outside the active interval beginning {active_start_ms}"
    )]
    InvalidIdleStart {
        active_start_ms: i64,
        idle_start_ms: i64,
    },

    #[error("return time {return_ms} is before idle start {idle_start_ms}")]
    InvalidReturn { idle_start_ms: i64, return_ms: i64 },

    #[error("task {task_id} does not belong to project {project_id}")]
    TaskProjectMismatch {
        project_id: ProjectId,
        task_id: TaskId,
    },

    #[error("entry overlaps existing entries: {conflicts:?}")]
    Overlap { conflicts: Vec<EntryId> },

    #[error("name must contain a non-whitespace character")]
    EmptyName,

    #[error("color must be a CSS hexadecimal color such as #3584e4")]
    InvalidColor,

    #[error("recovery end {end_ms} is outside [{start_ms}, {last_heartbeat_ms}]")]
    InvalidRecoveryEnd {
        start_ms: i64,
        last_heartbeat_ms: i64,
        end_ms: i64,
    },
}
