use serde::{Deserialize, Serialize};

use crate::DomainError;
use crate::id::{ActivityId, ProjectId};
use crate::validation::{validate_color, validate_name};

/// What the work is for.
/// Archived projects stop receiving new time.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Project {
    /// Stable identity used by entries and the tracker.
    pub id: ProjectId,
    /// Display name; must contain a non-whitespace character.
    pub name: String,
    /// CSS hexadecimal color shown in the UI.
    pub color: String,
    /// Archived projects no longer receive new time.
    pub archived: bool,
    /// Wall-clock creation time, in milliseconds.
    pub created_at_ms: i64,
    /// Wall-clock time of the last change, in milliseconds.
    pub updated_at_ms: i64,
}

impl Project {
    /// Rejects blank names and non-hex colors.
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_name(&self.name)?;
        validate_color(&self.color)
    }
}

/// The kind of work being done, independent of any project.
/// Usable with any project; archived activities stop receiving new time.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Activity {
    /// Stable identity used by entries and the tracker.
    pub id: ActivityId,
    /// Display name; must contain a non-whitespace character.
    pub name: String,
    /// Archived activities no longer receive new time.
    pub archived: bool,
    /// Wall-clock creation time, in milliseconds.
    pub created_at_ms: i64,
    /// Wall-clock time of the last change, in milliseconds.
    pub updated_at_ms: i64,
}

impl Activity {
    /// Rejects blank names.
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_name(&self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_and_activity_validate_names_and_project_color() {
        let mut project = Project {
            id: ProjectId(1),
            name: "Work".into(),
            color: "#123abc".into(),
            archived: true,
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        assert_eq!(project.validate(), Ok(()));
        project.color = "red".into();
        assert_eq!(project.validate(), Err(DomainError::InvalidColor));
        project.name = " ".into();
        assert_eq!(project.validate(), Err(DomainError::EmptyName));
        let mut activity = Activity {
            id: ActivityId(1),
            name: "Activity".into(),
            archived: true,
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        assert_eq!(activity.validate(), Ok(()));
        activity.name.clear();
        assert_eq!(activity.validate(), Err(DomainError::EmptyName));
    }
}
