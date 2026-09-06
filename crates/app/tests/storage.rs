use houra::storage::Store;
use houra::{TrackerService, backup::BackupDocument};
use houra_core::{EntryId, EntrySource, ProjectId, TaskId, TimeEntry, TrackerSnapshot};
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

#[test]
fn backup_round_trip_preserves_data() {
    let (_first_dir, mut first) = temporary_store();
    assert!(first.add_entry(&manual(None, 1, 100, 200)).is_ok());
    let backup = first
        .backup(1_000)
        .unwrap_or_else(|error| panic!("backup failed: {error}"));

    let (_second_dir, mut second) = temporary_store();
    assert!(second.restore(&backup).is_ok());
    let restored = second
        .list_all_entries()
        .unwrap_or_else(|error| panic!("list failed: {error}"));
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].start_ms, 100);
}

#[test]
fn backup_file_round_trip_is_versioned_and_validated() {
    let (directory, mut store) = temporary_store();
    assert!(store.add_entry(&manual(None, 1, 100, 200)).is_ok());
    let backup = store
        .backup(1_000)
        .unwrap_or_else(|error| panic!("backup failed: {error}"));
    let path = directory.path().join("backup.json");
    assert!(backup.write_to_path(&path).is_ok());
    let read = BackupDocument::read_from_path(&path)
        .unwrap_or_else(|error| panic!("read failed: {error}"));
    assert_eq!(read.version, 1);
    assert_eq!(read.entries.len(), 1);
}

#[test]
fn invalid_restore_is_transactionally_rejected() {
    let (_directory, mut store) = temporary_store();
    assert!(store.add_entry(&manual(None, 1, 100, 200)).is_ok());
    let invalid = BackupDocument {
        format: "houra-backup".into(),
        version: 1,
        exported_at_ms: 1,
        projects: store
            .list_projects(true)
            .unwrap_or_else(|error| panic!("projects failed: {error}")),
        tasks: vec![],
        entries: vec![manual(Some(10), 1, 100, 200), manual(Some(11), 1, 150, 250)],
        tracker: TrackerSnapshot::default(),
    };
    assert!(store.restore(&invalid).is_err());
    let original = store
        .list_all_entries()
        .unwrap_or_else(|error| panic!("list failed: {error}"));
    assert_eq!(original.len(), 1);
}

#[test]
fn concurrent_start_commands_are_serialized() {
    let directory = TempDir::new().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let service = TrackerService::start(directory.path().join("actor.sqlite3"))
        .unwrap_or_else(|error| panic!("service failed: {error}"));
    let first = service.handle.clone();
    let second = service.handle.clone();
    let command = || houra_core::TrackerCommand::Start {
        project_id: ProjectId(1),
        task_id: None,
        note: "concurrent".into(),
    };
    let first_join = std::thread::spawn(move || first.apply(command()));
    let second_join = std::thread::spawn(move || second.apply(command()));
    let first_result = first_join
        .join()
        .unwrap_or_else(|_| panic!("first client panicked"));
    let second_result = second_join
        .join()
        .unwrap_or_else(|_| panic!("second client panicked"));
    assert_ne!(first_result.is_ok(), second_result.is_ok());
    assert!(
        service
            .handle
            .apply(houra_core::TrackerCommand::Stop)
            .is_ok()
    );
    assert!(service.shutdown().is_ok());
}
