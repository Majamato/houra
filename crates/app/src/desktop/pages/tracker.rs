use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::{ProjectId, TrackerCommand, TrackerState};

use crate::desktop::window::MainWindow;

impl MainWindow {
    pub(in crate::desktop) fn reload_projects(&self) {
        let Some(handle) = self.handle() else { return };
        match handle.projects(false) {
            Ok(projects) => {
                let names: Vec<&str> = projects
                    .iter()
                    .map(|project| project.name.as_str())
                    .collect();
                self.imp()
                    .project_dropdown
                    .set_model(Some(&gtk::StringList::new(&names)));
                self.imp().projects.replace(projects);
            }
            Err(error) => self.show_database_error(&error.to_string()),
        }
    }

    fn selected_project_id(&self) -> ProjectId {
        let index = usize::try_from(self.imp().project_dropdown.selected()).unwrap_or(0);
        self.imp()
            .projects
            .borrow()
            .get(index)
            .map_or(ProjectId(1), |project| project.id)
    }

    fn selected_activity_id(&self) -> Option<houra_core::ActivityId> {
        let selected = self.imp().activity_dropdown.selected();
        if selected == 0 || selected == gtk::INVALID_LIST_POSITION {
            return None;
        }
        usize::try_from(selected.saturating_sub(1))
            .ok()
            .and_then(|index| {
                self.imp()
                    .activities
                    .borrow()
                    .get(index)
                    .map(|activity| activity.id)
            })
    }

    pub(in crate::desktop) fn reload_activities(&self) {
        let Some(handle) = self.handle() else { return };
        let project_id = self.selected_project_id();
        let activities = handle
            .activities(false)
            .unwrap_or_default()
            .into_iter()
            .filter(|activity| activity.project_id == project_id)
            .collect::<Vec<_>>();
        let mut names = vec!["No activity"];
        names.extend(activities.iter().map(|activity| activity.name.as_str()));
        self.imp()
            .activity_dropdown
            .set_model(Some(&gtk::StringList::new(&names)));
        self.imp().activities.replace(activities);
    }

    pub(in crate::desktop) fn update_active_details(&self) {
        let Some(handle) = self.handle() else { return };
        let Ok(snapshot) = handle.snapshot() else {
            return;
        };
        if matches!(snapshot.state, TrackerState::Running(_)) {
            let result = handle.apply(TrackerCommand::EditActive {
                project_id: self.selected_project_id(),
                activity_id: self.selected_activity_id(),
                note: self.imp().note_entry.text().to_string(),
            });
            if let Err(error) = result {
                self.show_database_error(&error.to_string());
            }
        }
    }

    pub fn toggle_timer(&self) {
        let Some(handle) = self.handle() else { return };
        let state = match handle.snapshot() {
            Ok(snapshot) => snapshot.state,
            Err(error) => {
                self.show_database_error(&error.to_string());
                return;
            }
        };
        let command = match state {
            TrackerState::Stopped => TrackerCommand::Start {
                project_id: self.selected_project_id(),
                activity_id: self.selected_activity_id(),
                note: self.imp().note_entry.text().to_string(),
            },
            TrackerState::Running(_) => TrackerCommand::Stop,
            TrackerState::IdlePending(_) => {
                self.show_idle_dialog();
                return;
            }
            TrackerState::RecoveryPending(_) => {
                self.show_recovery_dialog();
                return;
            }
        };
        match handle.apply(command) {
            Ok(_) => self.refresh(),
            Err(error) => self.show_database_error(&error.to_string()),
        }
    }

    pub(in crate::desktop) fn refresh_timer_only(&self) {
        let Some(handle) = self.handle() else { return };
        let Ok(snapshot) = handle.snapshot() else {
            return;
        };
        match snapshot.state {
            TrackerState::Running(active)
            | TrackerState::IdlePending(houra_core::PendingIdle { active, .. }) => {
                let elapsed = handle.live_elapsed().map_or_else(
                    |_| {
                        u64::try_from(
                            chrono::Utc::now()
                                .timestamp_millis()
                                .saturating_sub(active.start_ms)
                                .max(0)
                                / 1_000,
                        )
                        .unwrap_or(0)
                    },
                    |duration| duration.as_secs(),
                );
                self.imp().timer_label.set_label(&format!(
                    "{:02}:{:02}:{:02}",
                    elapsed / 3600,
                    (elapsed / 60) % 60,
                    elapsed % 60
                ));
                self.imp().start_button.set_label("Stop");
                self.imp().start_button.add_css_class("destructive-action");
            }
            TrackerState::RecoveryPending(_) => {
                self.imp().timer_label.set_label("Recovery needed");
                self.imp().start_button.set_label("Review");
            }
            TrackerState::Stopped => {
                self.imp().timer_label.set_label("00:00:00");
                self.imp().start_button.set_label("Start");
                self.imp()
                    .start_button
                    .remove_css_class("destructive-action");
                self.imp().start_button.add_css_class("suggested-action");
            }
        }
    }
}
