//! Time-entry queries, interval writes, and database reference checks.

use super::Store;
use super::projects::validate_project;
use crate::AppError;
use houra_core::{
    ActivityId, EntryId, EntrySource, IntervalId, ProjectId, TimeEntry, TrackedInterval,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

impl Store {
    fn validate_against_active(&self, entry: &TimeEntry) -> Result<(), AppError> {
        if let Some(active) = self.load_snapshot()?.state.active() {
            for interval in &entry.intervals {
                if interval.end_ms > active.start_ms
                    && interval.start_ms < active.last_heartbeat_ms.max(active.start_ms + 1)
                {
                    return Err(AppError::InvalidBackup(
                        "entry interval overlaps the active timer; stop it before editing".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn add_entry(&mut self, entry: &TimeEntry) -> Result<EntryId, AppError> {
        entry.validate()?;
        require_intervals(entry)?;
        self.validate_against_active(entry)?;
        let transaction = self.connection.transaction()?;
        validate_entry_references(&transaction, entry, false)?;
        let id = insert_new_entry(&transaction, entry, false)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Replaces shared details and all intervals in one transaction.
    pub fn update_entry(&mut self, entry: &TimeEntry) -> Result<(), AppError> {
        entry.validate()?;
        require_intervals(entry)?;
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
        if update_entry_details(&transaction, entry)? == 0 {
            return Err(AppError::InvalidBackup(format!(
                "entry {id:?} was not found"
            )));
        }
        let existing_ids = {
            let mut statement =
                transaction.prepare("SELECT id FROM entry_intervals WHERE entry_id=?1")?;
            statement
                .query_map([id.0], |row| row.get::<_, i64>(0).map(IntervalId))?
                .collect::<Result<Vec<_>, _>>()?
        };
        for existing_id in existing_ids {
            if !entry
                .intervals
                .iter()
                .any(|interval| interval.id == Some(existing_id))
            {
                transaction.execute("DELETE FROM entry_intervals WHERE id=?1", [existing_id.0])?;
            }
        }
        for interval in &entry.intervals {
            if let Some(interval_id) = interval.id {
                reject_interval_overlaps(&transaction, interval, Some(interval_id))?;
                let changed = transaction.execute(
                    "UPDATE entry_intervals SET start_ms=?1,end_ms=?2,source=?3 WHERE id=?4 AND entry_id=?5",
                    params![interval.start_ms, interval.end_ms, source_name(interval.source), interval_id.0, id.0],
                )?;
                if changed == 0 {
                    return Err(AppError::InvalidBackup(format!(
                        "interval {interval_id:?} was not found on entry {id:?}"
                    )));
                }
            } else {
                insert_interval(&transaction, id, interval)?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn entry(&self, id: EntryId) -> Result<TimeEntry, AppError> {
        let mut entries = read_entries(&self.connection, "WHERE e.id=?1", params![id.0])?;
        entries
            .pop()
            .ok_or_else(|| AppError::InvalidBackup(format!("entry {id:?} was not found")))
    }

    pub fn list_entries(&self, start_ms: i64, end_ms: i64) -> Result<Vec<TimeEntry>, AppError> {
        read_entries(
            &self.connection,
            "WHERE EXISTS (SELECT 1 FROM entry_intervals x WHERE x.entry_id=e.id AND x.start_ms < ?2 AND x.end_ms > ?1)",
            params![start_ms, end_ms],
        )
    }

    pub fn list_all_entries(&self) -> Result<Vec<TimeEntry>, AppError> {
        read_entries(&self.connection, "", [])
    }
}

fn require_intervals(entry: &TimeEntry) -> Result<(), AppError> {
    if entry.intervals.is_empty() {
        Err(AppError::InvalidBackup(
            "an entry needs at least one interval".into(),
        ))
    } else {
        Ok(())
    }
}

fn read_entries<P: rusqlite::Params>(
    connection: &Connection,
    predicate: &str,
    params: P,
) -> Result<Vec<TimeEntry>, AppError> {
    let sql = format!(
        "SELECT e.id,e.project_id,e.activity_id,e.note,e.created_at_ms,e.updated_at_ms FROM entries e {predicate}
         ORDER BY COALESCE((SELECT MAX(end_ms) FROM entry_intervals i WHERE i.entry_id=e.id), e.created_at_ms)"
    );
    let mut statement = connection.prepare(&sql)?;
    let parents = statement
        .query_map(params, |row| {
            Ok(TimeEntry {
                id: Some(EntryId(row.get(0)?)),
                project_id: ProjectId(row.get(1)?),
                activity_id: row.get::<_, Option<i64>>(2)?.map(ActivityId),
                note: row.get(3)?,
                intervals: Vec::new(),
                created_at_ms: row.get(4)?,
                updated_at_ms: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    parents
        .into_iter()
        .map(|mut entry| {
            let Some(entry_id) = entry.id else {
                return Err(AppError::InvalidBackup("stored entry has no ID".into()));
            };
            entry.intervals = load_intervals(connection, entry_id)?;
            Ok(entry)
        })
        .collect()
}

fn load_intervals(
    connection: &Connection,
    entry_id: EntryId,
) -> Result<Vec<TrackedInterval>, AppError> {
    let mut statement = connection.prepare(
        "SELECT id,start_ms,end_ms,source FROM entry_intervals WHERE entry_id=?1 ORDER BY start_ms,id",
    )?;
    statement
        .query_map([entry_id.0], |row| {
            let source: String = row.get(3)?;
            Ok(TrackedInterval {
                id: Some(IntervalId(row.get(0)?)),
                start_ms: row.get(1)?,
                end_ms: row.get(2)?,
                source: parse_source(&source),
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from)
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

pub(super) fn insert_new_entry(
    transaction: &Transaction<'_>,
    entry: &TimeEntry,
    allow_archived_activity: bool,
) -> Result<EntryId, AppError> {
    validate_entry_references(transaction, entry, allow_archived_activity)?;
    transaction.execute(
        "INSERT INTO entries(project_id,activity_id,note,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5)",
        params![entry.project_id.0, entry.activity_id.map(|id| id.0), entry.note, entry.created_at_ms, entry.updated_at_ms],
    )?;
    let id = EntryId(transaction.last_insert_rowid());
    for interval in &entry.intervals {
        insert_interval(transaction, id, interval)?;
    }
    Ok(id)
}

pub(super) fn append_to_entry(
    transaction: &Transaction<'_>,
    entry: &TimeEntry,
    allow_archived_activity: bool,
) -> Result<(), AppError> {
    validate_entry_references(transaction, entry, allow_archived_activity)?;
    let id = entry
        .id
        .ok_or_else(|| AppError::InvalidBackup("entry ID is required".into()))?;
    if update_entry_details(transaction, entry)? == 0 {
        return Err(AppError::InvalidBackup(format!(
            "entry {id:?} was not found"
        )));
    }
    for interval in &entry.intervals {
        insert_interval(transaction, id, interval)?;
    }
    Ok(())
}

pub(super) fn update_entry_details(
    transaction: &Transaction<'_>,
    entry: &TimeEntry,
) -> Result<usize, AppError> {
    let id = entry
        .id
        .ok_or_else(|| AppError::InvalidBackup("entry ID is required".into()))?;
    Ok(transaction.execute(
        "UPDATE entries SET project_id=?1,activity_id=?2,note=?3,updated_at_ms=?4 WHERE id=?5",
        params![
            entry.project_id.0,
            entry.activity_id.map(|id| id.0),
            entry.note,
            entry.updated_at_ms,
            id.0
        ],
    )?)
}

pub(super) fn insert_interval(
    transaction: &Transaction<'_>,
    entry_id: EntryId,
    interval: &TrackedInterval,
) -> Result<(), AppError> {
    interval.validate()?;
    reject_interval_overlaps(transaction, interval, None)?;
    transaction.execute(
        "INSERT INTO entry_intervals(entry_id,start_ms,end_ms,source) VALUES(?1,?2,?3,?4)",
        params![
            entry_id.0,
            interval.start_ms,
            interval.end_ms,
            source_name(interval.source)
        ],
    )?;
    Ok(())
}

fn reject_interval_overlaps(
    transaction: &Transaction<'_>,
    interval: &TrackedInterval,
    excluded: Option<IntervalId>,
) -> Result<(), AppError> {
    let mut statement = transaction.prepare(
        "SELECT DISTINCT entry_id FROM entry_intervals
         WHERE ?1 < end_ms AND ?2 > start_ms AND (?3 IS NULL OR id != ?3)
         ORDER BY entry_id LIMIT 20",
    )?;
    let conflicts = statement
        .query_map(
            params![interval.start_ms, interval.end_ms, excluded.map(|id| id.0)],
            |row| row.get::<_, i64>(0).map(EntryId),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    if !conflicts.is_empty() {
        return Err(houra_core::DomainError::Overlap { conflicts }.into());
    }
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

fn parse_source(source: &str) -> EntrySource {
    match source {
        "manual" => EntrySource::Manual,
        "idle_reassignment" => EntrySource::IdleReassignment,
        "recovery" => EntrySource::Recovery,
        _ => EntrySource::Timer,
    }
}
