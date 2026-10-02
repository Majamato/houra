use crate::DateFormat;
use crate::date_format::format_date;
use crate::desktop::log_background_error;
use crate::locale::{tr, trf};
use chrono::NaiveDate;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::DurationRounding;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::window::MainWindow;

impl MainWindow {
    pub fn show_preferences(&self) {
        let dialog = adw::PreferencesDialog::new();
        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::builder()
            .title(tr("Idle Detection"))
            .build();
        let threshold = adw::SpinRow::with_range(1.0, 120.0, 1.0);
        threshold.set_title(tr("Idle threshold (minutes)"));
        threshold.set_subtitle(tr("Changes take effect the next time the app starts"));
        let settings = crate::desktop::load_settings();
        let current_threshold = settings
            .as_ref()
            .map_or(5, |settings| settings.uint("idle-threshold-minutes"));
        threshold.set_value(f64::from(current_threshold));

        threshold.connect_value_notify({
            let settings = settings.clone();
            move |row| {
                if let Some(settings) = &settings {
                    let value = row.value().round().clamp(1.0, 120.0) as u32;
                    let _ignored = settings.set_uint("idle-threshold-minutes", value);
                }
            }
        });
        group.add(&threshold);

        let launch = adw::SwitchRow::builder()
            .title(tr("Launch at login"))
            .active(
                settings
                    .as_ref()
                    .is_some_and(|settings| settings.boolean("launch-at-login")),
            )
            .build();
        launch.connect_active_notify({
            let settings = settings.clone();
            move |row| {
                if let Some(settings) = &settings {
                    let _ignored = settings.set_boolean("launch-at-login", row.is_active());
                }
                match std::env::current_exe() {
                    Ok(executable) => {
                        if let Err(error) =
                            crate::autostart::set_enabled(row.is_active(), &executable)
                        {
                            log_background_error("updating autostart", error);
                        }
                    }
                    Err(error) => log_background_error("locating executable", error),
                }
            }
        });
        group.add(&launch);

        let notifications = adw::SwitchRow::builder()
            .title(tr("Notifications"))
            .active(
                settings
                    .as_ref()
                    .is_none_or(|settings| settings.boolean("notifications")),
            )
            .build();
        notifications.connect_active_notify({
            let settings = settings.clone();
            move |row| {
                if let Some(settings) = &settings {
                    let _ignored = settings.set_boolean("notifications", row.is_active());
                }
                if !row.is_active() {
                    crate::desktop::platform::withdraw_idle_notification();
                }
            }
        });
        group.add(&notifications);

        page.add(&group);

        let rounding_group = adw::PreferencesGroup::builder()
            .title(tr("Time Rounding"))
            .build();
        let rounding_names = DurationRounding::ALL.map(rounding_name);
        let rounding = adw::ComboRow::builder()
            .title(tr("Round finished entries"))
            .subtitle(tr(
                "The running timer stays exact; totals round when an entry is finished",
            ))
            .model(&gtk::StringList::new(&rounding_names))
            .build();
        let current_rounding = self.imp().duration_rounding.get();
        rounding.set_selected(
            DurationRounding::ALL
                .iter()
                .position(|mode| *mode == current_rounding)
                .and_then(|index| u32::try_from(index).ok())
                .unwrap_or(0),
        );
        rounding.connect_selected_notify({
            let settings = settings.clone();
            let weak = self.downgrade();
            move |row| {
                let Some(mode) = usize::try_from(row.selected())
                    .ok()
                    .and_then(|index| DurationRounding::ALL.get(index).copied())
                else {
                    return;
                };
                if let Some(settings) = &settings {
                    let _ignored = settings.set_string("duration-rounding", mode.key());
                }
                if let Some(window) = weak.upgrade() {
                    window.imp().duration_rounding.set(mode);
                    window.refresh();
                    window.refresh_report();
                }
            }
        });
        rounding_group.add(&rounding);
        page.add(&rounding_group);

        let date_group = adw::PreferencesGroup::builder()
            .title(tr("Date Display"))
            .build();
        let date_names = DateFormat::ALL.map(date_format_name);
        let date_format = adw::ComboRow::builder()
            .title(tr("Date format"))
            .model(&gtk::StringList::new(&date_names))
            .build();
        let current_format = self.date_format();
        date_format.set_selected(
            DateFormat::ALL
                .iter()
                .position(|format| *format == current_format)
                .and_then(|index| u32::try_from(index).ok())
                .unwrap_or(0),
        );
        date_format.set_subtitle(&date_format_preview(current_format));
        date_format.connect_selected_notify({
            let weak = self.downgrade();
            move |row| {
                let Some(format) = usize::try_from(row.selected())
                    .ok()
                    .and_then(|index| DateFormat::ALL.get(index).copied())
                else {
                    return;
                };
                let Some(window) = weak.upgrade() else {
                    return;
                };
                if let Some(settings) = window.imp().date_settings.borrow().as_ref()
                    && let Err(error) = settings.set_string("date-format", format.key())
                {
                    log_background_error("saving date format", error);
                }
                window.apply_date_format(format);
                row.set_subtitle(&date_format_preview(format));
            }
        });
        date_group.add(&date_format);
        page.add(&date_group);

        dialog.add(&page);
        dialog.present(Some(self));

        // Presentation can assign focus later, during the opening animation.
        // Clear it once, then leave mouse and keyboard focus to the user.
        let initial_focus_cleared = std::cell::Cell::new(false);
        dialog.connect_focus_widget_notify(glib::clone!(
            #[weak]
            threshold,
            move |dialog| {
                if !initial_focus_cleared.replace(true) {
                    // Let GTK finish assigning focus before removing it.
                    glib::idle_add_local_once(glib::clone!(
                        #[weak]
                        dialog,
                        #[weak]
                        threshold,
                        move || {
                            dialog.set_focus(Option::<&gtk::Widget>::None);
                            threshold.select_region(0, 0);
                        }
                    ));
                }
            }
        ));
        dialog.set_focus(Option::<&gtk::Widget>::None);
        threshold.select_region(0, 0);
    }
}

fn rounding_name(mode: DurationRounding) -> &'static str {
    match mode {
        DurationRounding::Up => tr("Round up"),
        DurationRounding::Nearest => tr("Round to nearest minute"),
        DurationRounding::Down => tr("Round down"),
    }
}

fn date_format_name(format: DateFormat) -> &'static str {
    match format {
        DateFormat::System => tr("System default"),
        DateFormat::DayMonthYear => "DD/MM/YYYY",
        DateFormat::MonthDayYear => "MM/DD/YYYY",
        DateFormat::YearMonthDay => "YYYY-MM-DD",
    }
}

fn date_format_preview(format: DateFormat) -> String {
    let example = NaiveDate::from_ymd_opt(2026, 9, 17)
        .map(|date| format_date(date, format))
        .unwrap_or_default();
    trf("Example: {date}", &[("date", &example)])
}
