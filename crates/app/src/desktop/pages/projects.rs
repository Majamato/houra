use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::ProjectId;
use libadwaita as adw;
use libadwaita::prelude::*;

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
            let row = adw::ActionRow::builder()
                .title(&project.name)
                .subtitle(if project.archived {
                    "Archived"
                } else {
                    &project.color
                })
                .build();
            if project.id != ProjectId(1) {
                let archive = gtk::Button::builder()
                    .icon_name(if project.archived {
                        "view-refresh-symbolic"
                    } else {
                        "user-trash-symbolic"
                    })
                    .tooltip_text(if project.archived {
                        "Restore"
                    } else {
                        "Archive"
                    })
                    .valign(gtk::Align::Center)
                    .build();
                archive.connect_clicked(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    #[strong]
                    handle,
                    move |_| {
                        let now = chrono::Utc::now().timestamp_millis();
                        if let Err(error) =
                            handle.set_project_archived(project.id, !project.archived, now)
                        {
                            window.show_database_error(&error.to_string());
                        }
                        window.reload_projects();
                        window.refresh_projects_page();
                    }
                ));
                row.add_suffix(&archive);
            }
            self.imp().projects_box.append(&row);
        }
        for activity in activities {
            let activity_row = adw::ActionRow::builder()
                .title(&activity.name)
                .subtitle(if activity.archived {
                    "Archived"
                } else {
                    "Active"
                })
                .build();
            let archive = gtk::Button::builder()
                .icon_name(if activity.archived {
                    "view-refresh-symbolic"
                } else {
                    "user-trash-symbolic"
                })
                .tooltip_text(if activity.archived {
                    "Restore"
                } else {
                    "Archive"
                })
                .valign(gtk::Align::Center)
                .build();
            let activity_id = activity.id;
            let archived = activity.archived;
            archive.connect_clicked(glib::clone!(
                #[weak(rename_to = window)]
                self,
                #[strong]
                handle,
                move |_| {
                    let now = chrono::Utc::now().timestamp_millis();
                    if let Err(error) = handle.set_activity_archived(activity_id, !archived, now) {
                        window.show_database_error(&error.to_string());
                    }
                    window.reload_activities();
                    window.refresh_projects_page();
                }
            ));
            activity_row.add_suffix(&archive);
            self.imp().activities_box.append(&activity_row);
        }
    }
}
