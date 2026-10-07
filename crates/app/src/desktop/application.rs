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
    #[cfg(feature = "dev-app")]
    if settings.is_none() {
        return Err(AppError::DevSettingsUnavailable);
    }
    crate::locale::initialize()?;
    register_resources()?;
    let application = adw::Application::builder()
        .application_id(APP_ID)
        .resource_base_path(crate::identity::RESOURCE_BASE_PATH)
        .build();
    // AdwApplication loads `style.css` from the resource base path, which
    // GApplication otherwise derives from the application ID.
    application.connect_startup(|application| {
        gtk::Window::set_default_icon_name(APP_ID);
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
    #[cfg(feature = "dev-app")]
    {
        load_dev_settings()
    }
    #[cfg(not(feature = "dev-app"))]
    {
        let installed = gio::SettingsSchemaSource::default()
            .and_then(|source| source.lookup(crate::APP_ID, true));
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
}

/// Loads the development schema from beside the executable, then from the
/// build-generated schema directory. Requests the exact dev schema ID and
/// never falls back to the production schema.
#[cfg(feature = "dev-app")]
fn load_dev_settings() -> Option<gio::Settings> {
    let schema = DEV_SCHEMA.with(|cached| {
        cached
            .get_or_init(|| {
                let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
                dev_schema_in(&[
                    exe_dir,
                    std::path::PathBuf::from(env!("HOURA_DEV_SCHEMA_DIR")),
                ])
            })
            .clone()
    })?;
    Some(gio::Settings::new_full(
        &schema,
        None::<&gio::SettingsBackend>,
        None,
    ))
}

#[cfg(feature = "dev-app")]
thread_local! {
    /// The resolved dev schema, looked up once per process. Settings are only
    /// used on the GTK main thread.
    static DEV_SCHEMA: std::cell::OnceCell<Option<gio::SettingsSchema>> =
        const { std::cell::OnceCell::new() };
}

/// Finds the dev schema in the first directory that compiles it, without
/// ever falling back to the production schema.
#[cfg(feature = "dev-app")]
fn dev_schema_in(directories: &[std::path::PathBuf]) -> Option<gio::SettingsSchema> {
    directories.iter().find_map(|directory| {
        if !directory.join("gschemas.compiled").is_file() {
            return None;
        }
        gio::SettingsSchemaSource::from_directory(directory, None, false)
            .ok()?
            .lookup(crate::APP_ID, false)
    })
}

fn synchronize_autostart(settings: &Option<gio::Settings>) {
    synchronize_autostart_inner(settings, &mut |enabled, executable| {
        crate::autostart::set_enabled(enabled, executable)
    })
}

fn synchronize_autostart_inner(
    settings: &Option<gio::Settings>,
    set_enabled: &mut dyn FnMut(bool, &std::path::Path) -> Result<(), AppError>,
) {
    let Some(settings) = settings else { return };
    let onboarding_complete = settings.boolean("onboarding-complete");
    let enable = desired_launch_at_login(onboarding_complete, settings.boolean("launch-at-login"));
    let enabled = if enable {
        match std::env::current_exe() {
            Ok(executable) => match set_enabled(true, &executable) {
                Ok(()) => true,
                Err(error) => {
                    warn!(%error, "could not write autostart launcher");
                    false
                }
            },
            Err(error) => {
                warn!(%error, "could not locate executable for autostart");
                false
            }
        }
    } else {
        if let Err(error) = set_enabled(false, std::path::Path::new("")) {
            warn!(%error, "could not remove autostart launcher");
        }
        false
    };
    if !onboarding_complete {
        let _ignored = settings.set_boolean("launch-at-login", enabled);
        let _ignored = settings.set_boolean("onboarding-complete", true);
    }
}

/// Whether setup should enable the autostart launcher: the stored preference
/// once onboarding completed, else the variant default. The executable path
/// is resolved only when an enabled launcher needs to be written.
fn desired_launch_at_login(onboarding_complete: bool, stored_launch_at_login: bool) -> bool {
    if onboarding_complete {
        stored_launch_at_login
    } else {
        crate::identity::DEFAULT_LAUNCH_AT_LOGIN
    }
}

#[cfg(test)]
mod tests {
    use gio::prelude::SettingsExt as _;

    use super::desired_launch_at_login;
    use crate::identity::DEFAULT_LAUNCH_AT_LOGIN;

    #[test]
    fn first_run_uses_the_variant_default() {
        for stored in [false, true] {
            assert_eq!(
                desired_launch_at_login(false, stored),
                DEFAULT_LAUNCH_AT_LOGIN
            );
        }
        #[cfg(not(feature = "dev-app"))]
        assert!(desired_launch_at_login(false, false));
        #[cfg(feature = "dev-app")]
        assert!(!desired_launch_at_login(false, true));
    }

    #[test]
    fn completed_onboarding_honors_the_stored_preference() {
        for stored in [false, true] {
            assert_eq!(desired_launch_at_login(true, stored), stored);
        }
    }

    /// Compiles the production schema alone into a directory.
    fn compile_stable_schema_into(directory: &std::path::Path) {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/io.github.majamato.Houra.gschema.xml");
        std::fs::copy(
            &source,
            directory.join("io.github.majamato.Houra.gschema.xml"),
        )
        .unwrap_or_else(|error| panic!("the stable schema should copy: {error}"));
        let status = std::process::Command::new("glib-compile-schemas")
            .arg("--strict")
            .arg(format!("--targetdir={}", directory.display()))
            .arg(directory)
            .status()
            .unwrap_or_else(|error| panic!("schema should compile: {error}"));
        assert!(status.success(), "schema should compile");
    }

    /// Opens this variant's schema from a compiled directory on a memory
    /// backend, so tests never alter real desktop preferences.
    fn settings_in(schema_dir: &std::path::Path) -> gio::Settings {
        let source = gio::SettingsSchemaSource::from_directory(schema_dir, None, false)
            .unwrap_or_else(|error| panic!("compiled schema should load: {error}"));
        let schema = source
            .lookup(crate::APP_ID, false)
            .unwrap_or_else(|| panic!("{} should exist", crate::APP_ID));
        let backend = gio::memory_settings_backend_new();
        gio::Settings::new_full(&schema, Some(&backend), None)
    }

    #[test]
    fn first_run_marks_onboarding_complete_with_the_variant_default() {
        #[cfg(not(feature = "dev-app"))]
        let schema_tempdir = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("test schema dir should build: {error}"));
        #[cfg(not(feature = "dev-app"))]
        compile_stable_schema_into(schema_tempdir.path());
        #[cfg(feature = "dev-app")]
        let schema_dir = std::path::Path::new(env!("HOURA_DEV_SCHEMA_DIR"));
        #[cfg(not(feature = "dev-app"))]
        let schema_dir = schema_tempdir.path();

        let settings = settings_in(schema_dir);
        assert!(!settings.boolean("onboarding-complete"));
        let autostart_dir = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("test autostart dir should build: {error}"));
        let launcher = autostart_dir.path().join("autostart").join("test.desktop");
        let mut calls = Vec::new();
        super::synchronize_autostart_inner(&Some(settings.clone()), &mut |enabled, executable| {
            calls.push(enabled);
            crate::autostart::set_enabled_at(&launcher, enabled, executable)
        });
        assert!(settings.boolean("onboarding-complete"));
        assert_eq!(settings.boolean("launch-at-login"), DEFAULT_LAUNCH_AT_LOGIN);
        assert_eq!(calls, vec![DEFAULT_LAUNCH_AT_LOGIN]);
        assert_eq!(launcher.is_file(), DEFAULT_LAUNCH_AT_LOGIN);
    }

    #[cfg(feature = "dev-app")]
    #[test]
    fn dev_schema_lookup_ignores_a_stable_only_directory() {
        let stable_dir = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("test schema dir should build: {error}"));
        compile_stable_schema_into(stable_dir.path());
        let stable_only = vec![stable_dir.path().to_path_buf()];
        assert!(
            super::dev_schema_in(&stable_only).is_none(),
            "a directory with only the stable schema must not resolve the dev schema"
        );
    }

    #[cfg(feature = "dev-app")]
    #[test]
    fn dev_schema_lookup_finds_the_dev_schema_after_a_stable_directory() {
        let stable_dir = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("test schema dir should build: {error}"));
        compile_stable_schema_into(stable_dir.path());
        let directories = vec![
            stable_dir.path().to_path_buf(),
            std::path::PathBuf::from(env!("HOURA_DEV_SCHEMA_DIR")),
        ];
        let schema = super::dev_schema_in(&directories)
            .unwrap_or_else(|| panic!("the dev schema should resolve after the stable directory"));
        assert_eq!(schema.id().as_str(), crate::APP_ID);
        assert_eq!(
            schema.path().as_deref(),
            Some(crate::identity::SETTINGS_PATH)
        );
    }
}
