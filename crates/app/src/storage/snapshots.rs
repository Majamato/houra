//! Tracker snapshots and atomic transition persistence.

use super::entries::{insert_entry, validate_entry_references};
use crate::AppError;
use houra_core::{EntryId, EntrySource, TimeEntry, TrackerSnapshot, TrackerState, Transition};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::Store;

impl Store {
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

    /// Writes completed entries and the new snapshot in one transaction.
    pub fn persist_transition(&mut self, transition: &Transition) -> Result<(), AppError> {
        let previous_activity = self
            .load_snapshot()?
            .state
            .active()
            .and_then(|active| active.activity_id);
        let transaction = self.connection.transaction()?;
        validate_active_references(&transaction, &transition.snapshot.state, previous_activity)?;
        for entry in &transition.completed_entries {
            insert_entry(
                &transaction,
                entry,
                entry.activity_id.is_some() && entry.activity_id == previous_activity,
            )?;
        }
        write_snapshot(&transaction, &transition.snapshot)?;
        transaction.commit()?;
        Ok(())
    }
}

fn validate_active_references(
    connection: &Connection,
    state: &TrackerState,
    previous_activity: Option<houra_core::ActivityId>,
) -> Result<(), AppError> {
    if let Some(active) = state.active() {
        let entry = TimeEntry {
            id: None,
            project_id: active.project_id,
            activity_id: active.activity_id,
            note: active.note.clone(),
            start_ms: active.start_ms,
            end_ms: active.start_ms.saturating_add(1),
            source: EntrySource::Timer,
            created_at_ms: active.start_ms,
            updated_at_ms: active.start_ms,
        };
        validate_entry_references(
            connection,
            &entry,
            active.activity_id.is_some() && active.activity_id == previous_activity,
        )?;
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

pub(super) fn write_snapshot(
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
