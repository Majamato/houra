use houra_core::{EntryId, EntrySource, ProjectId, TimeEntry};
pub fn entry(id: Option<i64>, start_ms: i64, end_ms: i64) -> TimeEntry {
    TimeEntry {
        id: id.map(EntryId),
        project_id: ProjectId(1),
        task_id: None,
        note: String::new(),
        start_ms,
        end_ms,
        source: EntrySource::Manual,
        created_at_ms: end_ms,
        updated_at_ms: end_ms,
    }
}
