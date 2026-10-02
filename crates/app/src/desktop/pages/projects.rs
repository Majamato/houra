use crate::locale::tr;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::ProjectId;

use crate::desktop::widgets::ManagementRow;
use crate::desktop::window::MainWindow;

impl MainWindow {
    pub(in crate::desktop) fn refresh_projects_page(&self) {
        let Some(handle) = self.handle() else { return };
        while let Some(child) = self.imp().projects_box.first_child() {
            self.imp().projects_box.remove(&child);
        }
        while let Some(child) = self.imp().activities_box.first_child() {
            self.imp().activities_box.remove(&child);
        }
        let projects = match handle.projects(true) {
            Ok(projects) => projects,
            Err(error) => {
                self.show_database_error(&error.to_string());
                return;
            }
        };
        let activities = handle.activities(true).unwrap_or_default();
        for project in projects {
            let row = ManagementRow::new(
                &project.name,
                &project.color,
                project.archived,
                project.id != ProjectId(1),
            );
            let project_id = project.id;
            row.connect_archive_toggle_requested(glib::clone!(
                #[weak(rename_to = window)]
                self,
                #[strong]
                handle,
                move |_, archived| {
                    let now = chrono::Utc::now().timestamp_millis();
                    if let Err(error) = handle.set_project_archived(project_id, archived, now) {
                        window.show_database_error(&error.to_string());
                    }
                    window.reload_projects();
                    window.refresh_projects_page();
                }
            ));
            self.imp().projects_box.append(&row);
        }
        for activity in activities {
            let activity_row =
                ManagementRow::new(&activity.name, tr("Active"), activity.archived, true);
            let activity_id = activity.id;
            activity_row.connect_archive_toggle_requested(glib::clone!(
                #[weak(rename_to = window)]
                self,
                #[strong]
                handle,
                move |_, archived| {
                    let now = chrono::Utc::now().timestamp_millis();
                    if let Err(error) = handle.set_activity_archived(activity_id, archived, now) {
                        window.show_database_error(&error.to_string());
                    }
                    window.reload_activities();
                    window.refresh_projects_page();
                }
            ));
            self.imp().activities_box.append(&activity_row);
        }
    }
}
