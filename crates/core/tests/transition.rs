mod common;
use std::time::Duration;

use houra_core::{
    IdleDecision, ManualClock, Notification, ProjectId, TrackerCommand, TrackerEngine, TrackerState,
};

fn start(engine: &mut TrackerEngine<ManualClock>) {
    let result = engine.apply(TrackerCommand::Start {
        project_id: ProjectId(1),
        activity_id: None,
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
    let mut expected = common::entry(None, 1_000, 43_000);
    expected.source = houra_core::EntrySource::Timer;
    expected.note = "design".into();
    assert_eq!(transition.completed_entries, vec![expected]);
    assert_eq!(
        transition.snapshot,
        houra_core::TrackerSnapshot {
            state: TrackerState::Stopped,
            revision: 2
        }
    );
    assert_eq!(transition.notifications, vec![Notification::TimerStopped]);
}

#[test]
fn switch_records_current_interval_and_starts_selected_work() {
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(42));
    let transition = engine
        .apply(TrackerCommand::Switch {
            project_id: ProjectId(2),
            activity_id: None,
            note: "review".into(),
        })
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));

    assert_eq!(transition.completed_entries.len(), 1);
    assert_eq!(transition.completed_entries[0].start_ms, 1_000);
    assert_eq!(transition.completed_entries[0].end_ms, 43_000);
    let TrackerState::Running(active) = transition.snapshot.state else {
        panic!("switch did not leave the timer running");
    };
    assert_eq!(active.project_id, ProjectId(2));
    assert_eq!(active.note, "review");
    assert_eq!(active.start_ms, 43_000);
    assert_eq!(
        transition.notifications,
        vec![Notification::TimerStopped, Notification::TimerStarted]
    );
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
            activity_id: None,
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

#[test]
fn every_command_state_combination_accepts_or_preserves_snapshot() {
    use houra_core::*;
    let active = ActiveTimer {
        project_id: ProjectId(1),
        activity_id: None,
        note: "design".into(),
        start_ms: 100,
        started_monotonic_ms: 0,
        last_heartbeat_ms: 200,
    };
    let states = [
        TrackerState::Stopped,
        TrackerState::Running(active.clone()),
        TrackerState::IdlePending(PendingIdle {
            active: active.clone(),
            idle_start_ms: 150,
            return_ms: Some(200),
        }),
        TrackerState::RecoveryPending(PendingRecovery {
            active,
            proposed_end_ms: 200,
            unresolved_idle_start_ms: None,
        }),
    ];
    let commands = [
        TrackerCommand::Start {
            project_id: ProjectId(1),
            activity_id: None,
            note: "design".into(),
        },
        TrackerCommand::Stop,
        TrackerCommand::EditActive {
            project_id: ProjectId(2),
            activity_id: Some(ActivityId(3)),
            note: "edited".into(),
        },
        TrackerCommand::Heartbeat,
        TrackerCommand::IdleDetected { idle_start_ms: 150 },
        TrackerCommand::UserReturned { return_ms: 200 },
        TrackerCommand::ResolveIdle(IdleDecision::Keep),
        TrackerCommand::ResolveRecovery {
            end_ms: 200,
            resume: false,
        },
        TrackerCommand::DiscardRecovery,
    ];
    let accepted: [&[usize]; 4] = [&[0], &[1, 2, 3, 4], &[3, 5, 6], &[7, 8]];
    for (i, state) in states.into_iter().enumerate() {
        for (j, command) in commands.iter().enumerate() {
            let before = TrackerSnapshot {
                state: state.clone(),
                revision: 10,
            };
            let mut engine = TrackerEngine::restore(ManualClock::at(300), before.clone(), false);
            let result = engine.apply(command.clone());
            if accepted[i].contains(&j) {
                let transition = result.unwrap_or_else(|e| panic!("{i}/{j}: {e}"));
                assert_eq!(transition.snapshot, *engine.snapshot());
                assert_eq!(transition.snapshot.revision, 11);
                assert_eq!(
                    transition.completed_entries.len(),
                    usize::from(j == 1 || j == 7)
                );
                let notifications = match j {
                    0 => vec![Notification::TimerStarted],
                    1 => vec![Notification::TimerStopped],
                    5 => vec![Notification::IdleNeedsResolution],
                    7 | 8 => vec![Notification::RecoveryResolved],
                    _ => vec![],
                };
                assert_eq!(transition.notifications, notifications);
                if j == 2 {
                    let mut expected = before
                        .state
                        .active()
                        .cloned()
                        .unwrap_or_else(|| panic!("active"));
                    expected.project_id = ProjectId(2);
                    expected.activity_id = Some(ActivityId(3));
                    expected.note = "edited".into();
                    expected.last_heartbeat_ms = 300;
                    assert_eq!(transition.snapshot.state, TrackerState::Running(expected));
                }
                if j == 3 {
                    assert_eq!(
                        transition
                            .snapshot
                            .state
                            .active()
                            .map(|a| a.last_heartbeat_ms),
                        Some(300)
                    );
                }
            } else {
                assert_eq!(result, Err(DomainError::InvalidState(state.clone())));
                assert_eq!(engine.snapshot(), &before);
            }
        }
    }
}

#[test]
fn invalid_times_and_unanswered_idle_leave_snapshot_unchanged() {
    use houra_core::DomainError;
    let clock = ManualClock::at(100);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_millis(100));
    for idle_start_ms in [99, 201] {
        let before = engine.snapshot().clone();
        assert_eq!(
            engine.apply(TrackerCommand::IdleDetected { idle_start_ms }),
            Err(DomainError::InvalidIdleStart {
                active_start_ms: 100,
                idle_start_ms
            })
        );
        assert_eq!(engine.snapshot(), &before);
    }
    engine
        .apply(TrackerCommand::IdleDetected { idle_start_ms: 150 })
        .unwrap_or_else(|e| panic!("{e}"));
    let before = engine.snapshot().clone();
    assert_eq!(
        engine.apply(TrackerCommand::UserReturned { return_ms: 149 }),
        Err(DomainError::InvalidReturn {
            idle_start_ms: 150,
            return_ms: 149
        })
    );
    assert_eq!(
        engine.apply(TrackerCommand::ResolveIdle(IdleDecision::Keep)),
        Err(DomainError::InvalidState(before.state.clone()))
    );
    assert_eq!(engine.snapshot(), &before);
}

#[test]
fn idle_decisions_produce_exact_records() {
    use houra_core::{EntrySource, TimeEntry};
    for decision in [
        IdleDecision::Keep,
        IdleDecision::Stop,
        IdleDecision::DiscardAndResume,
        IdleDecision::ReassignAndResume {
            project_id: ProjectId(2),
            activity_id: None,
            note: "away".into(),
        },
    ] {
        let clock = ManualClock::at(100);
        let mut engine = TrackerEngine::new(clock.clone());
        start(&mut engine);
        let original = engine
            .snapshot()
            .state
            .active()
            .cloned()
            .unwrap_or_else(|| panic!("active"));
        clock.advance(Duration::from_millis(100));
        engine
            .apply(TrackerCommand::IdleDetected { idle_start_ms: 150 })
            .unwrap_or_else(|e| panic!("{e}"));
        engine
            .apply(TrackerCommand::UserReturned { return_ms: 200 })
            .unwrap_or_else(|e| panic!("{e}"));
        let result = engine
            .apply(TrackerCommand::ResolveIdle(decision.clone()))
            .unwrap_or_else(|e| panic!("{e}"));
        let mut expected = vec![];
        if decision != IdleDecision::Keep {
            expected.push(TimeEntry {
                id: None,
                project_id: ProjectId(1),
                activity_id: None,
                note: "design".into(),
                start_ms: 100,
                end_ms: 150,
                source: EntrySource::Timer,
                created_at_ms: 150,
                updated_at_ms: 150,
            });
        }
        if matches!(decision, IdleDecision::ReassignAndResume { .. }) {
            expected.push(TimeEntry {
                id: None,
                project_id: ProjectId(2),
                activity_id: None,
                note: "away".into(),
                start_ms: 150,
                end_ms: 200,
                source: EntrySource::IdleReassignment,
                created_at_ms: 200,
                updated_at_ms: 200,
            });
        }
        assert_eq!(result.completed_entries, expected);
        let expected_state = match decision {
            IdleDecision::Stop => TrackerState::Stopped,
            IdleDecision::Keep => TrackerState::Running(original),
            _ => TrackerState::Running(houra_core::ActiveTimer {
                start_ms: 200,
                last_heartbeat_ms: 200,
                started_monotonic_ms: 100,
                ..original
            }),
        };
        assert_eq!(result.snapshot.state, expected_state);
        assert_eq!(
            result.notifications,
            if decision == IdleDecision::Stop {
                vec![Notification::TimerStopped]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn restore_recovery_boundaries_resume_discard_and_revision_saturation() {
    use houra_core::*;
    let clock = ManualClock::at(100);
    let mut engine = TrackerEngine::new(clock.clone());
    assert_eq!(engine.live_elapsed(), Duration::ZERO);
    start(&mut engine);
    clock.advance(Duration::from_millis(100));
    clock.set_wall_time_ms(50);
    assert_eq!(engine.live_elapsed(), Duration::from_millis(100));
    engine
        .apply(TrackerCommand::Heartbeat)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        engine
            .snapshot()
            .state
            .active()
            .map(|a| a.last_heartbeat_ms),
        Some(100)
    );
    clock.set_wall_time_ms(200);
    engine
        .apply(TrackerCommand::Heartbeat)
        .unwrap_or_else(|e| panic!("{e}"));
    engine
        .apply(TrackerCommand::IdleDetected { idle_start_ms: 150 })
        .unwrap_or_else(|e| panic!("{e}"));
    let snapshot = engine.snapshot().clone();
    assert_eq!(
        TrackerEngine::restore(clock.clone(), snapshot.clone(), false).snapshot(),
        &snapshot
    );
    let mut recovered = TrackerEngine::restore(clock.clone(), snapshot, true);
    let before = recovered.snapshot().clone();
    let active = before
        .state
        .active()
        .cloned()
        .unwrap_or_else(|| panic!("active"));
    assert_eq!(
        before.state,
        TrackerState::RecoveryPending(PendingRecovery {
            active: active.clone(),
            proposed_end_ms: 200,
            unresolved_idle_start_ms: Some(150)
        })
    );
    for end_ms in [99, 201] {
        assert_eq!(
            recovered.apply(TrackerCommand::ResolveRecovery {
                end_ms,
                resume: true
            }),
            Err(DomainError::InvalidRecoveryEnd {
                start_ms: 100,
                last_heartbeat_ms: 200,
                end_ms
            })
        );
        assert_eq!(recovered.snapshot(), &before);
    }
    clock.advance(Duration::from_millis(100));
    let transition = recovered
        .apply(TrackerCommand::ResolveRecovery {
            end_ms: 200,
            resume: true,
        })
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        transition.completed_entries,
        vec![TimeEntry {
            id: None,
            project_id: ProjectId(1),
            activity_id: None,
            note: "design".into(),
            start_ms: 100,
            end_ms: 200,
            source: EntrySource::Recovery,
            created_at_ms: 200,
            updated_at_ms: 200
        }]
    );
    assert_eq!(
        transition.snapshot.state,
        TrackerState::Running(ActiveTimer {
            start_ms: 300,
            last_heartbeat_ms: 300,
            started_monotonic_ms: 200,
            ..active
        })
    );
    let mut discarded = TrackerEngine::restore(
        clock.clone(),
        TrackerSnapshot {
            revision: u64::MAX,
            ..before
        },
        true,
    );
    let result = discarded
        .apply(TrackerCommand::DiscardRecovery)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        result.snapshot,
        TrackerSnapshot {
            state: TrackerState::Stopped,
            revision: u64::MAX
        }
    );
    assert!(result.completed_entries.is_empty());
    assert_eq!(result.notifications, vec![Notification::RecoveryResolved]);
    assert_eq!(
        TrackerEngine::restore(clock, result.snapshot.clone(), true).snapshot(),
        &result.snapshot
    );
}
