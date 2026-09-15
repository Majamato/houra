//! SQLite storage, grouped by the records and operations it persists.
//!
//! Callers use `Store`; its implementation is split across private modules.

mod backup;
mod entries;
mod migrations;
mod projects;
mod snapshots;

use crate::AppError;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

/// Owns the SQLite connection used by the tracker service's worker thread.
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
