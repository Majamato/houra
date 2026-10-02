use crate::date_format::LocalTimestampFormat;
use crate::locale::{tr, trf};
use chrono::{Local, TimeZone};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::{DurationRounding, EntrySource, ProjectId, TimeEntry, TrackedInterval};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::window::MainWindow;

const MILLIS_PER_MINUTE: i64 = 60_000;
const MAX_DURATION_HOURS: i64 = 999;
const MAX_DURATION_MINUTES: i64 = MAX_DURATION_HOURS * 60 + 59;

/// Hours and minutes the editor starts from: the duration rounded the way
/// the entry list shows it, so both agree.
fn rounded_duration_parts(duration_ms: i64, rounding: DurationRounding) -> (u32, u32) {
    let total_minutes =
        (rounding.round_ms(duration_ms) / MILLIS_PER_MINUTE).clamp(1, MAX_DURATION_MINUTES);
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

/// Rebuilds a single interval from the editor fields. Untouched fields keep
/// their exact values, so saving only shared details never moves the interval.
fn edited_single_interval(
    original: &TrackedInterval,
    (initial_start_text, start_text): (&str, &str),
    initial_parts: (u32, u32),
    hours: i32,
    minutes: i32,
    timestamps: &LocalTimestampFormat,
) -> Option<TrackedInterval> {
    let start_ms = if start_text == initial_start_text {
        original.start_ms
    } else {
        timestamps.parse(start_text)?
    };
    let parts_unchanged = u32::try_from(hours).ok() == Some(initial_parts.0)
        && u32::try_from(minutes).ok() == Some(initial_parts.1);
    let end_ms = if parts_unchanged {
        start_ms.checked_add(original.duration_ms())?
    } else {
        end_from_duration(start_ms, hours, minutes)?
    };
    Some(TrackedInterval {
        start_ms,
        end_ms,
        ..original.clone()
    })
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

fn duration_controls(
    duration_ms: i64,
    rounding: DurationRounding,
) -> (gtk::Box, gtk::SpinButton, gtk::SpinButton) {
    let (hours_value, minutes_value) = rounded_duration_parts(duration_ms, rounding);
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
        let timestamps = LocalTimestampFormat::new(self.date_format());
        let start = gtk::Entry::builder()
            .text(timestamps.format(start_local.timestamp_millis()))
            .placeholder_text(timestamps.example())
            .build();

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
        let (duration, duration_hours, duration_minutes) = duration_controls(
            chrono::Duration::hours(1).num_milliseconds(),
            DurationRounding::default(),
        );
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
        let timestamps_for_save = timestamps.clone();

        save.connect_clicked(move |_| {
            let Some(start_ms) = timestamps_for_save.parse(&start.text()) else {
                if let Some(window) = weak.upgrade() {
                    window.show_database_error(&trf(
                        "Start must look like {example} and identify one local time.",
                        &[("example", &timestamps_for_save.example())],
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
        let rounding = self.imp().duration_rounding.get();
        let timestamps = LocalTimestampFormat::new(self.date_format());
        let projects = self.imp().projects.borrow().clone();
        let activities = handle
            .activities(true)
            .unwrap_or_default()
            .into_iter()
            .filter(|activity| !activity.archived || Some(activity.id) == existing.activity_id)
            .collect::<Vec<_>>();

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
                .text(timestamps.format(interval.start_ms))
                .placeholder_text(timestamps.example())
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
                duration_controls(interval.duration_ms(), rounding);
            content.append(&duration);
            let initial_parts = rounded_duration_parts(interval.duration_ms(), rounding);
            single_interval_fields = Some((
                start,
                duration_hours,
                duration_minutes,
                interval.clone(),
                initial_parts,
            ));
        } else if existing.intervals.len() > 1 {
            content.append(
                &gtk::Label::builder()
                    .label(tr("Time spent"))
                    .halign(gtk::Align::Start)
                    .build(),
            );
            let initial_parts = rounded_duration_parts(existing.duration_ms(), rounding);
            let (duration, duration_hours, duration_minutes) =
                duration_controls(existing.duration_ms(), rounding);
            content.append(&duration);
            multi_interval_fields = Some((duration_hours, duration_minutes, initial_parts));
        }
        let save = gtk::Button::with_label(tr("Save Changes"));
        save.add_css_class("suggested-action");
        content.append(&save);
        set_dialog_content(&dialog, &content);
        let dialog_for_save = dialog.clone();
        let weak = self.downgrade();
        let timestamps_for_save = timestamps.clone();

        save.connect_clicked(move |_| {
            let intervals = if let Some((start, hours, minutes, original, initial_parts)) =
                &single_interval_fields
            {
                edited_single_interval(
                    original,
                    (
                        &timestamps_for_save.format(original.start_ms),
                        &start.text(),
                    ),
                    *initial_parts,
                    hours.value_as_int(),
                    minutes.value_as_int(),
                    &timestamps_for_save,
                )
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
                    if multi_interval_fields.is_some() {
                        window.show_database_error(tr("Total time must leave the final interval longer than zero and fit within the supported time range."));
                    } else {
                        window.show_database_error(&trf(
                            "Start must look like {example}, and duration must be at least one minute.",
                            &[("example", &timestamps_for_save.example())],
                        ));
                    }
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
    use super::{
        LocalTimestampFormat, edited_single_interval, end_from_duration,
        intervals_from_total_duration, rounded_duration_parts,
    };
    use crate::DateFormat;
    use houra_core::{DurationRounding, EntrySource, IntervalId, TrackedInterval};

    fn interval(id: i64, start_ms: i64, end_ms: i64, source: EntrySource) -> TrackedInterval {
        TrackedInterval {
            id: Some(IntervalId(id)),
            start_ms,
            end_ms,
            source,
        }
    }

    #[test]
    fn rounds_existing_durations_by_preference() {
        use DurationRounding::{Down, Nearest, Up};
        for (duration_ms, up, nearest, down) in [
            (60_000, (0, 1), (0, 1), (0, 1)),
            (70_000, (0, 2), (0, 1), (0, 1)),
            (89_999, (0, 2), (0, 1), (0, 1)),
            (90_000, (0, 2), (0, 2), (0, 1)),
            (5_430_000, (1, 31), (1, 31), (1, 30)),
        ] {
            assert_eq!(rounded_duration_parts(duration_ms, Up), up);
            assert_eq!(rounded_duration_parts(duration_ms, Nearest), nearest);
            assert_eq!(rounded_duration_parts(duration_ms, Down), down);
        }
    }

    #[test]
    fn keeps_positive_sub_minute_durations_editable() {
        for rounding in DurationRounding::ALL {
            assert_eq!(rounded_duration_parts(1, rounding), (0, 1));
            assert_eq!(rounded_duration_parts(30_000, rounding), (0, 1));
        }
    }

    #[test]
    fn untouched_single_interval_fields_keep_exact_times() {
        let original = interval(1, 1_500, 71_500, EntrySource::Timer);
        let parts = rounded_duration_parts(original.duration_ms(), DurationRounding::Up);
        assert_eq!(parts, (0, 2));
        let timestamps = LocalTimestampFormat::new(DateFormat::YearMonthDay);
        let text = "2026-01-01 00:00:01";
        assert_eq!(
            edited_single_interval(&original, (text, text), parts, 0, 2, &timestamps),
            Some(original.clone())
        );
        // A new duration replaces the end; the untouched start stays exact.
        assert_eq!(
            edited_single_interval(&original, (text, text), parts, 0, 3, &timestamps),
            Some(interval(1, 1_500, 181_500, EntrySource::Timer))
        );
        // Unparseable edited starts are rejected.
        assert_eq!(
            edited_single_interval(&original, (text, "soon"), parts, 0, 2, &timestamps),
            None
        );
    }

    #[test]
    fn untouched_timestamps_preserve_milliseconds_for_every_format() {
        let original = interval(1, 1_500, 71_500, EntrySource::Timer);
        let parts = rounded_duration_parts(original.duration_ms(), DurationRounding::Up);
        for format in DateFormat::ALL {
            let timestamps = LocalTimestampFormat::new(format);
            let initial = timestamps.format(original.start_ms);
            assert!(!initial.is_empty(), "{format:?}");
            assert_eq!(
                edited_single_interval(&original, (&initial, &initial), parts, 0, 2, &timestamps),
                Some(original.clone()),
                "{format:?}"
            );
        }
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
