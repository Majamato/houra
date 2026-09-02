use std::time::Duration;

use crate::{
    ActiveTimer, Clock, DomainError, EntrySource, IdleDecision, Notification, PendingIdle,
    PendingRecovery, TimeEntry, TrackerCommand, TrackerSnapshot, TrackerState, Transition,
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

    /// Rebuilds an engine from a persisted snapshot. After an unclean
    /// shutdown a live timer becomes a recovery question for the user.
    pub fn restore(clock: C, mut snapshot: TrackerSnapshot, unclean_shutdown: bool) -> Self {
        if unclean_shutdown {
            snapshot.state = match snapshot.state {
                TrackerState::Running(active) => {
                    let proposed_end_ms = active.last_heartbeat_ms.max(active.start_ms);
                    TrackerState::RecoveryPending(PendingRecovery {
                        active,
                        proposed_end_ms,
                        unresolved_idle_start_ms: None,
                    })
                }
                TrackerState::IdlePending(pending) => {
                    let proposed_end_ms = pending
                        .active
                        .last_heartbeat_ms
                        .max(pending.active.start_ms);
                    TrackerState::RecoveryPending(PendingRecovery {
                        active: pending.active,
                        proposed_end_ms,
                        unresolved_idle_start_ms: Some(pending.idle_start_ms),
                    })
                }
                state => state,
            };
        }
        Self { clock, snapshot }
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
            // Start a new timer with the selected project, task, and note.
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
            // Finish the running timer and save its elapsed time as an entry.
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
            // Update the details attached to the timer without interrupting it.
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
            // Record that the running timer is still alive at the current time.
            (TrackerState::Running(mut active), TrackerCommand::Heartbeat) => {
                active.last_heartbeat_ms = now.max(active.start_ms);
                TrackerState::Running(active)
            }
            // Pause normal tracking while the detected idle period awaits review.
            (TrackerState::Running(active), TrackerCommand::IdleDetected { idle_start_ms }) => {
                if idle_start_ms < active.start_ms || idle_start_ms > now {
                    return Err(DomainError::InvalidIdleStart {
                        active_start_ms: active.start_ms,
                        idle_start_ms,
                    });
                }
                TrackerState::IdlePending(PendingIdle {
                    active,
                    idle_start_ms,
                    return_ms: None,
                })
            }
            // Keep the active timer's heartbeat current while idle review is pending.
            (TrackerState::IdlePending(mut pending), TrackerCommand::Heartbeat) => {
                pending.active.last_heartbeat_ms = now.max(pending.active.start_ms);
                TrackerState::IdlePending(pending)
            }
            // Record when the user came back and ask them how to handle the idle time.
            (
                TrackerState::IdlePending(mut pending),
                TrackerCommand::UserReturned { return_ms },
            ) => {
                if return_ms < pending.idle_start_ms {
                    return Err(DomainError::InvalidReturn {
                        idle_start_ms: pending.idle_start_ms,
                        return_ms,
                    });
                }
                pending.return_ms = Some(return_ms);
                notifications.push(Notification::IdleNeedsResolution);
                TrackerState::IdlePending(pending)
            }
            // Apply the user's choice for keeping, removing, or reassigning idle time.
            (TrackerState::IdlePending(pending), TrackerCommand::ResolveIdle(decision)) => {
                let return_ms = pending.return_ms.ok_or_else(|| {
                    DomainError::InvalidState(TrackerState::IdlePending(pending.clone()))
                })?;
                resolve_idle(
                    pending,
                    decision,
                    return_ms,
                    monotonic_ms,
                    &mut completed_entries,
                    &mut notifications,
                )
            }
            // Save the recoverable portion of an interrupted timer, then resume or stop.
            (
                TrackerState::RecoveryPending(pending),
                TrackerCommand::ResolveRecovery { end_ms, resume },
            ) => {
                if end_ms < pending.active.start_ms || end_ms > pending.active.last_heartbeat_ms {
                    return Err(DomainError::InvalidRecoveryEnd {
                        start_ms: pending.active.start_ms,
                        last_heartbeat_ms: pending.active.last_heartbeat_ms,
                        end_ms,
                    });
                }
                push_entry(
                    &mut completed_entries,
                    &pending.active,
                    pending.active.start_ms,
                    end_ms,
                    EntrySource::Recovery,
                );
                notifications.push(Notification::RecoveryResolved);
                if resume {
                    TrackerState::Running(ActiveTimer {
                        start_ms: now,
                        started_monotonic_ms: monotonic_ms,
                        last_heartbeat_ms: now,
                        ..pending.active
                    })
                } else {
                    TrackerState::Stopped
                }
            }
            // Abandon the interrupted timer without creating a recovered entry.
            (TrackerState::RecoveryPending(_), TrackerCommand::DiscardRecovery) => {
                notifications.push(Notification::RecoveryResolved);
                TrackerState::Stopped
            }
            // Reject commands that are not valid for the tracker's current state.
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

/// Adds a completed time interval to the transition, skipping empty intervals.
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

/// Converts a reviewed idle period into entries and the next tracker state.
fn resolve_idle(
    pending: PendingIdle,
    decision: IdleDecision,
    return_ms: i64,
    monotonic_ms: u64,
    completed: &mut Vec<TimeEntry>,
    notifications: &mut Vec<Notification>,
) -> TrackerState {
    match decision {
        IdleDecision::Keep => TrackerState::Running(pending.active),
        IdleDecision::DiscardAndResume => {
            push_entry(
                completed,
                &pending.active,
                pending.active.start_ms,
                pending.idle_start_ms,
                EntrySource::Timer,
            );
            TrackerState::Running(resumed_active(pending.active, return_ms, monotonic_ms))
        }
        IdleDecision::ReassignAndResume {
            project_id,
            task_id,
            note,
        } => {
            push_entry(
                completed,
                &pending.active,
                pending.active.start_ms,
                pending.idle_start_ms,
                EntrySource::Timer,
            );
            let reassigned = ActiveTimer {
                project_id,
                task_id,
                note,
                start_ms: pending.idle_start_ms,
                started_monotonic_ms: 0,
                last_heartbeat_ms: return_ms,
            };
            push_entry(
                completed,
                &reassigned,
                pending.idle_start_ms,
                return_ms,
                EntrySource::IdleReassignment,
            );
            TrackerState::Running(resumed_active(pending.active, return_ms, monotonic_ms))
        }
        IdleDecision::Stop => {
            push_entry(
                completed,
                &pending.active,
                pending.active.start_ms,
                pending.idle_start_ms,
                EntrySource::Timer,
            );
            notifications.push(Notification::TimerStopped);
            TrackerState::Stopped
        }
    }
}

/// Restarts an active timer from the user's return time after an interruption.
fn resumed_active(mut active: ActiveTimer, return_ms: i64, monotonic_ms: u64) -> ActiveTimer {
    active.start_ms = return_ms;
    active.last_heartbeat_ms = return_ms;
    active.started_monotonic_ms = monotonic_ms;
    active
}
