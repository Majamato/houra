use crate::locale::tr;
use std::cell::{Cell, RefCell};

use chrono::Datelike;
use glib::subclass::InitializingObject;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::{Activity, EntryId, Project, TrackerCommand, TrackerState};
use libadwaita as adw;
use libadwaita::prelude::*;
use libadwaita::subclass::prelude::*;

use crate::TrackerHandle;
use crate::desktop::widgets::TimerActionButton;

pub(super) mod imp {
    use super::*;

    /// Private state of the window: template children plus Rust fields.
    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/majamato/Houra/ui/window.ui")]
    pub struct MainWindow {
        #[template_child]
        pub integration_banner: gtk::TemplateChild<adw::Banner>,
        #[template_child]
        pub timer_label: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub active_entry_total_label: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub start_button: gtk::TemplateChild<TimerActionButton>,
        #[template_child]
        pub stop_button: gtk::TemplateChild<TimerActionButton>,
        #[template_child]
        pub stopped_panel: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub running_panel: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub active_details_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub active_note_label: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub active_meta_label: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub project_dropdown: gtk::TemplateChild<gtk::DropDown>,
        #[template_child]
        pub activity_dropdown: gtk::TemplateChild<gtk::DropDown>,
        #[template_child]
        pub note_entry: gtk::TemplateChild<gtk::Entry>,
        #[template_child]
        pub entries_box: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub day_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub week_box: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub previous_week_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub next_week_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub today_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub entries_heading: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub entries_count: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub total_title: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub total_value: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub add_project_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub projects_box: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub add_activity_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub activities_box: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub report_box: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub report_week_label: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub report_previous_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub report_next_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub export_csv_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub report_full_switch: gtk::TemplateChild<gtk::Switch>,
        pub handle: RefCell<Option<TrackerHandle>>,
        pub projects: RefCell<Vec<Project>>,
        pub activities: RefCell<Vec<Activity>>,
        pub report_week_offset: Cell<i32>,
        pub selected_day_offset: Cell<i32>,
        pub visible_week_offset: Cell<i32>,
        pub stored_day_seconds: Cell<u64>,
        pub active_entry_id: Cell<Option<EntryId>>,
        pub active_entry_saved_ms: Cell<i64>,
        pub active_entry_duration_cached: Cell<bool>,
        pub displayed_today_ordinal: Cell<i32>,
        pub updating_activity_dropdown: Cell<bool>,
        pub idle_dialog_open: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MainWindow {
        const NAME: &'static str = "HouraWindow";
        type Type = super::MainWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(class: &mut Self::Class) {
            TimerActionButton::ensure_type();
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
            .update_property(&[gtk::accessible::Property::Label(tr("Elapsed tracked time"))]);
        self.imp()
            .active_entry_total_label
            .update_property(&[gtk::accessible::Property::Label(tr(
                "Total time on the active entry",
            ))]);
        self.reload_projects();
        self.reload_activities();
        self.refresh_projects_page();
        self.refresh_report();
        self.refresh();
        self.imp().start_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.toggle_timer()
        ));
        self.imp().stop_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.toggle_timer()
        ));
        self.imp()
            .active_details_button
            .connect_clicked(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| window.show_active_editor()
            ));
        self.imp().day_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.show_date_chooser()
        ));
        self.imp()
            .previous_week_button
            .connect_clicked(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| window.show_previous_week()
            ));
        self.imp().next_week_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.show_next_week()
        ));
        self.imp().today_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.show_today()
        ));
        self.imp()
            .project_dropdown
            .connect_selected_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| {
                    window.update_active_details();
                }
            ));
        self.imp()
            .activity_dropdown
            .connect_selected_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| {
                    if !window.imp().updating_activity_dropdown.get() {
                        window.update_active_details();
                    }
                }
            ));
        self.imp().note_entry.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.update_active_details()
        ));
        self.imp().note_entry.connect_activate(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.start_timer_from_note()
        ));
        self.imp().add_project_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.show_new_project()
        ));
        self.imp().add_activity_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.show_new_activity()
        ));
        self.imp()
            .report_previous_button
            .connect_clicked(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| {
                    window
                        .imp()
                        .report_week_offset
                        .set(window.imp().report_week_offset.get().saturating_sub(1));
                    window.refresh_report();
                }
            ));
        self.imp().report_next_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window
                    .imp()
                    .report_week_offset
                    .set(window.imp().report_week_offset.get().saturating_add(1));
                window.refresh_report();
            }
        ));
        self.imp()
            .report_full_switch
            .connect_active_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| window.refresh_report()
            ));
        self.imp().export_csv_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.export_report_csv()
        ));
        let weak = self.downgrade();
        glib::timeout_add_seconds_local(1, move || {
            let Some(window) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let today = chrono::Local::now().date_naive().num_days_from_ce();
            if window.imp().selected_day_offset.get() == 0
                && window.imp().displayed_today_ordinal.get() != today
            {
                window.refresh_entries();
            }
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

    pub(super) fn handle(&self) -> Option<TrackerHandle> {
        self.imp().handle.borrow().clone()
    }

    pub(super) fn refresh(&self) {
        self.refresh_active_entry_duration();
        self.refresh_timer_only();
        self.refresh_entries();
        if let Some(handle) = self.handle()
            && let Ok(snapshot) = handle.snapshot()
        {
            match snapshot.state {
                TrackerState::IdlePending(ref pending) if pending.return_ms.is_some() => {
                    self.show_idle_dialog();
                }
                TrackerState::RecoveryPending(_) => self.show_recovery_dialog(),
                _ => {}
            }
        }
    }

    pub fn show_integration_warning(&self, message: &str) {
        self.imp().integration_banner.set_title(message);
        self.imp().integration_banner.set_revealed(true);
    }

    pub(super) fn show_database_error(&self, message: &str) {
        let dialog = adw::AlertDialog::builder()
            .heading(tr("Could not save the change"))
            .body(message)
            .build();
        dialog.add_response("close", tr("Close"));
        dialog.present(Some(self));
    }
}
