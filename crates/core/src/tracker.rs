use serde::{Deserialize, Serialize};

use crate::{
    ActiveTimer, ActivityId, EntryId, IdleDecision, Notification, PausedTimer, PendingIdle,
    PendingRecovery, ProjectId, TimeEntry,
};

/// What the tracker is doing right now; only one state is active at a time.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TrackerState {
    /// Nothing is being tracked. Only `Start` and `Continue` lead out of here.
    #[default]
    Stopped,
    /// A timer is running normally; heartbeats keep it alive.
    Running(ActiveTimer),
    /// The user paused the timer; elapsed time stays frozen until resume.
    Paused(PausedTimer),
    /// The user went away mid-timer; tracking pauses until they resolve it.
    IdlePending(PendingIdle),
    /// The app stopped uncleanly with a live timer; the user must resolve it.
    RecoveryPending(PendingRecovery),
}

impl TrackerState {
    /// The timer being tracked, if any state carries one.
    pub fn active(&self) -> Option<&ActiveTimer> {
        match self {
            Self::Running(active) => Some(active),
            Self::Paused(paused) => Some(&paused.active),
            Self::IdlePending(pending) => Some(&pending.active),
            Self::RecoveryPending(pending) => Some(&pending.active),
            Self::Stopped => None,
        }
    }
}

/// The persisted tracker state plus a counter that grows with every accepted command.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct TrackerSnapshot {
    /// Reloaded on startup to resume tracking where it left off.
    pub state: TrackerState,
    /// Increments on every accepted command; storage persists only when it changes.
    pub revision: u64,
}

/// A request to change the tracker state.
/// The engine applies it atomically or rejects it, leaving state unchanged.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum TrackerCommand {
    /// Finishes the running timer and saves its elapsed time as an entry.
    Stop,
    /// Banks the current segment and freezes the timer; the entry stays open.
    Pause,
    /// Starts a new segment on the paused timer; the counter continues
    /// from the banked total, excluding the pause gap.
    Resume,
    /// Begins a new timer, creating a fresh time entry on stop.
    Start {
        /// Project the new timer records against.
        project_id: ProjectId,
        /// Kind of work, if the user picked one.
        activity_id: Option<ActivityId>,
        /// Note saved on the entry created at stop.
        note: String,
    },
    /// Stops the current timer and immediately starts another one.
    Switch {
        /// Project the new timer records against.
        project_id: ProjectId,
        /// Kind of work, if the user picked one.
        activity_id: Option<ActivityId>,
        /// Note saved on the entry created at stop.
        note: String,
    },
    /// Resumes timing under an existing entry, keeping its identity.
    Continue {
        /// Entry whose identity the resumed timer keeps.
        entry_id: EntryId,
        /// Project recorded on the resumed timer.
        project_id: ProjectId,
        /// Kind of work, if the entry has one.
        activity_id: Option<ActivityId>,
        /// Note recorded on the resumed timer.
        note: String,
    },
    /// Changes the running timer's details without interrupting it.
    EditActive {
        /// Replacement project for the running timer.
        project_id: ProjectId,
        /// Replacement activity for the running timer, if any.
        activity_id: Option<ActivityId>,
        /// Replacement note for the running timer.
        note: String,
    },
    /// Pauses tracking while the user's absence awaits review.
    IdleDetected {
        /// When the absence began; must fall inside the running timer.
        idle_start_ms: i64,
    },
    /// Records the user's return so the idle period can be resolved.
    UserReturned {
        /// When the user came back; must not precede the idle start.
        return_ms: i64,
    },
    /// Applies the user's decision for the idle period.
    ResolveIdle(IdleDecision),
    /// Saves the recoverable portion of an interrupted timer.
    ResolveRecovery {
        /// Trusted end of the recovered portion, within the heartbeats.
        end_ms: i64,
        /// Whether to restart the timer after saving the recovered portion.
        resume: bool,
    },
    /// Abandons the interrupted timer without saving anything.
    DiscardRecovery,
    /// Proves the timer is still alive; bounds what recovery can save.
    Heartbeat,
}

/// The result of applying one command.
/// Carries the completed entries to persist and the notifications to deliver.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Transition {
    /// State after the command; unchanged when the command was a no-op.
    pub snapshot: TrackerSnapshot,
    /// Finished time entries the caller must persist.
    pub completed_entries: Vec<TimeEntry>,
    /// UI signals produced by the command, such as timer started.
    pub notifications: Vec<Notification>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn active_access_covers_every_state() {
        let active = ActiveTimer {
            entry_id: None,
            project_id: ProjectId(1),
            activity_id: None,
            note: String::new(),
            start_ms: 1,
            started_monotonic_ms: 0,
            last_heartbeat_ms: 2,
            accumulated_ms: 0,
        };
        assert_eq!(TrackerState::Stopped.active(), None);
        for state in [
            TrackerState::Running(active.clone()),
            TrackerState::Paused(PausedTimer {
                active: active.clone(),
                paused_at_ms: 2,
            }),
            TrackerState::IdlePending(PendingIdle {
                active: active.clone(),
                idle_start_ms: 1,
                return_ms: None,
            }),
            TrackerState::RecoveryPending(PendingRecovery {
                active: active.clone(),
                proposed_end_ms: 2,
                unresolved_idle_start_ms: None,
            }),
        ] {
            assert_eq!(state.active(), Some(&active));
        }
    }
}
