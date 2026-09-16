//! Database schema creation and version upgrades.

use crate::AppError;

const SCHEMA_VERSION: i64 = 1;

use super::Store;

impl Store {
    pub(super) fn migrate(&mut self) -> Result<(), AppError> {
        let version: i64 = self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(AppError::InvalidBackup(format!(
                "database schema {version} is newer than supported {SCHEMA_VERSION}"
            )));
        }
        if version < 1 {
            let transaction = self.connection.transaction()?;
            transaction.execute_batch(
                "
                CREATE TABLE meta (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                CREATE TABLE projects (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL COLLATE NOCASE UNIQUE CHECK(length(trim(name)) > 0),
                    color TEXT NOT NULL,
                    archived INTEGER NOT NULL DEFAULT 0 CHECK(archived IN (0, 1)),
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );
                CREATE TABLE activities (
                    id INTEGER PRIMARY KEY,
                    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
                    name TEXT NOT NULL COLLATE NOCASE CHECK(length(trim(name)) > 0),
                    archived INTEGER NOT NULL DEFAULT 0 CHECK(archived IN (0, 1)),
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    UNIQUE(project_id, name)
                );
                CREATE TABLE entries (
                    id INTEGER PRIMARY KEY,
                    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
                    activity_id INTEGER REFERENCES activities(id) ON DELETE RESTRICT,
                    note TEXT NOT NULL DEFAULT '',
                    start_ms INTEGER NOT NULL,
                    end_ms INTEGER NOT NULL CHECK(end_ms > start_ms),
                    source TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );
                CREATE INDEX entries_interval ON entries(start_ms, end_ms);
                CREATE TRIGGER entries_no_overlap_insert BEFORE INSERT ON entries
                -- Half-open intervals overlap iff new.start < old.end and
                -- new.end > old.start; adjacent boundaries are therefore legal.
                WHEN EXISTS (
                    SELECT 1 FROM entries
                    WHERE NEW.start_ms < end_ms AND NEW.end_ms > start_ms
                ) BEGIN SELECT RAISE(ABORT, 'time entry overlaps existing entry'); END;
                CREATE TRIGGER entries_no_overlap_update BEFORE UPDATE OF start_ms, end_ms ON entries
                WHEN EXISTS (
                    SELECT 1 FROM entries
                    WHERE id != NEW.id AND NEW.start_ms < end_ms AND NEW.end_ms > start_ms
                ) BEGIN SELECT RAISE(ABORT, 'time entry overlaps existing entry'); END;
                CREATE TRIGGER entry_activity_project_insert BEFORE INSERT ON entries
                WHEN NEW.activity_id IS NOT NULL AND NOT EXISTS (
                    SELECT 1 FROM activities WHERE id = NEW.activity_id AND project_id = NEW.project_id
                ) BEGIN SELECT RAISE(ABORT, 'activity does not belong to project'); END;
                CREATE TRIGGER entry_activity_project_update BEFORE UPDATE OF project_id, activity_id ON entries
                WHEN NEW.activity_id IS NOT NULL AND NOT EXISTS (
                    SELECT 1 FROM activities WHERE id = NEW.activity_id AND project_id = NEW.project_id
                ) BEGIN SELECT RAISE(ABORT, 'activity does not belong to project'); END;
                CREATE TABLE tracker_state (
                    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                    snapshot_json TEXT NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );
                PRAGMA user_version = 1;
                ",
            )?;
            transaction.commit()?;
        }
        Ok(())
    }
}
