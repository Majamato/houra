mod window;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gio::prelude::*;
use gtk::prelude::*;
use libadwaita as adw;
use tracing::error;

use crate::{APP_ID, AppError, TrackerService};
use window::MainWindow;

/// Runs the GTK application until the main loop exits, then stops the
/// storage thread.
pub fn run(database_path: PathBuf) -> Result<(), AppError> {
    register_resources()?;
    let service = TrackerService::start(database_path)?;
    let handle = service.handle.clone();
    let application = adw::Application::builder().application_id(APP_ID).build();
    let main_window: Rc<RefCell<Option<MainWindow>>> = Rc::new(RefCell::new(None));

    application.connect_startup(|application| {
        load_css();
        application.set_accels_for_action("app.toggle-timer", &["<Control>space"]);
        application.set_accels_for_action("app.add-entry", &["<Control>n"]);
        application.set_accels_for_action("app.quit", &["<Control>q"]);
    });

    let activate_window = Rc::clone(&main_window);
    let activate_handle = handle.clone();
    application.connect_activate(move |application| {
        if activate_window.borrow().is_none() {
            let window = MainWindow::new(application, activate_handle.clone());
            window.connect_close_request(|window| {
                window.set_visible(false);
                glib::Propagation::Stop
            });
            activate_window.replace(Some(window));
        }
        if let Some(window) = activate_window.borrow().as_ref() {
            window.present();
        }
    });

    install_actions(&application, &main_window, handle);
    let _status = application.run();
    service.shutdown()
}

fn register_resources() -> Result<(), AppError> {
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
    let quit = gio::ActionEntry::builder("quit")
        .activate({
            let window = Rc::clone(window);
            let handle = handle.clone();
            move |application: &adw::Application, _, _| {
                let active = handle
                    .snapshot()
                    .map(|snapshot| snapshot.state.active().is_some())
                    .unwrap_or(false);
                if active {
                    if let Some(window) = window.borrow().as_ref() {
                        window.confirm_quit();
                    }
                } else {
                    application.quit();
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
    application.add_action_entries([toggle, add, backup, restore, quit]);
}

pub(crate) fn log_background_error(context: &'static str, error: impl std::fmt::Display) {
    error!(%error, %context, "background operation failed");
}
