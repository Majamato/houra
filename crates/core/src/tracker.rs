use serde::{Deserialize, Serialize};

use crate::{
    ActiveTimer, ActivityId, IdleDecision, Notification, PendingIdle, PendingRecovery, ProjectId,
    TimeEntry,
};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TrackerState {
    #[default]
    Stopped,
    Running(ActiveTimer),
    IdlePending(PendingIdle),
    RecoveryPending(PendingRecovery),
}

impl TrackerState {
    /// The timer being tracked, if any state carries one.
    pub fn active(&self) -> Option<&ActiveTimer> {
        match self {
            Self::Running(active) => Some(active),
            Self::IdlePending(pending) => Some(&pending.active),
            Self::RecoveryPending(pending) => Some(&pending.active),
            Self::Stopped => None,
        }
    }
}

/// The persisted tracker state plus a counter that grows with every accepted command.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct TrackerSnapshot {
    pub state: TrackerState,
    pub revision: u64,
}

/// A request to change the tracker state.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum TrackerCommand {
    Stop,
    Start {
        project_id: ProjectId,
        activity_id: Option<ActivityId>,
        note: String,
    },
    Switch {
        project_id: ProjectId,
        activity_id: Option<ActivityId>,
        note: String,
    },
    EditActive {
        project_id: ProjectId,
        activity_id: Option<ActivityId>,
        note: String,
    },
    IdleDetected {
        idle_start_ms: i64,
    },
    UserReturned {
        return_ms: i64,
    },
    ResolveIdle(IdleDecision),
    ResolveRecovery {
        end_ms: i64,
        resume: bool,
    },
    DiscardRecovery,
    Heartbeat,
}

/// The result of applying one command.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Transition {
    pub snapshot: TrackerSnapshot,
    pub completed_entries: Vec<TimeEntry>,
    pub notifications: Vec<Notification>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn active_access_covers_every_state() {
        let active = ActiveTimer {
            project_id: ProjectId(1),
            activity_id: None,
            note: String::new(),
            start_ms: 1,
            started_monotonic_ms: 0,
            last_heartbeat_ms: 2,
        };
        assert_eq!(TrackerState::Stopped.active(), None);
        for state in [
            TrackerState::Running(active.clone()),
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
