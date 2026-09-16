mod common;
use common::*;
use houra::backup::BackupDocument;
use houra_core::TrackerSnapshot;
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
    assert_eq!(restored, backup.entries);
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
    assert_document_eq(&backup, &read);
}

#[test]
fn overlapping_restore_is_rejected_before_writes() {
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
    let before = store.backup(1).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        matches!(store.restore(&invalid), Err(houra::AppError::Domain(houra_core::DomainError::Overlap { conflicts })) if conflicts == vec![houra_core::EntryId(10), houra_core::EntryId(11)])
    );
    assert_document_eq(&before, &store.backup(1).unwrap_or_else(|e| panic!("{e}")));
    let original = store
        .list_all_entries()
        .unwrap_or_else(|error| panic!("list failed: {error}"));
    assert_eq!(original.len(), 1);
}

#[test]
fn full_round_trip_preserves_archives_sources_and_snapshot() -> Result<(), houra::AppError> {
    use houra_core::{EntrySource, TrackerSnapshot, Transition};
    let (directory, mut store) = temporary_store();
    let project = store.create_project("Work", "#123456", 1)?;
    let task = store.create_task(project, "Task", 2)?;
    for (index, source) in [
        EntrySource::Timer,
        EntrySource::Manual,
        EntrySource::IdleReassignment,
        EntrySource::Recovery,
    ]
    .into_iter()
    .enumerate()
    {
        let start = index as i64 * 100;
        let mut entry = manual(None, project.0, start, start + 100);
        entry.task_id = Some(task);
        entry.source = source;
        store.add_entry(&entry)?;
    }
    store.set_task_archived(task, true, 500)?;
    store.set_project_archived(project, true, 600)?;
    store.persist_transition(&Transition {
        snapshot: TrackerSnapshot {
            revision: 42,
            ..Default::default()
        },
        completed_entries: vec![],
        notifications: vec![],
    })?;
    let original = store.backup(1000)?;
    let path = directory.path().join("nested/backup.json");
    original.write_to_path(&path)?;
    let parsed = BackupDocument::read_from_path(&path)?;
    assert_document_eq(&original, &parsed);
    let (_, mut restored) = temporary_store();
    restored.restore(&parsed)?;
    assert_document_eq(&original, &restored.backup(1000)?);
    let mut replaced = original.clone();
    replaced.exported_at_ms = 2000;
    replaced.write_to_path(&path)?;
    assert_document_eq(&replaced, &BackupDocument::read_from_path(&path)?);
    Ok(())
}

#[test]
fn restore_database_failure_after_deletes_rolls_back_every_table() -> Result<(), houra::AppError> {
    let (_, mut store) = temporary_store();
    let project = store.create_project("Work", "#123456", 1)?;
    let task = store.create_task(project, "Task", 2)?;
    let mut entry = manual(None, project.0, 100, 200);
    entry.task_id = Some(task);
    store.add_entry(&entry)?;
    store.persist_transition(&houra_core::Transition {
        snapshot: TrackerSnapshot {
            revision: 7,
            ..Default::default()
        },
        completed_entries: vec![],
        notifications: vec![],
    })?;
    let original = store.backup(1)?;
    let mut invalid = original.clone();
    invalid.projects.push(invalid.projects[0].clone());
    invalid.validate()?;
    assert!(matches!(
        store.restore(&invalid),
        Err(houra::AppError::Database(_))
    ));
    assert_document_eq(&original, &store.backup(1)?);
    Ok(())
}

#[test]
fn invalid_documents_and_files_report_specific_errors() -> Result<(), Box<dyn std::error::Error>> {
    use houra::{AppError, storage::Store};
    use houra_core::*;
    let (directory, mut store) = temporary_store();
    let original = store.backup(1)?;
    let mut bad = original.clone();
    bad.version = 2;
    assert!(matches!(
        bad.validate(),
        Err(AppError::UnsupportedBackupVersion {
            found: 2,
            expected: 1
        })
    ));
    bad = original.clone();
    bad.format = "other".into();
    assert!(
        matches!(bad.validate(), Err(AppError::InvalidBackup(message)) if message == "unknown format marker")
    );
    bad = original.clone();
    bad.projects.clear();
    assert!(
        matches!(bad.validate(), Err(AppError::InvalidBackup(message)) if message == "the required General project is missing")
    );
    bad = original.clone();
    bad.entries.push(manual(None, 99, 0, 100));
    assert!(
        matches!(bad.validate(), Err(AppError::InvalidBackup(message)) if message == "entry None references a missing project")
    );
    bad.entries[0].project_id = ProjectId(1);
    bad.entries[0].task_id = Some(TaskId(99));
    assert!(
        matches!(bad.validate(), Err(AppError::InvalidBackup(message)) if message == "entry None has a missing or foreign task")
    );
    bad = original.clone();
    bad.tasks.push(Task {
        id: TaskId(1),
        project_id: ProjectId(99),
        name: "Task".into(),
        archived: false,
        created_at_ms: 0,
        updated_at_ms: 0,
    });
    assert!(
        matches!(bad.validate(), Err(AppError::InvalidBackup(message)) if message == "task TaskId(1) references a missing project")
    );
    let mut engine = TrackerEngine::new(ManualClock::at(100));
    let started = engine.apply(TrackerCommand::Start {
        project_id: ProjectId(1),
        task_id: None,
        note: String::new(),
    })?;
    bad = original.clone();
    bad.tracker = started.snapshot.clone();
    assert!(
        matches!(bad.validate(), Err(AppError::InvalidBackup(message)) if message == "backups must not contain an active timer")
    );
    store.persist_transition(&started)?;
    assert!(matches!(
        store.restore(&original),
        Err(AppError::RestoreWhileActive)
    ));
    assert_eq!(store.load_snapshot()?, started.snapshot);
    let path = directory.path().join("backup.json");
    std::fs::write(&path, b"{broken")?;
    assert!(matches!(
        BackupDocument::read_from_path(&path),
        Err(AppError::Json(_))
    ));
    bad.write_to_path(&path)
        .err()
        .unwrap_or_else(|| panic!("invalid backup accepted"));
    assert_eq!(std::fs::read(&path)?, b"{broken");
    assert!(
        matches!(BackupDocument::read_from_path(&directory.path().join("missing")), Err(AppError::Io { path, .. }) if path.ends_with("missing"))
    );
    assert!(
        matches!(original.write_to_path(directory.path()), Err(AppError::Io { path, .. }) if path == directory.path())
    );
    assert!(matches!(
        original.write_to_path(&path.join("child")),
        Err(AppError::Io { .. })
    ));
    let mut empty = Store::open_in_memory()?;
    let mut anonymous = original.clone();
    anonymous.entries = vec![manual(None, 1, 0, 100), manual(None, 1, 50, 150)];
    assert!(
        matches!(empty.restore(&anonymous), Err(AppError::Domain(DomainError::Overlap { conflicts })) if conflicts.is_empty())
    );
    Ok(())
}
