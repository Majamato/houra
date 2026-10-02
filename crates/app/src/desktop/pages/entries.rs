use crate::date_format::format_date_with_weekday;
use crate::locale::{tr, trf, trn};
use chrono::{Datelike, Local, NaiveDate, TimeZone};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::{DurationRounding, EntryTotals, TimeEntry, TrackerState};
use libadwaita as adw;
use libadwaita::prelude::*;

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

/// How long an entry row takes to slide open or closed, in milliseconds.
const ROW_SLIDE_MS: u32 = 250;

/// Wraps a row so it slides open once shown. A revealer only animates while
/// mapped, so the reveal waits for the map.
fn slide_in(row: &impl IsA<gtk::Widget>) -> gtk::Revealer {
    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .transition_duration(ROW_SLIDE_MS)
        .child(row)
        .build();
    revealer.connect_map(|revealer| revealer.set_reveal_child(true));
    revealer
}

impl MainWindow {
    pub(in crate::desktop) fn refresh_entries(&self) {
        let Some(handle) = self.handle() else { return };

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
            .day_label
            .set_label(&format_date_with_weekday(date, self.date_format()));

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
        // A paused timer's frozen segment is already banked, so no live
        // portion is added on top of the stored intervals.
        let live_end_ms = snapshot
            .as_ref()
            .and_then(|snapshot| match &snapshot.state {
                TrackerState::Running(_) => Some(now_ms),
                TrackerState::IdlePending(pending) => Some(pending.return_ms.unwrap_or(now_ms)),
                TrackerState::RecoveryPending(pending) => Some(pending.proposed_end_ms),
                TrackerState::Stopped | TrackerState::Paused(_) => None,
            });
        let running = active.is_some();
        let review_required = snapshot.as_ref().is_some_and(|snapshot| {
            matches!(
                snapshot.state,
                TrackerState::IdlePending(_) | TrackerState::RecoveryPending(_)
            )
        });
        let paused = snapshot
            .as_ref()
            .is_some_and(|snapshot| matches!(snapshot.state, TrackerState::Paused(_)));
        let active_entry_id = active.as_ref().and_then(|active| active.entry_id);
        let totals = EntryTotals {
            rounding: self.imp().duration_rounding.get(),
            active_entry_id,
        };
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
                tr("Earlier today")
            } else if date == today {
                tr("Recorded today")
            } else {
                tr("Recorded")
            });
        self.imp().entries_count.set_label(
            &trn("{count} entry", "{count} entries", entries.len() as u32)
                .replace("{count}", &entries.len().to_string()),
        );
        self.imp()
            .entries_count
            .set_visible(running || date != today);

        while let Some(child) = self.imp().entries_box.first_child() {
            self.imp().entries_box.remove(&child);
        }
        if entries.is_empty() {
            let empty = gtk::Label::new(Some(tr("No time recorded for this day")));
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
                } else if is_active && paused {
                    EntryTrackingState::Paused
                } else if is_active {
                    EntryTrackingState::Tracking
                } else {
                    EntryTrackingState::Inactive
                };
                let row = EntryRow::new(
                    entry,
                    project,
                    activity,
                    (day_start_ms, day_end_ms),
                    &totals,
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
                let entry_to_delete = entry.clone();
                row.connect_delete_requested(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    move |row| window.show_delete_entry(entry_to_delete.clone(), row)
                ));
                if let Some(id) = entry.id {
                    row.connect_report_requested(glib::clone!(
                        #[weak(rename_to = window)]
                        self,
                        move |_| window.show_entry_report(id)
                    ));
                }
                // Starting or continuing lifts an entry to the top; let that
                // row slide open. Day changes and plain refreshes stay still.
                let arrives_on_top = position == 0
                    && is_active
                    && date == today
                    && self.imp().last_top_date.get() == Some(date)
                    && self.imp().last_top_entry.get() != entry.id;
                if arrives_on_top {
                    self.imp().entries_box.append(&slide_in(&row));
                } else {
                    self.imp().entries_box.append(&row);
                }
            }
        }
        self.imp()
            .last_top_entry
            .set(entries.last().and_then(|entry| entry.id));
        self.imp().last_top_date.set(Some(date));
        let stored_seconds = stored_seconds(&entries, &totals, day_start_ms, day_end_ms);
        self.imp().stored_day_seconds.set(stored_seconds);

        self.imp()
            .total_title
            .set_label(if running && date == today {
                tr("Today, including the active timer")
            } else if date == today {
                tr("Total today")
            } else {
                tr("Total")
            });
        self.imp()
            .total_value
            .set_label(&format_duration(stored_seconds));
        self.refresh_timer_only();
    }

    pub(in crate::desktop) fn show_delete_entry(&self, entry: TimeEntry, row: &EntryRow) {
        let Some(handle) = self.handle() else { return };
        let Some(id) = entry.id else { return };
        let dialog = adw::AlertDialog::builder()
            .heading(tr("Delete this time entry?"))
            .body(delete_confirmation_body(
                &entry,
                self.imp().duration_rounding.get(),
            ))
            .build();
        dialog.add_responses(&[("cancel", tr("Cancel")), ("delete", tr("Delete"))]);
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        let weak = self.downgrade();
        let row = row.downgrade();
        dialog.connect_response(Some("delete"), move |_, _| {
            let Some(window) = weak.upgrade() else { return };
            match handle.delete_entry(id) {
                Ok(()) => {
                    window.refresh_report();
                    match row.upgrade() {
                        Some(row) => window.slide_out_deleted_row(&row),
                        None => window.refresh(),
                    }
                }
                Err(error) => window.show_database_error(&error.to_string()),
            }
        });
        dialog.present(Some(self));
    }

    /// Collapses a deleted entry's row, then rebuilds the list without it.
    /// The entry is already gone; this only animates its departure. A row a
    /// later refresh already replaced just triggers the rebuild.
    fn slide_out_deleted_row(&self, row: &EntryRow) {
        let entries_box: &gtk::Widget = self.imp().entries_box.upcast_ref();
        // The row sits in the list directly, or inside its slide-in revealer.
        let slot: gtk::Widget = match row.parent() {
            Some(parent) if &parent == entries_box => row.clone().upcast(),
            Some(parent) if parent.parent().as_ref() == Some(entries_box) => parent,
            _ => {
                self.refresh();
                return;
            }
        };
        let previous = slot.prev_sibling();
        // Rows are joined by separators; the one this row leaves behind goes
        // with it.
        if let Some(separator) = previous
            .clone()
            .or_else(|| slot.next_sibling())
            .and_downcast::<gtk::Separator>()
        {
            separator.set_visible(false);
        }
        if let Some(revealer) = slot.downcast_ref::<gtk::Revealer>() {
            revealer.set_child(None::<&gtk::Widget>);
        }
        self.imp().entries_box.remove(&slot);

        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .transition_duration(ROW_SLIDE_MS)
            .reveal_child(true)
            .child(row)
            .build();
        revealer.connect_child_revealed_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |revealer| {
                if !revealer.is_child_revealed() {
                    window.refresh();
                }
            }
        ));
        // A revealer only animates while mapped, so the collapse waits for
        // the map.
        revealer.connect_map(|revealer| revealer.set_reveal_child(false));
        self.imp()
            .entries_box
            .insert_child_after(&revealer, previous.as_ref());
    }

    fn refresh_week(&self, selected: NaiveDate, today: NaiveDate) {
        let Some(handle) = self.handle() else { return };
        let totals = self.entry_totals();

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
        let shortcut_visible = today_shortcut_visible(visible_offset, selected, today);
        self.imp()
            .today_button
            .set_opacity(f64::from(u8::from(shortcut_visible)));
        self.imp().today_button.set_sensitive(shortcut_visible);
        for day_index in 0..7 {
            let Some(date) = week_start.checked_add_signed(chrono::Duration::days(day_index))
            else {
                continue;
            };
            let mut total = day_bounds(date).map_or(0, |(start, end)| {
                let (start_ms, end_ms) = (start.timestamp_millis(), end.timestamp_millis());
                handle.entries(start_ms, end_ms).map_or(0, |entries| {
                    stored_seconds(&entries, &totals, start_ms, end_ms)
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
                    TrackerState::Stopped | TrackerState::Paused(_) => active.start_ms,
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
            let button = WeekDayCell::new(date, total, date == selected, self.date_format());
            button.set_sensitive(crate::date_navigation::date_can_be_selected(date, today));

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

/// Whether the Today shortcut accepts input.
/// The row keeps its space when hidden so the week strip never moves.
fn today_shortcut_visible(visible_week_offset: i32, selected: NaiveDate, today: NaiveDate) -> bool {
    visible_week_offset != 0 || selected != today
}

/// Whole seconds the entries hold inside `[start_ms, end_ms)`, each entry
/// totaled as its row shows it, so the day total matches the rows.
fn stored_seconds(entries: &[TimeEntry], totals: &EntryTotals, start_ms: i64, end_ms: i64) -> u64 {
    let total_ms = entries.iter().fold(0_i64, |total, entry| {
        total.saturating_add(totals.total_ms(entry, start_ms, end_ms))
    });
    u64::try_from(total_ms.max(0) / 1_000).unwrap_or(0)
}

/// Confirms deletion with the note, tracked-interval count, and rounded
/// total duration. The active timer's entry can't be deleted, so it always
/// rounds.
fn delete_confirmation_body(entry: &TimeEntry, rounding: DurationRounding) -> String {
    let note = if entry.note.is_empty() {
        tr("Tracked work")
    } else {
        entry.note.as_str()
    };
    let intervals = trn(
        "{count} interval",
        "{count} intervals",
        u32::try_from(entry.intervals.len()).unwrap_or(u32::MAX),
    )
    .replace("{count}", &entry.intervals.len().to_string());
    let seconds = u64::try_from(rounding.round_ms(entry.duration_ms()) / 1_000).unwrap_or(0);
    trf(
        "\"{note}\" ({intervals}, {duration}) will be permanently deleted.",
        &[
            ("note", note),
            ("intervals", &intervals),
            ("duration", &format_duration(seconds)),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use houra_core::{EntrySource, ProjectId, TrackedInterval};

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap_or(NaiveDate::MIN)
    }

    fn interval(start_ms: i64, end_ms: i64) -> TrackedInterval {
        TrackedInterval {
            id: None,
            start_ms,
            end_ms,
            source: EntrySource::Timer,
        }
    }

    fn entry(note: &str, intervals: Vec<TrackedInterval>) -> TimeEntry {
        TimeEntry {
            id: None,
            project_id: ProjectId(1),
            activity_id: None,
            note: note.into(),
            intervals,
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }

    #[test]
    fn today_shortcut_hidden_on_current_day() {
        let today = date(2026, 9, 25);
        assert!(!today_shortcut_visible(0, today, today));
    }

    #[test]
    fn today_shortcut_shown_away_from_current_day() {
        let today = date(2026, 9, 25);
        assert!(today_shortcut_visible(0, date(2026, 9, 21), today));
        assert!(today_shortcut_visible(-1, today, today));
    }

    #[test]
    fn delete_confirmation_names_note_intervals_and_duration() {
        let entry = entry(
            "Design review",
            vec![interval(0, 3_600_000), interval(3_600_000, 3_900_000)],
        );
        assert_eq!(
            delete_confirmation_body(&entry, DurationRounding::Up),
            "\"Design review\" (2 intervals, 1h 05m) will be permanently deleted."
        );
    }

    #[test]
    fn delete_confirmation_falls_back_for_empty_notes() {
        let entry = entry("", vec![interval(0, 60_000)]);
        assert_eq!(
            delete_confirmation_body(&entry, DurationRounding::Up),
            "\"Tracked work\" (1 interval, 1m) will be permanently deleted."
        );
    }

    #[test]
    fn delete_confirmation_rounds_by_preference() {
        let entry = entry("", vec![interval(0, 70_000)]);
        for (rounding, duration) in [
            (DurationRounding::Up, "2m"),
            (DurationRounding::Nearest, "1m"),
            (DurationRounding::Down, "1m"),
        ] {
            assert_eq!(
                delete_confirmation_body(&entry, rounding),
                format!("\"Tracked work\" (1 interval, {duration}) will be permanently deleted.")
            );
        }
    }

    #[test]
    fn day_total_sums_rounded_finished_entries_and_exact_active_one() {
        let finished = entry("", vec![interval(0, 70_000), interval(100_000, 110_000)]);
        let mut active = entry("", vec![interval(200_000, 270_000)]);
        active.id = Some(houra_core::EntryId(7));
        let short = entry("", vec![interval(300_000, 340_000)]);
        let entries = [finished, active, short];
        let totals = |rounding| EntryTotals {
            rounding,
            active_entry_id: Some(houra_core::EntryId(7)),
        };
        // Finished: 80 s; active: 70 s exact; short: 40 s.
        assert_eq!(
            stored_seconds(&entries, &totals(DurationRounding::Up), 0, i64::MAX),
            120 + 70 + 60
        );
        assert_eq!(
            stored_seconds(&entries, &totals(DurationRounding::Nearest), 0, i64::MAX),
            60 + 70 + 60
        );
        assert_eq!(
            stored_seconds(&entries, &totals(DurationRounding::Down), 0, i64::MAX),
            60 + 70 + 60
        );
        // Clipping happens before rounding: 30 s of the first entry.
        assert_eq!(
            stored_seconds(
                &entries[..1],
                &totals(DurationRounding::Down),
                40_000,
                70_000
            ),
            60
        );
    }
}
