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
        let projects = match handle.projects(true) {
            Ok(projects) => projects,
            Err(error) => {
                self.show_database_error(&error.to_string());
                return;
            }
        };
        let tasks = handle.tasks(true).unwrap_or_default();
        for project in projects {
            let row = adw::ActionRow::builder()
                .title(&project.name)
                .subtitle(if project.archived {
                    "Archived"
                } else {
                    &project.color
                })
                .build();
            let add_task = gtk::Button::builder()
                .icon_name("list-add-symbolic")
                .tooltip_text("Add task")
                .valign(gtk::Align::Center)
                .build();
            add_task.connect_clicked(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| window.show_new_task(project.id)
            ));
            row.add_suffix(&add_task);
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
            for task in tasks.iter().filter(|task| task.project_id == project.id) {
                let task_row = adw::ActionRow::builder()
                    .title(format!("↳ {}", task.name))
                    .subtitle(if task.archived {
                        "Archived task"
                    } else {
                        "Task"
                    })
                    .build();
                let archive = gtk::Button::builder()
                    .icon_name(if task.archived {
                        "view-refresh-symbolic"
                    } else {
                        "user-trash-symbolic"
                    })
                    .tooltip_text(if task.archived { "Restore" } else { "Archive" })
                    .valign(gtk::Align::Center)
                    .build();
                let task_id = task.id;
                let archived = task.archived;
                archive.connect_clicked(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    #[strong]
                    handle,
                    move |_| {
                        let now = chrono::Utc::now().timestamp_millis();
                        if let Err(error) = handle.set_task_archived(task_id, !archived, now) {
                            window.show_database_error(&error.to_string());
                        }
                        window.refresh_projects_page();
                    }
                ));
                task_row.add_suffix(&archive);
                self.imp().projects_box.append(&task_row);
            }
        }
    }
}
