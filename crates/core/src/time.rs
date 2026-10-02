use crate::DomainError;
use serde::{Deserialize, Serialize};

use crate::id::{ActivityId, EntryId, IntervalId, ProjectId};

/// How a time entry came into existence.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntrySource {
    /// Recorded by a running timer.
    #[default]
    Timer,
    /// Typed in by hand after the fact.
    Manual,
    /// Carved out of an idle period by the user.
    IdleReassignment,
    /// Salvaged from an interrupted timer.
    Recovery,
}

/// One completed, half-open interval `[start_ms, end_ms)` belonging to an entry.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct TrackedInterval {
    /// Stored identity; `None` until the interval is saved.
    pub id: Option<IntervalId>,
    /// Inclusive start, in wall-clock milliseconds.
    pub start_ms: i64,
    /// Exclusive end, in wall-clock milliseconds.
    pub end_ms: i64,
    /// How this interval came into existence.
    pub source: EntrySource,
}

impl TrackedInterval {
    /// Returns the non-negative length of the interval.
    pub fn duration_ms(&self) -> i64 {
        self.end_ms.saturating_sub(self.start_ms).max(0)
    }

    /// Rejects intervals that end at or before they start.
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
    /// Stored identity; `None` until the entry is saved.
    pub id: Option<EntryId>,
    /// Project the tracked work is recorded against.
    pub project_id: ProjectId,
    /// Kind of work, if the user chose one.
    pub activity_id: Option<ActivityId>,
    /// Note shared by every interval in this entry.
    pub note: String,
    /// Completed spans of tracked time; gaps between them are untracked.
    pub intervals: Vec<TrackedInterval>,
    /// Wall-clock creation time, in milliseconds.
    pub created_at_ms: i64,
    /// Wall-clock time of the last change, in milliseconds.
    pub updated_at_ms: i64,
}

impl TimeEntry {
    /// Returns the summed length of every interval.
    pub fn duration_ms(&self) -> i64 {
        self.intervals.iter().fold(0, |total, interval| {
            total.saturating_add(interval.duration_ms())
        })
    }

    /// Returns the summed length of the intervals clipped to the half-open
    /// range `[start_ms, end_ms)`.
    pub fn duration_between_ms(&self, start_ms: i64, end_ms: i64) -> i64 {
        self.intervals.iter().fold(0, |total, interval| {
            total.saturating_add(
                interval
                    .end_ms
                    .min(end_ms)
                    .saturating_sub(interval.start_ms.max(start_ms))
                    .max(0),
            )
        })
    }

    /// Rejects entries containing an invalid interval.
    pub fn validate(&self) -> Result<(), DomainError> {
        for interval in &self.intervals {
            interval.validate()?;
        }
        Ok(())
    }

    /// Returns the latest interval end, or nothing when empty.
    pub fn latest_end_ms(&self) -> Option<i64> {
        self.intervals.iter().map(|interval| interval.end_ms).max()
    }
}

/// Work currently being measured; becomes an entry on stop.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ActiveTimer {
    /// The entry being continued. `None` means Stop will create a new entry.
    pub entry_id: Option<EntryId>,
    /// Project the running timer records against.
    pub project_id: ProjectId,
    /// Kind of work being timed, if the user picked one.
    pub activity_id: Option<ActivityId>,
    /// Note the running timer will save on its entry.
    pub note: String,
    /// Wall-clock start, stored on disk.
    pub start_ms: i64,
    /// Monotonic start, used only for the live display.
    pub started_monotonic_ms: u64,
    /// Last instant the running timer was known to be alive.
    pub last_heartbeat_ms: i64,
    /// Tracked time banked by earlier segments of this run, in milliseconds.
    /// Pause and idle reconciliation add to it; the live counter adds the
    /// current segment on top so breaks never count.
    #[serde(default)]
    pub accumulated_ms: i64,
}

/// A timer the user paused; elapsed time stays frozen until resume.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PausedTimer {
    /// Timer that was running when the user paused it.
    pub active: ActiveTimer,
    /// When the pause began; the frozen end of the elapsed time.
    pub paused_at_ms: i64,
}

/// A running timer whose user went away; waiting for their decision.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PendingIdle {
    /// Timer that was running when the user went away.
    pub active: ActiveTimer,
    /// When the absence began.
    pub idle_start_ms: i64,
    /// Set once the user is back; `None` while they are still away.
    pub return_ms: Option<i64>,
}

/// A timer that was running when the app stopped uncleanly.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PendingRecovery {
    /// Timer that was running when the app stopped.
    pub active: ActiveTimer,
    /// Trusted end for the recovered portion, derived from heartbeats.
    pub proposed_end_ms: i64,
    /// Idle start still awaiting review, if the timer was already idle.
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

    #[test]
    fn clipped_duration_counts_only_time_inside_the_range() {
        let interval = |start_ms, end_ms| TrackedInterval {
            id: None,
            start_ms,
            end_ms,
            source: EntrySource::Timer,
        };
        let entry = TimeEntry {
            id: None,
            project_id: ProjectId(1),
            activity_id: None,
            note: String::new(),
            intervals: vec![interval(0, 100), interval(200, 300), interval(400, 500)],
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        assert_eq!(entry.duration_between_ms(i64::MIN, i64::MAX), 300);
        assert_eq!(entry.duration_between_ms(50, 250), 100);
        assert_eq!(entry.duration_between_ms(100, 200), 0);
        assert_eq!(entry.duration_between_ms(450, 450), 0);
        assert_eq!(entry.duration_between_ms(500, 0), 0);
    }
}
