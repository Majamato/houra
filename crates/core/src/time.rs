use crate::DomainError;
use serde::{Deserialize, Serialize};

use crate::id::{ActivityId, EntryId, IntervalId, ProjectId};

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

/// One completed, half-open interval `[start_ms, end_ms)` belonging to an entry.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct TrackedInterval {
    pub id: Option<IntervalId>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub source: EntrySource,
}

impl TrackedInterval {
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

/// Shared work details and all of the intervals recorded for that work.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct TimeEntry {
    pub id: Option<EntryId>,
    pub project_id: ProjectId,
    pub activity_id: Option<ActivityId>,
    pub note: String,
    pub intervals: Vec<TrackedInterval>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl TimeEntry {
    pub fn duration_ms(&self) -> i64 {
        self.intervals.iter().fold(0, |total, interval| {
            total.saturating_add(interval.duration_ms())
        })
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        for interval in &self.intervals {
            interval.validate()?;
        }
        Ok(())
    }

    pub fn latest_end_ms(&self) -> Option<i64> {
        self.intervals.iter().map(|interval| interval.end_ms).max()
    }
}

/// The interval currently being tracked.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ActiveTimer {
    /// The entry being continued. `None` means Stop will create a new entry.
    pub entry_id: Option<EntryId>,
    pub project_id: ProjectId,
    pub activity_id: Option<ActivityId>,
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interval_validation_and_saturating_duration() {
        for (start_ms, end_ms, duration) in [
            (100, 200, 100),
            (200, 100, 0),
            (100, 100, 0),
            (i64::MIN, i64::MAX, i64::MAX),
        ] {
            let entry = TimeEntry {
                id: None,
                project_id: ProjectId(1),
                activity_id: None,
                note: String::new(),
                intervals: vec![TrackedInterval {
                    id: None,
                    start_ms,
                    end_ms,
                    source: EntrySource::Manual,
                }],
                created_at_ms: 0,
                updated_at_ms: 0,
            };
            assert_eq!(entry.duration_ms(), duration);
            assert_eq!(
                entry.validate(),
                if end_ms > start_ms {
                    Ok(())
                } else {
                    Err(DomainError::InvalidInterval { start_ms, end_ms })
                }
            );
        }
    }
}
