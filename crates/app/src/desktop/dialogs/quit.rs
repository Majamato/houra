use gtk::prelude::*;
use houra_core::TrackerCommand;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::window::MainWindow;

impl MainWindow {
    pub fn confirm_quit(&self) {
        let dialog = adw::AlertDialog::builder()
            .heading("A timer is still running")
            .body("Stop the timer and quit, or keep Houra running in the background.")
            .build();
        dialog.add_responses(&[("cancel", "Keep Running"), ("quit", "Stop and Quit")]);
        dialog.set_response_appearance("quit", adw::ResponseAppearance::Destructive);
        let handle = self.handle();
        let application = self.application();
        dialog.connect_response(Some("quit"), move |_, _| {
            if let Some(handle) = &handle {
                let _ignored = handle.apply(TrackerCommand::Stop);
            }
            if let Some(application) = &application {
                application.quit();
            }
        });
        dialog.present(Some(self));
    }
}
