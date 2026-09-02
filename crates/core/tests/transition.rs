use std::time::Duration;

use houra_core::{
    IdleDecision, ManualClock, Notification, ProjectId, TrackerCommand, TrackerEngine, TrackerState,
};

fn start(engine: &mut TrackerEngine<ManualClock>) {
    let result = engine.apply(TrackerCommand::Start {
        project_id: ProjectId(1),
        task_id: None,
        note: "design".into(),
    });
    assert!(result.is_ok());
}

#[test]
fn start_stop_records_exact_interval() {
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(42));
    let result = engine.apply(TrackerCommand::Stop);
    assert!(result.is_ok());
    let transition = result.unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(transition.completed_entries[0].duration_ms(), 42_000);
    assert_eq!(transition.notifications, vec![Notification::TimerStopped]);
}

#[test]
fn wall_clock_reversal_does_not_create_negative_entry() {
    let clock = ManualClock::at(20_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.set_wall_time_ms(10_000);
    let result = engine.apply(TrackerCommand::Stop);
    assert!(result.is_ok());
    assert!(
        result
            .unwrap_or_else(|error| panic!("unexpected error: {error}"))
            .completed_entries
            .is_empty()
    );
}

#[test]
fn discard_idle_resumes_at_recorded_return_not_dialog_time() {
    let clock = ManualClock::at(10_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(600));
    let idle = engine.apply(TrackerCommand::IdleDetected {
        idle_start_ms: 310_000,
    });
    assert!(idle.is_ok());
    let returned = engine.apply(TrackerCommand::UserReturned { return_ms: 610_000 });
    assert!(returned.is_ok());
    clock.advance(Duration::from_secs(90));
    let result = engine.apply(TrackerCommand::ResolveIdle(IdleDecision::DiscardAndResume));
    assert!(result.is_ok());
    let transition = result.unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(transition.completed_entries[0].start_ms, 10_000);
    assert_eq!(transition.completed_entries[0].end_ms, 310_000);
    match transition.snapshot.state {
        TrackerState::Running(active) => assert_eq!(active.start_ms, 610_000),
        state => panic!("expected running, got {state:?}"),
    }
}

#[test]
fn reassign_idle_preserves_whole_timeline() {
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(10));
    assert!(
        engine
            .apply(TrackerCommand::IdleDetected {
                idle_start_ms: 6_000
            })
            .is_ok()
    );
    assert!(
        engine
            .apply(TrackerCommand::UserReturned { return_ms: 11_000 })
            .is_ok()
    );
    let result = engine.apply(TrackerCommand::ResolveIdle(
        IdleDecision::ReassignAndResume {
            project_id: ProjectId(2),
            task_id: None,
            note: "break".into(),
        },
    ));
    assert!(result.is_ok());
    let entries = result
        .unwrap_or_else(|error| panic!("unexpected error: {error}"))
        .completed_entries;
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries.iter().map(|entry| entry.duration_ms()).sum::<i64>(),
        10_000
    );
}

#[test]
fn recovery_never_includes_time_after_heartbeat() {
    let clock = ManualClock::at(5_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(30));
    assert!(engine.apply(TrackerCommand::Heartbeat).is_ok());
    let snapshot = engine.snapshot().clone();
    clock.advance(Duration::from_secs(3_600));
    let mut recovered = TrackerEngine::restore(clock, snapshot, true);
    let pending = match &recovered.snapshot().state {
        TrackerState::RecoveryPending(pending) => pending,
        state => panic!("expected recovery, got {state:?}"),
    };
    assert_eq!(pending.proposed_end_ms, 35_000);
    let result = recovered.apply(TrackerCommand::ResolveRecovery {
        end_ms: 35_000,
        resume: false,
    });
    assert!(result.is_ok());
    assert_eq!(
        result
            .unwrap_or_else(|error| panic!("unexpected error: {error}"))
            .completed_entries[0]
            .end_ms,
        35_000
    );
}
