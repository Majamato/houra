//! Project and activity records.

use crate::AppError;
use houra_core::{Activity, ActivityId, Project, ProjectId};
use rusqlite::{Connection, params};

use super::Store;

impl Store {
    /// Project 1, "General", always exists; UI and backups rely on it.
    pub(super) fn ensure_general_project(&self) -> Result<(), AppError> {
        self.connection.execute(
            "INSERT OR IGNORE INTO projects(id, name, color, archived, created_at_ms, updated_at_ms)
             VALUES(1, 'General', '#3584e4', 0, unixepoch('subsec') * 1000, unixepoch('subsec') * 1000)",
            [],
        )?;
        Ok(())
    }

    pub fn list_projects(&self, include_archived: bool) -> Result<Vec<Project>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, color, archived, created_at_ms, updated_at_ms FROM projects
             WHERE ?1 OR archived = 0 ORDER BY archived, name COLLATE NOCASE",
        )?;
        let rows = statement.query_map([include_archived], |row| {
            Ok(Project {
                id: ProjectId(row.get(0)?),
                name: row.get(1)?,
                color: row.get(2)?,
                archived: row.get(3)?,
                created_at_ms: row.get(4)?,
                updated_at_ms: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn set_project_archived(
        &self,
        id: ProjectId,
        archived: bool,
        now_ms: i64,
    ) -> Result<(), AppError> {
        if id == ProjectId(1) && archived {
            return Err(AppError::InvalidBackup("General cannot be archived".into()));
        }
        self.connection.execute(
            "UPDATE projects SET archived=?1, updated_at_ms=?2 WHERE id=?3",
            params![archived, now_ms, id.0],
        )?;
        Ok(())
    }

    pub fn set_activity_archived(
        &self,
        id: ActivityId,
        archived: bool,
        now_ms: i64,
    ) -> Result<(), AppError> {
        self.connection.execute(
            "UPDATE activities SET archived=?1, updated_at_ms=?2 WHERE id=?3",
            params![archived, now_ms, id.0],
        )?;
        Ok(())
    }

    pub fn delete_project_permanently(&self, id: ProjectId) -> Result<(), AppError> {
        if id == ProjectId(1) {
            return Err(AppError::ReferencedItem);
        }
        let references: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE project_id=?1)",
            [id.0],
            |row| row.get(0),
        )?;
        if references {
            return Err(AppError::ReferencedItem);
        }
        self.connection
            .execute("DELETE FROM projects WHERE id=?1", [id.0])?;
        Ok(())
    }

    pub fn delete_activity_permanently(&self, id: ActivityId) -> Result<(), AppError> {
        let references: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE activity_id=?1)",
            [id.0],
            |row| row.get(0),
        )?;
        if references {
            return Err(AppError::ReferencedItem);
        }
        self.connection
            .execute("DELETE FROM activities WHERE id=?1", [id.0])?;
        Ok(())
    }

    pub fn list_activities(&self, include_archived: bool) -> Result<Vec<Activity>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, project_id, name, archived, created_at_ms, updated_at_ms FROM activities
             WHERE ?1 OR archived = 0 ORDER BY project_id, archived, name COLLATE NOCASE",
        )?;
        let rows = statement.query_map([include_archived], |row| {
            Ok(Activity {
                id: ActivityId(row.get(0)?),
                project_id: ProjectId(row.get(1)?),
                name: row.get(2)?,
                archived: row.get(3)?,
                created_at_ms: row.get(4)?,
                updated_at_ms: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn create_project(
        &self,
        name: &str,
        color: &str,
        now_ms: i64,
    ) -> Result<ProjectId, AppError> {
        let project = Project {
            id: ProjectId(0),
            name: name.trim().to_owned(),
            color: color.to_owned(),
            archived: false,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        };
        project.validate()?;
        self.connection.execute(
            "INSERT INTO projects(name, color, archived, created_at_ms, updated_at_ms)
             VALUES(?1, ?2, 0, ?3, ?3)",
            params![project.name, project.color, now_ms],
        )?;
        Ok(ProjectId(self.connection.last_insert_rowid()))
    }

    pub fn create_activity(
        &self,
        project_id: ProjectId,
        name: &str,
        now_ms: i64,
    ) -> Result<ActivityId, AppError> {
        validate_project(&self.connection, project_id)?;
        let trimmed = name.trim();
        houra_core::validate_name(trimmed)?;
        self.connection.execute(
            "INSERT INTO activities(project_id, name, archived, created_at_ms, updated_at_ms)
             VALUES(?1, ?2, 0, ?3, ?3)",
            params![project_id.0, trimmed, now_ms],
        )?;
        Ok(ActivityId(self.connection.last_insert_rowid()))
    }
}

pub(super) fn validate_project(connection: &Connection, id: ProjectId) -> Result<(), AppError> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1 AND archived=0)",
        [id.0],
        |row| row.get(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(AppError::InvalidProject(id))
    }
}
