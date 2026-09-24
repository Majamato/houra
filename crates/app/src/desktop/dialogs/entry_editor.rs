use crate::locale::{tr, trf};
use chrono::{Local, NaiveDateTime, TimeZone};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::{EntrySource, ProjectId, TimeEntry, TrackedInterval};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::window::MainWindow;

const MILLIS_PER_MINUTE: i64 = 60_000;
const MAX_DURATION_HOURS: i64 = 999;
const MAX_DURATION_MINUTES: i64 = MAX_DURATION_HOURS * 60 + 59;

fn parse_local_timestamp(text: &str) -> Option<i64> {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S")
        .ok()
        .and_then(|value| Local.from_local_datetime(&value).single())
        .map(|value| value.timestamp_millis())
}

fn rounded_duration_parts(duration_ms: i64) -> (u32, u32) {
    let total_minutes = duration_ms
        .saturating_add(MILLIS_PER_MINUTE / 2)
        .div_euclid(MILLIS_PER_MINUTE)
        .clamp(1, MAX_DURATION_MINUTES);
    (
        u32::try_from(total_minutes / 60).unwrap_or(0),
        u32::try_from(total_minutes % 60).unwrap_or(0),
    )
}

fn end_from_duration(start_ms: i64, hours: i32, minutes: i32) -> Option<i64> {
    if !(0..=999).contains(&hours) || !(0..=59).contains(&minutes) {
        return None;
    }
    let total_minutes = i64::from(hours)
        .checked_mul(60)?
        .checked_add(i64::from(minutes))?;
    if total_minutes == 0 {
        return None;
    }
    start_ms.checked_add(total_minutes.checked_mul(MILLIS_PER_MINUTE)?)
}

fn intervals_from_total_duration(
    intervals: &[TrackedInterval],
    initial_parts: (u32, u32),
    hours: i32,
    minutes: i32,
) -> Option<Vec<TrackedInterval>> {
    if !(0..=999).contains(&hours) || !(0..=59).contains(&minutes) {
        return None;
    }
    let selected_parts = (u32::try_from(hours).ok()?, u32::try_from(minutes).ok()?);
    if selected_parts == initial_parts {
        return Some(intervals.to_vec());
    }

    let final_index = intervals
        .iter()
        .enumerate()
        .max_by_key(|(_, interval)| interval.start_ms)?
        .0;
    let earlier_duration_ms = intervals
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != final_index)
        .try_fold(0_i64, |total, (_, interval)| {
            let duration = interval.end_ms.checked_sub(interval.start_ms)?;
            (duration > 0).then_some(())?;
            total.checked_add(duration)
        })?;
    let selected_minutes = i64::from(hours)
        .checked_mul(60)?
        .checked_add(i64::from(minutes))?;
    let selected_duration_ms = selected_minutes.checked_mul(MILLIS_PER_MINUTE)?;
    let final_duration_ms = selected_duration_ms.checked_sub(earlier_duration_ms)?;
    if final_duration_ms <= 0 {
        return None;
    }

    let mut edited = intervals.to_vec();
    edited[final_index].end_ms = edited[final_index]
        .start_ms
        .checked_add(final_duration_ms)?;
    Some(edited)
}

fn duration_controls(duration_ms: i64) -> (gtk::Box, gtk::SpinButton, gtk::SpinButton) {
    let (hours_value, minutes_value) = rounded_duration_parts(duration_ms);
    let hours = gtk::SpinButton::with_range(0.0, 999.0, 1.0);
    hours.set_value(f64::from(hours_value));
    let minutes = gtk::SpinButton::with_range(0.0, 59.0, 1.0);
    minutes.set_value(f64::from(minutes_value));

    let fields = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .homogeneous(true)
        .build();
    for (label_text, input) in [(tr("Hours"), &hours), (tr("Minutes"), &minutes)] {
        let field = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        field.append(
            &gtk::Label::builder()
                .label(label_text)
                .halign(gtk::Align::Start)
                .build(),
        );
        field.append(input);
        fields.append(&field);
    }
    (fields, hours, minutes)
}

fn set_dialog_content(dialog: &adw::Dialog, content: &impl IsA<gtk::Widget>) {
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    let close = gtk::Button::builder()
        .icon_name("window-close-symbolic")
        .tooltip_text(tr("Close"))
        .build();
    close.update_property(&[gtk::accessible::Property::Label(tr("Close"))]);
    close.add_css_class("flat");
    header.pack_end(&close);
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(content));
    dialog.set_child(Some(&toolbar));

    let dialog_to_close = dialog.clone();
    close.connect_clicked(move |_| {
        dialog_to_close.close();
    });
}

impl MainWindow {
    pub fn show_manual_entry(&self) {
        let Some(handle) = self.handle() else { return };
        let projects = self.imp().projects.borrow().clone();
        let activities = handle.activities(false).unwrap_or_default();

        // Build the form used to add a manually recorded entry.
        let dialog = adw::Dialog::builder()
            .title(tr("Manual Entry"))
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
        let mut activity_names = vec![tr("No activity")];
        activity_names.extend(activities.iter().map(|activity| activity.name.as_str()));
        let activity = gtk::DropDown::from_strings(&activity_names);
        let note = gtk::Entry::builder()
            .placeholder_text(tr("Optional note"))
            .build();
        let now = Local::now();
        let selected_date = now
            .date_naive()
            .checked_add_signed(chrono::Duration::days(i64::from(
                self.imp().selected_day_offset.get(),
            )))
            .unwrap_or_else(|| now.date_naive());
        let end_local = if self.imp().selected_day_offset.get() == 0 {
            now
        } else {
            selected_date
                .and_hms_opt(17, 0, 0)
                .and_then(|value| Local.from_local_datetime(&value).earliest())
                .unwrap_or(now)
        };
        let start_local = end_local - chrono::Duration::hours(1);
        let start = gtk::Entry::builder()
            .text(start_local.format("%Y-%m-%d %H:%M:%S").to_string())
            .build();

        // Stack each field label above its input widget.
        for (label_text, widget) in [
            (tr("Project"), project.clone().upcast::<gtk::Widget>()),
            (tr("Activity"), activity.clone().upcast()),
            (tr("Note"), note.clone().upcast()),
            (tr("Start (local)"), start.clone().upcast()),
        ] {
            let label = gtk::Label::builder()
                .label(label_text)
                .halign(gtk::Align::Start)
                .build();
            content.append(&label);
            content.append(&widget);
        }
        let (duration, duration_hours, duration_minutes) =
            duration_controls(chrono::Duration::hours(1).num_milliseconds());
        content.append(
            &gtk::Label::builder()
                .label(tr("Time spent"))
                .halign(gtk::Align::Start)
                .build(),
        );
        content.append(&duration);
        let save = gtk::Button::with_label(tr("Save Entry"));
        save.add_css_class("suggested-action");
        content.append(&save);
        set_dialog_content(&dialog, &content);
        let weak = self.downgrade();
        let dialog_for_save = dialog.clone();

        // Validate the start and duration, save the entry, and refresh the window.
        save.connect_clicked(move |_| {
            let Some(start_ms) = parse_local_timestamp(&start.text()) else {
                if let Some(window) = weak.upgrade() {
                    window.show_database_error(tr(
                        "Start must use YYYY-MM-DD HH:MM:SS and identify one local time.",
                    ));
                }
                return;
            };
            let Some(end_ms) = end_from_duration(
                start_ms,
                duration_hours.value_as_int(),
                duration_minutes.value_as_int(),
            ) else {
                if let Some(window) = weak.upgrade() {
                    window.show_database_error(tr("Duration must be at least one minute."));
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
                intervals: vec![TrackedInterval {
                    id: None,
                    start_ms,
                    end_ms,
                    source: EntrySource::Manual,
                }],
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

        // Build the same entry form with the existing values selected.
        let dialog = adw::Dialog::builder()
            .title(tr("Edit Entry"))
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
        let activity_names = std::iter::once(tr("No activity").to_owned())
            .chain(activities.iter().map(|activity| {
                if activity.archived {
                    trf("{activity} (Archived)", &[("activity", &activity.name)])
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
        // Shared details update the whole entry.
        for (label_text, widget) in [
            (tr("Project"), project.clone().upcast::<gtk::Widget>()),
            (tr("Activity"), activity.clone().upcast()),
            (tr("Note"), note.clone().upcast()),
        ] {
            content.append(
                &gtk::Label::builder()
                    .label(label_text)
                    .halign(gtk::Align::Start)
                    .build(),
            );
            content.append(&widget);
        }
        let mut single_interval_fields = None;
        let mut multi_interval_fields = None;
        if let [interval] = existing.intervals.as_slice() {
            content.append(
                &gtk::Label::builder()
                    .label(tr("Interval"))
                    .halign(gtk::Align::Start)
                    .css_classes(["heading"])
                    .build(),
            );
            let start = gtk::Entry::builder()
                .text(format_time(interval.start_ms))
                .build();
            content.append(
                &gtk::Label::builder()
                    .label(tr("Start (local)"))
                    .halign(gtk::Align::Start)
                    .build(),
            );
            content.append(&start);
            content.append(
                &gtk::Label::builder()
                    .label(tr("Time spent"))
                    .halign(gtk::Align::Start)
                    .build(),
            );
            let (duration, duration_hours, duration_minutes) =
                duration_controls(interval.end_ms.saturating_sub(interval.start_ms));
            content.append(&duration);
            single_interval_fields =
                Some((start, duration_hours, duration_minutes, interval.clone()));
        } else if existing.intervals.len() > 1 {
            content.append(
                &gtk::Label::builder()
                    .label(tr("Time spent"))
                    .halign(gtk::Align::Start)
                    .build(),
            );
            let initial_parts = rounded_duration_parts(existing.duration_ms());
            let (duration, duration_hours, duration_minutes) =
                duration_controls(existing.duration_ms());
            content.append(&duration);
            multi_interval_fields = Some((duration_hours, duration_minutes, initial_parts));
        }
        let save = gtk::Button::with_label(tr("Save Changes"));
        save.add_css_class("suggested-action");
        content.append(&save);
        set_dialog_content(&dialog, &content);
        let dialog_for_save = dialog.clone();
        let weak = self.downgrade();

        // Validate the edited values, update the entry, and refresh the related views.
        save.connect_clicked(move |_| {
            let intervals = if let Some((start, hours, minutes, original)) =
                &single_interval_fields
            {
                parse_local_timestamp(&start.text()).and_then(|start_ms| {
                    Some(TrackedInterval {
                        start_ms,
                        end_ms: end_from_duration(
                            start_ms,
                            hours.value_as_int(),
                            minutes.value_as_int(),
                        )?,
                        ..original.clone()
                    })
                })
                .map(|interval| vec![interval])
            } else if let Some((hours, minutes, initial_parts)) = &multi_interval_fields {
                intervals_from_total_duration(
                    &existing.intervals,
                    *initial_parts,
                    hours.value_as_int(),
                    minutes.value_as_int(),
                )
            } else {
                None
            };
            let Some(intervals) = intervals else {
                if let Some(window) = weak.upgrade() {
                    let message = if multi_interval_fields.is_some() {
                        tr("Total time must leave the final interval longer than zero and fit within the supported time range.")
                    } else {
                        tr("Start must use YYYY-MM-DD HH:MM:SS, and duration must be at least one minute.")
                    };
                    window.show_database_error(message);
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
                intervals,
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

#[cfg(test)]
mod tests {
    use super::{end_from_duration, intervals_from_total_duration, rounded_duration_parts};
    use houra_core::{EntrySource, IntervalId, TrackedInterval};

    fn interval(id: i64, start_ms: i64, end_ms: i64, source: EntrySource) -> TrackedInterval {
        TrackedInterval {
            id: Some(IntervalId(id)),
            start_ms,
            end_ms,
            source,
        }
    }

    #[test]
    fn rounds_existing_durations_to_the_nearest_minute() {
        assert_eq!(rounded_duration_parts(60_000), (0, 1));
        assert_eq!(rounded_duration_parts(89_999), (0, 1));
        assert_eq!(rounded_duration_parts(90_000), (0, 2));
        assert_eq!(rounded_duration_parts(5_430_000), (1, 31));
    }

    #[test]
    fn keeps_positive_sub_minute_durations_editable() {
        assert_eq!(rounded_duration_parts(1), (0, 1));
        assert_eq!(rounded_duration_parts(30_000), (0, 1));
    }

    #[test]
    fn calculates_end_from_elapsed_minutes() {
        assert_eq!(end_from_duration(1_000, 1, 30), Some(5_401_000));
        assert_eq!(end_from_duration(86_399_000, 0, 2), Some(86_519_000));
    }

    #[test]
    fn rejects_invalid_durations_and_overflow() {
        assert_eq!(end_from_duration(0, 0, 0), None);
        assert_eq!(end_from_duration(0, -1, 30), None);
        assert_eq!(end_from_duration(0, 1, 60), None);
        assert_eq!(end_from_duration(i64::MAX, 0, 1), None);
    }

    #[test]
    fn unchanged_multi_interval_total_preserves_exact_intervals() {
        let intervals = vec![
            interval(1, 1_000, 61_123, EntrySource::Timer),
            interval(2, 180_000, 240_456, EntrySource::Recovery),
        ];

        let edited = intervals_from_total_duration(&intervals, (0, 2), 0, 2)
            .unwrap_or_else(|| panic!("unchanged total should be valid"));

        assert_eq!(edited, intervals);
    }

    #[test]
    fn changed_total_adjusts_only_the_chronologically_final_interval() {
        let intervals = vec![
            interval(3, 600_000, 720_000, EntrySource::Recovery),
            interval(1, 0, 60_000, EntrySource::Timer),
            interval(2, 300_000, 360_000, EntrySource::IdleReassignment),
        ];

        let increased = intervals_from_total_duration(&intervals, (0, 4), 0, 5)
            .unwrap_or_else(|| panic!("increased total should be valid"));
        let mut expected_increased = intervals.clone();
        expected_increased[0].end_ms = 780_000;
        assert_eq!(increased, expected_increased);

        let decreased = intervals_from_total_duration(&intervals, (0, 4), 0, 3)
            .unwrap_or_else(|| panic!("decreased total should be valid"));
        let mut expected_decreased = intervals.clone();
        expected_decreased[0].end_ms = 660_000;
        assert_eq!(decreased, expected_decreased);
    }

    #[test]
    fn changed_total_must_exceed_earlier_interval_duration() {
        let intervals = vec![
            interval(1, 0, 120_000, EntrySource::Timer),
            interval(2, 300_000, 360_000, EntrySource::Timer),
        ];

        assert_eq!(
            intervals_from_total_duration(&intervals, (0, 3), 0, 2),
            None
        );
        assert_eq!(
            intervals_from_total_duration(&intervals, (0, 3), 0, 1),
            None
        );
    }

    #[test]
    fn changed_total_rejects_arithmetic_overflow() {
        let intervals = vec![
            interval(1, i64::MIN, 0, EntrySource::Timer),
            interval(2, i64::MAX - 30_000, i64::MAX, EntrySource::Timer),
        ];
        assert_eq!(
            intervals_from_total_duration(&intervals, (0, 1), 0, 2),
            None
        );

        let end_overflow = vec![
            interval(1, 0, 60_000, EntrySource::Timer),
            interval(2, i64::MAX - 30_000, i64::MAX - 1, EntrySource::Timer),
        ];
        assert_eq!(
            intervals_from_total_duration(&end_overflow, (0, 1), 0, 2),
            None
        );
    }
}
