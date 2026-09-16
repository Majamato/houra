use chrono::{Datelike, Local, NaiveDate, TimeZone};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use libadwaita as adw;

use crate::desktop::window::MainWindow;

impl MainWindow {
    fn report_bounds(&self) -> Option<(chrono::DateTime<Local>, chrono::DateTime<Local>)> {
        let today = Local::now().date_naive();
        let starts_monday = crate::desktop::load_settings()
            .is_none_or(|settings| settings.boolean("week-starts-monday"));
        let days_from_start = if starts_monday {
            today.weekday().num_days_from_monday()
        } else {
            today.weekday().num_days_from_sunday()
        };
        let week_start =
            today.checked_sub_signed(chrono::Duration::days(i64::from(days_from_start)))?;
        let start_date = week_start.checked_add_signed(chrono::Duration::weeks(i64::from(
            self.imp().report_week_offset.get(),
        )))?;
        let start = Local
            .from_local_datetime(&start_date.and_hms_opt(0, 0, 0)?)
            .earliest()?;
        let end_date = start_date.checked_add_signed(chrono::Duration::weeks(1))?;
        let end = Local
            .from_local_datetime(&end_date.and_hms_opt(0, 0, 0)?)
            .earliest()?;
        Some((start, end))
    }

    pub(in crate::desktop) fn refresh_report(&self) {
        let Some(handle) = self.handle() else { return };
        let Some((start, end)) = self.report_bounds() else {
            return;
        };
        self.imp().report_week_label.set_label(&format!(
            "{} – {}",
            start.format("%x"),
            end.date_naive()
                .pred_opt()
                .map_or_else(String::new, |date| date.format("%x").to_string())
        ));
        while let Some(child) = self.imp().report_box.first_child() {
            self.imp().report_box.remove(&child);
        }
        let entries = match handle.entries(start.timestamp_millis(), end.timestamp_millis()) {
            Ok(entries) => entries,
            Err(error) => {
                self.show_database_error(&error.to_string());
                return;
            }
        };
        let projects = handle.projects(true).unwrap_or_default();
        let activities = handle.activities(true).unwrap_or_default();
        let rows = houra_core::group_entries(&entries);
        if rows.is_empty() {
            self.imp()
                .report_box
                .append(&gtk::Label::new(Some("No tracked time this week")));
            return;
        }
        for row in rows {
            let date = NaiveDate::from_yo_opt(row.bucket.local_year, row.bucket.local_ordinal)
                .map_or_else(
                    || "Unknown day".into(),
                    |date| date.format("%A, %x").to_string(),
                );
            let project = projects
                .iter()
                .find(|project| project.id == row.bucket.project_id)
                .map_or("Missing project", |project| project.name.as_str());
            let activity = row
                .bucket
                .activity_id
                .and_then(|id| activities.iter().find(|activity| activity.id == id))
                .map(|activity| format!(" / {}", activity.name))
                .unwrap_or_default();
            let seconds = row.duration_ms / 1_000;
            let report_row = adw::ActionRow::builder()
                .title(format!("{project}{activity}"))
                .subtitle(format!(
                    "{date} · {}h {:02}m",
                    seconds / 3600,
                    (seconds / 60) % 60
                ))
                .build();
            self.imp().report_box.append(&report_row);
        }
    }

    pub(in crate::desktop) fn export_report_csv(&self) {
        let Some(handle) = self.handle() else { return };
        let Some((start, end)) = self.report_bounds() else {
            return;
        };
        let chooser = gtk::FileDialog::builder()
            .title("Export Weekly CSV")
            .initial_name(format!("houra-{}.csv", start.format("%Y-%m-%d")))
            .build();
        let weak = self.downgrade();
        chooser.save(Some(self), None::<&gio::Cancellable>, move |result| {
            let Some(window) = weak.upgrade() else { return };
            let result = result
                .map_err(|error| crate::AppError::InvalidBackup(error.to_string()))
                .and_then(|file| {
                    let path = file.path().ok_or_else(|| {
                        crate::AppError::InvalidBackup("CSV export requires a local file".into())
                    })?;
                    let entries =
                        handle.entries(start.timestamp_millis(), end.timestamp_millis())?;
                    let projects = handle.projects(true)?;
                    let activities = handle.activities(true)?;
                    crate::export::write_csv_path(&path, &entries, &projects, &activities)
                });
            if let Err(error) = result {
                window.show_database_error(&error.to_string());
            }
        });
    }
}
