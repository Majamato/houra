use houra_core::{EntryId, EntrySource, ProjectId, TimeEntry, TrackedInterval};
pub fn entry(id: Option<i64>, start_ms: i64, end_ms: i64) -> TimeEntry {
    TimeEntry {
        id: id.map(EntryId),
        project_id: ProjectId(1),
        activity_id: None,
        note: String::new(),
        intervals: vec![TrackedInterval {
            id: None,
            start_ms,
            end_ms,
            source: EntrySource::Manual,
        }],
        created_at_ms: end_ms,
        updated_at_ms: end_ms,
    }
}
