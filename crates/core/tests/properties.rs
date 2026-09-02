mod tests {
    use work_time_core::{
        DomainError, EntrySource, ProjectId, TimeEntry, validate_color, validate_name,
    };

    #[test]
    fn blank_names_are_rejected() {
        assert_eq!(validate_name("   "), Err(DomainError::EmptyName));
        assert!(validate_name("Design").is_ok());
    }

    #[test]
    fn colors_must_be_seven_char_hex() {
        assert!(validate_color("#3584e4").is_ok());
        assert_eq!(validate_color("3584e4"), Err(DomainError::InvalidColor));
        assert_eq!(validate_color("#35g4e4"), Err(DomainError::InvalidColor));
    }

    #[test]
    fn duration_never_goes_negative() {
        let entry = TimeEntry {
            id: None,
            project_id: ProjectId(1),
            task_id: None,
            note: String::new(),
            start_ms: 200,
            end_ms: 100,
            source: EntrySource::Manual,
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        assert_eq!(entry.duration_ms(), 0);
        assert!(entry.validate().is_err());
    }
}

use std::time::Duration;

use proptest::prelude::*;
use work_time_core::{
    IdleDecision, ManualClock, ProjectId, TrackerCommand, TrackerEngine, TrackerState,
};

proptest! {
    #[test]
    fn idle_reassignment_reconciles_the_original_timeline(
        focused_ms in 1_i64..3_600_000,
        idle_ms in 1_i64..3_600_000,
        answer_delay_ms in 0_i64..600_000,
    ) {
        let start_ms = 1_000_000_i64;
        let idle_start_ms = start_ms + focused_ms;
        let return_ms = idle_start_ms + idle_ms;
        let clock = ManualClock::at(start_ms);
        let mut engine = TrackerEngine::new(clock.clone());
        let started = engine.apply(TrackerCommand::Start {
            project_id: ProjectId(1),
            task_id: None,
            note: "focus".into(),
        });
        prop_assert!(started.is_ok());
        clock.advance(Duration::from_millis(
            u64::try_from(focused_ms + idle_ms).unwrap_or(u64::MAX),
        ));
        let idle = engine.apply(TrackerCommand::IdleDetected { idle_start_ms });
        prop_assert!(idle.is_ok());
        let returned = engine.apply(TrackerCommand::UserReturned { return_ms });
        prop_assert!(returned.is_ok());
        clock.advance(Duration::from_millis(
            u64::try_from(answer_delay_ms).unwrap_or(u64::MAX),
        ));
        let transition = engine.apply(TrackerCommand::ResolveIdle(
            IdleDecision::ReassignAndResume {
                project_id: ProjectId(2),
                task_id: None,
                note: "away".into(),
            },
        ));
        prop_assert!(transition.is_ok());
        let transition = transition.unwrap_or_else(|error| panic!("unexpected error: {error}"));
        let recorded: i64 = transition.completed_entries.iter().map(|entry| entry.duration_ms()).sum();
        prop_assert_eq!(recorded, focused_ms + idle_ms);
        prop_assert!(transition.completed_entries.iter().all(|entry| entry.duration_ms() > 0));
        match transition.snapshot.state {
            TrackerState::Running(active) => prop_assert_eq!(active.start_ms, return_ms),
            state => prop_assert!(false, "expected running, got {state:?}"),
        }
    }

    #[test]
    fn discard_idle_records_only_focused_time(
        focused_ms in 1_i64..3_600_000,
        idle_ms in 1_i64..3_600_000,
    ) {
        let start_ms = 10_000_i64;
        let idle_start_ms = start_ms + focused_ms;
        let return_ms = idle_start_ms + idle_ms;
        let clock = ManualClock::at(start_ms);
        let mut engine = TrackerEngine::new(clock.clone());
        let started = engine.apply(TrackerCommand::Start {
            project_id: ProjectId(1), task_id: None, note: String::new(),
        });
        prop_assert!(started.is_ok());
        clock.advance(Duration::from_millis(u64::try_from(focused_ms + idle_ms).unwrap_or(u64::MAX)));
        let idle = engine.apply(TrackerCommand::IdleDetected { idle_start_ms });
        prop_assert!(idle.is_ok());
        let returned = engine.apply(TrackerCommand::UserReturned { return_ms });
        prop_assert!(returned.is_ok());
        let transition = engine.apply(TrackerCommand::ResolveIdle(IdleDecision::DiscardAndResume));
        prop_assert!(transition.is_ok());
        let recorded = transition
            .unwrap_or_else(|error| panic!("unexpected error: {error}"))
            .completed_entries
            .iter()
            .map(|entry| entry.duration_ms())
            .sum::<i64>();
        prop_assert_eq!(recorded, focused_ms);
    }
}
