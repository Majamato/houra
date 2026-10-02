use crate::desktop::{load_settings, log_background_error, platform, widgets::format_duration};
use crate::locale::{tr, trf};
use chrono::{Local, TimeZone};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::{
    ActiveTimer, Activity, PendingIdle, Project, ProjectId, TrackerCommand, TrackerState,
};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::window::MainWindow;

impl MainWindow {
    pub(in crate::desktop) fn review_idle(&self) {
        let Some(handle) = self.handle() else { return };
        let snapshot = match handle.snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.show_database_error(&error.to_string());
                return;
            }
        };
        let TrackerState::IdlePending(pending) = snapshot.state else {
            return;
        };
        if pending.return_ms.is_none() {
            let notifications =
                load_settings().is_none_or(|settings| settings.boolean("notifications"));
            if let Err(error) = platform::apply_return_and_notify(
                &handle,
                chrono::Utc::now().timestamp_millis(),
                notifications,
            ) {
                self.show_database_error(&error.to_string());
                return;
            }
        }
        self.show_idle_dialog();
    }

    pub(in crate::desktop) fn show_idle_dialog(&self) {
        if self.imp().idle_dialog_open.get() {
            return;
        }
        let Some(handle) = self.handle() else { return };
        let Ok(snapshot) = handle.snapshot() else {
            return;
        };
        let TrackerState::IdlePending(pending) = snapshot.state else {
            return;
        };
        if pending.return_ms.is_none() {
            return;
        }
        self.imp().idle_dialog_open.set(true);
        let projects = self.imp().projects.borrow();
        let activities = self.imp().activities.borrow();
        let entry = idle_entry_label(&projects, &activities, &pending.active);
        let duration = idle_duration_text(&pending);
        let dialog = adw::AlertDialog::builder()
            .heading(tr("You were away"))
            .body(idle_review_body(&pending, &entry))
            .build();
        let discard_label = trf("Discard {duration}", &[("duration", &duration)]);
        let keep_label = trf("Keep {duration} on this entry", &[("duration", &duration)]);
        let move_label = trf(
            "Move {duration} to another project…",
            &[("duration", &duration)],
        );
        let stop_label = trf("Discard {duration} and stop", &[("duration", &duration)]);
        // Responses render bottom-up, so add them in reverse display order:
        // recommended choice first, timer-stopping choice last.
        dialog.add_responses(&[
            ("stop", stop_label.as_str()),
            ("reassign", move_label.as_str()),
            ("keep", keep_label.as_str()),
            ("discard", discard_label.as_str()),
        ]);
        dialog.set_response_appearance("discard", adw::ResponseAppearance::Suggested);
        dialog.set_response_appearance("stop", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("discard"));
        let weak = self.downgrade();
        dialog.connect_response(None, move |_, response| {
            if response == "reassign" {
                if let Some(window) = weak.upgrade() {
                    window.show_idle_reassign();
                }
                return;
            }
            let decision = match response {
                "keep" => houra_core::IdleDecision::Keep,
                "stop" => houra_core::IdleDecision::Stop,
                "discard" => houra_core::IdleDecision::DiscardAndResume,
                _ => {
                    if let Some(window) = weak.upgrade() {
                        window.imp().idle_dialog_open.set(false);
                    }
                    return;
                }
            };
            let stopped = matches!(decision, houra_core::IdleDecision::Stop);
            let result = handle.apply(TrackerCommand::ResolveIdle(decision));
            if let Err(error) = &result {
                log_background_error("resolving idle time", error);
            }
            if let Some(window) = weak.upgrade() {
                window.imp().idle_dialog_open.set(false);
                match result {
                    Ok(_) => {
                        platform::withdraw_idle_notification();
                        if stopped {
                            window.imp().note_entry.set_text("");
                        }
                        window.refresh();
                    }
                    Err(error) => window.show_database_error(&error.to_string()),
                }
            }
        });
        dialog.present(Some(self));
    }

    fn show_idle_reassign(&self) {
        let Some(handle) = self.handle() else {
            self.imp().idle_dialog_open.set(false);
            return;
        };
        let Ok(snapshot) = handle.snapshot() else {
            self.imp().idle_dialog_open.set(false);
            return;
        };
        let TrackerState::IdlePending(pending) = snapshot.state else {
            self.imp().idle_dialog_open.set(false);
            return;
        };
        if pending.return_ms.is_none() {
            self.imp().idle_dialog_open.set(false);
            return;
        }
        let projects = self.imp().projects.borrow().clone();
        let names = projects
            .iter()
            .map(|project| project.name.as_str())
            .collect::<Vec<_>>();
        let dropdown = gtk::DropDown::from_strings(&names);
        let activities = self.imp().activities.borrow();
        let entry = idle_entry_label(&projects, &activities, &pending.active);
        let duration = idle_duration_text(&pending);
        let dialog = adw::AlertDialog::builder()
            .heading(trf(
                "Move {duration} to another project",
                &[("duration", &duration)],
            ))
            .body(idle_reassign_body(&pending, &entry))
            .build();
        dialog.set_extra_child(Some(&dropdown));
        dialog.add_responses(&[("cancel", tr("Cancel")), ("reassign", tr("Move time"))]);
        dialog.set_default_response(Some("reassign"));
        let weak = self.downgrade();
        dialog.connect_response(None, move |_, response| {
            if response != "reassign" {
                if let Some(window) = weak.upgrade() {
                    window.imp().idle_dialog_open.set(false);
                }
                return;
            }
            let index = usize::try_from(dropdown.selected()).unwrap_or(0);
            let project_id = projects
                .get(index)
                .map_or(ProjectId(1), |project| project.id);
            let result = handle.apply(TrackerCommand::ResolveIdle(
                houra_core::IdleDecision::ReassignAndResume {
                    project_id,
                    activity_id: None,
                    note: tr("Idle time").into(),
                },
            ));
            if let Some(window) = weak.upgrade() {
                window.imp().idle_dialog_open.set(false);
                if let Err(error) = result {
                    window.show_database_error(&error.to_string());
                } else {
                    platform::withdraw_idle_notification();
                    window.refresh();
                }
            }
        });
        dialog.present(Some(self));
    }
}

fn idle_duration_text(pending: &PendingIdle) -> String {
    let duration_ms = pending
        .return_ms
        .unwrap_or(pending.idle_start_ms)
        .saturating_sub(pending.idle_start_ms);
    if duration_ms < 60_000 {
        tr("less than a minute").into()
    } else {
        format_duration(u64::try_from(duration_ms / 1_000).unwrap_or(0))
    }
}

fn idle_entry_label(projects: &[Project], activities: &[Activity], active: &ActiveTimer) -> String {
    let project = projects
        .iter()
        .find(|project| project.id == active.project_id)
        .map_or(tr("Missing project"), |project| project.name.as_str());
    active
        .activity_id
        .and_then(|id| activities.iter().find(|activity| activity.id == id))
        .map_or_else(
            || project.to_owned(),
            |activity| {
                trf(
                    "{project} · {activity}",
                    &[("project", project), ("activity", &activity.name)],
                )
            },
        )
}

fn idle_clock_text(ms: i64) -> String {
    Local.timestamp_millis_opt(ms).single().map_or_else(
        || crate::locale::ui_datetime(ms, "%X"),
        |value| value.format("%H:%M").to_string(),
    )
}

fn idle_review_body(pending: &PendingIdle, entry: &str) -> String {
    trf(
        "You stepped away from {entry} at {start} and came back at {end} ({duration}). Time tracked before that is already saved. Your timer keeps running unless you stop it.",
        &[
            ("entry", entry),
            ("start", &idle_clock_text(pending.idle_start_ms)),
            (
                "end",
                &idle_clock_text(pending.return_ms.unwrap_or(pending.idle_start_ms)),
            ),
            ("duration", &idle_duration_text(pending)),
        ],
    )
}

fn idle_reassign_body(pending: &PendingIdle, entry: &str) -> String {
    let range = format!(
        "{}–{}",
        idle_clock_text(pending.idle_start_ms),
        idle_clock_text(pending.return_ms.unwrap_or(pending.idle_start_ms))
    );
    trf(
        "The {duration} you were away ({range}) will be saved to the project you choose, and your timer will keep tracking {entry}.",
        &[
            ("duration", &idle_duration_text(pending)),
            ("range", &range),
            ("entry", entry),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::{
        idle_clock_text, idle_duration_text, idle_entry_label, idle_reassign_body, idle_review_body,
    };
    use houra_core::{ActiveTimer, Activity, ActivityId, PendingIdle, Project, ProjectId};

    fn pending(active: ActiveTimer) -> PendingIdle {
        PendingIdle {
            active,
            idle_start_ms: 1_000,
            return_ms: Some(61_000),
        }
    }

    fn active(project_id: ProjectId, activity_id: Option<ActivityId>) -> ActiveTimer {
        ActiveTimer {
            entry_id: None,
            project_id,
            activity_id,
            note: String::new(),
            start_ms: 0,
            started_monotonic_ms: 0,
            last_heartbeat_ms: 0,
            accumulated_ms: 0,
        }
    }

    fn project(id: ProjectId, name: &str) -> Project {
        Project {
            id,
            name: name.into(),
            color: "#3584e4".into(),
            archived: false,
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }

    fn activity(id: ActivityId, name: &str) -> Activity {
        Activity {
            id,
            name: name.into(),
            archived: false,
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }

    #[test]
    fn reports_the_recorded_idle_interval() {
        let mut pending = pending(active(ProjectId(1), None));
        pending.return_ms = Some(31_000);
        assert_eq!(idle_duration_text(&pending), "less than a minute");
        pending.return_ms = Some(6_301_000);
        assert_eq!(idle_duration_text(&pending), "1h 45m");
    }

    #[test]
    fn names_the_tracked_entry_with_project_and_activity() {
        let projects = vec![project(ProjectId(1), "General")];
        let activities = vec![activity(ActivityId(2), "Testing")];
        assert_eq!(
            idle_entry_label(
                &projects,
                &activities,
                &active(ProjectId(1), Some(ActivityId(2)))
            ),
            "General · Testing"
        );
        assert_eq!(
            idle_entry_label(&projects, &activities, &active(ProjectId(1), None)),
            "General"
        );
        assert_eq!(
            idle_entry_label(&[], &activities, &active(ProjectId(9), None)),
            "Missing project"
        );
    }

    #[test]
    fn review_body_states_the_gap_and_the_running_timer() {
        let body = idle_review_body(&pending(active(ProjectId(1), None)), "General · Testing");
        assert!(body.starts_with("You stepped away from General · Testing at "));
        assert!(body.contains(&idle_clock_text(1_000)));
        assert!(body.contains(&idle_clock_text(61_000)));
        assert!(body.contains("(1m)"));
        assert!(body.contains("Time tracked before that is already saved."));
        assert!(body.contains("Your timer keeps running unless you stop it."));
    }

    #[test]
    fn reassign_body_names_the_gap_destination_and_entry() {
        let body = idle_reassign_body(&pending(active(ProjectId(1), None)), "General");
        assert!(body.contains("The 1m you were away"));
        assert!(body.contains(&format!(
            "({}–{})",
            idle_clock_text(1_000),
            idle_clock_text(61_000)
        )));
        assert!(body.contains("will be saved to the project you choose"));
        assert!(body.contains("your timer will keep tracking General"));
    }

    #[test]
    fn clock_text_uses_hours_and_minutes() {
        let text = idle_clock_text(61_000);
        assert_eq!(text.len(), 5);
        assert_eq!(text.as_bytes()[2], b':');
        assert!(
            text.bytes()
                .enumerate()
                .all(|(index, byte)| { index == 2 || byte.is_ascii_digit() })
        );
    }
}
