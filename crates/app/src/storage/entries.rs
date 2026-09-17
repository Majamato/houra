//! Time-entry queries, writes, and database reference checks.

use super::projects::validate_project;
use crate::AppError;
use houra_core::{ActivityId, EntryId, EntrySource, ProjectId, TimeEntry};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::Store;

impl Store {
    fn validate_against_active(&self, entry: &TimeEntry) -> Result<(), AppError> {
        if let Some(active) = self.load_snapshot()?.state.active()
            && entry.end_ms > active.start_ms
        {
            return Err(AppError::InvalidBackup(
                "entry overlaps the active timer; stop it before editing this interval".into(),
            ));
        }
        Ok(())
    }

    pub fn add_entry(&mut self, entry: &TimeEntry) -> Result<EntryId, AppError> {
        entry.validate()?;
        self.validate_against_active(entry)?;
        let transaction = self.connection.transaction()?;
        validate_entry_references(&transaction, entry, false)?;
        reject_entry_overlaps(&transaction, entry, None)?;
        insert_entry(&transaction, entry, false)?;
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
        let existing_activity = transaction
            .query_row(
                "SELECT activity_id FROM entries WHERE id=?1",
                [id.0],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten()
            .map(ActivityId);
        validate_entry_references(&transaction, entry, entry.activity_id == existing_activity)?;
        reject_entry_overlaps(&transaction, entry, Some(id))?;
        let changed = transaction.execute(
            "UPDATE entries SET project_id=?1, activity_id=?2, note=?3, start_ms=?4, end_ms=?5,
             source=?6, updated_at_ms=?7 WHERE id=?8",
            params![
                entry.project_id.0,
                entry.activity_id.map(|value| value.0),
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
            "SELECT id, project_id, activity_id, note, start_ms, end_ms, source, created_at_ms, updated_at_ms
             FROM entries WHERE start_ms < ?2 AND end_ms > ?1 ORDER BY start_ms",
        )?;
        let rows = statement.query_map(params![start_ms, end_ms], read_entry)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn list_all_entries(&self) -> Result<Vec<TimeEntry>, AppError> {
        self.list_entries(i64::MIN, i64::MAX)
    }
}

pub(super) fn validate_entry_references(
    connection: &Connection,
    entry: &TimeEntry,
    allow_archived_activity: bool,
) -> Result<(), AppError> {
    validate_project(connection, entry.project_id)?;
    if let Some(activity_id) = entry.activity_id {
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM activities WHERE id=?1 AND (?2 OR archived=0))",
            params![activity_id.0, allow_archived_activity],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(AppError::InvalidActivity(activity_id));
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

pub(super) fn insert_entry(
    transaction: &Transaction<'_>,
    entry: &TimeEntry,
    allow_archived_activity: bool,
) -> Result<(), AppError> {
    validate_entry_references(transaction, entry, allow_archived_activity)?;
    transaction.execute(
        "INSERT INTO entries(project_id,activity_id,note,start_ms,end_ms,source,created_at_ms,updated_at_ms)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![entry.project_id.0, entry.activity_id.map(|id| id.0), entry.note, entry.start_ms,
            entry.end_ms, source_name(entry.source), entry.created_at_ms, entry.updated_at_ms],
    )?;
    Ok(())
}

pub(super) fn source_name(source: EntrySource) -> &'static str {
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
        activity_id: row.get::<_, Option<i64>>(2)?.map(ActivityId),
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
