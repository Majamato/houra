//! What the top bar shows, derived from the tracker state.

use std::time::Duration;

use houra_core::{Activity, PendingIdle, Project, TrackerState};

use crate::desktop::widgets::{active_entry_total_duration, counted_live_elapsed};

/// The tracker state as Houra's top-bar extension understands it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum TopBarState {
    /// No active timer.
    #[default]
    Stopped,
    /// The active timer is counting.
    Running,
    /// The active timer is paused.
    Paused,
    /// Idle time awaits the user's review.
    Idle,
    /// An interrupted timer awaits recovery.
    Recovery,
}

impl TopBarState {
    pub(super) fn of(state: &TrackerState) -> Self {
        match state {
            TrackerState::Stopped => Self::Stopped,
            TrackerState::Running(_) => Self::Running,
            TrackerState::Paused(_) => Self::Paused,
            TrackerState::IdlePending(_) => Self::Idle,
            TrackerState::RecoveryPending(_) => Self::Recovery,
        }
    }

    /// Name sent over D-Bus; the extension matches these exact strings.
    pub(super) fn wire_name(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Idle => "idle",
            Self::Recovery => "recovery",
        }
    }
}

/// Values published through the `ActiveTimer` D-Bus properties.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct TopBarStatus {
    pub(super) state: TopBarState,
    /// The active time entry's total in milliseconds when published, matching
    /// the main screen; zero when the top bar shows no time.
    pub(super) elapsed_ms: u64,
    /// "note · project · activity" for accessible names; empty when stopped.
    pub(super) summary: String,
}

impl TopBarStatus {
    /// `saved_ms` is the active time entry's saved duration, which includes
    /// segments already banked by pause. `untitled` names work without a
    /// note, matching the tracker page.
    pub(super) fn new(
        state: &TrackerState,
        saved_ms: i64,
        live_elapsed: Duration,
        projects: &[Project],
        activities: &[Activity],
        untitled: &str,
    ) -> Self {
        let top_bar_state = TopBarState::of(state);
        let total = match state {
            TrackerState::Running(active)
            | TrackerState::IdlePending(PendingIdle { active, .. }) => active_entry_total_duration(
                saved_ms,
                counted_live_elapsed(live_elapsed, active.accumulated_ms),
            ),
            // Pause banked the frozen segment, so the saved duration is the total.
            TrackerState::Paused(_) => active_entry_total_duration(saved_ms, Duration::ZERO),
            // A recovered timer's monotonic start belongs to a previous process.
            TrackerState::Stopped | TrackerState::RecoveryPending(_) => Duration::ZERO,
        };
        let elapsed_ms = u64::try_from(total.as_millis()).unwrap_or(u64::MAX);
        let summary = state.active().map_or_else(String::new, |active| {
            let note = active.note.trim();
            let work = if note.is_empty() { untitled } else { note };
            let project = projects
                .iter()
                .find(|project| project.id == active.project_id)
                .map(|project| project.name.as_str());
            let activity = active
                .activity_id
                .and_then(|id| activities.iter().find(|activity| activity.id == id))
                .map(|activity| activity.name.as_str());
            std::iter::once(work)
                .chain(project)
                .chain(activity)
                .collect::<Vec<_>>()
                .join(" · ")
        });
        Self {
            state: top_bar_state,
            elapsed_ms,
            summary,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use houra_core::{
        ActiveTimer, Activity, ActivityId, EntryId, PausedTimer, PendingIdle, PendingRecovery,
        Project, ProjectId, TrackerState,
    };

    use super::{TopBarState, TopBarStatus};

    const HOUR_MS: i64 = 3_600 * 1_000;

    fn active(note: &str, activity_id: Option<ActivityId>) -> ActiveTimer {
        ActiveTimer {
            entry_id: None,
            project_id: ProjectId(1),
            activity_id,
            note: note.into(),
            start_ms: 1,
            started_monotonic_ms: 0,
            last_heartbeat_ms: 2,
            accumulated_ms: 0,
        }
    }

    fn projects() -> Vec<Project> {
        vec![Project {
            id: ProjectId(1),
            name: "General".into(),
            color: "#3584e4".into(),
            archived: false,
            created_at_ms: 0,
            updated_at_ms: 0,
        }]
    }

    fn activities() -> Vec<Activity> {
        vec![Activity {
            id: ActivityId(7),
            name: "Programming".into(),
            archived: false,
            created_at_ms: 0,
            updated_at_ms: 0,
        }]
    }

    fn status(state: &TrackerState) -> TopBarStatus {
        status_with(state, 0, Duration::from_millis(90_500))
    }

    fn status_with(state: &TrackerState, saved_ms: i64, live_elapsed: Duration) -> TopBarStatus {
        TopBarStatus::new(
            state,
            saved_ms,
            live_elapsed,
            &projects(),
            &activities(),
            "Tracked work",
        )
    }

    #[test]
    fn every_tracker_state_has_a_wire_name() {
        let timer = active("Write report", None);
        for (state, expected, wire) in [
            (TrackerState::Stopped, TopBarState::Stopped, "stopped"),
            (
                TrackerState::Running(timer.clone()),
                TopBarState::Running,
                "running",
            ),
            (
                TrackerState::Paused(PausedTimer {
                    active: timer.clone(),
                    paused_at_ms: 2,
                }),
                TopBarState::Paused,
                "paused",
            ),
            (
                TrackerState::IdlePending(PendingIdle {
                    active: timer.clone(),
                    idle_start_ms: 1,
                    return_ms: None,
                }),
                TopBarState::Idle,
                "idle",
            ),
            (
                TrackerState::RecoveryPending(PendingRecovery {
                    active: timer.clone(),
                    proposed_end_ms: 2,
                    unresolved_idle_start_ms: None,
                }),
                TopBarState::Recovery,
                "recovery",
            ),
        ] {
            assert_eq!(TopBarState::of(&state), expected);
            assert_eq!(expected.wire_name(), wire);
        }
    }

    #[test]
    fn stopped_publishes_no_time_or_summary() {
        assert_eq!(status(&TrackerState::Stopped), TopBarStatus::default());
    }

    #[test]
    fn running_joins_note_project_and_activity() {
        let running = TrackerState::Running(active("  Write report ", Some(ActivityId(7))));
        assert_eq!(
            status(&running),
            TopBarStatus {
                state: TopBarState::Running,
                elapsed_ms: 90_500,
                summary: "Write report · General · Programming".into(),
            }
        );
    }

    #[test]
    fn blank_notes_use_the_untitled_label() {
        let running = TrackerState::Running(active(" \t", None));
        assert_eq!(status(&running).summary, "Tracked work · General");
    }

    #[test]
    fn unknown_projects_and_activities_are_left_out() {
        let mut timer = active("Write report", Some(ActivityId(99)));
        timer.project_id = ProjectId(42);
        assert_eq!(
            status(&TrackerState::Running(timer)).summary,
            "Write report"
        );
    }

    #[test]
    fn a_new_timer_publishes_its_live_time() {
        let running = TrackerState::Running(active("Write report", None));
        assert_eq!(
            status_with(&running, 0, Duration::from_secs(5 * 60)).elapsed_ms,
            5 * 60 * 1_000
        );
    }

    #[test]
    fn a_continued_time_entry_publishes_its_total() {
        let mut timer = active("Write report", None);
        timer.entry_id = Some(EntryId(3));
        let running = TrackerState::Running(timer);
        assert_eq!(
            status_with(&running, 2 * HOUR_MS, Duration::from_secs(5 * 60)).elapsed_ms,
            u64::try_from(2 * HOUR_MS + 5 * 60 * 1_000).unwrap_or_default()
        );
    }

    #[test]
    fn a_banked_segment_is_not_counted_twice() {
        // 10m saved before this run, then 5m run and banked by an earlier pause.
        let mut timer = active("Write report", None);
        timer.entry_id = Some(EntryId(3));
        timer.accumulated_ms = 5 * 60 * 1_000;
        let saved_ms = 15 * 60 * 1_000;
        let running = TrackerState::Running(timer.clone());
        assert_eq!(
            status_with(&running, saved_ms, Duration::from_secs(7 * 60)).elapsed_ms,
            17 * 60 * 1_000
        );
        let paused = TrackerState::Paused(PausedTimer {
            active: timer,
            paused_at_ms: 2,
        });
        assert_eq!(
            status_with(&paused, saved_ms, Duration::from_secs(5 * 60)).elapsed_ms,
            15 * 60 * 1_000
        );
    }

    #[test]
    fn paused_publishes_the_saved_total_and_idle_keeps_counting() {
        let timer = active("Write report", None);
        let paused = TrackerState::Paused(PausedTimer {
            active: timer.clone(),
            paused_at_ms: 2,
        });
        let idle = TrackerState::IdlePending(PendingIdle {
            active: timer.clone(),
            idle_start_ms: 1,
            return_ms: None,
        });
        let recovery = TrackerState::RecoveryPending(PendingRecovery {
            active: timer,
            proposed_end_ms: 2,
            unresolved_idle_start_ms: None,
        });
        assert_eq!(
            status_with(&paused, 60_000, Duration::from_millis(90_500)).elapsed_ms,
            60_000
        );
        assert_eq!(
            status_with(&idle, 60_000, Duration::from_millis(90_500)).elapsed_ms,
            150_500
        );
        let recovered = status_with(&recovery, 60_000, Duration::from_millis(90_500));
        assert_eq!(recovered.elapsed_ms, 0);
        assert_eq!(recovered.summary, "Write report · General");
    }
}
