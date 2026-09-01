use serde::{Deserialize, Serialize};

use crate::{
    ActiveTimer, IdleDecision, Notification, PendingIdle, PendingRecovery, ProjectId, TaskId,
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
        task_id: Option<TaskId>,
        note: String,
    },
    EditActive {
        project_id: ProjectId,
        task_id: Option<TaskId>,
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
