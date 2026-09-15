use chrono::{Local, TimeZone};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::window::MainWindow;

impl MainWindow {
    pub(in crate::desktop) fn refresh_entries(&self) {
        let Some(handle) = self.handle() else { return };
        while let Some(child) = self.imp().entries_box.first_child() {
            self.imp().entries_box.remove(&child);
        }
        let Some(date) = Local::now()
            .date_naive()
            .checked_add_signed(chrono::Duration::days(i64::from(
                self.imp().selected_day_offset.get(),
            )))
        else {
            return;
        };
        let day_label = if self.imp().selected_day_offset.get() == 0 {
            "Today".to_owned()
        } else {
            date.format("%A, %x").to_string()
        };
        self.imp().day_label.set_label(&day_label);
        let Some(start) = date
            .and_hms_opt(0, 0, 0)
            .and_then(|value| Local.from_local_datetime(&value).earliest())
        else {
            return;
        };
        let Some(next_date) = date.succ_opt() else {
            return;
        };
        let Some(end) = next_date
            .and_hms_opt(0, 0, 0)
            .and_then(|value| Local.from_local_datetime(&value).earliest())
        else {
            return;
        };
        match handle.entries(start.timestamp_millis(), end.timestamp_millis()) {
            Ok(entries) if entries.is_empty() => {
                let label = gtk::Label::new(Some("No entries for today"));
                label.add_css_class("dim-label");
                self.imp().entries_box.append(&label);
            }
            Ok(entries) => {
                for entry in entries {
                    let duration = entry.duration_ms() / 1_000;
                    let row = adw::ActionRow::builder()
                        .title(if entry.note.is_empty() {
                            "Tracked work"
                        } else {
                            &entry.note
                        })
                        .subtitle(format!(
                            "{}h {:02}m {:02}s",
                            duration / 3600,
                            (duration / 60) % 60,
                            duration % 60
                        ))
                        .build();
                    row.set_activatable(true);
                    row.connect_activated(glib::clone!(
                        #[weak(rename_to = window)]
                        self,
                        #[strong]
                        entry,
                        move |_| window.show_edit_entry(entry.clone())
                    ));
                    self.imp().entries_box.append(&row);
                }
            }
            Err(error) => self.show_database_error(&error.to_string()),
        }
    }
}
