use chrono::{Local, NaiveDateTime, TimeZone};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::{EntrySource, ProjectId, TimeEntry};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::window::MainWindow;

impl MainWindow {
    pub fn show_manual_entry(&self) {
        let Some(handle) = self.handle() else { return };
        let projects = self.imp().projects.borrow().clone();
        let activities = handle.activities(false).unwrap_or_default();
        let dialog = adw::Dialog::builder()
            .title("Manual Entry")
            .content_width(480)
            .content_height(480)
            .build();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .build();
        let project_names: Vec<&str> = projects
            .iter()
            .map(|project| project.name.as_str())
            .collect();
        let project = gtk::DropDown::from_strings(&project_names);
        let mut activity_names = vec!["No activity"];
        activity_names.extend(activities.iter().map(|activity| activity.name.as_str()));
        let activity = gtk::DropDown::from_strings(&activity_names);
        let note = gtk::Entry::builder()
            .placeholder_text("Optional note")
            .build();
        let end_local = Local::now();
        let start_local = end_local - chrono::Duration::hours(1);
        let start = gtk::Entry::builder()
            .text(start_local.format("%Y-%m-%d %H:%M:%S").to_string())
            .build();
        let end = gtk::Entry::builder()
            .text(end_local.format("%Y-%m-%d %H:%M:%S").to_string())
            .build();
        for (label_text, widget) in [
            ("Project", project.clone().upcast::<gtk::Widget>()),
            ("Activity", activity.clone().upcast()),
            ("Note", note.clone().upcast()),
            ("Start (local)", start.clone().upcast()),
            ("End (local)", end.clone().upcast()),
        ] {
            let label = gtk::Label::builder()
                .label(label_text)
                .halign(gtk::Align::Start)
                .build();
            content.append(&label);
            content.append(&widget);
        }
        let save = gtk::Button::with_label("Save Entry");
        save.add_css_class("suggested-action");
        content.append(&save);
        dialog.set_child(Some(&content));
        let weak = self.downgrade();
        let dialog_for_save = dialog.clone();
        save.connect_clicked(move |_| {
            let parse_local = |text: &str| {
                NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S")
                    .ok()
                    .and_then(|value| Local.from_local_datetime(&value).single())
                    .map(|value| value.timestamp_millis())
            };
            let Some(start_ms) = parse_local(&start.text()) else {
                if let Some(window) = weak.upgrade() {
                    window.show_database_error(
                        "Start must use YYYY-MM-DD HH:MM:SS and identify one local time.",
                    );
                }
                return;
            };
            let Some(end_ms) = parse_local(&end.text()) else {
                if let Some(window) = weak.upgrade() {
                    window.show_database_error(
                        "End must use YYYY-MM-DD HH:MM:SS and identify one local time.",
                    );
                }
                return;
            };
            let index = usize::try_from(project.selected()).unwrap_or(0);
            let project_id = projects
                .get(index)
                .map_or(ProjectId(1), |project| project.id);
            let activity_index = usize::try_from(activity.selected()).unwrap_or(0);
            let activity_id = activity_index
                .checked_sub(1)
                .and_then(|index| activities.get(index))
                .map(|activity| activity.id);
            let now = chrono::Utc::now().timestamp_millis();
            let entry = TimeEntry {
                id: None,
                project_id,
                activity_id,
                note: note.text().to_string(),
                start_ms,
                end_ms,
                source: EntrySource::Manual,
                created_at_ms: now,
                updated_at_ms: now,
            };
            match handle.add_entry(entry) {
                Ok(_) => {
                    dialog_for_save.close();
                    if let Some(window) = weak.upgrade() {
                        window.refresh();
                    }
                }
                Err(error) => {
                    if let Some(window) = weak.upgrade() {
                        window.show_database_error(&error.to_string());
                    }
                }
            }
        });
        dialog.present(Some(self));
    }

    pub(in crate::desktop) fn show_edit_entry(&self, existing: TimeEntry) {
        let Some(handle) = self.handle() else { return };
        let projects = self.imp().projects.borrow().clone();
        let activities = handle
            .activities(true)
            .unwrap_or_default()
            .into_iter()
            .filter(|activity| !activity.archived || Some(activity.id) == existing.activity_id)
            .collect::<Vec<_>>();
        let dialog = adw::Dialog::builder()
            .title("Edit Entry")
            .content_width(480)
            .content_height(480)
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
        if let Some(index) = projects
            .iter()
            .position(|project| project.id == existing.project_id)
            .and_then(|index| u32::try_from(index).ok())
        {
            project.set_selected(index);
        }
        let activity_names = std::iter::once("No activity".to_owned())
            .chain(activities.iter().map(|activity| {
                if activity.archived {
                    format!("{} (Archived)", activity.name)
                } else {
                    activity.name.clone()
                }
            }))
            .collect::<Vec<_>>();
        let activity_name_refs = activity_names
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let activity = gtk::DropDown::from_strings(&activity_name_refs);
        if let Some(index) = existing
            .activity_id
            .and_then(|id| activities.iter().position(|candidate| candidate.id == id))
            .and_then(|index| u32::try_from(index + 1).ok())
        {
            activity.set_selected(index);
        }
        let note = gtk::Entry::builder().text(&existing.note).build();
        let format_time = |timestamp| {
            Local
                .timestamp_millis_opt(timestamp)
                .single()
                .map_or_else(String::new, |value| {
                    value.format("%Y-%m-%d %H:%M:%S").to_string()
                })
        };
        let start = gtk::Entry::builder()
            .text(format_time(existing.start_ms))
            .build();
        let end = gtk::Entry::builder()
            .text(format_time(existing.end_ms))
            .build();
        for (label_text, widget) in [
            ("Project", project.clone().upcast::<gtk::Widget>()),
            ("Activity", activity.clone().upcast()),
            ("Note", note.clone().upcast()),
            ("Start (local)", start.clone().upcast()),
            ("End (local)", end.clone().upcast()),
        ] {
            content.append(
                &gtk::Label::builder()
                    .label(label_text)
                    .halign(gtk::Align::Start)
                    .build(),
            );
            content.append(&widget);
        }
        let save = gtk::Button::with_label("Save Changes");
        save.add_css_class("suggested-action");
        content.append(&save);
        dialog.set_child(Some(&content));
        let dialog_for_save = dialog.clone();
        let weak = self.downgrade();
        save.connect_clicked(move |_| {
            let parse = |entry: &gtk::Entry| {
                NaiveDateTime::parse_from_str(&entry.text(), "%Y-%m-%d %H:%M:%S")
                    .ok()
                    .and_then(|value| Local.from_local_datetime(&value).single())
                    .map(|value| value.timestamp_millis())
            };
            let (Some(start_ms), Some(end_ms)) = (parse(&start), parse(&end)) else {
                if let Some(window) = weak.upgrade() {
                    window.show_database_error(
                        "Times must use YYYY-MM-DD HH:MM:SS and identify one local time.",
                    );
                }
                return;
            };
            let index = usize::try_from(project.selected()).unwrap_or(0);
            let project_id = projects
                .get(index)
                .map_or(existing.project_id, |project| project.id);
            let activity_index = usize::try_from(activity.selected()).unwrap_or(0);
            let activity_id = activity_index
                .checked_sub(1)
                .and_then(|index| activities.get(index))
                .map(|activity| activity.id);
            let updated = TimeEntry {
                project_id,
                activity_id,
                note: note.text().to_string(),
                start_ms,
                end_ms,
                updated_at_ms: chrono::Utc::now().timestamp_millis(),
                ..existing.clone()
            };
            match handle.update_entry(updated) {
                Ok(()) => {
                    dialog_for_save.close();
                    if let Some(window) = weak.upgrade() {
                        window.refresh();
                        window.refresh_report();
                    }
                }
                Err(error) => {
                    if let Some(window) = weak.upgrade() {
                        window.show_database_error(&error.to_string());
                    }
                }
            }
        });
        dialog.present(Some(self));
    }
}
