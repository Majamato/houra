//! Database schema creation and version upgrades.

use crate::AppError;

const SCHEMA_VERSION: i64 = 3;

use super::Store;

impl Store {
    pub(super) fn migrate(&mut self) -> Result<(), AppError> {
        let version: i64 = self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version != 0 && version != SCHEMA_VERSION {
            return Err(AppError::UnsupportedDatabaseSchema {
                found: version,
                expected: SCHEMA_VERSION,
            });
        }
        if version == 0 {
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
                    name TEXT NOT NULL COLLATE NOCASE UNIQUE CHECK(length(trim(name)) > 0),
                    archived INTEGER NOT NULL DEFAULT 0 CHECK(archived IN (0, 1)),
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );
                CREATE TABLE entries (
                    id INTEGER PRIMARY KEY,
                    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
                    activity_id INTEGER REFERENCES activities(id) ON DELETE RESTRICT,
                    note TEXT NOT NULL DEFAULT '',
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );
                CREATE TABLE entry_intervals (
                    id INTEGER PRIMARY KEY,
                    entry_id INTEGER NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
                    start_ms INTEGER NOT NULL,
                    end_ms INTEGER NOT NULL CHECK(end_ms > start_ms),
                    source TEXT NOT NULL
                );
                CREATE INDEX entry_intervals_range ON entry_intervals(start_ms, end_ms);
                CREATE INDEX entry_intervals_entry ON entry_intervals(entry_id, start_ms);
                CREATE TRIGGER intervals_no_overlap_insert BEFORE INSERT ON entry_intervals
                -- Half-open intervals overlap iff new.start < old.end and
                -- new.end > old.start; adjacent boundaries are therefore legal.
                WHEN EXISTS (
                    SELECT 1 FROM entry_intervals
                    WHERE NEW.start_ms < end_ms AND NEW.end_ms > start_ms
                ) BEGIN SELECT RAISE(ABORT, 'time entry overlaps existing entry'); END;
                CREATE TRIGGER intervals_no_overlap_update BEFORE UPDATE OF start_ms, end_ms ON entry_intervals
                WHEN EXISTS (
                    SELECT 1 FROM entry_intervals
                    WHERE id != NEW.id AND NEW.start_ms < end_ms AND NEW.end_ms > start_ms
                ) BEGIN SELECT RAISE(ABORT, 'time entry overlaps existing entry'); END;
                CREATE TABLE tracker_state (
                    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                    snapshot_json TEXT NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );
                INSERT INTO projects(id, name, color, archived, created_at_ms, updated_at_ms)
                VALUES(1, 'General', '#3584e4', 0, unixepoch('subsec') * 1000, unixepoch('subsec') * 1000);
                INSERT INTO activities(name, archived, created_at_ms, updated_at_ms) VALUES
                    ('Programming', 0, unixepoch('subsec') * 1000, unixepoch('subsec') * 1000),
                    ('Design', 0, unixepoch('subsec') * 1000, unixepoch('subsec') * 1000),
                    ('Planning', 0, unixepoch('subsec') * 1000, unixepoch('subsec') * 1000),
                    ('Code Review', 0, unixepoch('subsec') * 1000, unixepoch('subsec') * 1000),
                    ('Testing', 0, unixepoch('subsec') * 1000, unixepoch('subsec') * 1000),
                    ('Documentation', 0, unixepoch('subsec') * 1000, unixepoch('subsec') * 1000),
                    ('Meetings', 0, unixepoch('subsec') * 1000, unixepoch('subsec') * 1000),
                    ('Research', 0, unixepoch('subsec') * 1000, unixepoch('subsec') * 1000);
                PRAGMA user_version = 3;
                ",
            )?;
            transaction.commit()?;
        }
        Ok(())
    }
}
