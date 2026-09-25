use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gio::prelude::*;
use gtk::prelude::*;
use libadwaita as adw;
use tracing::{error, warn};

use super::{platform, window::MainWindow};
use crate::{APP_ID, AppError, TrackerService};

/// Runs the GTK application until the main loop exits, then stops the
/// storage thread.
pub fn run(database_path: PathBuf) -> Result<(), AppError> {
    let settings = load_settings();
    crate::locale::initialize()?;
    register_resources()?;
    let application = adw::Application::builder().application_id(APP_ID).build();
    application.add_main_option(
        "background",
        glib::Char(0),
        glib::OptionFlags::HIDDEN,
        glib::OptionArg::None,
        "",
        None,
    );
    application.connect_startup(|application| {
        load_css();
        application.set_accels_for_action("app.toggle-timer", &["<Control>space"]);
        application.set_accels_for_action("app.add-entry", &["<Control>n"]);
        application.set_accels_for_action("app.preferences", &["<Control>comma"]);
        application.set_accels_for_action("app.quit", &["<Control>q"]);
    });
    application
        .register(None::<&gio::Cancellable>)
        .map_err(AppError::DesktopRegistration)?;
    if application.is_remote() {
        application.activate();
        return Ok(());
    }

    let service = TrackerService::start(database_path)?;
    let handle = service.handle.clone();
    synchronize_autostart(&settings);
    let main_window: Rc<RefCell<Option<MainWindow>>> = Rc::new(RefCell::new(None));

    let activate_window = Rc::clone(&main_window);
    let activate_handle = handle.clone();
    application.connect_activate(move |application| {
        if activate_window.borrow().is_none() {
            let window = MainWindow::new(application, activate_handle.clone());
            window.connect_close_request(|window| {
                if let Some(application) = window.application() {
                    application.activate_action("quit", None);
                }
                glib::Propagation::Stop
            });
            activate_window.replace(Some(window));
        }
        if let Some(window) = activate_window.borrow().as_ref() {
            window.present();
        }
    });

    install_actions(&application, &main_window, handle, &settings);
    let _status = application.run();
    service.shutdown()
}

pub(super) fn register_resources() -> Result<(), AppError> {
    let bytes =
        glib::Bytes::from_static(include_bytes!(concat!(env!("OUT_DIR"), "/houra.gresource")));
    let resource = gio::Resource::from_data(&bytes).map_err(|error| {
        AppError::InvalidBackup(format!("could not load application resources: {error}"))
    })?;
    gio::resources_register(&resource);
    Ok(())
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_resource("/io/github/majamato/Houra/style.css");
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn install_actions(
    application: &adw::Application,
    window: &Rc<RefCell<Option<MainWindow>>>,
    handle: crate::TrackerHandle,
    settings: &Option<gio::Settings>,
) {
    let toggle = gio::ActionEntry::builder("toggle-timer")
        .activate({
            let window = Rc::clone(window);
            move |_: &adw::Application, _, _| {
                if let Some(window) = window.borrow().as_ref() {
                    window.present();
                    window.toggle_timer();
                }
            }
        })
        .build();
    let add = gio::ActionEntry::builder("add-entry")
        .activate({
            let window = Rc::clone(window);
            move |_: &adw::Application, _, _| {
                if let Some(window) = window.borrow().as_ref() {
                    window.show_manual_entry();
                }
            }
        })
        .build();
    let preferences = gio::ActionEntry::builder("preferences")
        .activate({
            let window = Rc::clone(window);
            move |_: &adw::Application, _, _| {
                if let Some(window) = window.borrow().as_ref() {
                    window.show_preferences();
                }
            }
        })
        .build();
    let quit = gio::ActionEntry::builder("quit")
        .activate({
            let window = Rc::clone(window);
            let handle = handle.clone();
            move |application: &adw::Application, _, _| match handle.snapshot() {
                Ok(snapshot) if snapshot.state.active().is_some() => {
                    if let Some(window) = window.borrow().as_ref() {
                        window.confirm_quit();
                    }
                }
                Ok(_) => application.quit(),
                Err(error) => {
                    if let Some(window) = window.borrow().as_ref() {
                        window.show_database_error(&error.to_string());
                    }
                }
            }
        })
        .build();
    let backup = gio::ActionEntry::builder("backup")
        .activate({
            let window = Rc::clone(window);
            move |_: &adw::Application, _, _| {
                if let Some(window) = window.borrow().as_ref() {
                    window.backup_data();
                }
            }
        })
        .build();
    let restore = gio::ActionEntry::builder("restore")
        .activate({
            let window = Rc::clone(window);
            move |_: &adw::Application, _, _| {
                if let Some(window) = window.borrow().as_ref() {
                    window.restore_data();
                }
            }
        })
        .build();
    application.add_action_entries([toggle, add, preferences, backup, restore, quit]);

    let idle_threshold = settings
        .as_ref()
        .map_or(5, |settings| settings.uint("idle-threshold-minutes"))
        .clamp(1, 120);
    let notifications = settings
        .as_ref()
        .is_none_or(|settings| settings.boolean("notifications"));
    if let Err(error) = platform::start_integrations(handle, idle_threshold, notifications) {
        warn!(%error, "GNOME idle/session integration is unavailable; manual tracking remains active");
        let message = error.to_string();
        let window = Rc::clone(window);
        glib::idle_add_local_once(move || {
            if let Some(window) = window.borrow().as_ref() {
                window.show_integration_warning(&message);
            }
        });
    }
}

pub(crate) fn log_background_error(context: &'static str, error: impl std::fmt::Display) {
    error!(%error, %context, "background operation failed");
}

pub(crate) fn load_settings() -> Option<gio::Settings> {
    let installed =
        gio::SettingsSchemaSource::default().and_then(|source| source.lookup(crate::APP_ID, true));
    let schema = installed.or_else(|| {
        let directory = std::env::current_exe().ok()?.parent()?.to_path_buf();
        if !directory.join("gschemas.compiled").is_file() {
            return None;
        }
        gio::SettingsSchemaSource::from_directory(&directory, None, false)
            .ok()?
            .lookup(crate::APP_ID, false)
    })?;
    Some(gio::Settings::new_full(
        &schema,
        None::<&gio::SettingsBackend>,
        None,
    ))
}

fn synchronize_autostart(settings: &Option<gio::Settings>) {
    let Some(settings) = settings else { return };
    if settings.boolean("onboarding-complete") {
        if settings.boolean("launch-at-login") {
            match std::env::current_exe() {
                Ok(executable) => {
                    if let Err(error) = crate::autostart::set_enabled(true, &executable) {
                        warn!(%error, "could not refresh autostart launcher");
                    }
                }
                Err(error) => warn!(%error, "could not locate executable for autostart"),
            }
        } else if let Err(error) = crate::autostart::set_enabled(false, std::path::Path::new("")) {
            warn!(%error, "could not disable autostart launcher");
        }
        return;
    }
    let launch_enabled = match std::env::current_exe() {
        Ok(executable) => {
            if let Err(error) = crate::autostart::set_enabled(true, &executable) {
                warn!(%error, "could not enable first-run autostart");
                false
            } else {
                true
            }
        }
        Err(error) => {
            warn!(%error, "could not locate executable for first-run autostart");
            false
        }
    };
    let _ignored = settings.set_boolean("launch-at-login", launch_enabled);
    let _ignored = settings.set_boolean("onboarding-complete", true);
}
