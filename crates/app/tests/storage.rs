mod common;
use common::*;
use houra::{AppError, storage::Store};
use houra_core::{DomainError, EntryId, ProjectId};
use tempfile::TempDir;
#[test]
fn migration_creates_non_archivable_general_project() {
    let (_directory, store) = temporary_store();
    let projects = store
        .list_projects(false)
        .unwrap_or_else(|error| panic!("list failed: {error}"));
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "General");
    assert!(
        matches!(store.set_project_archived(ProjectId(1), true, 1), Err(AppError::InvalidBackup(message)) if message == "General cannot be archived")
    );
}

#[test]
fn corrupt_database_returns_database_error() {
    let directory = TempDir::new().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let path = directory.path().join("corrupt.sqlite3");
    std::fs::write(&path, b"this is not sqlite")
        .unwrap_or_else(|error| panic!("fixture write failed: {error}"));
    assert!(matches!(Store::open(&path), Err(AppError::Database(_))));
}

#[test]
fn overlapping_manual_entries_report_conflicting_id() {
    let (_directory, mut store) = temporary_store();
    assert!(store.add_entry(&manual(None, 1, 100, 200)).is_ok());
    let overlap = store.add_entry(&manual(None, 1, 199, 300));
    assert!(
        matches!(overlap, Err(AppError::Domain(DomainError::Overlap { conflicts })) if conflicts == vec![EntryId(1)])
    );
    let entries = store
        .list_all_entries()
        .unwrap_or_else(|error| panic!("list failed: {error}"));
    assert_eq!(entries, vec![manual(Some(1), 1, 100, 200)]);
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
    assert!(matches!(store.add_entry(&entry), Err(AppError::InvalidTask(id)) if id == task));
}

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
    assert!(matches!(
        store.delete_task_permanently(task),
        Err(AppError::ReferencedItem)
    ));
    assert_eq!(
        store.list_tasks(true).unwrap_or_else(|e| panic!("{e}")),
        vec![houra_core::Task {
            id: task,
            project_id: ProjectId(1),
            name: "Task".into(),
            archived: true,
            created_at_ms: 1,
            updated_at_ms: 300
        }]
    );
}

#[test]
fn reopening_preserves_records_snapshot_and_shutdown_marker() -> Result<(), AppError> {
    let (directory, mut store) = temporary_store();
    assert!(store.previous_shutdown_clean());
    store.add_entry(&manual(None, 1, 100, 200))?;
    let transition = houra_core::Transition {
        snapshot: houra_core::TrackerSnapshot {
            revision: 42,
            ..Default::default()
        },
        completed_entries: vec![],
        notifications: vec![],
    };
    store.persist_transition(&transition)?;
    let before = store.backup(1)?;
    drop(store);
    let store = Store::open(&directory.path().join("tracker.sqlite3"))?;
    assert!(!store.previous_shutdown_clean());
    assert_document_eq(&before, &store.backup(1)?);
    store.mark_clean_shutdown()?;
    drop(store);
    let store = Store::open(&directory.path().join("tracker.sqlite3"))?;
    assert!(store.previous_shutdown_clean());
    assert_document_eq(&before, &store.backup(1)?);
    Ok(())
}

#[test]
fn newer_schema_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let path = directory.path().join("future.sqlite3");
    let connection = rusqlite::Connection::open(&path)?;
    connection.pragma_update(None, "user_version", 2)?;
    drop(connection);
    assert!(
        matches!(Store::open(&path), Err(AppError::InvalidBackup(message)) if message == "database schema 2 is newer than supported 1")
    );
    Ok(())
}

#[test]
fn project_task_uniqueness_archiving_and_deletion() -> Result<(), AppError> {
    let mut store = Store::open_in_memory()?;
    let project = store.create_project(" Work ", "#123456", 10)?;
    assert!(matches!(
        store.create_project("work", "#123456", 11),
        Err(AppError::Database(_))
    ));
    let task = store.create_task(project, " Task ", 12)?;
    assert!(matches!(
        store.create_task(project, "task", 13),
        Err(AppError::Database(_))
    ));
    let general_task = store.create_task(ProjectId(1), "Task", 14)?;
    assert_eq!(store.list_tasks(false)?.len(), 2);
    let mut entry = manual(None, project.0, 100, 200);
    entry.task_id = Some(task);
    store.add_entry(&entry)?;
    assert!(matches!(
        store.delete_project_permanently(project),
        Err(AppError::ReferencedItem)
    ));
    assert!(matches!(
        store.delete_project_permanently(ProjectId(1)),
        Err(AppError::ReferencedItem)
    ));
    store.set_task_archived(task, true, 20)?;
    assert_eq!(store.list_tasks(false)?.len(), 1);
    let archived = store
        .list_tasks(true)?
        .into_iter()
        .find(|t| t.id == task)
        .unwrap_or_else(|| panic!("task"));
    assert_eq!(
        archived,
        houra_core::Task {
            id: task,
            project_id: project,
            name: "Task".into(),
            archived: true,
            created_at_ms: 12,
            updated_at_ms: 20
        }
    );
    entry.start_ms = 200;
    entry.end_ms = 300;
    assert!(matches!(store.add_entry(&entry), Err(AppError::InvalidTask(id)) if id == task));
    store.set_project_archived(project, true, 30)?;
    assert_eq!(store.list_projects(false)?.len(), 1);
    assert_eq!(store.list_projects(true)?.len(), 2);
    assert!(
        matches!(store.create_task(project, "Other", 30), Err(AppError::InvalidProject(id)) if id == project)
    );
    assert!(matches!(store.add_entry(&entry), Err(AppError::InvalidProject(id)) if id == project));
    store.set_project_archived(project, false, 40)?;
    store.set_task_archived(task, false, 40)?;
    store.add_entry(&entry)?;
    store.delete_task_permanently(general_task)?;
    let empty = store.create_project("Empty", "#ffffff", 50)?;
    store.delete_project_permanently(empty)?;
    assert_eq!(store.list_tasks(true)?.len(), 1);
    assert_eq!(store.list_projects(true)?.len(), 2);
    Ok(())
}

#[test]
fn entry_updates_ranges_and_invalid_references() -> Result<(), AppError> {
    let mut store = Store::open_in_memory()?;
    let mut first = manual(None, 1, 100, 200);
    first.id = Some(store.add_entry(&first)?);
    let mut second = manual(None, 1, 200, 300);
    second.id = Some(store.add_entry(&second)?);
    assert_eq!(store.list_entries(200, 300)?, vec![second.clone()]);
    assert_eq!(store.list_entries(0, 100)?, vec![]);
    assert_eq!(store.list_entries(300, 400)?, vec![]);
    first.note = "changed".into();
    first.updated_at_ms = 400;
    store.update_entry(&first)?;
    assert_eq!(
        store.list_all_entries()?,
        vec![first.clone(), second.clone()]
    );
    let before = store.backup(1)?;
    let mut invalid = first.clone();
    invalid.end_ms = 201;
    assert!(
        matches!(store.update_entry(&invalid), Err(AppError::Domain(DomainError::Overlap { conflicts })) if conflicts == vec![EntryId(2)])
    );
    invalid = first.clone();
    invalid.id = None;
    assert!(
        matches!(store.update_entry(&invalid), Err(AppError::InvalidBackup(message)) if message == "entry ID is required for update")
    );
    invalid.id = Some(EntryId(99));
    invalid.start_ms = 400;
    invalid.end_ms = 500;
    assert!(
        matches!(store.update_entry(&invalid), Err(AppError::InvalidBackup(message)) if message == "entry EntryId(99) was not found")
    );
    invalid.project_id = ProjectId(99);
    assert!(matches!(
        store.add_entry(&invalid),
        Err(AppError::InvalidProject(ProjectId(99)))
    ));
    invalid.project_id = ProjectId(1);
    invalid.task_id = Some(houra_core::TaskId(99));
    assert!(matches!(
        store.add_entry(&invalid),
        Err(AppError::InvalidTask(houra_core::TaskId(99)))
    ));
    invalid.task_id = None;
    invalid.end_ms = invalid.start_ms;
    assert!(matches!(
        store.add_entry(&invalid),
        Err(AppError::Domain(DomainError::InvalidInterval {
            start_ms: 400,
            end_ms: 400
        }))
    ));
    assert_document_eq(&before, &store.backup(1)?);
    Ok(())
}

#[test]
fn later_transition_failure_rolls_back_entries_and_snapshot() -> Result<(), AppError> {
    let mut store = Store::open_in_memory()?;
    store.add_entry(&manual(None, 1, 10, 20))?;
    let before = store.backup(1)?;
    for (case, entries) in [
        vec![manual(None, 1, 100, 200), manual(None, 99, 200, 300)],
        vec![manual(None, 1, 100, 200), manual(None, 1, 150, 300)],
    ]
    .into_iter()
    .enumerate()
    {
        let transition = houra_core::Transition {
            snapshot: houra_core::TrackerSnapshot {
                revision: 99,
                ..Default::default()
            },
            completed_entries: entries,
            notifications: vec![],
        };
        let result = store.persist_transition(&transition);
        if case == 0 {
            assert!(matches!(
                result,
                Err(AppError::InvalidProject(ProjectId(99)))
            ));
        } else {
            assert!(
                matches!(result, Err(AppError::Database(rusqlite::Error::SqliteFailure(error, _))) if error.code == rusqlite::ErrorCode::ConstraintViolation)
            );
        }
        assert_document_eq(&before, &store.backup(1)?);
    }
    Ok(())
}

#[test]
fn active_timer_conflicts_and_snapshot_persistence() -> Result<(), AppError> {
    use houra_core::{ManualClock, TrackerCommand, TrackerEngine};
    let mut store = Store::open_in_memory()?;
    let mut engine = TrackerEngine::new(ManualClock::at(200));
    let started = engine.apply(TrackerCommand::Start {
        project_id: ProjectId(1),
        task_id: None,
        note: "active".into(),
    })?;
    store.persist_transition(&started)?;
    assert_eq!(store.load_snapshot()?, started.snapshot);
    store.add_entry(&manual(None, 1, 100, 200))?;
    assert!(matches!(
        store.add_entry(&manual(None, 1, 200, 300)),
        Err(AppError::InvalidBackup(_))
    ));
    let before = store.backup(1)?;
    let mut invalid = started.clone();
    if let houra_core::TrackerState::Running(active) = &mut invalid.snapshot.state {
        active.start_ms = 150;
    }
    assert!(
        matches!(store.persist_transition(&invalid), Err(AppError::Domain(DomainError::Overlap { conflicts })) if conflicts == vec![EntryId(1)])
    );
    assert_document_eq(&before, &store.backup(1)?);
    Ok(())
}
