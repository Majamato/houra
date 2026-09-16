use serde::{Deserialize, Serialize};

use crate::DomainError;
use crate::id::{ProjectId, TaskId};
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
pub struct Task {
    pub id: TaskId,
    pub project_id: ProjectId,
    pub name: String,
    pub archived: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl Task {
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_name(&self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_and_task_validate_names_and_project_color() {
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
        let mut task = Task {
            id: TaskId(1),
            project_id: project.id,
            name: "Task".into(),
            archived: true,
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        assert_eq!(task.validate(), Ok(()));
        task.name.clear();
        assert_eq!(task.validate(), Err(DomainError::EmptyName));
    }
}
