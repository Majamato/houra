mod common;
use std::time::Duration;

use houra_core::{
    IdleDecision, ManualClock, Notification, ProjectId, TrackedInterval, TrackerCommand,
    TrackerEngine, TrackerState,
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
    expected.intervals[0].source = houra_core::EntrySource::Timer;
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
    assert_eq!(transition.completed_entries[0].intervals[0].start_ms, 1_000);
    assert_eq!(transition.completed_entries[0].intervals[0].end_ms, 43_000);
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
fn continue_cycles_keep_one_identity_exclude_breaks_and_ignore_duplicates() {
    use houra_core::EntryId;
    let clock = ManualClock::at(0);
    let mut engine = TrackerEngine::new(clock.clone());
    let continue_work = || TrackerCommand::Continue {
        entry_id: EntryId(7),
        project_id: ProjectId(1),
        activity_id: None,
        note: "design".into(),
    };

    engine
        .apply(continue_work())
        .unwrap_or_else(|error| panic!("{error}"));
    let before_duplicate = engine.snapshot().clone();
    let duplicate = engine
        .apply(continue_work())
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(duplicate.snapshot, before_duplicate);
    assert!(duplicate.completed_entries.is_empty());

    clock.advance(Duration::from_secs(16 * 60));
    let first = engine
        .apply(TrackerCommand::Stop)
        .unwrap_or_else(|error| panic!("{error}"));
    clock.advance(Duration::from_secs(5 * 60));
    engine
        .apply(continue_work())
        .unwrap_or_else(|error| panic!("{error}"));
    clock.advance(Duration::from_secs(60));
    let second = engine
        .apply(TrackerCommand::Stop)
        .unwrap_or_else(|error| panic!("{error}"));

    let completed = [
        first.completed_entries[0].clone(),
        second.completed_entries[0].clone(),
    ];
    assert!(completed.iter().all(|entry| entry.id == Some(EntryId(7))));
    assert_eq!(
        completed
            .iter()
            .map(|entry| entry.duration_ms())
            .sum::<i64>(),
        17 * 60 * 1_000
    );
}

#[test]
fn zero_duration_continue_stop_adds_no_interval() {
    let clock = ManualClock::at(100);
    let mut engine = TrackerEngine::new(clock);
    engine
        .apply(TrackerCommand::Continue {
            entry_id: houra_core::EntryId(4),
            project_id: ProjectId(1),
            activity_id: None,
            note: String::new(),
        })
        .unwrap_or_else(|error| panic!("{error}"));
    let stopped = engine
        .apply(TrackerCommand::Stop)
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(stopped.completed_entries.is_empty());
    assert_eq!(stopped.snapshot.state, TrackerState::Stopped);
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
    assert_eq!(
        transition.completed_entries[0].intervals[0].start_ms,
        10_000
    );
    assert_eq!(transition.completed_entries[0].intervals[0].end_ms, 310_000);
    match transition.snapshot.state {
        TrackerState::Running(active) => assert_eq!(active.start_ms, 610_000),
        state => panic!("expected running, got {state:?}"),
    }
}

#[test]
fn repeated_return_keeps_first_return_time_and_emits_no_second_alert() {
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(10));
    let idle = engine
        .apply(TrackerCommand::IdleDetected {
            idle_start_ms: 5_000,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(idle.notifications, vec![Notification::IdleDetected]);
    let first = engine
        .apply(TrackerCommand::UserReturned { return_ms: 11_000 })
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(first.notifications, vec![Notification::IdleNeedsResolution]);
    let repeated = engine
        .apply(TrackerCommand::UserReturned { return_ms: 20_000 })
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(repeated.snapshot, first.snapshot);
    assert!(repeated.notifications.is_empty());
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
            .intervals[0]
            .end_ms,
        35_000
    );
}

#[test]
fn every_command_state_combination_accepts_or_preserves_snapshot() {
    use houra_core::*;
    let active = ActiveTimer {
        entry_id: None,
        project_id: ProjectId(1),
        activity_id: None,
        note: "design".into(),
        start_ms: 100,
        started_monotonic_ms: 0,
        last_heartbeat_ms: 200,
        accumulated_ms: 0,
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
            active: active.clone(),
            proposed_end_ms: 200,
            unresolved_idle_start_ms: None,
        }),
        TrackerState::Paused(PausedTimer {
            active,
            paused_at_ms: 250,
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
        TrackerCommand::Continue {
            entry_id: EntryId(9),
            project_id: ProjectId(2),
            activity_id: None,
            note: "continued".into(),
        },
        TrackerCommand::Pause,
        TrackerCommand::Resume,
    ];
    let accepted: [&[usize]; 5] = [
        &[0, 9],
        &[1, 2, 3, 4, 9, 10],
        &[3, 5, 6],
        &[7, 8],
        &[1, 2, 3, 9, 11],
    ];
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
                let is_noop = (i == 2 && j == 5) || (i == 4 && j == 3);
                assert_eq!(transition.snapshot.revision, 10 + u64::from(!is_noop));
                assert_eq!(
                    transition.completed_entries.len(),
                    usize::from((j == 1 && i != 4) || j == 7 || (i == 1 && (j == 9 || j == 10)))
                );
                let notifications = match j {
                    0 => vec![Notification::TimerStarted],
                    1 => vec![Notification::TimerStopped],
                    4 => vec![Notification::IdleDetected],
                    5 if i == 2 => vec![],
                    5 => vec![Notification::IdleNeedsResolution],
                    7 | 8 => vec![Notification::RecoveryResolved],
                    9 if i == 0 => vec![Notification::TimerStarted],
                    9 if i == 4 => vec![Notification::TimerStarted],
                    9 => vec![Notification::TimerStopped, Notification::TimerStarted],
                    10 => vec![Notification::TimerPaused],
                    11 => vec![Notification::TimerResumed],
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
                    if i == 4 {
                        assert_eq!(
                            transition.snapshot.state,
                            TrackerState::Paused(PausedTimer {
                                active: expected,
                                paused_at_ms: 250,
                            })
                        );
                    } else {
                        expected.last_heartbeat_ms = 300;
                        assert_eq!(transition.snapshot.state, TrackerState::Running(expected));
                    }
                }
                if j == 3 {
                    let expected_heartbeat = if i == 4 { 200 } else { 300 };
                    assert_eq!(
                        transition
                            .snapshot
                            .state
                            .active()
                            .map(|a| a.last_heartbeat_ms),
                        Some(expected_heartbeat)
                    );
                }
            } else {
                assert_eq!(
                    result,
                    Err(DomainError::InvalidState(Box::new(state.clone())))
                );
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
        Err(DomainError::InvalidState(Box::new(before.state.clone())))
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
                intervals: vec![TrackedInterval {
                    id: None,
                    start_ms: 100,
                    end_ms: 150,
                    source: EntrySource::Timer,
                }],
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
                intervals: vec![TrackedInterval {
                    id: None,
                    start_ms: 150,
                    end_ms: 200,
                    source: EntrySource::IdleReassignment,
                }],
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
                accumulated_ms: 50,
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
            intervals: vec![TrackedInterval {
                id: None,
                start_ms: 100,
                end_ms: 200,
                source: EntrySource::Recovery
            }],
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
            accumulated_ms: 100,
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

fn pause(engine: &mut TrackerEngine<ManualClock>) {
    engine
        .apply(TrackerCommand::Pause)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
}

#[test]
fn pause_freezes_elapsed_and_resume_excludes_the_gap() {
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(60));
    let paused = engine
        .apply(TrackerCommand::Pause)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(paused.completed_entries.len(), 1);
    assert_eq!(paused.completed_entries[0].intervals[0].start_ms, 1_000);
    assert_eq!(paused.completed_entries[0].intervals[0].end_ms, 61_000);
    assert_eq!(paused.notifications, vec![Notification::TimerPaused]);
    let houra_core::PausedTimer {
        active,
        paused_at_ms,
    } = match &paused.snapshot.state {
        TrackerState::Paused(paused) => paused.clone(),
        state => panic!("pause did not freeze the timer, got {state:?}"),
    };
    assert_eq!(paused_at_ms, 61_000);
    assert_eq!(active.start_ms, 1_000);
    assert_eq!(engine.live_elapsed(), Duration::from_secs(60));
    clock.advance(Duration::from_secs(300));
    assert_eq!(engine.live_elapsed(), Duration::from_secs(60));
    let resumed = engine
        .apply(TrackerCommand::Resume)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(resumed.notifications, vec![Notification::TimerResumed]);
    let TrackerState::Running(running) = resumed.snapshot.state else {
        panic!("resume did not restart the timer");
    };
    assert_eq!(running.start_ms, 361_000);
    assert_eq!(running.accumulated_ms, 60_000);
    assert_eq!(engine.live_elapsed(), Duration::from_secs(60));
    clock.advance(Duration::from_secs(30));
    assert_eq!(engine.live_elapsed(), Duration::from_secs(90));
    let stopped = engine
        .apply(TrackerCommand::Stop)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(stopped.completed_entries.len(), 1);
    assert_eq!(stopped.completed_entries[0].intervals[0].start_ms, 361_000);
    assert_eq!(stopped.completed_entries[0].intervals[0].end_ms, 391_000);
    assert_eq!(
        paused.completed_entries[0].duration_ms() + stopped.completed_entries[0].duration_ms(),
        90_000
    );
    assert_eq!(engine.live_elapsed(), Duration::ZERO);
}

#[test]
fn pause_banks_the_segment_and_stop_from_paused_saves_nothing_more() {
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(10));
    let paused = engine
        .apply(TrackerCommand::Pause)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(paused.completed_entries.len(), 1);
    assert_eq!(paused.completed_entries[0].intervals[0].start_ms, 1_000);
    assert_eq!(paused.completed_entries[0].intervals[0].end_ms, 11_000);
    clock.advance(Duration::from_secs(100));
    let stopped = engine
        .apply(TrackerCommand::Stop)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(stopped.snapshot.state, TrackerState::Stopped);
    assert_eq!(stopped.notifications, vec![Notification::TimerStopped]);
    assert!(stopped.completed_entries.is_empty());
}

#[test]
fn continue_from_paused_starts_selected_work_without_rebanking() {
    use houra_core::EntryId;
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(10));
    let paused = engine
        .apply(TrackerCommand::Pause)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(paused.completed_entries.len(), 1);
    clock.advance(Duration::from_secs(100));
    let continued = engine
        .apply(TrackerCommand::Continue {
            entry_id: EntryId(7),
            project_id: ProjectId(2),
            activity_id: None,
            note: "review".into(),
        })
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert!(continued.completed_entries.is_empty());
    let TrackerState::Running(running) = continued.snapshot.state else {
        panic!("continue did not leave the timer running");
    };
    assert_eq!(running.entry_id, Some(EntryId(7)));
    assert_eq!(running.start_ms, 111_000);
    assert_eq!(continued.notifications, vec![Notification::TimerStarted]);
}

#[test]
fn heartbeat_and_idle_commands_leave_paused_snapshot_unchanged() {
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(10));
    pause(&mut engine);
    let before = engine.snapshot().clone();
    assert_eq!(engine.live_elapsed(), Duration::from_secs(10));
    clock.advance(Duration::from_secs(60));
    let heartbeat = engine
        .apply(TrackerCommand::Heartbeat)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(heartbeat.snapshot, before);
    assert!(heartbeat.notifications.is_empty());
    assert_eq!(engine.snapshot(), &before);
    assert_eq!(engine.live_elapsed(), Duration::from_secs(10));
    for command in [
        TrackerCommand::IdleDetected {
            idle_start_ms: 5_000,
        },
        TrackerCommand::UserReturned { return_ms: 12_000 },
        TrackerCommand::ResolveIdle(houra_core::IdleDecision::Keep),
    ] {
        assert_eq!(
            engine.apply(command),
            Err(houra_core::DomainError::InvalidState(Box::new(
                before.state.clone()
            )))
        );
        assert_eq!(engine.snapshot(), &before);
    }
}

#[test]
fn pause_and_resume_reject_foreign_states() {
    use houra_core::*;
    let clock = ManualClock::at(300);
    let active = ActiveTimer {
        entry_id: None,
        project_id: ProjectId(1),
        activity_id: None,
        note: "design".into(),
        start_ms: 100,
        started_monotonic_ms: 0,
        last_heartbeat_ms: 200,
        accumulated_ms: 0,
    };
    for state in [
        TrackerState::Stopped,
        TrackerState::Running(active.clone()),
        TrackerState::IdlePending(PendingIdle {
            active: active.clone(),
            idle_start_ms: 150,
            return_ms: Some(200),
        }),
        TrackerState::RecoveryPending(PendingRecovery {
            active: active.clone(),
            proposed_end_ms: 200,
            unresolved_idle_start_ms: None,
        }),
        TrackerState::Paused(PausedTimer {
            active,
            paused_at_ms: 250,
        }),
    ] {
        let mut engine =
            TrackerEngine::restore(clock.clone(), TrackerSnapshot { state, revision: 4 }, false);
        let before = engine.snapshot().clone();
        let pause = engine.apply(TrackerCommand::Pause);
        if matches!(before.state, TrackerState::Running(_)) {
            assert!(pause.is_ok());
        } else {
            assert_eq!(
                pause,
                Err(DomainError::InvalidState(Box::new(before.state.clone())))
            );
            assert_eq!(engine.snapshot(), &before);
        }
        let resume = engine.apply(TrackerCommand::Resume);
        // A running timer was paused just above, so resume succeeds there too.
        if matches!(
            before.state,
            TrackerState::Running(_) | TrackerState::Paused(_)
        ) {
            assert!(resume.is_ok());
        } else {
            assert_eq!(
                resume,
                Err(DomainError::InvalidState(Box::new(before.state.clone())))
            );
            assert_eq!(engine.snapshot(), &before);
        }
    }
}

#[test]
fn paused_timer_survives_unclean_shutdown_without_recovery() {
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(10));
    pause(&mut engine);
    let snapshot = engine.snapshot().clone();
    clock.advance(Duration::from_secs(3_600));
    let mut restored = TrackerEngine::restore(clock.clone(), snapshot.clone(), true);
    assert_eq!(restored.snapshot(), &snapshot);
    assert_eq!(restored.live_elapsed(), Duration::from_secs(10));
    let resumed = restored
        .apply(TrackerCommand::Resume)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    let TrackerState::Running(running) = resumed.snapshot.state else {
        panic!("resume did not restart the restored timer");
    };
    assert_eq!(running.start_ms, 3_611_000);
    assert_eq!(running.accumulated_ms, 10_000);
    assert_eq!(restored.live_elapsed(), Duration::from_secs(10));
}

#[test]
fn repeated_pause_cycles_accumulate_every_segment_and_exclude_every_gap() {
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(60));
    let first = engine
        .apply(TrackerCommand::Pause)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(first.completed_entries[0].intervals[0].start_ms, 1_000);
    assert_eq!(first.completed_entries[0].intervals[0].end_ms, 61_000);
    assert_eq!(engine.live_elapsed(), Duration::from_secs(60));
    clock.advance(Duration::from_secs(300));
    assert_eq!(engine.live_elapsed(), Duration::from_secs(60));
    engine
        .apply(TrackerCommand::Resume)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(engine.live_elapsed(), Duration::from_secs(60));
    clock.advance(Duration::from_secs(30));
    assert_eq!(engine.live_elapsed(), Duration::from_secs(90));
    let second = engine
        .apply(TrackerCommand::Pause)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(second.completed_entries[0].intervals[0].start_ms, 361_000);
    assert_eq!(second.completed_entries[0].intervals[0].end_ms, 391_000);
    assert_eq!(engine.live_elapsed(), Duration::from_secs(90));
    clock.advance(Duration::from_secs(300));
    engine
        .apply(TrackerCommand::Resume)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    clock.advance(Duration::from_secs(10));
    assert_eq!(engine.live_elapsed(), Duration::from_secs(100));
    let stopped = engine
        .apply(TrackerCommand::Stop)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(stopped.completed_entries[0].intervals[0].start_ms, 691_000);
    assert_eq!(stopped.completed_entries[0].intervals[0].end_ms, 701_000);
    let recorded: i64 = [&first, &second, &stopped]
        .iter()
        .map(|transition| transition.completed_entries[0].duration_ms())
        .sum();
    assert_eq!(recorded, 100_000);
}

#[test]
fn zero_duration_pause_banks_nothing_and_resume_starts_from_zero() {
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    let paused = engine
        .apply(TrackerCommand::Pause)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert!(paused.completed_entries.is_empty());
    assert_eq!(engine.live_elapsed(), Duration::ZERO);
    clock.advance(Duration::from_secs(100));
    engine
        .apply(TrackerCommand::Resume)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(engine.live_elapsed(), Duration::ZERO);
    clock.advance(Duration::from_secs(10));
    assert_eq!(engine.live_elapsed(), Duration::from_secs(10));
    let stopped = engine
        .apply(TrackerCommand::Stop)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(stopped.completed_entries.len(), 1);
    assert_eq!(stopped.completed_entries[0].intervals[0].start_ms, 101_000);
    assert_eq!(stopped.completed_entries[0].intervals[0].end_ms, 111_000);
}

#[test]
fn pause_keeps_entry_identity_across_resume_and_second_pause() {
    use houra_core::EntryId;
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    engine
        .apply(TrackerCommand::Continue {
            entry_id: EntryId(7),
            project_id: ProjectId(1),
            activity_id: None,
            note: "design".into(),
        })
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    clock.advance(Duration::from_secs(60));
    let first = engine
        .apply(TrackerCommand::Pause)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    clock.advance(Duration::from_secs(300));
    engine
        .apply(TrackerCommand::Resume)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    clock.advance(Duration::from_secs(30));
    let second = engine
        .apply(TrackerCommand::Pause)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    clock.advance(Duration::from_secs(300));
    engine
        .apply(TrackerCommand::Resume)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    clock.advance(Duration::from_secs(10));
    let stopped = engine
        .apply(TrackerCommand::Stop)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    for transition in [&first, &second, &stopped] {
        assert_eq!(transition.completed_entries.len(), 1);
        assert_eq!(transition.completed_entries[0].id, Some(EntryId(7)));
    }
    assert_eq!(engine.live_elapsed(), Duration::ZERO);
}

#[test]
fn idle_discard_after_pause_keeps_banked_total_and_drops_only_the_idle_gap() {
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(60));
    let paused = engine
        .apply(TrackerCommand::Pause)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    clock.advance(Duration::from_secs(300));
    engine
        .apply(TrackerCommand::Resume)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    clock.advance(Duration::from_secs(30));
    engine
        .apply(TrackerCommand::IdleDetected {
            idle_start_ms: 381_000,
        })
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    engine
        .apply(TrackerCommand::UserReturned { return_ms: 391_000 })
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    let resumed = engine
        .apply(TrackerCommand::ResolveIdle(
            houra_core::IdleDecision::DiscardAndResume,
        ))
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(resumed.completed_entries.len(), 1);
    assert_eq!(resumed.completed_entries[0].intervals[0].start_ms, 361_000);
    assert_eq!(resumed.completed_entries[0].intervals[0].end_ms, 381_000);
    let TrackerState::Running(running) = resumed.snapshot.state else {
        panic!("discard did not resume the timer");
    };
    assert_eq!(running.start_ms, 391_000);
    assert_eq!(running.accumulated_ms, 80_000);
    assert_eq!(engine.live_elapsed(), Duration::from_secs(80));
    assert_eq!(
        paused.completed_entries[0].duration_ms() + resumed.completed_entries[0].duration_ms(),
        80_000
    );
}

#[test]
fn idle_keep_after_pause_preserves_the_running_total() {
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(60));
    engine
        .apply(TrackerCommand::Pause)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    clock.advance(Duration::from_secs(300));
    engine
        .apply(TrackerCommand::Resume)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    clock.advance(Duration::from_secs(30));
    engine
        .apply(TrackerCommand::IdleDetected {
            idle_start_ms: 370_000,
        })
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(engine.live_elapsed(), Duration::from_secs(90));
    engine
        .apply(TrackerCommand::UserReturned { return_ms: 391_000 })
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    let kept = engine
        .apply(TrackerCommand::ResolveIdle(houra_core::IdleDecision::Keep))
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert!(kept.completed_entries.is_empty());
    assert_eq!(engine.live_elapsed(), Duration::from_secs(90));
    clock.advance(Duration::from_secs(10));
    assert_eq!(engine.live_elapsed(), Duration::from_secs(100));
}

#[test]
fn recovery_resume_after_pause_continues_the_banked_total() {
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(60));
    engine
        .apply(TrackerCommand::Pause)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    clock.advance(Duration::from_secs(300));
    engine
        .apply(TrackerCommand::Resume)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    clock.advance(Duration::from_secs(30));
    engine
        .apply(TrackerCommand::Heartbeat)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    let snapshot = engine.snapshot().clone();
    clock.advance(Duration::from_secs(3_600));
    let mut recovered = TrackerEngine::restore(clock.clone(), snapshot, true);
    let recovered_transition = recovered
        .apply(TrackerCommand::ResolveRecovery {
            end_ms: 391_000,
            resume: true,
        })
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(recovered_transition.completed_entries.len(), 1);
    assert_eq!(
        recovered_transition.completed_entries[0].intervals[0].start_ms,
        361_000
    );
    assert_eq!(
        recovered_transition.completed_entries[0].intervals[0].end_ms,
        391_000
    );
    let TrackerState::Running(running) = recovered_transition.snapshot.state else {
        panic!("recovery did not resume the timer");
    };
    assert_eq!(running.accumulated_ms, 90_000);
    assert_eq!(recovered.live_elapsed(), Duration::from_secs(90));
}

#[test]
fn switch_and_continue_reset_the_run_total() {
    use houra_core::EntryId;
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(60));
    engine
        .apply(TrackerCommand::Pause)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    let switched = engine
        .apply(TrackerCommand::Switch {
            project_id: ProjectId(2),
            activity_id: None,
            note: "review".into(),
        })
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    let TrackerState::Running(running) = switched.snapshot.state else {
        panic!("switch did not start a new timer");
    };
    assert_eq!(running.accumulated_ms, 0);
    assert_eq!(engine.live_elapsed(), Duration::ZERO);
    clock.advance(Duration::from_secs(10));
    let paused = engine
        .apply(TrackerCommand::Pause)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(paused.completed_entries[0].duration_ms(), 10_000);
    assert_eq!(engine.live_elapsed(), Duration::from_secs(10));
    let continued = engine
        .apply(TrackerCommand::Continue {
            entry_id: EntryId(9),
            project_id: ProjectId(2),
            activity_id: None,
            note: "continued".into(),
        })
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    let TrackerState::Running(running) = continued.snapshot.state else {
        panic!("continue did not start a new timer");
    };
    assert_eq!(running.accumulated_ms, 0);
    assert_eq!(engine.live_elapsed(), Duration::ZERO);
}

#[test]
fn wall_reversal_around_pause_never_banks_negative_time() {
    let clock = ManualClock::at(20_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(10));
    let paused = engine
        .apply(TrackerCommand::Pause)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(paused.completed_entries[0].duration_ms(), 10_000);
    assert_eq!(engine.live_elapsed(), Duration::from_secs(10));
    clock.set_wall_time_ms(5_000);
    assert_eq!(engine.live_elapsed(), Duration::from_secs(10));
    engine
        .apply(TrackerCommand::Resume)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(engine.live_elapsed(), Duration::from_secs(10));
    clock.advance(Duration::from_secs(5));
    assert_eq!(engine.live_elapsed(), Duration::from_secs(15));
    let stopped = engine
        .apply(TrackerCommand::Stop)
        .unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(stopped.completed_entries[0].duration_ms(), 5_000);
    assert_eq!(
        paused.completed_entries[0].duration_ms() + stopped.completed_entries[0].duration_ms(),
        15_000
    );
}

#[test]
fn adopt_paused_entry_id_only_fills_a_missing_paused_identity() {
    use houra_core::*;
    let clock = ManualClock::at(300);
    let active = ActiveTimer {
        entry_id: None,
        project_id: ProjectId(1),
        activity_id: None,
        note: "design".into(),
        start_ms: 100,
        started_monotonic_ms: 0,
        last_heartbeat_ms: 100,
        accumulated_ms: 0,
    };
    let mut engine = TrackerEngine::restore(
        clock.clone(),
        TrackerSnapshot {
            state: TrackerState::Paused(PausedTimer {
                active: active.clone(),
                paused_at_ms: 200,
            }),
            revision: 4,
        },
        false,
    );
    engine.adopt_paused_entry_id(EntryId(9));
    let TrackerState::Paused(paused) = &engine.snapshot().state else {
        panic!("adopt changed the tracker state");
    };
    assert_eq!(paused.active.entry_id, Some(EntryId(9)));
    assert_eq!(engine.snapshot().revision, 4);
    engine.adopt_paused_entry_id(EntryId(10));
    let TrackerState::Paused(paused) = &engine.snapshot().state else {
        panic!("adopt changed the tracker state");
    };
    assert_eq!(paused.active.entry_id, Some(EntryId(9)));
    for state in [
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
    ] {
        let before = TrackerSnapshot { state, revision: 4 };
        let mut engine = TrackerEngine::restore(clock.clone(), before.clone(), false);
        engine.adopt_paused_entry_id(EntryId(9));
        assert_eq!(engine.snapshot(), &before);
    }
}
