use std::path::Path;

use houra_core::{Project, ProjectId, TrackerSnapshot};
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::AppError;

const SCHEMA_VERSION: i64 = 1;

/// Owns the SQLite connection. Only one `Store` exists per database, and it
/// moves into the storage thread in Chapter 13.
pub struct Store {
    connection: Connection,
    previous_shutdown_clean: bool,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| AppError::io(parent, source))?;
        }
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let mut store = Self {
            connection,
            previous_shutdown_clean: true,
        };
        store.migrate()?;
        store.ensure_general_project()?;
        store.previous_shutdown_clean = store.meta_bool("clean_shutdown")?.unwrap_or(true);
        store.set_meta("clean_shutdown", "0")?;
        Ok(store)
    }

    /// Same schema and constraints as `open`, without a file. For tests.
    pub fn open_in_memory() -> Result<Self, AppError> {
        let connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let mut store = Self {
            connection,
            previous_shutdown_clean: true,
        };
        store.migrate()?;
        store.ensure_general_project()?;
        store.set_meta("clean_shutdown", "0")?;
        Ok(store)
    }

    pub fn previous_shutdown_clean(&self) -> bool {
        self.previous_shutdown_clean
    }

    pub fn mark_clean_shutdown(&self) -> Result<(), AppError> {
        self.set_meta("clean_shutdown", "1")
    }

    fn migrate(&mut self) -> Result<(), AppError> {
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
                CREATE TABLE tasks (
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
                    task_id INTEGER REFERENCES tasks(id) ON DELETE RESTRICT,
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
                CREATE TRIGGER entry_task_project_insert BEFORE INSERT ON entries
                WHEN NEW.task_id IS NOT NULL AND NOT EXISTS (
                    SELECT 1 FROM tasks WHERE id = NEW.task_id AND project_id = NEW.project_id
                ) BEGIN SELECT RAISE(ABORT, 'task does not belong to project'); END;
                CREATE TRIGGER entry_task_project_update BEFORE UPDATE OF project_id, task_id ON entries
                WHEN NEW.task_id IS NOT NULL AND NOT EXISTS (
                    SELECT 1 FROM tasks WHERE id = NEW.task_id AND project_id = NEW.project_id
                ) BEGIN SELECT RAISE(ABORT, 'task does not belong to project'); END;
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

    /// Project 1, "General", always exists; UI and backups rely on it.
    fn ensure_general_project(&self) -> Result<(), AppError> {
        self.connection.execute(
            "INSERT OR IGNORE INTO projects(id, name, color, archived, created_at_ms, updated_at_ms)
             VALUES(1, 'General', '#3584e4', 0, unixepoch('subsec') * 1000, unixepoch('subsec') * 1000)",
            [],
        )?;
        Ok(())
    }

    pub fn load_snapshot(&self) -> Result<TrackerSnapshot, AppError> {
        let json: Option<String> = self
            .connection
            .query_row(
                "SELECT snapshot_json FROM tracker_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        json.map_or_else(
            || Ok(TrackerSnapshot::default()),
            |value| serde_json::from_str(&value).map_err(AppError::from),
        )
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

    fn meta_bool(&self, key: &str) -> Result<Option<bool>, AppError> {
        let value: Option<String> = self
            .connection
            .query_row("SELECT value FROM meta WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(value.map(|value| value == "1"))
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<(), AppError> {
        self.connection.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}
