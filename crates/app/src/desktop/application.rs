use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gio::prelude::*;
use gtk::prelude::*;
use libadwaita as adw;
use tracing::{error, warn};

use super::top_bar::{self, TopBar};
use super::{platform, window::MainWindow};
use crate::locale::tr;
use crate::{APP_ID, AppError, TrackerService};

thread_local! {
    /// Runs on the main thread after the storage thread reports a change.
    static ON_TRACKER_CHANGE: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
}

/// Storage-thread observer: hands the change to the main thread.
fn tracker_changed() {
    glib::MainContext::default().invoke(|| {
        ON_TRACKER_CHANGE.with(|callback| {
            if let Some(callback) = callback.borrow().as_ref() {
                callback();
            }
        });
    });
}

/// Runs the GTK application until the main loop exits, then stops the
/// storage thread.
pub fn run(database_path: PathBuf) -> Result<(), AppError> {
    let settings = load_settings();
    crate::locale::initialize()?;
    register_resources()?;
    let application = adw::Application::builder().application_id(APP_ID).build();
    // AdwApplication loads `style.css` from the resource base path, which
    // GApplication derives from the application ID.
    application.connect_startup(|application| {
        application.set_accels_for_action("app.toggle-timer", &["<Control>space"]);
        application.set_accels_for_action("app.finish-timer", &["<Control>s"]);
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

    let service = TrackerService::start(database_path, Box::new(tracker_changed))?;
    let handle = service.handle.clone();
    synchronize_autostart(&settings);
    let top_bar = match TopBar::register(&application, handle.clone()) {
        Ok(top_bar) => Some(top_bar),
        Err(error) => {
            warn!(%error, "the top bar cannot show Houra's active timer");
            None
        }
    };
    top_bar::enable_shell_extension();
    let main_window: Rc<RefCell<Option<MainWindow>>> = Rc::new(RefCell::new(None));

    let activate_window = Rc::clone(&main_window);
    let activate_handle = handle.clone();
    let activate_top_bar = top_bar.clone();
    application.connect_activate(move |application| {
        if activate_window.borrow().is_none() {
            let window = MainWindow::new(application, activate_handle.clone());
            if let Some(top_bar) = &activate_top_bar {
                window.set_top_bar(top_bar.clone());
            }
            // Weak: the thread-local lives until the process exits.
            let weak = window.downgrade();
            ON_TRACKER_CHANGE.with(|callback| {
                callback.replace(Some(Box::new(move || {
                    if let Some(window) = weak.upgrade() {
                        window.follow_tracker_change();
                    }
                })));
            });
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

    let integrations = install_actions(&application, &main_window, handle, &settings);
    let _status = application.run();
    drop(integrations);
    drop(top_bar);
    service.shutdown()
}

pub(super) fn register_resources() -> Result<(), AppError> {
    let bytes =
        glib::Bytes::from_static(include_bytes!(concat!(env!("OUT_DIR"), "/houra.gresource")));
    let resource =
        gio::Resource::from_data(&bytes).map_err(|error| AppError::Resources(error.to_string()))?;
    gio::resources_register(&resource);
    Ok(())
}

fn install_actions(
    application: &adw::Application,
    window: &Rc<RefCell<Option<MainWindow>>>,
    handle: crate::TrackerHandle,
    settings: &Option<gio::Settings>,
) -> platform::Integrations {
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
    // For the top bar: unlike `toggle-timer`, never presents the window or starts a timer.
    let toggle_pause = gio::ActionEntry::builder("toggle-pause")
        .activate({
            let window = Rc::clone(window);
            move |_: &adw::Application, _, _| {
                if let Some(window) = window.borrow().as_ref() {
                    window.pause_or_resume();
                }
            }
        })
        .build();
    let review_pending = gio::ActionEntry::builder("review-pending")
        .activate({
            let window = Rc::clone(window);
            move |_: &adw::Application, _, _| {
                if let Some(window) = window.borrow().as_ref() {
                    window.review_pending();
                }
            }
        })
        .build();
    let finish = gio::ActionEntry::builder("finish-timer")
        .activate({
            let window = Rc::clone(window);
            move |_: &adw::Application, _, _| {
                if let Some(window) = window.borrow().as_ref() {
                    window.finish_timer();
                }
            }
        })
        .build();
    let review_idle = gio::ActionEntry::builder("review-idle")
        .activate({
            let window = Rc::clone(window);
            move |application: &adw::Application, _, _| {
                application.activate();
                if let Some(window) = window.borrow().as_ref() {
                    window.review_idle();
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
    application.add_action_entries([
        toggle,
        toggle_pause,
        review_pending,
        finish,
        review_idle,
        add,
        preferences,
        backup,
        restore,
        quit,
    ]);

    let idle_threshold = settings
        .as_ref()
        .map_or(5, |settings| settings.uint("idle-threshold-minutes"))
        .clamp(1, 120);
    let notifications = settings
        .as_ref()
        .is_none_or(|settings| settings.boolean("notifications"));
    let (integrations, connected) =
        platform::start_integrations(handle, idle_threshold, notifications);
    if let Err(error) = connected {
        warn!(%error, "GNOME idle/session integration is unavailable; manual tracking remains active");
        let window = Rc::clone(window);
        glib::idle_add_local_once(move || {
            if let Some(window) = window.borrow().as_ref() {
                window.show_integration_warning(tr(
                    "Automatic idle detection needs GNOME. Timers still work.",
                ));
            }
        });
    }
    integrations
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
