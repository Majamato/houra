use crate::locale::tr;
use gtk::prelude::*;
use houra_core::{TrackerCommand, TrackerState};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::window::MainWindow;

impl MainWindow {
    pub fn confirm_quit(&self) {
        let paused = self
            .handle()
            .and_then(|handle| handle.snapshot().ok())
            .is_some_and(|snapshot| matches!(snapshot.state, TrackerState::Paused(_)));
        let dialog = if paused {
            adw::AlertDialog::builder()
                .heading(tr("A timer is paused"))
                .body(tr("Quit and resume later, or finish the entry now."))
                .build()
        } else {
            adw::AlertDialog::builder()
                .heading(tr("A timer is still running"))
                .body(tr("Finish the timer and quit, or keep Houra open."))
                .build()
        };
        if paused {
            dialog.add_responses(&[("quit", tr("Quit")), ("finish", tr("Finish and Quit"))]);
        } else {
            dialog.add_responses(&[
                ("cancel", tr("Keep Running")),
                ("quit", tr("Finish and Quit")),
            ]);
        }
        if paused {
            dialog.set_response_appearance("quit", adw::ResponseAppearance::Suggested);
            dialog.set_response_appearance("finish", adw::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("quit"));
        } else {
            dialog.set_response_appearance("quit", adw::ResponseAppearance::Destructive);
        }
        let handle = self.handle();
        let application = self.application();
        let window = self.clone();
        let stop_and_quit = move || {
            if let Some(handle) = &handle {
                if let Err(error) = handle.apply(TrackerCommand::Stop) {
                    window.show_database_error(&error.to_string());
                    return;
                }
                if let Some(application) = &application {
                    application.quit();
                }
            }
        };
        if paused {
            let application = self.application();
            dialog.connect_response(Some("quit"), move |_, _| {
                if let Some(application) = &application {
                    application.quit();
                }
            });
            dialog.connect_response(Some("finish"), move |_, _| stop_and_quit());
        } else {
            dialog.connect_response(Some("quit"), move |_, _| stop_and_quit());
        }
        dialog.present(Some(self));
    }
}
