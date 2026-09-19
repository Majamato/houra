//! Export and transactional restoration of stored records.

use super::{entries::source_name, snapshots::write_snapshot};
use crate::{
    AppError,
    backup::{BACKUP_VERSION, BackupDocument},
};
use rusqlite::params;

use super::Store;

impl Store {
    pub fn backup(&self, exported_at_ms: i64) -> Result<BackupDocument, AppError> {
        Ok(BackupDocument {
            format: "houra-backup".into(),
            version: BACKUP_VERSION,
            exported_at_ms,
            projects: self.list_projects(true)?,
            activities: self.list_activities(true)?,
            entries: self.list_all_entries()?,
            tracker: self.load_snapshot()?,
        })
    }

    /// Replaces every table from a validated backup inside one transaction.
    pub fn restore(&mut self, document: &BackupDocument) -> Result<(), AppError> {
        document.validate()?;
        if self.load_snapshot()?.state.active().is_some() {
            return Err(AppError::RestoreWhileActive);
        }
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM tracker_state", [])?;
        transaction.execute("DELETE FROM entry_intervals", [])?;
        transaction.execute("DELETE FROM entries", [])?;
        transaction.execute("DELETE FROM activities", [])?;
        transaction.execute("DELETE FROM projects", [])?;
        for project in &document.projects {
            transaction.execute(
                "INSERT INTO projects(id,name,color,archived,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6)",
                params![project.id.0, project.name, project.color, project.archived, project.created_at_ms, project.updated_at_ms],
            )?;
        }
        for activity in &document.activities {
            transaction.execute(
                "INSERT INTO activities(id,name,archived,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5)",
                params![activity.id.0, activity.name, activity.archived, activity.created_at_ms, activity.updated_at_ms],
            )?;
        }
        for entry in &document.entries {
            transaction.execute(
                "INSERT INTO entries(id,project_id,activity_id,note,created_at_ms,updated_at_ms)
                 VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    entry.id.map(|id| id.0),
                    entry.project_id.0,
                    entry.activity_id.map(|id| id.0),
                    entry.note,
                    entry.created_at_ms,
                    entry.updated_at_ms
                ],
            )?;
            let entry_id = entry
                .id
                .ok_or_else(|| AppError::InvalidBackup("stored entry ID is required".into()))?;
            for interval in &entry.intervals {
                interval.validate()?;
                transaction.execute(
                    "INSERT INTO entry_intervals(id,entry_id,start_ms,end_ms,source) VALUES(?1,?2,?3,?4,?5)",
                    params![interval.id.map(|id| id.0), entry_id.0, interval.start_ms, interval.end_ms, source_name(interval.source)],
                )?;
            }
        }
        write_snapshot(&transaction, &document.tracker)?;
        transaction.execute(
            "INSERT INTO meta(key,value) VALUES('last_restore_ms',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [document.exported_at_ms.to_string()],
        )?;
        transaction.commit()?;
        Ok(())
    }
}
