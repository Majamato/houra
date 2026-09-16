use houra::storage::Store;
use houra_core::{EntryId, EntrySource, ProjectId, TimeEntry};
use tempfile::TempDir;
pub fn temporary_store() -> (TempDir, Store) {
    let directory = TempDir::new().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let store = Store::open(&directory.path().join("tracker.sqlite3"))
        .unwrap_or_else(|error| panic!("store failed: {error}"));
    (directory, store)
}

pub fn manual(id: Option<i64>, project_id: i64, start_ms: i64, end_ms: i64) -> TimeEntry {
    TimeEntry {
        id: id.map(EntryId),
        project_id: ProjectId(project_id),
        activity_id: None,
        note: "manual".into(),
        start_ms,
        end_ms,
        source: EntrySource::Manual,
        created_at_ms: end_ms,
        updated_at_ms: end_ms,
    }
}

pub fn assert_document_eq(a: &houra::backup::BackupDocument, b: &houra::backup::BackupDocument) {
    assert_eq!(a.format, b.format);
    assert_eq!(a.version, b.version);
    assert_eq!(a.exported_at_ms, b.exported_at_ms);
    assert_eq!(a.projects, b.projects);
    assert_eq!(a.activities, b.activities);
    assert_eq!(a.entries, b.entries);
    assert_eq!(a.tracker, b.tracker);
}
