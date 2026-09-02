use std::time::Duration;

use work_time_core::{ManualClock, Notification, ProjectId, TrackerCommand, TrackerEngine};

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
