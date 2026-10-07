mod common;
use common::*;
use houra::{
    AppError,
    storage::{DATABASE_FILENAME, Store},
};
use houra_core::{DomainError, EntryId, ProjectId};
use tempfile::TempDir;
#[test]
fn fresh_store_seeds_general_and_global_activities() {
    let (_directory, store) = temporary_store();
    let projects = store
        .list_projects(false)
        .unwrap_or_else(|error| panic!("list failed: {error}"));
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "General");
    let activities = store
        .list_activities(false)
        .unwrap_or_else(|error| panic!("list failed: {error}"));
    assert_eq!(
        activities
            .iter()
            .map(|activity| activity.name.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            "Code Review",
            "Design",
            "Documentation",
            "Meetings",
            "Planning",
            "Programming",
            "Research",
            "Testing",
        ])
    );
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
fn growing_final_interval_of_multi_interval_entry_reports_overlap() -> Result<(), AppError> {
    let mut store = Store::open_in_memory()?;
    let mut first = manual(None, 1, 0, 60_000);
    first.intervals.push(houra_core::TrackedInterval {
        id: None,
        start_ms: 120_000,
        end_ms: 180_000,
        source: houra_core::EntrySource::Timer,
    });
    let first_id = store.add_entry(&first)?;
    let second_id = store.add_entry(&manual(None, 1, 240_000, 300_000))?;
    let before = store.entry(first_id)?;
    let mut edited = before.clone();
    edited.intervals[1].end_ms = 241_000;

    assert!(
        matches!(store.update_entry(&edited), Err(AppError::Domain(DomainError::Overlap { conflicts })) if conflicts == vec![second_id])
    );
    assert_eq!(store.entry(first_id)?, before);
    Ok(())
}

#[test]
fn activity_can_be_used_under_multiple_projects() {
    let (_directory, mut store) = temporary_store();
    let second = store
        .create_project("Second", "#ff0000", 1)
        .unwrap_or_else(|error| panic!("project failed: {error}"));
    let activity = store
        .create_activity("Custom", 1)
        .unwrap_or_else(|error| panic!("activity failed: {error}"));
    let mut entry = manual(None, 1, 100, 200);
    entry.activity_id = Some(activity);
    assert!(store.add_entry(&entry).is_ok());
    entry.project_id = second;
    entry.intervals[0].start_ms = 200;
    entry.intervals[0].end_ms = 300;
    assert!(store.add_entry(&entry).is_ok());
}

#[test]
fn referenced_activity_is_archived_not_deleted() {
    let (_directory, mut store) = temporary_store();
    let activity = store
        .create_activity("Custom", 1)
        .unwrap_or_else(|error| panic!("activity failed: {error}"));
    let mut entry = manual(None, 1, 100, 200);
    entry.activity_id = Some(activity);
    assert!(store.add_entry(&entry).is_ok());
    assert!(store.set_activity_archived(activity, true, 300).is_ok());
    assert!(matches!(
        store.delete_activity_permanently(activity),
        Err(AppError::ReferencedItem)
    ));
    let archived = store
        .list_activities(true)
        .unwrap_or_else(|e| panic!("{e}"))
        .into_iter()
        .find(|candidate| candidate.id == activity)
        .unwrap_or_else(|| panic!("activity"));
    assert_eq!(archived.name, "Custom");
    assert!(archived.archived);
}

#[test]
fn activity_used_by_active_timer_cannot_be_deleted() -> Result<(), AppError> {
    use houra_core::{ManualClock, TrackerCommand, TrackerEngine};
    let mut store = Store::open_in_memory()?;
    let activity = store.create_activity("Custom", 1)?;
    let mut engine = TrackerEngine::new(ManualClock::at(100));
    let started = engine.apply(TrackerCommand::Start {
        project_id: ProjectId(1),
        activity_id: Some(activity),
        note: String::new(),
    })?;
    store.persist_transition(&started)?;
    assert!(matches!(
        store.delete_activity_permanently(activity),
        Err(AppError::ReferencedItem)
    ));
    Ok(())
}

#[test]
fn archived_activity_can_remain_on_an_edited_entry() -> Result<(), AppError> {
    let mut store = Store::open_in_memory()?;
    let second = store.create_project("Second", "#ff0000", 1)?;
    let activity = store.create_activity("Custom", 2)?;
    let mut entry = manual(None, 1, 100, 200);
    entry.activity_id = Some(activity);
    entry.id = Some(store.add_entry(&entry)?);
    entry.intervals[0].id = Some(houra_core::IntervalId(1));
    store.set_activity_archived(activity, true, 3)?;

    entry.project_id = second;
    entry.updated_at_ms = 4;
    store.update_entry(&entry)?;
    assert_eq!(store.list_all_entries()?, vec![entry.clone()]);

    let mut new_entry = entry;
    new_entry.id = None;
    new_entry.intervals[0].start_ms = 200;
    new_entry.intervals[0].end_ms = 300;
    new_entry.intervals[0].id = None;
    assert!(matches!(
        store.add_entry(&new_entry),
        Err(AppError::InvalidActivity(id)) if id == activity
    ));
    Ok(())
}

#[test]
fn archived_default_activity_is_not_reseeded_on_reopen() -> Result<(), AppError> {
    let (directory, store) = temporary_store();
    let activity = store
        .list_activities(false)?
        .into_iter()
        .find(|activity| activity.name == "Programming")
        .unwrap_or_else(|| panic!("Programming activity"));
    store.set_activity_archived(activity.id, true, 1)?;
    drop(store);

    let reopened = Store::open(&directory.path().join(DATABASE_FILENAME))?;
    assert_eq!(reopened.list_activities(false)?.len(), 7);
    let all = reopened.list_activities(true)?;
    assert_eq!(all.len(), 8);
    assert!(
        all.iter()
            .find(|candidate| candidate.id == activity.id)
            .is_some_and(|candidate| candidate.archived)
    );
    Ok(())
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
    let store = Store::open(&directory.path().join(DATABASE_FILENAME))?;
    assert!(!store.previous_shutdown_clean());
    assert_document_eq(&before, &store.backup(1)?);
    store.mark_clean_shutdown()?;
    drop(store);
    let store = Store::open(&directory.path().join(DATABASE_FILENAME))?;
    assert!(store.previous_shutdown_clean());
    assert_document_eq(&before, &store.backup(1)?);
    Ok(())
}

#[test]
fn unsupported_schemas_are_rejected_without_modification() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = TempDir::new()?;
    let path = directory.path().join("future.sqlite3");
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch("CREATE TABLE legacy(value TEXT); INSERT INTO legacy VALUES('keep'); PRAGMA user_version=1;")?;
    drop(connection);
    let before = std::fs::read(&path)?;
    assert!(matches!(
        Store::open(&path),
        Err(AppError::UnsupportedDatabaseSchema {
            found: 1,
            expected: 3
        })
    ));
    assert_eq!(std::fs::read(&path)?, before);
    Ok(())
}

#[test]
fn global_activity_uniqueness_archiving_and_deletion() -> Result<(), AppError> {
    let mut store = Store::open_in_memory()?;
    let project = store.create_project(" Work ", "#123456", 10)?;
    assert!(matches!(
        store.create_project("work", "#123456", 11),
        Err(AppError::Database(_))
    ));
    let activity = store.create_activity(" Activity ", 12)?;
    assert!(matches!(
        store.create_activity("activity", 13),
        Err(AppError::Database(_))
    ));
    assert_eq!(store.list_activities(false)?.len(), 9);
    let mut entry = manual(None, project.0, 100, 200);
    entry.activity_id = Some(activity);
    store.add_entry(&entry)?;
    assert!(matches!(
        store.delete_project_permanently(project),
        Err(AppError::ReferencedItem)
    ));
    assert!(matches!(
        store.delete_project_permanently(ProjectId(1)),
        Err(AppError::ReferencedItem)
    ));
    store.set_activity_archived(activity, true, 20)?;
    assert_eq!(store.list_activities(false)?.len(), 8);
    let archived = store
        .list_activities(true)?
        .into_iter()
        .find(|t| t.id == activity)
        .unwrap_or_else(|| panic!("activity"));
    assert_eq!(
        archived,
        houra_core::Activity {
            id: activity,
            name: "Activity".into(),
            archived: true,
            created_at_ms: 12,
            updated_at_ms: 20
        }
    );
    entry.intervals[0].start_ms = 200;
    entry.intervals[0].end_ms = 300;
    assert!(
        matches!(store.add_entry(&entry), Err(AppError::InvalidActivity(id)) if id == activity)
    );
    store.set_project_archived(project, true, 30)?;
    assert_eq!(store.list_projects(false)?.len(), 1);
    assert_eq!(store.list_projects(true)?.len(), 2);
    let other = store.create_activity("Other", 30)?;
    assert!(
        matches!(store.add_entry(&entry), Err(AppError::InvalidActivity(id)) if id == activity)
    );
    store.set_project_archived(project, false, 40)?;
    store.set_activity_archived(activity, false, 40)?;
    store.add_entry(&entry)?;
    store.delete_activity_permanently(other)?;
    let empty = store.create_project("Empty", "#ffffff", 50)?;
    store.delete_project_permanently(empty)?;
    assert_eq!(store.list_activities(true)?.len(), 9);
    assert_eq!(store.list_projects(true)?.len(), 2);
    Ok(())
}

#[test]
fn entry_updates_ranges_and_invalid_references() -> Result<(), AppError> {
    let mut store = Store::open_in_memory()?;
    let mut first = manual(None, 1, 100, 200);
    first.id = Some(store.add_entry(&first)?);
    first.intervals[0].id = Some(houra_core::IntervalId(1));
    let mut second = manual(None, 1, 200, 300);
    second.id = Some(store.add_entry(&second)?);
    second.intervals[0].id = Some(houra_core::IntervalId(2));
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
    invalid.intervals[0].end_ms = 201;
    assert!(
        matches!(store.update_entry(&invalid), Err(AppError::Domain(DomainError::Overlap { conflicts })) if conflicts == vec![EntryId(2)])
    );
    invalid = first.clone();
    invalid.id = None;
    assert!(
        matches!(store.update_entry(&invalid), Err(AppError::InvalidBackup(message)) if message == "entry ID is required for update")
    );
    invalid.id = Some(EntryId(99));
    invalid.intervals[0].start_ms = 400;
    invalid.intervals[0].end_ms = 500;
    assert!(
        matches!(store.update_entry(&invalid), Err(AppError::InvalidBackup(message)) if message == "entry EntryId(99) was not found")
    );
    invalid.project_id = ProjectId(99);
    assert!(matches!(
        store.add_entry(&invalid),
        Err(AppError::InvalidProject(ProjectId(99)))
    ));
    invalid.project_id = ProjectId(1);
    invalid.activity_id = Some(houra_core::ActivityId(99));
    assert!(matches!(
        store.add_entry(&invalid),
        Err(AppError::InvalidActivity(houra_core::ActivityId(99)))
    ));
    invalid.activity_id = None;
    invalid.intervals[0].end_ms = invalid.intervals[0].start_ms;
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
            assert!(matches!(
                result,
                Err(AppError::Domain(DomainError::Overlap { conflicts })) if conflicts == vec![EntryId(2)]
            ));
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
        activity_id: None,
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

#[test]
fn continued_intervals_append_to_one_entry_and_exclude_the_break() -> Result<(), AppError> {
    use houra_core::{ManualClock, TrackerCommand, TrackerEngine};
    let mut store = Store::open_in_memory()?;
    let mut original = manual(None, 1, 0, 16 * 60 * 1_000);
    original.intervals[0].source = houra_core::EntrySource::Timer;
    let entry_id = store.add_entry(&original)?;
    let clock = ManualClock::at(21 * 60 * 1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    let continued = engine.apply(TrackerCommand::Continue {
        entry_id,
        project_id: ProjectId(1),
        activity_id: None,
        note: "manual".into(),
    })?;
    store.persist_transition(&continued)?;
    clock.advance(std::time::Duration::from_secs(60));
    store.persist_transition(&engine.apply(TrackerCommand::Stop)?)?;

    let entries = store.list_all_entries()?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, Some(entry_id));
    assert_eq!(entries[0].intervals.len(), 2);
    assert_eq!(entries[0].duration_ms(), 17 * 60 * 1_000);
    Ok(())
}

#[test]
fn deleting_entry_removes_all_intervals_and_keeps_others() -> Result<(), AppError> {
    let mut store = Store::open_in_memory()?;
    let mut first = manual(None, 1, 0, 60_000);
    first.intervals.push(houra_core::TrackedInterval {
        id: None,
        start_ms: 120_000,
        end_ms: 180_000,
        source: houra_core::EntrySource::Timer,
    });
    let first_id = store.add_entry(&first)?;
    let second_id = store.add_entry(&manual(None, 1, 240_000, 300_000))?;
    store.delete_entry(first_id)?;
    assert!(matches!(
        store.entry(first_id),
        Err(AppError::InvalidBackup(_))
    ));
    let remaining = store.list_all_entries()?;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, Some(second_id));
    assert_eq!(store.list_entries(0, 180_000)?, vec![]);
    store.add_entry(&manual(None, 1, 0, 60_000))?;
    Ok(())
}

#[test]
fn deleting_unknown_entry_reports_not_found() -> Result<(), AppError> {
    let mut store = Store::open_in_memory()?;
    assert!(matches!(
        store.delete_entry(EntryId(99)),
        Err(AppError::InvalidBackup(message)) if message == "entry EntryId(99) was not found"
    ));
    let id = store.add_entry(&manual(None, 1, 100, 200))?;
    store.delete_entry(id)?;
    assert!(matches!(
        store.delete_entry(id),
        Err(AppError::InvalidBackup(_))
    ));
    Ok(())
}

#[test]
fn deleted_range_accepts_new_entries() -> Result<(), AppError> {
    let mut store = Store::open_in_memory()?;
    let id = store.add_entry(&manual(None, 1, 100, 200))?;
    assert!(store.add_entry(&manual(None, 1, 150, 250)).is_err());
    store.delete_entry(id)?;
    store.add_entry(&manual(None, 1, 150, 250))?;
    Ok(())
}

#[test]
fn active_entry_cannot_be_deleted_in_any_active_state() -> Result<(), AppError> {
    use houra_core::{ManualClock, TrackerCommand, TrackerEngine, TrackerState, Transition};
    for pending in ["running", "idle", "recovery"] {
        let mut store = Store::open_in_memory()?;
        let entry_id = store.add_entry(&manual(None, 1, 100, 200))?;
        let clock = ManualClock::at(1_000);
        let mut engine = TrackerEngine::new(clock.clone());
        let continued = engine.apply(TrackerCommand::Continue {
            entry_id,
            project_id: ProjectId(1),
            activity_id: None,
            note: "manual".into(),
        });
        let continued = continued.unwrap_or_else(|error| panic!("continue failed: {error}"));
        store.persist_transition(&continued)?;
        if pending == "idle" {
            clock.advance(std::time::Duration::from_millis(600));
            let idle = engine.apply(TrackerCommand::IdleDetected {
                idle_start_ms: 1_500,
            });
            let idle = idle.unwrap_or_else(|error| panic!("idle failed: {error}"));
            store.persist_transition(&idle)?;
            let returned = engine.apply(TrackerCommand::UserReturned { return_ms: 1_600 });
            let returned = returned.unwrap_or_else(|error| panic!("return failed: {error}"));
            store.persist_transition(&returned)?;
            assert!(matches!(
                store.load_snapshot()?.state,
                TrackerState::IdlePending(_)
            ));
        } else if pending == "recovery" {
            let snapshot = engine
                .snapshot()
                .state
                .active()
                .cloned()
                .unwrap_or_else(|| panic!("expected an active timer"));
            assert_eq!(snapshot.entry_id, Some(entry_id));
            let recovered = TrackerEngine::restore(clock.clone(), engine.snapshot().clone(), true);
            store.persist_transition(&Transition {
                snapshot: recovered.snapshot().clone(),
                completed_entries: vec![],
                notifications: vec![],
            })?;
            assert!(matches!(
                store.load_snapshot()?.state,
                TrackerState::RecoveryPending(_)
            ));
        }
        let before = store.load_snapshot()?;
        assert!(
            matches!(store.delete_entry(entry_id), Err(AppError::ActiveTimeEntry)),
            "{pending} state must refuse to delete the tracked entry"
        );
        assert_eq!(store.load_snapshot()?, before);
        assert!(store.entry(entry_id).is_ok());
    }
    Ok(())
}

#[test]
fn deleting_other_entries_while_running_keeps_timer_stoppable() -> Result<(), AppError> {
    use houra_core::{ManualClock, TrackerCommand, TrackerEngine};
    let mut store = Store::open_in_memory()?;
    let first_id = store.add_entry(&manual(None, 1, 0, 100))?;
    let other_id = store.add_entry(&manual(None, 1, 200, 300))?;
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    let continued = engine.apply(TrackerCommand::Continue {
        entry_id: first_id,
        project_id: ProjectId(1),
        activity_id: None,
        note: "manual".into(),
    });
    let continued = continued.unwrap_or_else(|error| panic!("continue failed: {error}"));
    store.persist_transition(&continued)?;
    store.delete_entry(other_id)?;
    clock.advance(std::time::Duration::from_secs(60));
    let stopped = engine.apply(TrackerCommand::Stop);
    let stopped = stopped.unwrap_or_else(|error| panic!("stop failed: {error}"));
    store.persist_transition(&stopped)?;
    let entries = store.list_all_entries()?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, Some(first_id));
    assert_eq!(entries[0].intervals.len(), 2);
    Ok(())
}

#[test]
fn pause_banks_one_segment_and_resume_appends_to_the_same_entry() -> Result<(), AppError> {
    use houra_core::{ManualClock, TrackerCommand, TrackerEngine, TrackerState};
    let mut store = Store::open_in_memory()?;
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    store.persist_transition(&engine.apply(TrackerCommand::Start {
        project_id: ProjectId(1),
        activity_id: None,
        note: "work".into(),
    })?)?;
    clock.advance(std::time::Duration::from_secs(60));
    let paused = engine.apply(TrackerCommand::Pause)?;
    let adopted = store
        .persist_transition(&paused)?
        .ok_or_else(|| AppError::InvalidBackup("pause banked no entry".into()))?;
    engine.adopt_paused_entry_id(adopted);
    let snapshot = store.load_snapshot()?;
    assert!(matches!(
        snapshot.state,
        TrackerState::Paused(ref paused)
        if paused.active.entry_id == Some(adopted)
            && paused.active.accumulated_ms == 60_000
    ));
    assert_eq!(engine.live_elapsed(), std::time::Duration::from_secs(60));
    let banked = store.entry(adopted)?;
    assert_eq!(banked.intervals.len(), 1);
    assert_eq!(banked.intervals[0].start_ms, 1_000);
    assert_eq!(banked.intervals[0].end_ms, 61_000);
    store.persist_transition(&engine.apply(TrackerCommand::EditActive {
        project_id: ProjectId(1),
        activity_id: None,
        note: "edited while paused".into(),
    })?)?;
    assert_eq!(store.entry(adopted)?.note, "edited while paused");
    clock.advance(std::time::Duration::from_secs(300));
    store.persist_transition(&engine.apply(TrackerCommand::Resume)?)?;
    assert_eq!(engine.live_elapsed(), std::time::Duration::from_secs(60));
    clock.advance(std::time::Duration::from_secs(30));
    assert_eq!(engine.live_elapsed(), std::time::Duration::from_secs(90));
    store.persist_transition(&engine.apply(TrackerCommand::Stop)?)?;
    let entries = store.list_all_entries()?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, Some(adopted));
    assert_eq!(entries[0].intervals.len(), 2);
    assert_eq!(entries[0].duration_ms(), 90_000);
    Ok(())
}

#[test]
fn dev_database_leaves_production_database_unchanged() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/app-variants.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path)?)?;
    let subdir = |variant: &str| {
        manifest
            .get(variant)
            .and_then(|variant| variant.get("data_subdir"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("the manifest must define {variant}.data_subdir"))
    };
    let (stable_subdir, dev_subdir) = (subdir("stable"), subdir("devel"));
    assert_ne!(stable_subdir, dev_subdir);
    let directory = TempDir::new()?;
    let base = directory.path().join("data with spaces");
    std::fs::create_dir(&base)?;
    let stable_path = base.join(stable_subdir).join(DATABASE_FILENAME);
    let dev_path = base.join(dev_subdir).join(DATABASE_FILENAME);
    let store = Store::open(&stable_path)?;
    store.create_project("Stable Only", "#3584e4", 1)?;
    let projects_before = store.list_projects(false)?;
    let entries_before = store.list_all_entries()?;
    drop(store);
    let dev = Store::open(&dev_path)?;
    dev.create_project("Dev Only", "#9141ac", 1)?;
    dev.mark_clean_shutdown()?;
    drop(dev);
    let store = Store::open(&stable_path)?;
    assert_eq!(store.list_projects(false)?, projects_before);
    assert_eq!(store.list_all_entries()?, entries_before);
    assert!(
        store
            .list_projects(false)?
            .iter()
            .all(|project| project.name != "Dev Only")
    );
    Ok(())
}
