use crate::DomainError;
use serde::{Deserialize, Serialize};

use crate::id::{EntryId, ProjectId, TaskId};

/// How a time entry came into existence.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntrySource {
    #[default]
    Timer,
    Manual,
    IdleReassignment,
    Recovery,
}

/// A completed, half-open interval `[start_ms, end_ms)` of tracked time.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct TimeEntry {
    pub id: Option<EntryId>,
    pub project_id: ProjectId,
    pub task_id: Option<TaskId>,
    pub note: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub source: EntrySource,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl TimeEntry {
    pub fn duration_ms(&self) -> i64 {
        self.end_ms.saturating_sub(self.start_ms).max(0)
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        if self.end_ms <= self.start_ms {
            return Err(DomainError::InvalidInterval {
                start_ms: self.start_ms,
                end_ms: self.end_ms,
            });
        }
        Ok(())
    }
}

/// The interval currently being tracked.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ActiveTimer {
    pub project_id: ProjectId,
    pub task_id: Option<TaskId>,
    pub note: String,
    /// Wall-clock start, stored on disk.
    pub start_ms: i64,
    /// Monotonic start, used only for the live display.
    pub started_monotonic_ms: u64,
    /// Last instant the running timer was known to be alive.
    pub last_heartbeat_ms: i64,
}

/// A running timer whose user went away; waiting for their decision.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PendingIdle {
    pub active: ActiveTimer,
    pub idle_start_ms: i64,
    /// Set once the user is back; `None` while they are still away.
    pub return_ms: Option<i64>,
}

/// A timer that was running when the app stopped uncleanly.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PendingRecovery {
    pub active: ActiveTimer,
    pub proposed_end_ms: i64,
    pub unresolved_idle_start_ms: Option<i64>,
}
