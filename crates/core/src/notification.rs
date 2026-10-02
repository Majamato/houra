use serde::{Deserialize, Serialize};

/// Something the interface may want to tell the user after a transition.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum Notification {
    /// A timer began; the UI can show it running.
    TimerStarted,
    /// A timer finished and its time was saved.
    TimerStopped,
    /// A timer was paused; the UI can show it frozen.
    TimerPaused,
    /// A paused timer runs again; the UI can show it running.
    TimerResumed,
    /// The user went away mid-timer; the tracker now waits on them.
    IdleDetected,
    /// The user is back and must decide how the idle time counts.
    IdleNeedsResolution,
    /// The interrupted timer was resolved and needs no more attention.
    RecoveryResolved,
}
