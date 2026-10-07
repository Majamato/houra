use houra::{AppError, TrackerService};
use houra_core::{DomainError, ProjectId};
use tempfile::TempDir;
#[test]
fn concurrent_start_commands_are_serialized() {
    let directory = TempDir::new().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let service = TrackerService::start(directory.path().join("actor.sqlite3"), Box::new(|| {}))
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
        matches!(rejected, Err(AppError::Domain(DomainError::InvalidState(state))) if *state == transition.snapshot.state)
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
    let service = TrackerService::start(directory.path().join("service.sqlite3"), Box::new(|| {}))?;
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
    let service = TrackerService::start(path.clone(), Box::new(|| {}))?;
    let handle = &service.handle;
    let project = handle.create_project("Work".into(), "#123456".into(), 1)?;
    let activity = handle.create_activity("Custom".into(), 2)?;
    let mut entry = TimeEntry {
        id: None,
        project_id: project,
        activity_id: Some(activity),
        note: "note".into(),
        intervals: vec![TrackedInterval {
            id: None,
            start_ms: 100,
            end_ms: 200,
            source: EntrySource::Manual,
        }],
        created_at_ms: 200,
        updated_at_ms: 200,
    };
    entry.id = Some(handle.add_entry(entry.clone())?);
    entry.intervals[0].id = Some(IntervalId(1));
    entry.note = "updated".into();
    entry.updated_at_ms = 300;
    handle.update_entry(entry.clone())?;
    handle.set_activity_archived(activity, true, 400)?;
    handle.set_project_archived(project, true, 400)?;
    let backup = handle.backup(500)?;
    service.shutdown()?;
    let service = TrackerService::start(path, Box::new(|| {}))?;
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
    let service = TrackerService::start(
        directory.path().join("global-activity.sqlite3"),
        Box::new(|| {}),
    )?;
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
    let service = TrackerService::start(path.clone(), Box::new(|| {}))?;
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
            intervals: vec![TrackedInterval {
                id: Some(IntervalId(1)),
                start_ms: 100,
                end_ms: 200,
                source: EntrySource::Recovery
            }],
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

#[test]
fn continue_resolves_identity_and_active_edits_update_shared_details()
-> Result<(), Box<dyn std::error::Error>> {
    use houra_core::*;
    let directory = TempDir::new()?;
    let service =
        TrackerService::start(directory.path().join("continue.sqlite3"), Box::new(|| {}))?;
    let entry_id = service.handle.add_entry(TimeEntry {
        id: None,
        project_id: ProjectId(1),
        activity_id: None,
        note: "original".into(),
        intervals: vec![TrackedInterval {
            id: None,
            start_ms: 100,
            end_ms: 200,
            source: EntrySource::Manual,
        }],
        created_at_ms: 200,
        updated_at_ms: 200,
    })?;
    let continued = service.handle.continue_entry(entry_id)?;
    assert_eq!(
        continued
            .snapshot
            .state
            .active()
            .and_then(|active| active.entry_id),
        Some(entry_id)
    );
    let duplicate = service.handle.continue_entry(entry_id)?;
    assert_eq!(duplicate.snapshot.revision, continued.snapshot.revision);
    service.handle.apply(TrackerCommand::EditActive {
        project_id: ProjectId(1),
        activity_id: None,
        note: "edited while active".into(),
    })?;
    assert_eq!(service.handle.entry(entry_id)?.note, "edited while active");
    service.handle.apply(TrackerCommand::Stop)?;
    service.shutdown()?;
    Ok(())
}

#[test]
fn continuing_an_entry_that_reaches_past_now_is_refused() -> Result<(), Box<dyn std::error::Error>>
{
    use houra_core::*;
    let directory = TempDir::new()?;
    let service = TrackerService::start(directory.path().join("future.sqlite3"), Box::new(|| {}))?;
    let now = chrono::Utc::now().timestamp_millis();
    let entry_id = service.handle.add_entry(TimeEntry {
        id: None,
        project_id: ProjectId(1),
        activity_id: None,
        note: "edited past now".into(),
        intervals: vec![TrackedInterval {
            id: None,
            start_ms: now - 60_000,
            end_ms: now + 12 * 60 * 60 * 1_000,
            source: EntrySource::Manual,
        }],
        created_at_ms: now,
        updated_at_ms: now,
    })?;
    assert!(matches!(
        service.handle.continue_entry(entry_id),
        Err(AppError::Domain(DomainError::Overlap { conflicts })) if conflicts == vec![entry_id]
    ));
    assert!(service.handle.snapshot()?.state.active().is_none());
    service.shutdown()?;
    Ok(())
}

#[test]
fn delete_entry_round_trip_reports_not_found_afterwards() -> Result<(), Box<dyn std::error::Error>>
{
    use houra_core::*;
    let directory = TempDir::new()?;
    let service = TrackerService::start(directory.path().join("delete.sqlite3"), Box::new(|| {}))?;
    let entry_id = service.handle.add_entry(TimeEntry {
        id: None,
        project_id: ProjectId(1),
        activity_id: None,
        note: "doomed".into(),
        intervals: vec![TrackedInterval {
            id: None,
            start_ms: 100,
            end_ms: 200,
            source: EntrySource::Manual,
        }],
        created_at_ms: 200,
        updated_at_ms: 200,
    })?;
    service.handle.delete_entry(entry_id)?;
    assert!(matches!(
        service.handle.entry(entry_id),
        Err(AppError::InvalidBackup(_))
    ));
    assert!(matches!(
        service.handle.delete_entry(entry_id),
        Err(AppError::InvalidBackup(_))
    ));
    assert!(matches!(
        service.handle.continue_entry(entry_id),
        Err(AppError::InvalidBackup(_))
    ));
    assert_eq!(service.handle.entries(0, 1_000_000_000_000)?, vec![]);
    service.shutdown()?;
    Ok(())
}

#[test]
fn active_entry_cannot_be_deleted_until_stopped() -> Result<(), Box<dyn std::error::Error>> {
    use houra_core::*;
    let directory = TempDir::new()?;
    let service = TrackerService::start(
        directory.path().join("delete-active.sqlite3"),
        Box::new(|| {}),
    )?;
    let entry_id = service.handle.add_entry(TimeEntry {
        id: None,
        project_id: ProjectId(1),
        activity_id: None,
        note: "tracked".into(),
        intervals: vec![TrackedInterval {
            id: None,
            start_ms: 100,
            end_ms: 200,
            source: EntrySource::Manual,
        }],
        created_at_ms: 200,
        updated_at_ms: 200,
    })?;
    service.handle.continue_entry(entry_id)?;
    assert!(matches!(
        service.handle.delete_entry(entry_id),
        Err(AppError::ActiveTimeEntry)
    ));
    service.handle.apply(TrackerCommand::Stop)?;
    service.handle.delete_entry(entry_id)?;
    assert_eq!(service.handle.entries(0, 1_000_000_000_000)?, vec![]);
    service.shutdown()?;
    Ok(())
}

#[test]
fn paused_timer_restarts_paused_and_resumes_onto_the_same_entry()
-> Result<(), Box<dyn std::error::Error>> {
    use houra_core::*;
    let directory = TempDir::new()?;
    let path = directory.path().join("pause-restart.sqlite3");
    let service = TrackerService::start(path.clone(), Box::new(|| {}))?;
    service.handle.apply(TrackerCommand::Start {
        project_id: ProjectId(1),
        activity_id: None,
        note: "work".into(),
    })?;
    std::thread::sleep(std::time::Duration::from_millis(15));
    let paused = service.handle.apply(TrackerCommand::Pause)?;
    assert_eq!(paused.completed_entries.len(), 1);
    let TrackerState::Paused(timer) = paused.snapshot.state else {
        panic!("pause did not freeze the timer");
    };
    let entry_id = timer
        .active
        .entry_id
        .unwrap_or_else(|| panic!("pause adopted no entry identity"));
    let frozen = service.handle.live_elapsed()?;
    assert!(frozen > std::time::Duration::ZERO);
    service.shutdown()?;
    let service = TrackerService::start(path, Box::new(|| {}))?;
    let snapshot = service.handle.snapshot()?;
    assert!(matches!(
        snapshot.state,
        TrackerState::Paused(ref timer) if timer.active.entry_id == Some(entry_id)
    ));
    assert_eq!(service.handle.live_elapsed()?, frozen);
    service.handle.apply(TrackerCommand::Resume)?;
    // Resume continues from the banked total instead of restarting at zero.
    let resumed = service.handle.live_elapsed()?;
    assert!(resumed >= frozen);
    assert!(resumed - frozen < std::time::Duration::from_secs(1));
    std::thread::sleep(std::time::Duration::from_millis(15));
    let running = service.handle.live_elapsed()?;
    assert!(running > frozen);
    service.handle.apply(TrackerCommand::Stop)?;
    let entry = service.handle.entry(entry_id)?;
    assert_eq!(entry.intervals.len(), 2);
    let frozen_ms = i64::try_from(frozen.as_millis()).unwrap_or(i64::MAX);
    assert_eq!(entry.intervals[0].duration_ms(), frozen_ms);
    assert!(entry.intervals[1].duration_ms() > 0);
    assert_eq!(
        entry.duration_ms(),
        frozen_ms + entry.intervals[1].duration_ms()
    );
    service.shutdown()?;
    Ok(())
}

#[test]
fn observer_hears_only_state_changes() -> Result<(), Box<dyn std::error::Error>> {
    use houra_core::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let directory = TempDir::new()?;
    let count = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&count);
    let service = TrackerService::start(
        directory.path().join("observer.sqlite3"),
        Box::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        }),
    )?;
    let handle = service.handle.clone();
    // The observer runs after the reply; the next request waits for it.
    let changes = || -> Result<usize, AppError> {
        handle.snapshot()?;
        Ok(count.load(Ordering::SeqCst))
    };
    assert!(handle.apply(TrackerCommand::Resume).is_err());
    assert_eq!(changes()?, 0);
    handle.apply(TrackerCommand::Start {
        project_id: ProjectId(1),
        activity_id: None,
        note: "observed".into(),
    })?;
    assert_eq!(changes()?, 1);
    // A running heartbeat is persisted as a new revision but isn't announced.
    let started = handle.snapshot()?.revision;
    assert!(handle.apply(TrackerCommand::Heartbeat)?.snapshot.revision > started);
    assert_eq!(changes()?, 1);
    std::thread::sleep(std::time::Duration::from_millis(15));
    let paused = handle.apply(TrackerCommand::Pause)?;
    assert_eq!(changes()?, 2);
    handle.apply(TrackerCommand::Heartbeat)?;
    assert_eq!(changes()?, 2);
    assert!(handle.apply(TrackerCommand::Pause).is_err());
    assert_eq!(changes()?, 2);
    handle.apply(TrackerCommand::Stop)?;
    assert_eq!(changes()?, 3);
    handle.restore(handle.backup(0)?)?;
    assert_eq!(changes()?, 4);
    let TrackerState::Paused(timer) = paused.snapshot.state else {
        panic!("pause did not freeze the timer");
    };
    let entry_id = timer
        .active
        .entry_id
        .unwrap_or_else(|| panic!("pause adopted no entry identity"));
    handle.continue_entry(entry_id)?;
    assert_eq!(changes()?, 5);
    // Continuing the running entry is accepted but changes nothing.
    handle.continue_entry(entry_id)?;
    assert_eq!(changes()?, 5);
    handle.apply(TrackerCommand::Stop)?;
    service.shutdown()?;
    Ok(())
}
