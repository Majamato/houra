use std::time::Duration;

use chrono::{Datelike, Local, TimeZone};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::{ActivityId, ProjectId, TimeEntry, TrackerCommand, TrackerState};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::window::MainWindow;

impl MainWindow {
    pub(in crate::desktop) fn refresh_active_entry_duration(&self) {
        let Some(handle) = self.handle() else { return };
        let Ok(snapshot) = handle.snapshot() else {
            return;
        };
        let Some(active) = snapshot.state.active() else {
            self.clear_active_entry_duration();
            return;
        };
        let saved_ms = active
            .entry_id
            .and_then(|entry_id| handle.entry(entry_id).ok())
            .map_or(0, |entry| entry.duration_ms());
        self.imp().active_entry_id.set(active.entry_id);
        self.imp().active_entry_saved_ms.set(saved_ms);
        self.imp().active_entry_duration_cached.set(true);
    }

    fn clear_active_entry_duration(&self) {
        self.imp().active_entry_id.set(None);
        self.imp().active_entry_saved_ms.set(0);
        self.imp().active_entry_duration_cached.set(false);
        self.imp().active_entry_total_label.set_visible(false);
    }

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

    fn selected_activity_id(&self) -> Option<ActivityId> {
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
        let selected_id = self.selected_activity_id();
        let activities = handle.activities(false).unwrap_or_default();
        let selected = selected_id
            .and_then(|id| activities.iter().position(|activity| activity.id == id))
            .and_then(|index| u32::try_from(index + 1).ok())
            .unwrap_or(0);
        let names = std::iter::once("No activity".to_owned())
            .chain(activities.iter().map(|activity| activity.name.clone()))
            .collect::<Vec<_>>();
        let name_refs = names.iter().map(String::as_str).collect::<Vec<_>>();
        self.imp().updating_activity_dropdown.set(true);
        self.imp().activities.replace(activities);
        self.imp()
            .activity_dropdown
            .set_model(Some(&gtk::StringList::new(&name_refs)));
        self.imp().activity_dropdown.set_selected(selected);
        self.imp().updating_activity_dropdown.set(false);
    }

    pub(in crate::desktop) fn update_active_details(&self) {
        let Some(handle) = self.handle() else { return };
        let Ok(snapshot) = handle.snapshot() else {
            return;
        };
        if matches!(snapshot.state, TrackerState::Running(_)) {
            if let Err(error) = handle.apply(TrackerCommand::EditActive {
                project_id: self.selected_project_id(),
                activity_id: self.selected_activity_id(),
                note: self.imp().note_entry.text().to_string(),
            }) {
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
        let clears_note = matches!(command, TrackerCommand::Stop);
        match handle.apply(command) {
            Ok(_) => {
                if clears_note {
                    self.imp().note_entry.set_text("");
                }
                self.refresh();
            }
            Err(error) => self.show_database_error(&error.to_string()),
        }
    }

    pub(in crate::desktop) fn continue_entry(&self, entry: &TimeEntry) {
        let Some(handle) = self.handle() else { return };
        let Some(entry_id) = entry.id else { return };
        match handle.snapshot().map(|snapshot| snapshot.state) {
            Ok(TrackerState::Stopped | TrackerState::Running(_)) => {}
            Ok(TrackerState::IdlePending(_)) => {
                self.show_idle_dialog();
                return;
            }
            Ok(TrackerState::RecoveryPending(_)) => {
                self.show_recovery_dialog();
                return;
            }
            Err(error) => {
                self.show_database_error(&error.to_string());
                return;
            }
        }
        match handle.continue_entry(entry_id) {
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
                if !self.imp().active_entry_duration_cached.get()
                    || self.imp().active_entry_id.get() != active.entry_id
                {
                    self.refresh_active_entry_duration();
                }
                let elapsed = handle.live_elapsed().unwrap_or_else(|_| {
                    Duration::from_secs(
                        u64::try_from(
                            chrono::Utc::now()
                                .timestamp_millis()
                                .saturating_sub(active.start_ms)
                                .max(0)
                                / 1_000,
                        )
                        .unwrap_or(0),
                    )
                });
                let elapsed_seconds = elapsed.as_secs();
                self.imp().timer_label.set_label(&format!(
                    "{:02}:{:02}:{:02}",
                    elapsed_seconds / 3600,
                    (elapsed_seconds / 60) % 60,
                    elapsed_seconds % 60
                ));
                let total_seconds =
                    active_entry_total_seconds(self.imp().active_entry_saved_ms.get(), elapsed);
                self.imp().active_entry_total_label.set_label(&format!(
                    "{} total on this entry",
                    crate::desktop::widgets::format_duration(total_seconds)
                ));
                self.imp().active_entry_total_label.set_visible(true);
                self.imp().stopped_panel.set_visible(false);
                self.imp().running_panel.set_visible(true);
                self.set_active_labels(&active);
                let now = Local::now();
                let midnight_ms = now
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .and_then(|value| Local.from_local_datetime(&value).earliest())
                    .map_or(active.start_ms, |value| value.timestamp_millis());
                let live_today = u64::try_from(
                    now.timestamp_millis()
                        .saturating_sub(active.start_ms.max(midnight_ms))
                        .max(0)
                        / 1_000,
                )
                .unwrap_or(0);
                self.update_live_total(live_today);
            }
            TrackerState::RecoveryPending(_) => {
                self.imp().timer_label.set_label("Review");
                self.imp().active_entry_total_label.set_visible(false);
                self.imp().stopped_panel.set_visible(false);
                self.imp().running_panel.set_visible(true);
            }
            TrackerState::Stopped => {
                self.clear_active_entry_duration();
                self.imp().stopped_panel.set_visible(true);
                self.imp().running_panel.set_visible(false);
            }
        }
    }

    fn set_active_labels(&self, active: &houra_core::ActiveTimer) {
        self.imp()
            .active_note_label
            .set_label(if active.note.is_empty() {
                "Tracked work"
            } else {
                &active.note
            });
        let projects = self.imp().projects.borrow();
        let activities = self.imp().activities.borrow();
        let project = projects
            .iter()
            .find(|project| project.id == active.project_id)
            .map_or("Missing project", |project| project.name.as_str());
        let activity = active
            .activity_id
            .and_then(|id| activities.iter().find(|activity| activity.id == id))
            .map(|activity| activity.name.as_str());
        self.imp()
            .active_meta_label
            .set_label(&activity.map_or_else(
                || project.to_owned(),
                |activity| format!("{project} · {activity}"),
            ));
    }

    fn update_live_total(&self, elapsed: u64) {
        if self.imp().selected_day_offset.get() != 0 {
            return;
        }
        let stored = self.imp().stored_day_seconds.get();
        self.imp()
            .total_value
            .set_label(&crate::desktop::widgets::format_duration(
                stored.saturating_add(elapsed),
            ));
    }

    pub(in crate::desktop) fn show_active_editor(&self) {
        let Some(handle) = self.handle() else { return };
        let Ok(snapshot) = handle.snapshot() else {
            return;
        };
        let Some(active) = snapshot.state.active().cloned() else {
            return;
        };
        let projects = self.imp().projects.borrow().clone();
        let activities = self.imp().activities.borrow().clone();
        let dialog = adw::Dialog::builder()
            .title("Edit current session")
            .content_width(440)
            .build();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .build();
        let project_names = projects
            .iter()
            .map(|project| project.name.as_str())
            .collect::<Vec<_>>();
        let project = gtk::DropDown::from_strings(&project_names);
        project.set_selected(
            projects
                .iter()
                .position(|item| item.id == active.project_id)
                .and_then(|index| u32::try_from(index).ok())
                .unwrap_or(0),
        );
        let mut activity_names = vec!["No activity"];
        activity_names.extend(activities.iter().map(|activity| activity.name.as_str()));
        let activity = gtk::DropDown::from_strings(&activity_names);
        activity.set_selected(
            active
                .activity_id
                .and_then(|id| activities.iter().position(|item| item.id == id))
                .and_then(|index| u32::try_from(index + 1).ok())
                .unwrap_or(0),
        );
        let note = gtk::Entry::builder()
            .text(active.note)
            .placeholder_text("What are you working on?")
            .build();
        for (label, widget) in [
            ("Project", project.clone().upcast::<gtk::Widget>()),
            ("Activity", activity.clone().upcast()),
            ("Note", note.clone().upcast()),
        ] {
            content.append(
                &gtk::Label::builder()
                    .label(label)
                    .halign(gtk::Align::Start)
                    .build(),
            );
            content.append(&widget);
        }
        let save = gtk::Button::with_label("Save changes");
        save.add_css_class("suggested-action");
        content.append(&save);
        dialog.set_child(Some(&content));
        let weak = self.downgrade();
        let dialog_to_close = dialog.clone();
        save.connect_clicked(move |_| {
            let Some(window) = weak.upgrade() else { return };
            let project_id = usize::try_from(project.selected())
                .ok()
                .and_then(|index| projects.get(index))
                .map_or(ProjectId(1), |item| item.id);
            let activity_id = usize::try_from(activity.selected())
                .ok()
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| activities.get(index))
                .map(|item| item.id);
            match handle.apply(TrackerCommand::EditActive {
                project_id,
                activity_id,
                note: note.text().to_string(),
            }) {
                Ok(_) => {
                    dialog_to_close.close();
                    window.refresh();
                }
                Err(error) => window.show_database_error(&error.to_string()),
            }
        });
        dialog.present(Some(self));
    }

    pub(in crate::desktop) fn show_date_chooser(&self) {
        let dialog = adw::Dialog::builder()
            .title("Choose date")
            .content_width(360)
            .build();
        let calendar = gtk::Calendar::new();
        let date = Local::now()
            .date_naive()
            .checked_add_signed(chrono::Duration::days(i64::from(
                self.imp().selected_day_offset.get(),
            )))
            .unwrap_or_else(|| Local::now().date_naive());
        if let Ok(value) = glib::DateTime::new(
            &glib::TimeZone::local(),
            date.year(),
            i32::try_from(date.month()).unwrap_or(1),
            i32::try_from(date.day()).unwrap_or(1),
            12,
            0,
            0.0,
        ) {
            calendar.select_day(&value);
        }
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(18)
            .margin_bottom(18)
            .margin_start(18)
            .margin_end(18)
            .build();
        content.append(&calendar);
        let choose = gtk::Button::with_label("Choose date");
        choose.add_css_class("suggested-action");
        let selection_is_allowed = |calendar: &gtk::Calendar| {
            let selected = calendar.date();
            chrono::NaiveDate::from_ymd_opt(
                selected.year(),
                u32::try_from(selected.month()).unwrap_or(1),
                u32::try_from(selected.day_of_month()).unwrap_or(1),
            )
            .is_some_and(|date| {
                crate::date_navigation::date_can_be_selected(date, Local::now().date_naive())
            })
        };
        choose.set_sensitive(selection_is_allowed(&calendar));
        calendar.connect_day_selected(glib::clone!(
            #[weak]
            choose,
            move |calendar| choose.set_sensitive(selection_is_allowed(calendar))
        ));
        content.append(&choose);
        dialog.set_child(Some(&content));
        let weak = self.downgrade();
        let dialog_to_close = dialog.clone();
        choose.connect_clicked(move |_| {
            if let Some(window) = weak.upgrade() {
                let selected = calendar.date();
                if let Ok(date) = chrono::NaiveDate::from_ymd_opt(
                    selected.year(),
                    u32::try_from(selected.month()).unwrap_or(1),
                    u32::try_from(selected.day_of_month()).unwrap_or(1),
                )
                .ok_or(())
                {
                    let today = Local::now().date_naive();
                    if !crate::date_navigation::date_can_be_selected(date, today) {
                        return;
                    }
                    let offset = date.signed_duration_since(today).num_days();
                    window
                        .imp()
                        .selected_day_offset
                        .set(i32::try_from(offset).unwrap_or(0));
                    if let Some(week_offset) =
                        crate::date_navigation::week_offset_for_date(date, today)
                    {
                        window.imp().visible_week_offset.set(week_offset);
                    }
                    window.refresh_entries();
                }
                dialog_to_close.close();
            }
        });
        dialog.present(Some(self));
    }
}

fn active_entry_total_seconds(saved_duration_ms: i64, live_duration: Duration) -> u64 {
    let live_ms = i64::try_from(live_duration.as_millis()).unwrap_or(i64::MAX);
    u64::try_from(saved_duration_ms.saturating_add(live_ms).max(0) / 1_000).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::active_entry_total_seconds;

    #[test]
    fn active_entry_total_combines_saved_intervals_and_live_session() {
        assert_eq!(
            active_entry_total_seconds(3_075_500, Duration::from_millis(44_499)),
            51 * 60 + 59
        );
        assert_eq!(
            active_entry_total_seconds(3_075_500, Duration::from_millis(44_500)),
            52 * 60
        );
        assert_eq!(active_entry_total_seconds(0, Duration::from_secs(61)), 61);
    }
}
