use serde::{Deserialize, Serialize};

use crate::{ActivityId, ProjectId};

/// How the user wants an idle interval to be counted.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum IdleDecision {
    Keep,
    DiscardAndResume,
    ReassignAndResume {
        project_id: ProjectId,
        activity_id: Option<ActivityId>,
        note: String,
    },
    Stop,
}
