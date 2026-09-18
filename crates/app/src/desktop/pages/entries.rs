use chrono::{Datelike, Local, NaiveDate, TimeZone};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::{Project, TimeEntry, TrackerState};

use crate::desktop::window::MainWindow;

pub(super) fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds / 60) % 60;
    if hours == 0 {
        format!("{minutes}m")
    } else {
        format!("{hours}h {minutes:02}m")
    }
}

fn day_bounds(date: NaiveDate) -> Option<(chrono::DateTime<Local>, chrono::DateTime<Local>)> {
    let start = Local
        .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
        .earliest()?;
    let end = Local
        .from_local_datetime(&date.succ_opt()?.and_hms_opt(0, 0, 0)?)
        .earliest()?;
    Some((start, end))
}

impl MainWindow {
    pub(in crate::desktop) fn refresh_entries(&self) {
        let Some(handle) = self.handle() else { return };

        // Update the selected date and the date shown above the entries list.
        let today = Local::now().date_naive();
        self.imp()
            .displayed_today_ordinal
            .set(today.num_days_from_ce());
        let Some(date) = today.checked_add_signed(chrono::Duration::days(i64::from(
            self.imp().selected_day_offset.get(),
        ))) else {
            return;
        };
        self.imp()
            .day_button
            .set_label(&date.format("%A, %B %-d").to_string());

        // Rebuild the seven-day navigation strip for the selected date.
        self.refresh_week(date);
        let Some((start, end)) = day_bounds(date) else {
            return;
        };
        let entries = match handle.entries(start.timestamp_millis(), end.timestamp_millis()) {
            Ok(entries) => entries,
            Err(error) => {
                self.show_database_error(&error.to_string());
                return;
            }
        };
        let projects = handle.projects(true).unwrap_or_default();
        let activities = handle.activities(true).unwrap_or_default();
        let running = handle.snapshot().is_ok_and(|snapshot| {
            matches!(
                snapshot.state,
                TrackerState::Running(_) | TrackerState::IdlePending(_)
            )
        });
        self.imp()
            .entries_heading
            .set_label(if running && date == today {
                "Earlier today"
            } else if date == today {
                "Recorded today"
            } else {
                "Recorded"
            });
        self.imp().entries_count.set_label(&format!(
            "{} {}",
            entries.len(),
            if entries.len() == 1 {
                "entry"
            } else {
                "entries"
            }
        ));
        self.imp()
            .entries_count
            .set_visible(running || date != today);

        // The entries card contains either an empty-state message or one row per entry.
        while let Some(child) = self.imp().entries_box.first_child() {
            self.imp().entries_box.remove(&child);
        }
        if entries.is_empty() {
            let empty = gtk::Label::new(Some("No time recorded for this day"));
            empty.set_margin_top(22);
            empty.set_margin_bottom(22);
            empty.add_css_class("dim-label");
            self.imp().entries_box.append(&empty);
        } else {
            for (position, entry) in entries.iter().rev().enumerate() {
                if position > 0 {
                    self.imp()
                        .entries_box
                        .append(&gtk::Separator::new(gtk::Orientation::Horizontal));
                }
                self.imp().entries_box.append(&self.entry_row(
                    entry,
                    &projects,
                    &activities,
                    running,
                ));
            }
        }
        let stored_seconds = entries
            .iter()
            .map(|entry| u64::try_from(entry.duration_ms().max(0) / 1_000).unwrap_or(0))
            .sum::<u64>();
        self.imp().stored_day_seconds.set(stored_seconds);

        // Show the total recorded time below the entries card.
        self.imp()
            .total_title
            .set_label(if running && date == today {
                "Today, including current session"
            } else if date == today {
                "Total today"
            } else {
                "Total"
            });
        self.imp()
            .total_value
            .set_label(&format_duration(stored_seconds));
        self.refresh_timer_only();
    }

    fn refresh_week(&self, selected: NaiveDate) {
        let Some(handle) = self.handle() else { return };

        // The week strip shows each day's date and recorded duration.
        while let Some(child) = self.imp().week_box.first_child() {
            self.imp().week_box.remove(&child);
        }
        let starts_monday = crate::desktop::load_settings()
            .is_none_or(|settings| settings.boolean("week-starts-monday"));
        let from_start = if starts_monday {
            selected.weekday().num_days_from_monday()
        } else {
            selected.weekday().num_days_from_sunday()
        };
        let Some(week_start) =
            selected.checked_sub_signed(chrono::Duration::days(i64::from(from_start)))
        else {
            return;
        };
        let today = Local::now().date_naive();
        for day_index in 0..7 {
            let Some(date) = week_start.checked_add_signed(chrono::Duration::days(day_index))
            else {
                continue;
            };
            let mut total = day_bounds(date)
                .and_then(|(start, end)| {
                    handle
                        .entries(start.timestamp_millis(), end.timestamp_millis())
                        .ok()
                })
                .map_or(0, |entries| {
                    entries
                        .iter()
                        .map(|entry| u64::try_from(entry.duration_ms().max(0) / 1_000).unwrap_or(0))
                        .sum()
                });
            if date == today
                && let Ok(snapshot) = handle.snapshot()
                && let Some(active) = snapshot.state.active()
                && let Some((start, _)) = day_bounds(date)
            {
                total = total.saturating_add(
                    u64::try_from(
                        Local::now()
                            .timestamp_millis()
                            .saturating_sub(active.start_ms.max(start.timestamp_millis()))
                            .max(0)
                            / 1_000,
                    )
                    .unwrap_or(0),
                );
            }
            let button = gtk::Button::new();
            button.add_css_class("week-day");
            if date == selected {
                button.add_css_class("selected");
            }
            let column = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(4)
                .build();
            let weekday = gtk::Label::new(Some(&date.format("%a").to_string()));
            weekday.add_css_class("week-weekday");
            let number = gtk::Label::new(Some(&date.day().to_string()));
            number.add_css_class("week-number");
            let total_text = if total == 0 {
                "—".to_owned()
            } else {
                format_duration(total)
            };
            let total_label = gtk::Label::new(Some(&total_text));
            total_label.add_css_class("week-total");

            // Each day is clickable and reloads the entries for that date.
            column.append(&weekday);
            column.append(&number);
            column.append(&total_label);
            button.set_child(Some(&column));
            let weak = self.downgrade();
            button.connect_clicked(move |_| {
                if let Some(window) = weak.upgrade() {
                    let offset = date.signed_duration_since(today).num_days();
                    window
                        .imp()
                        .selected_day_offset
                        .set(i32::try_from(offset).unwrap_or(0));
                    window.refresh_entries();
                }
            });
            self.imp().week_box.append(&button);
        }
    }

    fn entry_row(
        &self,
        entry: &TimeEntry,
        projects: &[Project],
        activities: &[houra_core::Activity],
        running: bool,
    ) -> gtk::Box {
        let project = projects
            .iter()
            .find(|project| project.id == entry.project_id);
        let project_name = project.map_or("Missing project", |project| project.name.as_str());
        let activity_name = entry
            .activity_id
            .and_then(|id| activities.iter().find(|activity| activity.id == id))
            .map(|activity| activity.name.as_str());

        let start = Local.timestamp_millis_opt(entry.start_ms).single();
        let end = Local.timestamp_millis_opt(entry.end_ms).single();
        let times = match (start, end) {
            (Some(start), Some(end)) => {
                format!("{}–{}", start.format("%H:%M"), end.format("%H:%M"))
            }
            _ => String::new(),
        };
        let meta = activity_name.map_or_else(
            || format!("{project_name} · {times}"),
            |activity| format!("{project_name} · {activity} · {times}"),
        );

        let row = gtk::Box::builder()
            .spacing(10)
            .margin_top(13)
            .margin_bottom(13)
            .margin_start(14)
            .margin_end(14)
            .build();

        // The colored dot identifies the project for this entry.
        let dot = gtk::DrawingArea::builder()
            .width_request(10)
            .height_request(10)
            .valign(gtk::Align::Center)
            .build();
        let color = project
            .and_then(|project| gtk::gdk::RGBA::parse(&project.color).ok())
            .unwrap_or_else(|| gtk::gdk::RGBA::new(0.5, 0.7, 0.95, 1.0));
        dot.set_draw_func(move |_, context, width, height| {
            context.set_source_rgba(
                f64::from(color.red()),
                f64::from(color.green()),
                f64::from(color.blue()),
                1.0,
            );
            context.arc(
                f64::from(width) / 2.0,
                f64::from(height) / 2.0,
                4.5,
                0.0,
                std::f64::consts::TAU,
            );
            let _ignored = context.fill();
        });
        row.append(&dot);

        // The note and metadata button opens the entry editor.
        let edit = gtk::Button::new();
        edit.set_has_frame(false);
        edit.set_hexpand(true);
        edit.add_css_class("entry-details");
        let labels = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .build();
        let title = gtk::Label::new(Some(if entry.note.is_empty() {
            "Tracked work"
        } else {
            &entry.note
        }));
        title.set_halign(gtk::Align::Start);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.add_css_class("entry-title");
        let subtitle = gtk::Label::new(Some(&meta));
        subtitle.set_halign(gtk::Align::Start);
        subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
        subtitle.add_css_class("entry-meta");
        labels.append(&title);
        labels.append(&subtitle);
        edit.set_child(Some(&labels));
        let weak = self.downgrade();
        let entry_to_edit = entry.clone();
        edit.connect_clicked(move |_| {
            if let Some(window) = weak.upgrade() {
                window.show_edit_entry(entry_to_edit.clone());
            }
        });
        row.append(&edit);

        // Duration is shown beside the entry details.
        let duration = u64::try_from(entry.duration_ms().max(0) / 1_000).unwrap_or(0);
        let duration_label = gtk::Label::new(Some(&format_duration(duration)));
        duration_label.add_css_class("entry-duration");
        row.append(&duration_label);

        // Continue resumes this entry in the tracker.
        let resume = gtk::Button::with_label(if running { "▶" } else { "Continue" });
        resume.add_css_class("flat");
        resume.add_css_class("continue-button");
        resume.set_tooltip_text(Some("Continue this work"));
        let weak = self.downgrade();
        let entry_to_resume = entry.clone();
        resume.connect_clicked(move |_| {
            if let Some(window) = weak.upgrade() {
                window.continue_entry(&entry_to_resume);
            }
        });
        row.append(&resume);
        row
    }
}

#[cfg(test)]
mod tests {
    use super::format_duration;
    #[test]
    fn formats_compact_durations() {
        assert_eq!(format_duration(45 * 60), "45m");
        assert_eq!(format_duration(6_300), "1h 45m");
    }
}
