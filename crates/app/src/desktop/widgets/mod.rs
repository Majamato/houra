mod entry_row;
mod management_row;
mod timer_action_button;
mod week_day_cell;

use crate::locale::trf;

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

#[cfg(test)]
mod tests {
    use glib::subclass::types::ObjectSubclassIsExt;
    use gtk::prelude::ButtonExt;
    use std::cell::Cell;
    use std::rc::Rc;

    use super::format_duration;

    #[test]
    fn formats_compact_durations() {
        assert_eq!(format_duration(30), "0m");
        assert_eq!(format_duration(45 * 60), "45m");
        assert_eq!(format_duration(6_300), "1h 45m");
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

        let _week_day = super::WeekDayCell::new(date, 60, true);
        let entry_row = super::EntryRow::new(
            &entry,
            Some(&project),
            Some(&activity),
            0,
            86_400_000,
            0,
            super::EntryTrackingState::Inactive,
        );
        let requested = Rc::new(Cell::new(false));
        let requested_for_signal = requested.clone();
        entry_row.connect_report_requested(move |_| requested_for_signal.set(true));
        entry_row.imp().report_button.emit_clicked();
        assert!(requested.get());
        let _management = super::ManagementRow::new("General", "#3584e4", false, true);
        let _timer: super::TimerActionButton = glib::Object::builder().build();
        let _window: crate::desktop::window::MainWindow = glib::Object::builder().build();
    }
}
