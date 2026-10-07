mod entry_row;
mod management_row;
mod timer_action_button;
mod week_day_cell;

use std::time::Duration;

use crate::locale::{tr, trf};

pub(super) use entry_row::{EntryRow, EntryTrackingState};
pub(super) use management_row::ManagementRow;
pub(super) use timer_action_button::TimerActionButton;
pub(super) use week_day_cell::WeekDayCell;

pub(super) fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds / 60) % 60;
    if hours == 0 {
        trf("{minutes}m", &[("minutes", &minutes.to_string())])
    } else {
        trf(
            "{hours}h {minutes}m",
            &[
                ("hours", &hours.to_string()),
                ("minutes", &format!("{minutes:02}")),
            ],
        )
    }
}

/// Pango markup for the big clock: numerals at full size, unit letters
/// small and quiet on the same baseline. Without seconds it reads the same
/// as [`format_duration`].
pub(super) fn format_clock_markup(seconds: u64, show_seconds: bool) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds / 60) % 60;
    let hours_text = hours.to_string();
    let seconds_text = format!("{:02}", seconds % 60);
    let (template, minutes_text) = match (hours, show_seconds) {
        (0, false) => (tr("{minutes}m"), minutes.to_string()),
        (0, true) => (tr("{minutes}m {seconds}s"), minutes.to_string()),
        (_, false) => (tr("{hours}h {minutes}m"), format!("{minutes:02}")),
        (_, true) => (
            tr("{hours}h {minutes}m {seconds}s"),
            format!("{minutes:02}"),
        ),
    };
    duration_markup(
        template,
        &[
            ("hours", &hours_text),
            ("minutes", &minutes_text),
            ("seconds", &seconds_text),
        ],
    )
}

/// Live time counted toward the entry total: the run total minus already
/// banked pause segments, which the stored entry duration includes.
pub(super) fn counted_live_elapsed(elapsed: Duration, accumulated_ms: i64) -> Duration {
    elapsed.saturating_sub(Duration::from_millis(
        u64::try_from(accumulated_ms.max(0)).unwrap_or(u64::MAX),
    ))
}

/// The active time entry's total: its saved duration plus live time not yet
/// banked into it.
pub(super) fn active_entry_total_duration(
    saved_duration_ms: i64,
    live_duration: Duration,
) -> Duration {
    let saved_ms = u64::try_from(saved_duration_ms.max(0)).unwrap_or(0);
    Duration::from_millis(saved_ms).saturating_add(live_duration)
}

const UNIT_OPEN: &str = r#"<span size="42%" alpha="55%">"#;

/// Wraps the literal text of a translated duration template as units. Space
/// that leads into the next number stays full size, so groups read apart
/// ("1h 30m") while a space before a unit ("1 h") stays tight.
fn duration_markup(template: &str, values: &[(&str, &str)]) -> String {
    let mut markup = String::new();
    let mut rest = template;
    while !rest.is_empty() {
        let placeholder = rest.find('{').and_then(|open| {
            let close = open + rest[open..].find('}')?;
            let key = &rest[open + 1..close];
            let value = values.iter().find(|(name, _)| *name == key)?.1;
            Some((open, close, value))
        });
        let literal_end = placeholder.map_or(rest.len(), |(open, _, _)| open);
        let literal = &rest[..literal_end];
        let unit = if placeholder.is_some() {
            literal.trim_end()
        } else {
            literal
        };
        if !unit.trim().is_empty() {
            markup.push_str(UNIT_OPEN);
            markup.push_str(&glib::markup_escape_text(unit));
            markup.push_str("</span>");
        } else {
            markup.push_str(&glib::markup_escape_text(unit));
        }
        markup.push_str(&glib::markup_escape_text(&literal[unit.len()..]));
        let Some((_, close, value)) = placeholder else {
            break;
        };
        markup.push_str(&glib::markup_escape_text(value));
        rest = &rest[close + 1..];
    }
    markup
}

#[cfg(test)]
mod tests {
    use glib::subclass::types::ObjectSubclassIsExt;
    use gtk::prelude::{ButtonExt, OrientableExt, WidgetExt};
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    use super::{
        UNIT_OPEN, active_entry_total_duration, counted_live_elapsed, duration_markup,
        format_clock_markup, format_duration,
    };

    #[test]
    fn formats_compact_durations() {
        assert_eq!(format_duration(30), "0m");
        assert_eq!(format_duration(45 * 60), "45m");
        assert_eq!(format_duration(6_300), "1h 45m");
    }

    fn clock_text(seconds: u64, show_seconds: bool) -> String {
        let markup = format_clock_markup(seconds, show_seconds);
        let Ok((_, text, _)) = gtk::pango::parse_markup(&markup, '\0') else {
            panic!("invalid clock markup {markup:?}");
        };
        text.to_string()
    }

    #[test]
    fn clock_markup_without_seconds_keeps_the_duration_text() {
        for seconds in [0, 59, 60, 45 * 60, 3_599, 3_600, 5_025, 360_000] {
            assert_eq!(
                clock_text(seconds, false),
                format_duration(seconds),
                "{seconds} s"
            );
        }
    }

    #[test]
    fn clock_markup_with_seconds_pads_minutes_and_seconds() {
        for (seconds, expected) in [
            (0, "0m 00s"),
            (5, "0m 05s"),
            (59, "0m 59s"),
            (754, "12m 34s"),
            (3_599, "59m 59s"),
            (3_600, "1h 00m 00s"),
            (3_909, "1h 05m 09s"),
            (360_000, "100h 00m 00s"),
        ] {
            assert_eq!(clock_text(seconds, true), expected, "{seconds} s");
        }
    }

    #[test]
    fn clock_markup_keeps_units_small() {
        assert_eq!(
            format_clock_markup(45 * 60, false),
            format!("45{UNIT_OPEN}m</span>")
        );
        assert_eq!(
            format_clock_markup(5_025, false),
            format!("1{UNIT_OPEN}h</span> 23{UNIT_OPEN}m</span>")
        );
        assert_eq!(
            format_clock_markup(5_025, true),
            format!("1{UNIT_OPEN}h</span> 23{UNIT_OPEN}m</span> 45{UNIT_OPEN}s</span>")
        );
    }

    #[test]
    fn clock_markup_follows_translated_units() {
        assert_eq!(
            duration_markup(
                "{hours} h {minutes} min",
                &[("hours", "1"), ("minutes", "05")]
            ),
            format!("1{UNIT_OPEN} h</span> 05{UNIT_OPEN} min</span>")
        );
        assert_eq!(
            duration_markup(
                "{hours}時間{minutes}分",
                &[("hours", "1"), ("minutes", "05")]
            ),
            format!("1{UNIT_OPEN}時間</span>05{UNIT_OPEN}分</span>")
        );
        assert_eq!(
            duration_markup("{minutes} m&s", &[("minutes", "7")]),
            format!("7{UNIT_OPEN} m&amp;s</span>")
        );
    }

    #[test]
    fn counted_live_excludes_banked_pause_segments() {
        assert_eq!(
            counted_live_elapsed(Duration::from_secs(90), 60_000),
            Duration::from_secs(30)
        );
        assert_eq!(
            counted_live_elapsed(Duration::from_secs(90), 0),
            Duration::from_secs(90)
        );
        assert_eq!(
            counted_live_elapsed(Duration::from_secs(30), 60_000),
            Duration::ZERO
        );
        assert_eq!(
            counted_live_elapsed(Duration::from_secs(30), -5),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn active_entry_total_combines_saved_intervals_and_live_time() {
        assert_eq!(
            active_entry_total_duration(0, Duration::from_secs(61)),
            Duration::from_secs(61)
        );
        assert_eq!(
            active_entry_total_duration(45 * 60 * 1_000, Duration::from_secs(5 * 60)),
            Duration::from_secs(50 * 60)
        );
        let counted = counted_live_elapsed(Duration::from_secs(7 * 60), 5 * 60 * 1_000);
        assert_eq!(
            active_entry_total_duration(50 * 60 * 1_000, counted),
            Duration::from_secs(52 * 60)
        );
        assert_eq!(
            active_entry_total_duration(52 * 60 * 1_000, Duration::ZERO),
            Duration::from_secs(52 * 60)
        );
        assert_eq!(
            active_entry_total_duration(26 * 3_600 * 1_000, Duration::from_secs(60)),
            Duration::from_secs(26 * 3_600 + 60)
        );
    }

    #[test]
    fn templates_construct_when_a_display_is_available() {
        if libadwaita::init().is_err() {
            return;
        }
        if let Err(error) = crate::desktop::application::register_resources() {
            panic!("could not register test resources: {error}");
        }

        let Some(date) = chrono::NaiveDate::from_ymd_opt(2026, 9, 17) else {
            panic!("test date should be valid");
        };
        let project = houra_core::Project {
            id: houra_core::ProjectId(1),
            name: "General".into(),
            color: "#3584e4".into(),
            archived: false,
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        let activity = houra_core::Activity {
            id: houra_core::ActivityId(1),
            name: "Programming".into(),
            archived: false,
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        let entry = houra_core::TimeEntry {
            id: None,
            project_id: project.id,
            activity_id: Some(activity.id),
            note: "Tracked work".into(),
            intervals: vec![houra_core::TrackedInterval {
                id: None,
                start_ms: 0,
                end_ms: 60_000,
                source: houra_core::EntrySource::Timer,
            }],
            created_at_ms: 0,
            updated_at_ms: 0,
        };

        let _week_day = super::WeekDayCell::new(date, 60, true, crate::DateFormat::YearMonthDay);
        let entry_row = super::EntryRow::new(
            &entry,
            Some(&project),
            Some(&activity),
            (0, 86_400_000),
            &houra_core::EntryTotals::default(),
            0,
            super::EntryTrackingState::Inactive,
        );
        let requested = Rc::new(Cell::new(false));
        let requested_for_signal = requested.clone();
        entry_row.connect_report_requested(move |_| requested_for_signal.set(true));
        entry_row.imp().report_button.emit_clicked();
        assert!(requested.get());
        assert!(!entry_row.imp().total_duration.is_visible());
        for (state, status) in [
            (super::EntryTrackingState::Inactive, None),
            (
                super::EntryTrackingState::Tracking,
                Some(crate::locale::tr("Currently tracking")),
            ),
            (
                super::EntryTrackingState::Paused,
                Some(crate::locale::tr("Paused")),
            ),
            (
                super::EntryTrackingState::ReviewRequired,
                Some(crate::locale::tr("Review required")),
            ),
        ] {
            let row = super::EntryRow::new(
                &entry,
                Some(&project),
                Some(&activity),
                (0, 86_400_000),
                &houra_core::EntryTotals::default(),
                60_000,
                state,
            );
            assert_eq!(row.imp().duration.is_visible(), status.is_none());
            assert_eq!(row.imp().duration.label(), super::format_duration(120));
            assert!(!row.imp().total_duration.is_visible());
            assert_eq!(row.imp().tracking_status.is_visible(), status.is_some());
            if let Some(status) = status {
                assert_eq!(row.imp().tracking_status.label(), status);
            }
            assert!(row.imp().report_button.is_visible());
            assert_eq!(row.imp().delete_button.is_visible(), status.is_none());
            assert_eq!(row.imp().continue_button.is_visible(), status.is_none());
            let requested = Rc::new(Cell::new(false));
            let requested_for_signal = requested.clone();
            row.connect_report_requested(move |_| requested_for_signal.set(true));
            row.imp().report_button.emit_clicked();
            assert!(requested.get());
        }
        let multi_day = houra_core::TimeEntry {
            id: None,
            project_id: project.id,
            activity_id: Some(activity.id),
            note: "Tracked work".into(),
            intervals: vec![
                houra_core::TrackedInterval {
                    id: None,
                    start_ms: 0,
                    end_ms: 3_600_000,
                    source: houra_core::EntrySource::Timer,
                },
                houra_core::TrackedInterval {
                    id: None,
                    start_ms: 86_400_000,
                    end_ms: 88_200_000,
                    source: houra_core::EntrySource::Timer,
                },
            ],
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        let day_row = super::EntryRow::new(
            &multi_day,
            Some(&project),
            Some(&activity),
            (0, 86_400_000),
            &houra_core::EntryTotals::default(),
            0,
            super::EntryTrackingState::Inactive,
        );
        assert_eq!(
            day_row.imp().duration.label(),
            super::format_duration(3_600)
        );
        assert!(day_row.imp().duration.is_visible());
        let total_text = super::format_duration(5_400);
        assert_eq!(
            day_row.imp().total_duration.label(),
            crate::locale::trf("Total {duration}", &[("duration", total_text.as_str())])
        );
        assert!(day_row.imp().total_duration.is_visible());
        let live_row = super::EntryRow::new(
            &multi_day,
            Some(&project),
            Some(&activity),
            (0, 86_400_000),
            &houra_core::EntryTotals::default(),
            60_000,
            super::EntryTrackingState::Inactive,
        );
        assert_eq!(
            live_row.imp().duration.label(),
            super::format_duration(3_660)
        );
        let live_total_text = super::format_duration(5_460);
        assert_eq!(
            live_row.imp().total_duration.label(),
            crate::locale::trf(
                "Total {duration}",
                &[("duration", live_total_text.as_str())]
            )
        );
        let tracking_row = super::EntryRow::new(
            &multi_day,
            Some(&project),
            Some(&activity),
            (0, 86_400_000),
            &houra_core::EntryTotals::default(),
            0,
            super::EntryTrackingState::Tracking,
        );
        assert!(!tracking_row.imp().total_duration.is_visible());
        let split_seconds = houra_core::TimeEntry {
            intervals: vec![
                houra_core::TrackedInterval {
                    id: None,
                    start_ms: 0,
                    end_ms: 61_000,
                    source: houra_core::EntrySource::Timer,
                },
                houra_core::TrackedInterval {
                    id: None,
                    start_ms: 86_400_000,
                    end_ms: 86_461_000,
                    source: houra_core::EntrySource::Timer,
                },
            ],
            ..multi_day.clone()
        };
        let rounded_row = super::EntryRow::new(
            &split_seconds,
            Some(&project),
            Some(&activity),
            (0, 86_400_000),
            &houra_core::EntryTotals::default(),
            0,
            super::EntryTrackingState::Inactive,
        );
        assert_eq!(
            rounded_row.imp().duration.label(),
            super::format_duration(120)
        );
        let rounded_total_text = super::format_duration(180);
        assert_eq!(
            rounded_row.imp().total_duration.label(),
            crate::locale::trf(
                "Total {duration}",
                &[("duration", rounded_total_text.as_str())]
            )
        );
        assert!(
            entry_row
                .imp()
                .actions_separator
                .has_css_class("entry-actions-separator")
        );
        assert_eq!(
            entry_row.imp().actions_separator.orientation(),
            gtk::Orientation::Vertical
        );
        assert_eq!(
            entry_row.imp().continue_button.icon_name().as_deref(),
            Some("media-playback-start-symbolic")
        );
        assert_eq!(
            entry_row.imp().delete_button.icon_name().as_deref(),
            Some("user-trash-symbolic")
        );
        let delete_requested = Rc::new(Cell::new(false));
        let delete_for_signal = delete_requested.clone();
        entry_row.connect_delete_requested(move |_| delete_for_signal.set(true));
        entry_row.imp().delete_button.emit_clicked();
        assert!(delete_requested.get());
        let _management = super::ManagementRow::new("General", "#3584e4", false, true);
        let _timer: super::TimerActionButton = glib::Object::builder().build();
        let _window: crate::desktop::window::MainWindow = glib::Object::builder().build();
    }
}
