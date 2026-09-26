use crate::desktop::{load_settings, log_background_error, platform, widgets::format_duration};
use crate::locale::{tr, trf};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::{PendingIdle, ProjectId, TrackerCommand, TrackerState};
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
        let dialog = adw::AlertDialog::builder()
            .heading(tr("You were away"))
            .body(idle_review_body(&pending))
            .build();
        dialog.add_responses(&[
            ("keep", tr("Keep")),
            ("discard", tr("Discard and Resume")),
            ("reassign", tr("Reassign and Resume")),
            ("stop", tr("Stop")),
        ]);
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
        let dialog = adw::AlertDialog::builder()
            .heading(tr("Reassign idle interval"))
            .body(trf(
                "Choose the project that should receive {duration} of idle time.",
                &[("duration", &idle_duration_text(&pending))],
            ))
            .build();
        dialog.set_extra_child(Some(&dropdown));
        dialog.add_responses(&[("cancel", tr("Cancel")), ("reassign", tr("Reassign"))]);
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

fn idle_review_body(pending: &PendingIdle) -> String {
    trf(
        "You were away for {duration}. How should this time be counted?",
        &[("duration", &idle_duration_text(pending))],
    )
}

#[cfg(test)]
mod tests {
    use super::idle_duration_text;
    use houra_core::{ActiveTimer, PendingIdle, ProjectId};

    #[test]
    fn reports_the_recorded_idle_interval() {
        let mut pending = PendingIdle {
            active: ActiveTimer {
                entry_id: None,
                project_id: ProjectId(1),
                activity_id: None,
                note: String::new(),
                start_ms: 0,
                started_monotonic_ms: 0,
                last_heartbeat_ms: 0,
            },
            idle_start_ms: 1_000,
            return_ms: Some(31_000),
        };
        assert_eq!(idle_duration_text(&pending), "less than a minute");
        pending.return_ms = Some(6_301_000);
        assert_eq!(idle_duration_text(&pending), "1h 45m");
    }
}
