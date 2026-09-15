use crate::TrackerHandle;
use gtk::prelude::*;
use houra_core::ProjectId;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::window::MainWindow;

impl MainWindow {
    pub(in crate::desktop) fn show_new_project(&self) {
        self.show_name_dialog("New Project", move |handle, name, now| {
            handle
                .create_project(name, "#3584e4".into(), now)
                .map(|_| ())
        });
    }

    pub(in crate::desktop) fn show_new_task(&self, project_id: ProjectId) {
        self.show_name_dialog("New Task", move |handle, name, now| {
            handle.create_task(project_id, name, now).map(|_| ())
        });
    }

    fn show_name_dialog<F>(&self, title: &str, save: F)
    where
        F: Fn(&TrackerHandle, String, i64) -> Result<(), crate::AppError> + 'static,
    {
        let Some(handle) = self.handle() else { return };
        let dialog = adw::AlertDialog::builder().heading(title).build();
        let entry = gtk::Entry::builder()
            .placeholder_text("Name")
            .activates_default(true)
            .build();
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", "Cancel"), ("save", "Save")]);
        dialog.set_default_response(Some("save"));
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        let weak = self.downgrade();
        dialog.connect_response(Some("save"), move |_, _| {
            let result = save(
                &handle,
                entry.text().to_string(),
                chrono::Utc::now().timestamp_millis(),
            );
            if let Some(window) = weak.upgrade() {
                if let Err(error) = result {
                    window.show_database_error(&error.to_string());
                }
                window.reload_projects();
                window.refresh_projects_page();
            }
        });
        dialog.present(Some(self));
    }
}
