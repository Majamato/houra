use crate::date_format::format_date_range;
use crate::locale::tr;
use chrono::{Local, TimeZone};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use libadwaita as adw;

use crate::desktop::dialogs::chosen_path;
use crate::desktop::window::MainWindow;
use crate::export::{self, ReportMode, ReportOptions};

impl MainWindow {
    fn report_bounds(&self) -> Option<(chrono::DateTime<Local>, chrono::DateTime<Local>)> {
        let today = Local::now().date_naive();
        let week_start = crate::date_navigation::week_start(today)?;
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

    fn report_mode(&self) -> ReportMode {
        if self.imp().report_full_switch.is_active() {
            ReportMode::Full
        } else {
            ReportMode::Tasks
        }
    }

    pub(in crate::desktop) fn refresh_report(&self) {
        let Some(handle) = self.handle() else { return };
        let Some((start, end)) = self.report_bounds() else {
            return;
        };
        let Some(last_day) = end.date_naive().pred_opt() else {
            return;
        };
        self.imp().report_week_label.set_label(&format_date_range(
            start.date_naive(),
            last_day,
            self.date_format(),
        ));
        while let Some(child) = self.imp().report_box.first_child() {
            self.imp().report_box.remove(&child);
        }
        let week = (start.timestamp_millis(), end.timestamp_millis());
        let entries = match handle.entries(week.0, week.1) {
            Ok(entries) => entries,
            Err(error) => {
                self.show_database_error(&error.to_string());
                return;
            }
        };
        let projects = handle.projects(true).unwrap_or_default();
        let activities = handle.activities(true).unwrap_or_default();
        let options = ReportOptions {
            mode: self.report_mode(),
            totals: self.entry_totals(),
            date_format: self.date_format(),
        };
        let rows =
            export::localized_report_display_rows(&entries, &projects, &activities, week, &options);
        if rows.is_empty() {
            self.imp()
                .report_box
                .append(&gtk::Label::new(Some(tr("No tracked time this week"))));
            return;
        }
        for row in rows {
            let widget = adw::ActionRow::builder()
                .title(row.title)
                .subtitle(row.subtitle)
                .build();
            self.imp().report_box.append(&widget);
        }
    }

    pub(in crate::desktop) fn export_report_csv(&self) {
        let Some(handle) = self.handle() else { return };
        let Some((start, end)) = self.report_bounds() else {
            return;
        };
        let mode = self.report_mode();
        let chooser = gtk::FileDialog::builder()
            .title(tr("Export Weekly CSV"))
            .initial_name(format!("houra-{}.csv", start.format("%Y-%m-%d")))
            .build();
        let weak = self.downgrade();
        chooser.save(Some(self), None::<&gio::Cancellable>, move |result| {
            let Some(window) = weak.upgrade() else { return };
            let Some(path) = chosen_path(result, || {
                crate::AppError::CsvExport(tr("CSV export requires a local file").into())
            })
            .transpose() else {
                return;
            };
            let result = path.and_then(|path| {
                let week = (start.timestamp_millis(), end.timestamp_millis());
                let entries = handle.entries(week.0, week.1)?;
                let projects = handle.projects(true)?;
                let activities = handle.activities(true)?;
                let options = ReportOptions {
                    mode,
                    totals: window.entry_totals(),
                    date_format: window.date_format(),
                };
                export::write_csv_path(&path, &entries, &projects, &activities, week, &options)
            });
            if let Err(error) = result {
                window.show_database_error(&error.to_string());
            }
        });
    }
}
