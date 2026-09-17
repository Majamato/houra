use serde::{Deserialize, Serialize};

use crate::DomainError;
use crate::id::{ActivityId, ProjectId};
use crate::validation::{validate_color, validate_name};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub color: String,
    pub archived: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl Project {
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_name(&self.name)?;
        validate_color(&self.color)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Activity {
    pub id: ActivityId,
    pub name: String,
    pub archived: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl Activity {
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
