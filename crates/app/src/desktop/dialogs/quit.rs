use crate::locale::tr;
use gtk::prelude::*;
use houra_core::TrackerCommand;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::window::MainWindow;

impl MainWindow {
    pub fn confirm_quit(&self) {
        let dialog = adw::AlertDialog::builder()
            .heading(tr("A timer is still running"))
            .body(tr("Stop the timer and quit, or keep Houra open."))
            .build();
        dialog.add_responses(&[
            ("cancel", tr("Keep Running")),
            ("quit", tr("Stop and Quit")),
        ]);
        dialog.set_response_appearance("quit", adw::ResponseAppearance::Destructive);
        let handle = self.handle();
        let application = self.application();
        let window = self.clone();
        dialog.connect_response(Some("quit"), move |_, _| {
            if let Some(handle) = &handle {
                if let Err(error) = handle.apply(TrackerCommand::Stop) {
                    window.show_database_error(&error.to_string());
                    return;
                }
                if let Some(application) = &application {
                    application.quit();
                }
            }
        });
        dialog.present(Some(self));
    }
}
