use std::time::Duration;

use crate::{
    ActiveTimer, Clock, DomainError, EntrySource, Notification, TimeEntry, TrackerCommand,
    TrackerSnapshot, TrackerState, Transition,
};

/// The timer state machine. `C` is the time source.
#[derive(Clone, Debug)]
pub struct TrackerEngine<C> {
    clock: C,
    snapshot: TrackerSnapshot,
}

impl<C: Clock> TrackerEngine<C> {
    pub fn new(clock: C) -> Self {
        Self {
            clock,
            snapshot: TrackerSnapshot::default(),
        }
    }

    pub fn snapshot(&self) -> &TrackerSnapshot {
        &self.snapshot
    }

    /// Time shown by the live counter; zero when nothing is running.
    pub fn live_elapsed(&self) -> Duration {
        let Some(active) = self.snapshot.state.active() else {
            return Duration::ZERO;
        };
        let started = Duration::from_millis(active.started_monotonic_ms);
        self.clock.monotonic().saturating_sub(started)
    }

    /// Applies one command. On error the engine is unchanged.
    pub fn apply(&mut self, command: TrackerCommand) -> Result<Transition, DomainError> {
        let now = self.clock.wall_time_ms();
        let monotonic_ms = u64::try_from(self.clock.monotonic().as_millis()).unwrap_or(u64::MAX);
        let mut completed_entries = Vec::new();
        let mut notifications = Vec::new();

        let next_state = match (self.snapshot.state.clone(), command) {
            (
                TrackerState::Stopped,
                TrackerCommand::Start {
                    project_id,
                    task_id,
                    note,
                },
            ) => {
                notifications.push(Notification::TimerStarted);
                TrackerState::Running(ActiveTimer {
                    project_id,
                    task_id,
                    note,
                    start_ms: now,
                    started_monotonic_ms: monotonic_ms,
                    last_heartbeat_ms: now,
                })
            }
            (TrackerState::Running(active), TrackerCommand::Stop) => {
                push_entry(
                    &mut completed_entries,
                    &active,
                    active.start_ms,
                    now,
                    EntrySource::Timer,
                );
                notifications.push(Notification::TimerStopped);
                TrackerState::Stopped
            }
            (
                TrackerState::Running(mut active),
                TrackerCommand::EditActive {
                    project_id,
                    task_id,
                    note,
                },
            ) => {
                active.project_id = project_id;
                active.task_id = task_id;
                active.note = note;
                active.last_heartbeat_ms = now.max(active.start_ms);
                TrackerState::Running(active)
            }
            (TrackerState::Running(mut active), TrackerCommand::Heartbeat) => {
                active.last_heartbeat_ms = now.max(active.start_ms);
                TrackerState::Running(active)
            }
            (state, _) => return Err(DomainError::InvalidState(state)),
        };

        self.snapshot.state = next_state;
        self.snapshot.revision = self.snapshot.revision.saturating_add(1);
        Ok(Transition {
            snapshot: self.snapshot.clone(),
            completed_entries,
            notifications,
        })
    }
}

fn push_entry(
    completed: &mut Vec<TimeEntry>,
    active: &ActiveTimer,
    start_ms: i64,
    end_ms: i64,
    source: EntrySource,
) {
    if end_ms <= start_ms {
        return;
    }
    completed.push(TimeEntry {
        id: None,
        project_id: active.project_id,
        task_id: active.task_id,
        note: active.note.clone(),
        start_ms,
        end_ms,
        source,
        created_at_ms: end_ms,
        updated_at_ms: end_ms,
    });
}
