use chrono::{Datelike, Local, NaiveDate, TimeZone};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::TrackerState;

use crate::desktop::widgets::{EntryRow, EntryTrackingState, WeekDayCell, format_duration};
use crate::desktop::window::MainWindow;

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

        // Rebuild the independently browsable seven-day navigation strip.
        self.refresh_week(date, today);
        let Some((start, end)) = day_bounds(date) else {
            return;
        };
        let day_start_ms = start.timestamp_millis();
        let day_end_ms = end.timestamp_millis();
        let mut entries = match handle.entries(day_start_ms, day_end_ms) {
            Ok(entries) => entries,
            Err(error) => {
                self.show_database_error(&error.to_string());
                return;
            }
        };
        let projects = handle.projects(true).unwrap_or_default();
        let activities = handle.activities(true).unwrap_or_default();
        let snapshot = handle.snapshot().ok();
        let active = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.state.active())
            .cloned();
        let now_ms = Local::now().timestamp_millis();
        let live_end_ms = snapshot
            .as_ref()
            .and_then(|snapshot| match &snapshot.state {
                TrackerState::Running(_) => Some(now_ms),
                TrackerState::IdlePending(pending) => Some(pending.return_ms.unwrap_or(now_ms)),
                TrackerState::RecoveryPending(pending) => Some(pending.proposed_end_ms),
                TrackerState::Stopped => None,
            });
        let running = active.is_some();
        let review_required = snapshot.as_ref().is_some_and(|snapshot| {
            matches!(
                snapshot.state,
                TrackerState::IdlePending(_) | TrackerState::RecoveryPending(_)
            )
        });
        let active_entry_id = active.as_ref().and_then(|active| active.entry_id);
        if date == today
            && let Some(id) = active_entry_id
            && !entries.iter().any(|entry| entry.id == Some(id))
            && let Ok(entry) = handle.entry(id)
        {
            entries.push(entry);
        }
        entries.sort_by_key(|entry| {
            if entry.id == active_entry_id && date == today {
                i64::MAX
            } else {
                entry
                    .intervals
                    .iter()
                    .filter(|interval| {
                        interval.start_ms < day_end_ms && interval.end_ms > day_start_ms
                    })
                    .map(|interval| interval.end_ms.min(day_end_ms))
                    .max()
                    .unwrap_or(i64::MIN)
            }
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
                let project = projects
                    .iter()
                    .find(|project| project.id == entry.project_id);
                let activity = entry
                    .activity_id
                    .and_then(|id| activities.iter().find(|activity| activity.id == id));
                let is_active = entry.id == active_entry_id;
                let live_ms = if is_active && date == today {
                    active
                        .as_ref()
                        .zip(live_end_ms)
                        .map_or(0, |(active, live_end)| {
                            live_end
                                .min(day_end_ms)
                                .saturating_sub(active.start_ms.max(day_start_ms))
                                .max(0)
                        })
                } else {
                    0
                };
                let tracking = if is_active && review_required {
                    EntryTrackingState::ReviewRequired
                } else if is_active {
                    EntryTrackingState::Tracking
                } else {
                    EntryTrackingState::Inactive
                };
                let row = EntryRow::new(
                    entry,
                    project,
                    activity,
                    day_start_ms,
                    day_end_ms,
                    live_ms,
                    tracking,
                );
                let entry_to_edit = entry.clone();
                row.connect_edit_requested(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    move |_| {
                        if entry_to_edit.id == active_entry_id {
                            window.show_active_editor();
                        } else {
                            window.show_edit_entry(entry_to_edit.clone());
                        }
                    }
                ));
                let entry_to_resume = entry.clone();
                row.connect_continue_requested(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    move |_| window.continue_entry(&entry_to_resume)
                ));
                if let Some(id) = entry.id {
                    row.connect_report_requested(glib::clone!(
                        #[weak(rename_to = window)]
                        self,
                        move |_| window.show_entry_report(id)
                    ));
                }
                self.imp().entries_box.append(&row);
            }
        }
        let stored_seconds = entries
            .iter()
            .flat_map(|entry| &entry.intervals)
            .map(|interval| {
                u64::try_from(
                    interval
                        .end_ms
                        .min(day_end_ms)
                        .saturating_sub(interval.start_ms.max(day_start_ms))
                        .max(0)
                        / 1_000,
                )
                .unwrap_or(0)
            })
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

    fn refresh_week(&self, selected: NaiveDate, today: NaiveDate) {
        let Some(handle) = self.handle() else { return };

        // The week strip shows each day's date and recorded duration.
        while let Some(child) = self.imp().week_box.first_child() {
            self.imp().week_box.remove(&child);
        }
        let visible_offset = self.imp().visible_week_offset.get().min(0);
        self.imp().visible_week_offset.set(visible_offset);
        let Some(week_start) = crate::date_navigation::visible_week_start(today, visible_offset)
        else {
            return;
        };
        self.imp().next_week_button.set_visible(visible_offset < 0);
        self.imp()
            .today_row
            .set_visible(visible_offset != 0 || selected != today);
        for day_index in 0..7 {
            let Some(date) = week_start.checked_add_signed(chrono::Duration::days(day_index))
            else {
                continue;
            };
            let mut total = day_bounds(date).map_or(0, |(start, end)| {
                handle
                    .entries(start.timestamp_millis(), end.timestamp_millis())
                    .map_or(0, |entries| {
                        entries
                            .iter()
                            .flat_map(|entry| &entry.intervals)
                            .map(|interval| {
                                u64::try_from(
                                    interval
                                        .end_ms
                                        .min(end.timestamp_millis())
                                        .saturating_sub(
                                            interval.start_ms.max(start.timestamp_millis()),
                                        )
                                        .max(0)
                                        / 1_000,
                                )
                                .unwrap_or(0)
                            })
                            .sum()
                    })
            });
            if date == today
                && let Ok(snapshot) = handle.snapshot()
                && let Some(active) = snapshot.state.active()
                && let Some((start, _)) = day_bounds(date)
            {
                let live_end = match &snapshot.state {
                    TrackerState::Running(_) => Local::now().timestamp_millis(),
                    TrackerState::IdlePending(pending) => pending
                        .return_ms
                        .unwrap_or_else(|| Local::now().timestamp_millis()),
                    TrackerState::RecoveryPending(pending) => pending.proposed_end_ms,
                    TrackerState::Stopped => active.start_ms,
                };
                total = total.saturating_add(
                    u64::try_from(
                        live_end
                            .saturating_sub(active.start_ms.max(start.timestamp_millis()))
                            .max(0)
                            / 1_000,
                    )
                    .unwrap_or(0),
                );
            }
            let button = WeekDayCell::new(date, total, date == selected);
            button.set_sensitive(crate::date_navigation::date_can_be_selected(date, today));

            // Each day is clickable and reloads the entries for that date.
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

    pub(in crate::desktop) fn show_previous_week(&self) {
        self.imp()
            .visible_week_offset
            .set(crate::date_navigation::previous_week_offset(
                self.imp().visible_week_offset.get(),
            ));
        self.refresh_visible_week();
    }

    pub(in crate::desktop) fn show_next_week(&self) {
        self.imp()
            .visible_week_offset
            .set(crate::date_navigation::next_week_offset(
                self.imp().visible_week_offset.get(),
            ));
        self.refresh_visible_week();
    }

    pub(in crate::desktop) fn show_today(&self) {
        self.imp().visible_week_offset.set(0);
        self.imp().selected_day_offset.set(0);
        self.refresh_entries();
    }

    fn refresh_visible_week(&self) {
        let today = Local::now().date_naive();
        let selected = today
            .checked_add_signed(chrono::Duration::days(i64::from(
                self.imp().selected_day_offset.get(),
            )))
            .unwrap_or(today);
        self.refresh_week(selected, today);
    }
}
