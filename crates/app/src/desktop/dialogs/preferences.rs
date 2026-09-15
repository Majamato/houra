use crate::desktop::log_background_error;
use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::window::MainWindow;

impl MainWindow {
    pub fn show_preferences(&self) {
        let dialog = adw::PreferencesDialog::new();
        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::builder()
            .title("Idle Detection")
            .build();
        let threshold = adw::SpinRow::with_range(1.0, 120.0, 1.0);
        threshold.set_title("Idle threshold (minutes)");
        threshold.set_subtitle("Changes take effect the next time the app starts");
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
            .title("Launch at login")
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
            .title("Notifications")
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
            }
        });
        group.add(&notifications);
        let week_start = adw::SwitchRow::builder()
            .title("Weeks start on Monday")
            .active(
                settings
                    .as_ref()
                    .is_none_or(|settings| settings.boolean("week-starts-monday")),
            )
            .build();
        week_start.connect_active_notify({
            let settings = settings.clone();
            let weak = self.downgrade();
            move |row| {
                if let Some(settings) = &settings {
                    let _ignored = settings.set_boolean("week-starts-monday", row.is_active());
                }
                if let Some(window) = weak.upgrade() {
                    window.refresh_report();
                }
            }
        });
        group.add(&week_start);
        page.add(&group);
        dialog.add(&page);
        dialog.present(Some(self));
    }
}
