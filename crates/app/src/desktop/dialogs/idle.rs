use crate::desktop::log_background_error;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::{ProjectId, TrackerCommand};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::window::MainWindow;

impl MainWindow {
    pub(in crate::desktop) fn show_idle_dialog(&self) {
        let dialog = adw::AlertDialog::builder()
            .heading("You were away")
            .body("How should the idle interval be counted?")
            .build();
        dialog.add_responses(&[
            ("keep", "Keep"),
            ("discard", "Discard and Resume"),
            ("reassign", "Reassign and Resume"),
            ("stop", "Stop"),
        ]);
        dialog.set_default_response(Some("discard"));
        let handle = self.handle();
        let weak = self.downgrade();
        dialog.connect_response(None, move |_, response| {
            if response == "reassign" {
                if let Some(window) = weak.upgrade() {
                    window.show_idle_reassign();
                }
                return;
            }
            let decision = match response {
                "keep" => houra_core::IdleDecision::Keep,
                "stop" => houra_core::IdleDecision::Stop,
                _ => houra_core::IdleDecision::DiscardAndResume,
            };
            if let Some(handle) = &handle
                && let Err(error) = handle.apply(TrackerCommand::ResolveIdle(decision))
            {
                log_background_error("resolving idle time", error);
            }
            if let Some(window) = weak.upgrade() {
                window.refresh();
            }
        });
        dialog.present(Some(self));
    }

    fn show_idle_reassign(&self) {
        let Some(handle) = self.handle() else { return };
        let projects = self.imp().projects.borrow().clone();
        let names = projects
            .iter()
            .map(|project| project.name.as_str())
            .collect::<Vec<_>>();
        let dropdown = gtk::DropDown::from_strings(&names);
        let dialog = adw::AlertDialog::builder()
            .heading("Reassign idle interval")
            .body("Choose the project that should receive the time you were away.")
            .build();
        dialog.set_extra_child(Some(&dropdown));
        dialog.add_responses(&[("cancel", "Cancel"), ("reassign", "Reassign")]);
        dialog.set_default_response(Some("reassign"));
        let weak = self.downgrade();
        dialog.connect_response(Some("reassign"), move |_, _| {
            let index = usize::try_from(dropdown.selected()).unwrap_or(0);
            let project_id = projects
                .get(index)
                .map_or(ProjectId(1), |project| project.id);
            let result = handle.apply(TrackerCommand::ResolveIdle(
                houra_core::IdleDecision::ReassignAndResume {
                    project_id,
                    task_id: None,
                    note: "Idle time".into(),
                },
            ));
            if let Some(window) = weak.upgrade() {
                if let Err(error) = result {
                    window.show_database_error(&error.to_string());
                }
                window.refresh();
            }
        });
        dialog.present(Some(self));
    }
}
