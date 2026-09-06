use houra::storage::Store;
use houra_core::{EntryId, EntrySource, ProjectId, TaskId, TimeEntry};
use tempfile::TempDir;

fn temporary_store() -> (TempDir, Store) {
    let directory = TempDir::new().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let store = Store::open(&directory.path().join("tracker.sqlite3"))
        .unwrap_or_else(|error| panic!("store failed: {error}"));
    (directory, store)
}

#[test]
fn migration_creates_non_archivable_general_project() {
    let (_directory, store) = temporary_store();
    let projects = store
        .list_projects(false)
        .unwrap_or_else(|error| panic!("list failed: {error}"));
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "General");
    assert!(store.set_project_archived(ProjectId(1), true, 1).is_err());
}

#[test]
fn corrupt_database_returns_a_contextual_error() {
    let directory = TempDir::new().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let path = directory.path().join("corrupt.sqlite3");
    std::fs::write(&path, b"this is not sqlite")
        .unwrap_or_else(|error| panic!("fixture write failed: {error}"));
    assert!(Store::open(&path).is_err());
}

fn manual(id: Option<i64>, project_id: i64, start_ms: i64, end_ms: i64) -> TimeEntry {
    TimeEntry {
        id: id.map(EntryId),
        project_id: ProjectId(project_id),
        task_id: None,
        note: "manual".into(),
        start_ms,
        end_ms,
        source: EntrySource::Manual,
        created_at_ms: end_ms,
        updated_at_ms: end_ms,
    }
}

// ... migration_creates_non_archivable_general_project ...

#[test]
fn database_constraint_rejects_overlapping_manual_entries() {
    let (_directory, mut store) = temporary_store();
    assert!(store.add_entry(&manual(None, 1, 100, 200)).is_ok());
    let overlap = store.add_entry(&manual(None, 1, 199, 300));
    assert!(overlap.is_err());
    assert!(
        overlap
            .err()
            .map(|error| error.to_string().contains("EntryId(1)"))
            .unwrap_or(false)
    );
    let entries = store
        .list_all_entries()
        .unwrap_or_else(|error| panic!("list failed: {error}"));
    assert_eq!(entries.len(), 1);
}

#[test]
fn task_must_belong_to_entry_project() {
    let (_directory, mut store) = temporary_store();
    let second = store
        .create_project("Second", "#ff0000", 1)
        .unwrap_or_else(|error| panic!("project failed: {error}"));
    let task = store
        .create_task(second, "Task", 1)
        .unwrap_or_else(|error| panic!("task failed: {error}"));
    let mut entry = manual(None, 1, 100, 200);
    entry.task_id = Some(task);
    assert!(store.add_entry(&entry).is_err());
}

// ... corrupt_database_returns_a_contextual_error ...

#[test]
fn referenced_task_is_archived_not_deleted() {
    let (_directory, mut store) = temporary_store();
    let task = store
        .create_task(ProjectId(1), "Task", 1)
        .unwrap_or_else(|error| panic!("task failed: {error}"));
    let mut entry = manual(None, 1, 100, 200);
    entry.task_id = Some(task);
    assert!(store.add_entry(&entry).is_ok());
    assert!(store.set_task_archived(task, true, 300).is_ok());
    assert!(store.delete_task_permanently(task).is_err());
    assert_eq!(task, TaskId(task.0));
}
