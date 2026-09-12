use std::cell::{Cell, RefCell};

use chrono::{Local, TimeZone};
use glib::subclass::InitializingObject;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::{Project, ProjectId, Task, TrackerCommand, TrackerState};
use libadwaita as adw;
use libadwaita::prelude::*;
use libadwaita::subclass::prelude::*;

use crate::TrackerHandle;

mod imp {
    use super::*;

    /// Private state of the window: template children plus Rust fields.
    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/majamato/Houra/ui/window.ui")]
    pub struct MainWindow {
        #[template_child]
        pub timer_label: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub start_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub project_dropdown: gtk::TemplateChild<gtk::DropDown>,
        #[template_child]
        pub task_dropdown: gtk::TemplateChild<gtk::DropDown>,
        #[template_child]
        pub note_entry: gtk::TemplateChild<gtk::Entry>,
        #[template_child]
        pub entries_box: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub day_label: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub day_previous_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub day_next_button: gtk::TemplateChild<gtk::Button>,
        pub handle: RefCell<Option<TrackerHandle>>,
        pub projects: RefCell<Vec<Project>>,
        pub tasks: RefCell<Vec<Task>>,
        pub selected_day_offset: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MainWindow {
        const NAME: &'static str = "HouraWindow";
        type Type = super::MainWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(class: &mut Self::Class) {
            class.bind_template();
        }

        fn instance_init(object: &InitializingObject<Self>) {
            object.init_template();
        }
    }

    impl ObjectImpl for MainWindow {}
    impl WidgetImpl for MainWindow {}
    impl WindowImpl for MainWindow {}
    impl ApplicationWindowImpl for MainWindow {}
    impl AdwApplicationWindowImpl for MainWindow {}
}

glib::wrapper! {
    pub struct MainWindow(ObjectSubclass<imp::MainWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl MainWindow {
    pub fn new(application: &adw::Application, handle: TrackerHandle) -> Self {
        let window: Self = glib::Object::builder()
            .property("application", application)
            .build();
        window.imp().handle.replace(Some(handle));
        window.setup();
        window
    }

    fn setup(&self) {
        self.imp()
            .timer_label
            .update_property(&[gtk::accessible::Property::Label("Elapsed tracked time")]);
        self.reload_projects();
        self.reload_tasks();
        self.refresh();
        self.imp().start_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.toggle_timer()
        ));
        self.imp()
            .project_dropdown
            .connect_selected_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| {
                    window.reload_tasks();
                    window.update_active_details();
                }
            ));
        self.imp()
            .task_dropdown
            .connect_selected_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| window.update_active_details()
            ));
        self.imp().note_entry.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.update_active_details()
        ));
        self.imp().day_previous_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window
                    .imp()
                    .selected_day_offset
                    .set(window.imp().selected_day_offset.get().saturating_sub(1));
                window.refresh_entries();
            }
        ));
        self.imp().day_next_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window
                    .imp()
                    .selected_day_offset
                    .set(window.imp().selected_day_offset.get().saturating_add(1));
                window.refresh_entries();
            }
        ));
        let weak = self.downgrade();
        glib::timeout_add_seconds_local(1, move || {
            let Some(window) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            window.refresh_timer_only();
            glib::ControlFlow::Continue
        });
        let heartbeat = self.handle();
        glib::timeout_add_seconds_local(30, move || {
            if let Some(handle) = &heartbeat {
                let handle = handle.clone();
                gio::spawn_blocking(move || handle.apply(TrackerCommand::Heartbeat));
            }
            glib::ControlFlow::Continue
        });
    }

    fn handle(&self) -> Option<TrackerHandle> {
        self.imp().handle.borrow().clone()
    }

    fn reload_projects(&self) {
        let Some(handle) = self.handle() else { return };
        match handle.projects(false) {
            Ok(projects) => {
                let names: Vec<&str> = projects
                    .iter()
                    .map(|project| project.name.as_str())
                    .collect();
                self.imp()
                    .project_dropdown
                    .set_model(Some(&gtk::StringList::new(&names)));
                self.imp().projects.replace(projects);
            }
            Err(error) => self.show_database_error(&error.to_string()),
        }
    }

    fn selected_project_id(&self) -> ProjectId {
        let index = usize::try_from(self.imp().project_dropdown.selected()).unwrap_or(0);
        self.imp()
            .projects
            .borrow()
            .get(index)
            .map_or(ProjectId(1), |project| project.id)
    }

    fn selected_task_id(&self) -> Option<houra_core::TaskId> {
        let selected = self.imp().task_dropdown.selected();
        if selected == 0 || selected == gtk::INVALID_LIST_POSITION {
            return None;
        }
        usize::try_from(selected.saturating_sub(1))
            .ok()
            .and_then(|index| self.imp().tasks.borrow().get(index).map(|task| task.id))
    }

    fn reload_tasks(&self) {
        let Some(handle) = self.handle() else { return };
        let project_id = self.selected_project_id();
        let tasks = handle
            .tasks(false)
            .unwrap_or_default()
            .into_iter()
            .filter(|task| task.project_id == project_id)
            .collect::<Vec<_>>();
        let mut names = vec!["No task"];
        names.extend(tasks.iter().map(|task| task.name.as_str()));
        self.imp()
            .task_dropdown
            .set_model(Some(&gtk::StringList::new(&names)));
        self.imp().tasks.replace(tasks);
    }

    fn update_active_details(&self) {
        let Some(handle) = self.handle() else { return };
        let Ok(snapshot) = handle.snapshot() else {
            return;
        };
        if matches!(snapshot.state, TrackerState::Running(_)) {
            let result = handle.apply(TrackerCommand::EditActive {
                project_id: self.selected_project_id(),
                task_id: self.selected_task_id(),
                note: self.imp().note_entry.text().to_string(),
            });
            if let Err(error) = result {
                self.show_database_error(&error.to_string());
            }
        }
    }

    pub fn toggle_timer(&self) {
        let Some(handle) = self.handle() else { return };
        let state = match handle.snapshot() {
            Ok(snapshot) => snapshot.state,
            Err(error) => {
                self.show_database_error(&error.to_string());
                return;
            }
        };
        let command = match state {
            TrackerState::Stopped => TrackerCommand::Start {
                project_id: self.selected_project_id(),
                task_id: self.selected_task_id(),
                note: self.imp().note_entry.text().to_string(),
            },
            TrackerState::Running(_) => TrackerCommand::Stop,
            TrackerState::IdlePending(_) | TrackerState::RecoveryPending(_) => return,
        };
        match handle.apply(command) {
            Ok(_) => self.refresh(),
            Err(error) => self.show_database_error(&error.to_string()),
        }
    }

    fn refresh(&self) {
        self.refresh_timer_only();
        self.refresh_entries();
    }

    fn refresh_timer_only(&self) {
        let Some(handle) = self.handle() else { return };
        let Ok(snapshot) = handle.snapshot() else {
            return;
        };
        match snapshot.state {
            TrackerState::Running(active)
            | TrackerState::IdlePending(houra_core::PendingIdle { active, .. }) => {
                let elapsed = handle.live_elapsed().map_or_else(
                    |_| {
                        u64::try_from(
                            chrono::Utc::now()
                                .timestamp_millis()
                                .saturating_sub(active.start_ms)
                                .max(0)
                                / 1_000,
                        )
                        .unwrap_or(0)
                    },
                    |duration| duration.as_secs(),
                );
                self.imp().timer_label.set_label(&format!(
                    "{:02}:{:02}:{:02}",
                    elapsed / 3600,
                    (elapsed / 60) % 60,
                    elapsed % 60
                ));
                self.imp().start_button.set_label("Stop");
                self.imp().start_button.add_css_class("destructive-action");
            }
            TrackerState::RecoveryPending(_) => {
                self.imp().timer_label.set_label("Recovery needed");
                self.imp().start_button.set_label("Review");
            }
            TrackerState::Stopped => {
                self.imp().timer_label.set_label("00:00:00");
                self.imp().start_button.set_label("Start");
                self.imp()
                    .start_button
                    .remove_css_class("destructive-action");
                self.imp().start_button.add_css_class("suggested-action");
            }
        }
    }

    fn refresh_entries(&self) {
        let Some(handle) = self.handle() else { return };
        while let Some(child) = self.imp().entries_box.first_child() {
            self.imp().entries_box.remove(&child);
        }
        let Some(date) = Local::now()
            .date_naive()
            .checked_add_signed(chrono::Duration::days(i64::from(
                self.imp().selected_day_offset.get(),
            )))
        else {
            return;
        };
        let day_label = if self.imp().selected_day_offset.get() == 0 {
            "Today".to_owned()
        } else {
            date.format("%A, %x").to_string()
        };
        self.imp().day_label.set_label(&day_label);
        let Some(start) = date
            .and_hms_opt(0, 0, 0)
            .and_then(|value| Local.from_local_datetime(&value).earliest())
        else {
            return;
        };
        let Some(next_date) = date.succ_opt() else {
            return;
        };
        let Some(end) = next_date
            .and_hms_opt(0, 0, 0)
            .and_then(|value| Local.from_local_datetime(&value).earliest())
        else {
            return;
        };
        match handle.entries(start.timestamp_millis(), end.timestamp_millis()) {
            Ok(entries) if entries.is_empty() => {
                let label = gtk::Label::new(Some("No entries for today"));
                label.add_css_class("dim-label");
                self.imp().entries_box.append(&label);
            }
            Ok(entries) => {
                for entry in entries {
                    let duration = entry.duration_ms() / 1_000;
                    let row = adw::ActionRow::builder()
                        .title(if entry.note.is_empty() {
                            "Tracked work"
                        } else {
                            &entry.note
                        })
                        .subtitle(format!(
                            "{}h {:02}m {:02}s",
                            duration / 3600,
                            (duration / 60) % 60,
                            duration % 60
                        ))
                        .build();
                    self.imp().entries_box.append(&row);
                }
            }
            Err(error) => self.show_database_error(&error.to_string()),
        }
    }

    fn show_database_error(&self, message: &str) {
        let dialog = adw::AlertDialog::builder()
            .heading("Could not save the change")
            .body(message)
            .build();
        dialog.add_response("close", "Close");
        dialog.present(Some(self));
    }
}
