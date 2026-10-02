use thiserror::Error;

use crate::{TrackerState, id::EntryId};

/// A rejected domain operation. Infrastructure errors belong in the app crate.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    /// The command does not apply to the current tracker state.
    /// Boxed: the state is large and errors are cold paths.
    #[error("the tracker is not in the required state (current state: {0:?})")]
    InvalidState(Box<TrackerState>),

    /// An interval ends at or before it starts.
    #[error("end time {end_ms} must be after start time {start_ms}")]
    InvalidInterval {
        /// Start of the rejected interval.
        start_ms: i64,
        /// End of the rejected interval, not after the start.
        end_ms: i64,
    },

    /// The reported idle start falls outside the running timer.
    #[error(
        "idle start {idle_start_ms} is outside the active interval beginning {active_start_ms}"
    )]
    InvalidIdleStart {
        /// Start of the running timer; the idle start must not precede it.
        active_start_ms: i64,
        /// Reported idle start that fell outside the running timer.
        idle_start_ms: i64,
    },

    /// The reported return precedes the idle start.
    #[error("return time {return_ms} is before idle start {idle_start_ms}")]
    InvalidReturn {
        /// Idle start the return was checked against.
        idle_start_ms: i64,
        /// Reported return that preceded the idle start.
        return_ms: i64,
    },

    /// New intervals collide with already stored entries.
    #[error("this time overlaps an existing time entry; each minute can only be tracked once")]
    Overlap {
        /// Stored entries the new intervals collide with.
        conflicts: Vec<EntryId>,
    },

    /// A project or activity name is blank or whitespace-only.
    #[error("name must contain a non-whitespace character")]
    EmptyName,

    /// A project color is not a CSS hexadecimal color.
    #[error("color must be a CSS hexadecimal color such as #3584e4")]
    InvalidColor,

    /// The chosen recovery end falls outside the trusted range.
    #[error("recovery end {end_ms} is outside [{start_ms}, {last_heartbeat_ms}]")]
    InvalidRecoveryEnd {
        /// Timer start; the recovery end must not precede it.
        start_ms: i64,
        /// Last proof the timer was alive; the recovery end must not pass it.
        last_heartbeat_ms: i64,
        /// Chosen end that fell outside the trusted range.
        end_ms: i64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_message_names_no_internal_ids() {
        assert_eq!(
            DomainError::Overlap {
                conflicts: vec![EntryId(1), EntryId(2)],
            }
            .to_string(),
            "this time overlaps an existing time entry; each minute can only be tracked once"
        );
    }
}
