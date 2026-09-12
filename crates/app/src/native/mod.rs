mod window;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gio::prelude::*;
use gtk::prelude::*;
use libadwaita as adw;

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

    application.connect_startup(|_| load_css());

    let activate_window = Rc::clone(&main_window);
    application.connect_activate(move |application| {
        if activate_window.borrow().is_none() {
            let window = MainWindow::new(application, handle.clone());
            activate_window.replace(Some(window));
        }
        if let Some(window) = activate_window.borrow().as_ref() {
            window.present();
        }
    });

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
