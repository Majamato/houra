use houra::{AppError, TrackerService};
use houra_core::{DomainError, ProjectId};
use tempfile::TempDir;
#[test]
fn concurrent_start_commands_are_serialized() {
    let directory = TempDir::new().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let service = TrackerService::start(directory.path().join("actor.sqlite3"))
        .unwrap_or_else(|error| panic!("service failed: {error}"));
    let first = service.handle.clone();
    let second = service.handle.clone();
    let command = || houra_core::TrackerCommand::Start {
        project_id: ProjectId(1),
        activity_id: None,
        note: "concurrent".into(),
    };
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let first_barrier = barrier.clone();
    let first_join = std::thread::spawn(move || {
        first_barrier.wait();
        first.apply(command())
    });
    let second_join = std::thread::spawn(move || {
        barrier.wait();
        second.apply(command())
    });
    let first_result = first_join
        .join()
        .unwrap_or_else(|_| panic!("first client panicked"));
    let second_result = second_join
        .join()
        .unwrap_or_else(|_| panic!("second client panicked"));
    let (accepted, rejected) = if first_result.is_ok() {
        (first_result, second_result)
    } else {
        (second_result, first_result)
    };
    let transition = accepted.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        service.handle.snapshot().unwrap_or_else(|e| panic!("{e}")),
        transition.snapshot
    );
    assert!(
        matches!(rejected, Err(AppError::Domain(DomainError::InvalidState(state))) if state == transition.snapshot.state)
    );
    assert!(
        service
            .handle
            .apply(houra_core::TrackerCommand::Stop)
            .is_ok()
    );
    assert!(service.shutdown().is_ok());
}
#[test]
fn rejected_persistence_keeps_engine_unchanged_and_shutdown_stops_handles()
-> Result<(), Box<dyn std::error::Error>> {
    use houra_core::{TrackerCommand, TrackerSnapshot};
    let directory = TempDir::new()?;
    let service = TrackerService::start(directory.path().join("service.sqlite3"))?;
    let handle = service.handle.clone();
    assert!(matches!(
        handle.apply(TrackerCommand::Start {
            project_id: ProjectId(99),
            activity_id: None,
            note: String::new()
        }),
        Err(AppError::InvalidProject(ProjectId(99)))
    ));
    assert_eq!(handle.snapshot()?, TrackerSnapshot::default());
    assert_eq!(handle.live_elapsed()?, std::time::Duration::ZERO);
    let started = handle.apply(TrackerCommand::Start {
        project_id: ProjectId(1),
        activity_id: None,
        note: "work".into(),
    })?;
    assert!(matches!(
        handle.apply(TrackerCommand::EditActive {
            project_id: ProjectId(99),
            activity_id: None,
            note: "bad".into()
        }),
        Err(AppError::InvalidProject(ProjectId(99)))
    ));
    assert_eq!(handle.snapshot()?, started.snapshot);
    service.shutdown()?;
    assert!(matches!(handle.snapshot(), Err(AppError::WorkerStopped)));
    assert!(matches!(
        handle.apply(TrackerCommand::Stop),
        Err(AppError::WorkerStopped)
    ));
    assert!(matches!(
        handle.projects(true),
        Err(AppError::WorkerStopped)
    ));
    assert!(matches!(
        handle.live_elapsed(),
        Err(AppError::WorkerStopped)
    ));
    Ok(())
}

#[test]
fn service_data_survives_restart_and_restore_refreshes_snapshot()
-> Result<(), Box<dyn std::error::Error>> {
    use houra_core::*;
    let directory = TempDir::new()?;
    let path = directory.path().join("service.sqlite3");
    let service = TrackerService::start(path.clone())?;
    let handle = &service.handle;
    let project = handle.create_project("Work".into(), "#123456".into(), 1)?;
    let activity = handle.create_activity("Custom".into(), 2)?;
    let mut entry = TimeEntry {
        id: None,
        project_id: project,
        activity_id: Some(activity),
        note: "note".into(),
        start_ms: 100,
        end_ms: 200,
        source: EntrySource::Manual,
        created_at_ms: 200,
        updated_at_ms: 200,
    };
    entry.id = Some(handle.add_entry(entry.clone())?);
    entry.note = "updated".into();
    entry.updated_at_ms = 300;
    handle.update_entry(entry.clone())?;
    handle.set_activity_archived(activity, true, 400)?;
    handle.set_project_archived(project, true, 400)?;
    let backup = handle.backup(500)?;
    service.shutdown()?;
    let service = TrackerService::start(path)?;
    assert_eq!(service.handle.entries(0, 300)?, vec![entry]);
    assert_eq!(service.handle.projects(true)?, backup.projects);
    assert_eq!(service.handle.activities(true)?, backup.activities);
    let mut restored = backup.clone();
    restored.tracker.revision = 42;
    service.handle.restore(restored.clone())?;
    assert_eq!(service.handle.snapshot()?, restored.tracker);
    service.shutdown()?;
    Ok(())
}

#[test]
fn active_timer_keeps_global_activity_across_projects_and_archiving()
-> Result<(), Box<dyn std::error::Error>> {
    use houra_core::{TrackerCommand, TrackerState};
    let directory = TempDir::new()?;
    let service = TrackerService::start(directory.path().join("global-activity.sqlite3"))?;
    let project = service
        .handle
        .create_project("Second".into(), "#123456".into(), 1)?;
    let activity = service.handle.create_activity("Custom".into(), 2)?;
    service.handle.apply(TrackerCommand::Start {
        project_id: ProjectId(1),
        activity_id: Some(activity),
        note: String::new(),
    })?;
    service.handle.apply(TrackerCommand::EditActive {
        project_id: project,
        activity_id: Some(activity),
        note: String::new(),
    })?;
    service.handle.set_activity_archived(activity, true, 3)?;
    service.handle.apply(TrackerCommand::Heartbeat)?;
    assert!(matches!(
        service.handle.snapshot()?.state,
        TrackerState::Running(ref active)
            if active.project_id == project && active.activity_id == Some(activity)
    ));
    service.handle.apply(TrackerCommand::Stop)?;
    service.shutdown()?;
    Ok(())
}

#[test]
fn interrupted_timer_recovers_only_persisted_interval() -> Result<(), Box<dyn std::error::Error>> {
    use houra_core::*;
    let directory = TempDir::new()?;
    let path = directory.path().join("service.sqlite3");
    let mut store = houra::storage::Store::open(&path)?;
    let clock = ManualClock::at(100);
    let mut engine = TrackerEngine::new(clock.clone());
    store.persist_transition(&engine.apply(TrackerCommand::Start {
        project_id: ProjectId(1),
        activity_id: None,
        note: "interrupted".into(),
    })?)?;
    clock.advance(std::time::Duration::from_millis(100));
    store.persist_transition(&engine.apply(TrackerCommand::Heartbeat)?)?;
    drop(store);
    let service = TrackerService::start(path.clone())?;
    let expected_active = engine
        .snapshot()
        .state
        .active()
        .cloned()
        .unwrap_or_else(|| panic!("active"));
    assert_eq!(
        service.handle.snapshot()?.state,
        TrackerState::RecoveryPending(PendingRecovery {
            active: expected_active,
            proposed_end_ms: 200,
            unresolved_idle_start_ms: None
        })
    );
    service.handle.apply(TrackerCommand::ResolveRecovery {
        end_ms: 200,
        resume: false,
    })?;
    let entries = service.handle.entries(i64::MIN, i64::MAX)?;
    assert_eq!(
        entries,
        vec![TimeEntry {
            id: Some(EntryId(1)),
            project_id: ProjectId(1),
            activity_id: None,
            note: "interrupted".into(),
            start_ms: 100,
            end_ms: 200,
            source: EntrySource::Recovery,
            created_at_ms: 200,
            updated_at_ms: 200
        }]
    );
    service.shutdown()?;
    let store = houra::storage::Store::open(&path)?;
    assert!(store.previous_shutdown_clean());
    assert_eq!(store.load_snapshot()?.state, TrackerState::Stopped);
    assert_eq!(store.list_all_entries()?, entries);
    Ok(())
}
