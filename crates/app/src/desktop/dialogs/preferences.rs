use crate::desktop::log_background_error;
use crate::locale::tr;
use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::window::MainWindow;

impl MainWindow {
    pub fn show_preferences(&self) {
        // The preferences dialog groups the app's behavior settings in one page.
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

        // Store the idle timeout whenever the spin row changes.
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

        // Toggle starting the app automatically when the user logs in.
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

        // Toggle desktop notifications for tracker events.
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
            }
        });
        group.add(&notifications);

        page.add(&group);
        dialog.add(&page);
        dialog.present(Some(self));
    }
}
