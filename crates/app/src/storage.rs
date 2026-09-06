use std::path::Path;

use houra_core::{
    EntryId, EntrySource, Project, ProjectId, Task, TaskId, TimeEntry, TrackerSnapshot,
    TrackerState, Transition,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::backup::{BACKUP_VERSION, BackupDocument};
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

    fn validate_against_active(&self, entry: &TimeEntry) -> Result<(), AppError> {
        if let Some(active) = self.load_snapshot()?.state.active()
            && entry.end_ms > active.start_ms
        {
            return Err(AppError::InvalidBackup(
                "entry overlaps the active timer; stop it before e<D-z>diting this interval".into(),
            ));
        }
        Ok(())
    }

    pub fn set_task_archived(
        &self,
        id: TaskId,
        archived: bool,
        now_ms: i64,
    ) -> Result<(), AppError> {
        self.connection.execute(
            "UPDATE tasks SET archived=?1, updated_at_ms=?2 WHERE id=?3",
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

    pub fn delete_task_permanently(&self, id: TaskId) -> Result<(), AppError> {
        let references: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE task_id=?1)",
            [id.0],
            |row| row.get(0),
        )?;
        if references {
            return Err(AppError::ReferencedItem);
        }
        self.connection
            .execute("DELETE FROM tasks WHERE id=?1", [id.0])?;
        Ok(())
    }

    pub fn backup(&self, exported_at_ms: i64) -> Result<BackupDocument, AppError> {
        Ok(BackupDocument {
            format: "houra-backup".into(),
            version: BACKUP_VERSION,
            exported_at_ms,
            projects: self.list_projects(true)?,
            tasks: self.list_tasks(true)?,
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
        transaction.execute("DELETE FROM entries", [])?;
        transaction.execute("DELETE FROM tasks", [])?;
        transaction.execute("DELETE FROM projects", [])?;
        for project in &document.projects {
            transaction.execute(
                "INSERT INTO projects(id,name,color,archived,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6)",
                params![project.id.0, project.name, project.color, project.archived, project.created_at_ms, project.updated_at_ms],
            )?;
        }
        for task in &document.tasks {
            transaction.execute(
                "INSERT INTO tasks(id,project_id,name,archived,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6)",
                params![task.id.0, task.project_id.0, task.name, task.archived, task.created_at_ms, task.updated_at_ms],
            )?;
        }
        for entry in &document.entries {
            transaction.execute(
                "INSERT INTO entries(id,project_id,task_id,note,start_ms,end_ms,source,created_at_ms,updated_at_ms)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![entry.id.map(|id| id.0), entry.project_id.0, entry.task_id.map(|id| id.0), entry.note,
                    entry.start_ms, entry.end_ms, source_name(entry.source), entry.created_at_ms, entry.updated_at_ms],
            )?;
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

    /// Writes completed entries and the new snapshot in one transaction.
    pub fn persist_transition(&mut self, transition: &Transition) -> Result<(), AppError> {
        let transaction = self.connection.transaction()?;
        validate_active_references(&transaction, &transition.snapshot.state)?;
        for entry in &transition.completed_entries {
            insert_entry(&transaction, entry)?;
        }
        write_snapshot(&transaction, &transition.snapshot)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn add_entry(&mut self, entry: &TimeEntry) -> Result<EntryId, AppError> {
        entry.validate()?;
        self.validate_against_active(entry)?;
        let transaction = self.connection.transaction()?;
        validate_entry_references(&transaction, entry)?;
        reject_entry_overlaps(&transaction, entry, None)?;
        insert_entry(&transaction, entry)?;
        let id = EntryId(transaction.last_insert_rowid());
        transaction.commit()?;
        Ok(id)
    }

    pub fn update_entry(&mut self, entry: &TimeEntry) -> Result<(), AppError> {
        entry.validate()?;
        self.validate_against_active(entry)?;
        let id = entry
            .id
            .ok_or_else(|| AppError::InvalidBackup("entry ID is required for update".into()))?;
        let transaction = self.connection.transaction()?;
        validate_entry_references(&transaction, entry)?;
        reject_entry_overlaps(&transaction, entry, Some(id))?;
        let changed = transaction.execute(
            "UPDATE entries SET project_id=?1, task_id=?2, note=?3, start_ms=?4, end_ms=?5,
             source=?6, updated_at_ms=?7 WHERE id=?8",
            params![
                entry.project_id.0,
                entry.task_id.map(|value| value.0),
                entry.note,
                entry.start_ms,
                entry.end_ms,
                source_name(entry.source),
                entry.updated_at_ms,
                id.0
            ],
        )?;
        if changed == 0 {
            return Err(AppError::InvalidBackup(format!(
                "entry {id:?} was not found"
            )));
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn list_entries(&self, start_ms: i64, end_ms: i64) -> Result<Vec<TimeEntry>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, project_id, task_id, note, start_ms, end_ms, source, created_at_ms, updated_at_ms
             FROM entries WHERE start_ms < ?2 AND end_ms > ?1 ORDER BY start_ms",
        )?;
        let rows = statement.query_map(params![start_ms, end_ms], read_entry)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn list_all_entries(&self) -> Result<Vec<TimeEntry>, AppError> {
        self.list_entries(i64::MIN, i64::MAX)
    }

    pub fn list_tasks(&self, include_archived: bool) -> Result<Vec<Task>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, project_id, name, archived, created_at_ms, updated_at_ms FROM tasks
             WHERE ?1 OR archived = 0 ORDER BY project_id, archived, name COLLATE NOCASE",
        )?;
        let rows = statement.query_map([include_archived], |row| {
            Ok(Task {
                id: TaskId(row.get(0)?),
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

    pub fn create_task(
        &self,
        project_id: ProjectId,
        name: &str,
        now_ms: i64,
    ) -> Result<TaskId, AppError> {
        validate_project(&self.connection, project_id)?;
        let trimmed = name.trim();
        houra_core::validate_name(trimmed)?;
        self.connection.execute(
            "INSERT INTO tasks(project_id, name, archived, created_at_ms, updated_at_ms)
             VALUES(?1, ?2, 0, ?3, ?3)",
            params![project_id.0, trimmed, now_ms],
        )?;
        Ok(TaskId(self.connection.last_insert_rowid()))
    }
}

fn validate_project(connection: &Connection, id: ProjectId) -> Result<(), AppError> {
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

fn validate_entry_references(connection: &Connection, entry: &TimeEntry) -> Result<(), AppError> {
    validate_project(connection, entry.project_id)?;
    if let Some(task_id) = entry.task_id {
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1 AND project_id=?2 AND archived=0)",
            params![task_id.0, entry.project_id.0],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(AppError::InvalidTask(task_id));
        }
    }
    Ok(())
}

fn validate_active_references(
    connection: &Connection,
    state: &TrackerState,
) -> Result<(), AppError> {
    if let Some(active) = state.active() {
        let entry = TimeEntry {
            id: None,
            project_id: active.project_id,
            task_id: active.task_id,
            note: active.note.clone(),
            start_ms: active.start_ms,
            end_ms: active.start_ms.saturating_add(1),
            source: EntrySource::Timer,
            created_at_ms: active.start_ms,
            updated_at_ms: active.start_ms,
        };
        validate_entry_references(connection, &entry)?;
        let mut statement = connection
            .prepare("SELECT id FROM entries WHERE end_ms > ?1 ORDER BY start_ms LIMIT 20")?;
        let conflicts = statement
            .query_map([active.start_ms], |row| row.get::<_, i64>(0).map(EntryId))?
            .collect::<Result<Vec<_>, _>>()?;
        if !conflicts.is_empty() {
            return Err(houra_core::DomainError::Overlap { conflicts }.into());
        }
    }
    Ok(())
}

fn reject_entry_overlaps(
    connection: &Connection,
    entry: &TimeEntry,
    excluded_id: Option<EntryId>,
) -> Result<(), AppError> {
    let mut statement = connection.prepare(
        "SELECT id FROM entries
         WHERE ?1 < end_ms AND ?2 > start_ms AND (?3 IS NULL OR id != ?3)
         ORDER BY start_ms LIMIT 20",
    )?;
    let conflicts = statement
        .query_map(
            params![entry.start_ms, entry.end_ms, excluded_id.map(|id| id.0)],
            |row| row.get::<_, i64>(0).map(EntryId),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(houra_core::DomainError::Overlap { conflicts }.into())
    }
}

fn insert_entry(transaction: &Transaction<'_>, entry: &TimeEntry) -> Result<(), AppError> {
    validate_entry_references(transaction, entry)?;
    transaction.execute(
        "INSERT INTO entries(project_id,task_id,note,start_ms,end_ms,source,created_at_ms,updated_at_ms)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![entry.project_id.0, entry.task_id.map(|id| id.0), entry.note, entry.start_ms,
            entry.end_ms, source_name(entry.source), entry.created_at_ms, entry.updated_at_ms],
    )?;
    Ok(())
}

fn write_snapshot(
    transaction: &Transaction<'_>,
    snapshot: &TrackerSnapshot,
) -> Result<(), AppError> {
    let json = serde_json::to_string(snapshot)?;
    let updated_at_ms = snapshot
        .state
        .active()
        .map_or(0, |active| active.last_heartbeat_ms);
    transaction.execute(
        "INSERT INTO tracker_state(singleton,snapshot_json,updated_at_ms) VALUES(1,?1,?2)
         ON CONFLICT(singleton) DO UPDATE SET snapshot_json=excluded.snapshot_json, updated_at_ms=excluded.updated_at_ms",
        params![json, updated_at_ms],
    )?;
    Ok(())
}

fn source_name(source: EntrySource) -> &'static str {
    match source {
        EntrySource::Timer => "timer",
        EntrySource::Manual => "manual",
        EntrySource::IdleReassignment => "idle_reassignment",
        EntrySource::Recovery => "recovery",
    }
}

fn read_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<TimeEntry> {
    let source: String = row.get(6)?;
    Ok(TimeEntry {
        id: Some(EntryId(row.get(0)?)),
        project_id: ProjectId(row.get(1)?),
        task_id: row.get::<_, Option<i64>>(2)?.map(TaskId),
        note: row.get(3)?,
        start_ms: row.get(4)?,
        end_ms: row.get(5)?,
        source: match source.as_str() {
            "manual" => EntrySource::Manual,
            "idle_reassignment" => EntrySource::IdleReassignment,
            "recovery" => EntrySource::Recovery,
            _ => EntrySource::Timer,
        },
        created_at_ms: row.get(7)?,
        updated_at_ms: row.get(8)?,
    })
}
