use std::fs;
use std::path::Path;

use houra_core::{Project, Task, TimeEntry, TrackerSnapshot, validate_no_overlaps};
use serde::{Deserialize, Serialize};

use crate::AppError;

pub const BACKUP_VERSION: u32 = 1;

/// A complete copy of the database as one JSON document.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackupDocument {
    pub format: String,
    pub version: u32,
    pub exported_at_ms: i64,
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub entries: Vec<TimeEntry>,
    pub tracker: TrackerSnapshot,
}

impl BackupDocument {
    /// Parses and validates; an internally inconsistent backup is rejected here.
    pub fn read_from_path(path: &Path) -> Result<Self, AppError> {
        let bytes = fs::read(path).map_err(|source| AppError::io(path, source))?;
        let document: Self = serde_json::from_slice(&bytes)?;
        document.validate()?;
        Ok(document)
    }

    /// Writes atomically: temp file, fsync, then rename into place.
    pub fn write_to_path(&self, path: &Path) -> Result<(), AppError> {
        self.validate()?;
        let Some(parent) = path.parent() else {
            return Err(AppError::DataDirectoryUnavailable);
        };
        fs::create_dir_all(parent).map_err(|source| AppError::io(parent, source))?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .map_err(|source| AppError::io(parent, source))?;
        serde_json::to_writer_pretty(temporary.as_file_mut(), self)?;
        temporary
            .as_file_mut()
            .sync_all()
            .map_err(|source| AppError::io(path, source))?;
        temporary
            .persist(path)
            .map_err(|error| AppError::io(path, error.error))?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), AppError> {
        if self.format != "houra-backup" {
            return Err(AppError::InvalidBackup("unknown format marker".into()));
        }
        if self.version != BACKUP_VERSION {
            return Err(AppError::UnsupportedBackupVersion {
                found: self.version,
                expected: BACKUP_VERSION,
            });
        }
        if self.tracker.state.active().is_some() {
            return Err(AppError::InvalidBackup(
                "backups must not contain an active timer".into(),
            ));
        }
        if !self
            .projects
            .iter()
            .any(|project| project.id == houra_core::ProjectId(1))
        {
            return Err(AppError::InvalidBackup(
                "the required General project is missing".into(),
            ));
        }
        for project in &self.projects {
            project.validate()?;
        }
        for task in &self.tasks {
            task.validate()?;
            if !self
                .projects
                .iter()
                .any(|project| project.id == task.project_id)
            {
                return Err(AppError::InvalidBackup(format!(
                    "task {:?} references a missing project",
                    task.id
                )));
            }
        }
        for entry in &self.entries {
            entry.validate()?;
            if !self
                .projects
                .iter()
                .any(|project| project.id == entry.project_id)
            {
                return Err(AppError::InvalidBackup(format!(
                    "entry {:?} references a missing project",
                    entry.id
                )));
            }
            if let Some(task_id) = entry.task_id {
                let task_matches = self
                    .tasks
                    .iter()
                    .any(|task| task.id == task_id && task.project_id == entry.project_id);
                if !task_matches {
                    return Err(AppError::InvalidBackup(format!(
                        "entry {:?} has a missing or foreign task",
                        entry.id
                    )));
                }
            }
        }
        validate_no_overlaps(&self.entries)?;
        Ok(())
    }
}
