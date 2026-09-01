use serde::{Deserialize, Serialize};

/// Something the interface may want to tell the user after a transition.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum Notification {
    TimerStarted,
    TimerStopped,
    IdleNeedsResolution,
    RecoveryNeedsResolution,
    RecoveryResolved,
}
