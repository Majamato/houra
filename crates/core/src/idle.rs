use serde::{Deserialize, Serialize};

use crate::{ActivityId, ProjectId};

/// How the user wants an idle interval to be counted.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum IdleDecision {
    /// Counts the idle period as tracked time, as if the user never left.
    Keep,
    /// Drops the idle period and restarts the timer at the user's return.
    DiscardAndResume,
    /// Banks the idle period as its own entry, then resumes the timer.
    ReassignAndResume {
        /// Project credited with the idle period.
        project_id: ProjectId,
        /// Activity credited with the idle period, if any.
        activity_id: Option<ActivityId>,
        /// Note saved on the entry created for the idle period.
        note: String,
    },
    /// Ends the timer at the idle start, dropping everything after.
    Stop,
}
