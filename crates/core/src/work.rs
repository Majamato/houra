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
