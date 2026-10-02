use std::time::Duration;

use crate::{
    ActiveTimer, Clock, DomainError, EntryId, EntrySource, IdleDecision, Notification, PausedTimer,
    PendingIdle, PendingRecovery, TimeEntry, TrackedInterval, TrackerCommand, TrackerSnapshot,
    TrackerState, Transition,
};

/// Owns tracker state and applies commands against a clock.
/// State changes only through `apply`, which leaves the engine untouched on error.
#[derive(Clone, Debug)]
pub struct TrackerEngine<C> {
    clock: C,
    snapshot: TrackerSnapshot,
}

impl<C: Clock> TrackerEngine<C> {
    /// Creates an engine with a fresh, stopped tracker.
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

    /// Borrows the current state for persistence and inspection.
    pub fn snapshot(&self) -> &TrackerSnapshot {
        &self.snapshot
    }

    /// Records the entry identity storage assigned to the segment Pause banked.
    /// Only fills a missing identity on a paused timer; anything else is a no-op.
    /// The revision stays untouched so memory and the persisted snapshot agree.
    pub fn adopt_paused_entry_id(&mut self, entry_id: EntryId) {
        if let TrackerState::Paused(paused) = &mut self.snapshot.state
            && paused.active.entry_id.is_none()
        {
            paused.active.entry_id = Some(entry_id);
        }
    }

    /// Time shown by the live counter; zero when nothing is running.
    /// Covers the whole run: banked pause segments plus the live one.
    pub fn live_elapsed(&self) -> Duration {
        if let TrackerState::Paused(paused) = &self.snapshot.state {
            return accumulated(paused.active.accumulated_ms);
        }
        let Some(active) = self.snapshot.state.active() else {
            return Duration::ZERO;
        };
        let started = Duration::from_millis(active.started_monotonic_ms);
        accumulated(active.accumulated_ms)
            .saturating_add(self.clock.monotonic().saturating_sub(started))
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
                    activity_id,
                    note,
                },
            ) => {
                notifications.push(Notification::TimerStarted);
                TrackerState::Running(ActiveTimer {
                    entry_id: None,
                    project_id,
                    activity_id,
                    note,
                    start_ms: now,
                    started_monotonic_ms: monotonic_ms,
                    last_heartbeat_ms: now,
                    accumulated_ms: 0,
                })
            }
            (
                TrackerState::Stopped,
                TrackerCommand::Continue {
                    entry_id,
                    project_id,
                    activity_id,
                    note,
                },
            ) => {
                notifications.push(Notification::TimerStarted);
                TrackerState::Running(ActiveTimer {
                    entry_id: Some(entry_id),
                    project_id,
                    activity_id,
                    note,
                    start_ms: now,
                    started_monotonic_ms: monotonic_ms,
                    last_heartbeat_ms: now,
                    accumulated_ms: 0,
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
                TrackerState::Running(active),
                TrackerCommand::Switch {
                    project_id,
                    activity_id,
                    note,
                },
            ) => {
                push_entry(
                    &mut completed_entries,
                    &active,
                    active.start_ms,
                    now,
                    EntrySource::Timer,
                );
                notifications.push(Notification::TimerStopped);
                notifications.push(Notification::TimerStarted);
                TrackerState::Running(ActiveTimer {
                    entry_id: None,
                    project_id,
                    activity_id,
                    note,
                    start_ms: now,
                    started_monotonic_ms: monotonic_ms,
                    last_heartbeat_ms: now,
                    accumulated_ms: 0,
                })
            }
            (
                TrackerState::Running(active),
                TrackerCommand::Continue {
                    entry_id,
                    project_id,
                    activity_id,
                    note,
                },
            ) => {
                if active.entry_id == Some(entry_id) {
                    return Ok(Transition {
                        snapshot: self.snapshot.clone(),
                        completed_entries,
                        notifications,
                    });
                }
                push_entry(
                    &mut completed_entries,
                    &active,
                    active.start_ms,
                    now,
                    EntrySource::Timer,
                );
                notifications.push(Notification::TimerStopped);
                notifications.push(Notification::TimerStarted);
                TrackerState::Running(ActiveTimer {
                    entry_id: Some(entry_id),
                    project_id,
                    activity_id,
                    note,
                    start_ms: now,
                    started_monotonic_ms: monotonic_ms,
                    last_heartbeat_ms: now,
                    accumulated_ms: 0,
                })
            }
            (
                TrackerState::Running(mut active),
                TrackerCommand::EditActive {
                    project_id,
                    activity_id,
                    note,
                },
            ) => {
                active.project_id = project_id;
                active.activity_id = activity_id;
                active.note = note;
                active.last_heartbeat_ms = now.max(active.start_ms);
                TrackerState::Running(active)
            }
            (TrackerState::Running(mut active), TrackerCommand::Heartbeat) => {
                active.last_heartbeat_ms = now.max(active.start_ms);
                TrackerState::Running(active)
            }
            (TrackerState::Running(mut active), TrackerCommand::Pause) => {
                push_entry(
                    &mut completed_entries,
                    &active,
                    active.start_ms,
                    now,
                    EntrySource::Timer,
                );
                let segment_start = active.start_ms;
                bank_segment(&mut active, segment_start, now);
                notifications.push(Notification::TimerPaused);
                TrackerState::Paused(PausedTimer {
                    active,
                    paused_at_ms: now,
                })
            }
            (TrackerState::Paused(paused), TrackerCommand::Resume) => {
                notifications.push(Notification::TimerResumed);
                TrackerState::Running(ActiveTimer {
                    entry_id: paused.active.entry_id,
                    project_id: paused.active.project_id,
                    activity_id: paused.active.activity_id,
                    note: paused.active.note,
                    start_ms: now,
                    started_monotonic_ms: monotonic_ms,
                    last_heartbeat_ms: now,
                    accumulated_ms: paused.active.accumulated_ms,
                })
            }
            // The paused segment was already banked by Pause; nothing left to save.
            (TrackerState::Paused(_), TrackerCommand::Stop) => {
                notifications.push(Notification::TimerStopped);
                TrackerState::Stopped
            }
            (
                TrackerState::Paused(_),
                TrackerCommand::Switch {
                    project_id,
                    activity_id,
                    note,
                },
            ) => {
                notifications.push(Notification::TimerStarted);
                TrackerState::Running(ActiveTimer {
                    entry_id: None,
                    project_id,
                    activity_id,
                    note,
                    start_ms: now,
                    started_monotonic_ms: monotonic_ms,
                    last_heartbeat_ms: now,
                    accumulated_ms: 0,
                })
            }
            (
                TrackerState::Paused(paused),
                TrackerCommand::Continue {
                    entry_id,
                    project_id,
                    activity_id,
                    note,
                },
            ) => {
                if paused.active.entry_id == Some(entry_id) {
                    return Ok(Transition {
                        snapshot: self.snapshot.clone(),
                        completed_entries,
                        notifications,
                    });
                }
                notifications.push(Notification::TimerStarted);
                TrackerState::Running(ActiveTimer {
                    entry_id: Some(entry_id),
                    project_id,
                    activity_id,
                    note,
                    start_ms: now,
                    started_monotonic_ms: monotonic_ms,
                    last_heartbeat_ms: now,
                    accumulated_ms: 0,
                })
            }
            (
                TrackerState::Paused(mut paused),
                TrackerCommand::EditActive {
                    project_id,
                    activity_id,
                    note,
                },
            ) => {
                paused.active.project_id = project_id;
                paused.active.activity_id = activity_id;
                paused.active.note = note;
                TrackerState::Paused(paused)
            }
            (TrackerState::Paused(_), TrackerCommand::Heartbeat) => {
                return Ok(Transition {
                    snapshot: self.snapshot.clone(),
                    completed_entries,
                    notifications,
                });
            }
            (TrackerState::Running(active), TrackerCommand::IdleDetected { idle_start_ms }) => {
                if idle_start_ms < active.start_ms || idle_start_ms > now {
                    return Err(DomainError::InvalidIdleStart {
                        active_start_ms: active.start_ms,
                        idle_start_ms,
                    });
                }
                notifications.push(Notification::IdleDetected);
                TrackerState::IdlePending(PendingIdle {
                    active,
                    idle_start_ms,
                    return_ms: None,
                })
            }
            (TrackerState::IdlePending(mut pending), TrackerCommand::Heartbeat) => {
                pending.active.last_heartbeat_ms = now.max(pending.active.start_ms);
                TrackerState::IdlePending(pending)
            }
            (
                TrackerState::IdlePending(mut pending),
                TrackerCommand::UserReturned { return_ms },
            ) => {
                if pending.return_ms.is_some() {
                    return Ok(Transition {
                        snapshot: self.snapshot.clone(),
                        completed_entries,
                        notifications,
                    });
                }
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
            (TrackerState::IdlePending(pending), TrackerCommand::ResolveIdle(decision)) => {
                let return_ms = pending.return_ms.ok_or_else(|| {
                    DomainError::InvalidState(Box::new(TrackerState::IdlePending(pending.clone())))
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
                    let mut active = pending.active;
                    let segment_start = active.start_ms;
                    bank_segment(&mut active, segment_start, end_ms);
                    active.start_ms = now;
                    active.started_monotonic_ms = monotonic_ms;
                    active.last_heartbeat_ms = now;
                    TrackerState::Running(active)
                } else {
                    TrackerState::Stopped
                }
            }
            (TrackerState::RecoveryPending(_), TrackerCommand::DiscardRecovery) => {
                notifications.push(Notification::RecoveryResolved);
                TrackerState::Stopped
            }
            (state, _) => return Err(DomainError::InvalidState(Box::new(state))),
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

/// Reads banked run time as a duration, tolerating corrupt values.
fn accumulated(accumulated_ms: i64) -> Duration {
    Duration::from_millis(u64::try_from(accumulated_ms.max(0)).unwrap_or(u64::MAX))
}

/// Adds the banked wall-clock segment to the run total, tolerating reversals.
fn bank_segment(active: &mut ActiveTimer, start_ms: i64, end_ms: i64) {
    active.accumulated_ms = active
        .accumulated_ms
        .saturating_add(end_ms.saturating_sub(start_ms).max(0));
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
        id: active.entry_id,
        project_id: active.project_id,
        activity_id: active.activity_id,
        note: active.note.clone(),
        intervals: vec![TrackedInterval {
            id: None,
            start_ms,
            end_ms,
            source,
        }],
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
            TrackerState::Running(resumed_active(
                pending.active,
                pending.idle_start_ms,
                return_ms,
                monotonic_ms,
            ))
        }
        IdleDecision::ReassignAndResume {
            project_id,
            activity_id,
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
                entry_id: None,
                project_id,
                activity_id,
                note,
                start_ms: pending.idle_start_ms,
                started_monotonic_ms: 0,
                last_heartbeat_ms: return_ms,
                accumulated_ms: 0,
            };
            push_entry(
                completed,
                &reassigned,
                pending.idle_start_ms,
                return_ms,
                EntrySource::IdleReassignment,
            );
            TrackerState::Running(resumed_active(
                pending.active,
                pending.idle_start_ms,
                return_ms,
                monotonic_ms,
            ))
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
/// The banked prefix stays in the run total; only the idle gap is dropped.
fn resumed_active(
    mut active: ActiveTimer,
    idle_start_ms: i64,
    return_ms: i64,
    monotonic_ms: u64,
) -> ActiveTimer {
    let segment_start = active.start_ms;
    bank_segment(&mut active, segment_start, idle_start_ms);
    active.start_ms = return_ms;
    active.last_heartbeat_ms = return_ms;
    active.started_monotonic_ms = monotonic_ms;
    active
}
