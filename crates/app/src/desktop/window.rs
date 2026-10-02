use crate::DateFormat;
use crate::locale::tr;
use std::cell::{Cell, RefCell};

use chrono::Datelike;
use gio::prelude::SettingsExt as _;
use glib::subclass::InitializingObject;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::{
    Activity, DurationRounding, EntryId, EntryTotals, Project, TrackerCommand, TrackerState,
};
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
        pub start_button: gtk::TemplateChild<TimerActionButton>,
        #[template_child]
        pub stop_button: gtk::TemplateChild<TimerActionButton>,
        #[template_child]
        pub pause_button: gtk::TemplateChild<TimerActionButton>,
        #[template_child]
        pub tracking_eyebrow: gtk::TemplateChild<gtk::Label>,
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
        pub day_label: gtk::TemplateChild<gtk::Label>,
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
        pub duration_rounding: Cell<DurationRounding>,
        pub date_format: Cell<DateFormat>,
        pub date_settings: RefCell<Option<gio::Settings>>,
        pub active_entry_id: Cell<Option<EntryId>>,
        pub active_entry_saved_ms: Cell<i64>,
        pub active_entry_duration_cached: Cell<bool>,
        pub displayed_today_ordinal: Cell<i32>,
        pub updating_activity_dropdown: Cell<bool>,
        pub idle_dialog_open: Cell<bool>,
        pub clock_tick: RefCell<Option<glib::SourceId>>,
        pub heartbeat: RefCell<Option<glib::SourceId>>,
        pub date_popover: RefCell<Option<glib::WeakRef<gtk::Popover>>>,
        pub(in crate::desktop) top_bar: RefCell<Option<crate::desktop::top_bar::TopBar>>,
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
    /// Builds the main window and wires it to the storage worker.
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
            .update_property(&[gtk::accessible::Property::Label(tr(
                "Total time on the active entry",
            ))]);
        self.imp().duration_rounding.set(
            crate::desktop::load_settings()
                .and_then(|settings| {
                    DurationRounding::from_key(&settings.string("duration-rounding"))
                })
                .unwrap_or_default(),
        );
        let date_settings = crate::desktop::load_settings();
        self.imp().date_format.set(
            date_settings
                .as_ref()
                .and_then(|settings| DateFormat::from_key(&settings.string("date-format")))
                .unwrap_or_default(),
        );
        if let Some(settings) = &date_settings {
            settings.connect_changed(
                Some("date-format"),
                glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    move |settings: &gio::Settings, _key: &str| {
                        window.apply_date_format(
                            DateFormat::from_key(&settings.string("date-format"))
                                .unwrap_or_default(),
                        );
                    }
                ),
            );
        }
        self.imp().date_settings.replace(date_settings);
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
            move |_| window.finish_timer()
        ));
        self.imp().pause_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.toggle_pause()
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
        self.imp().day_button.connect_unrealize(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                // Detach a still-open date picker so teardown stays quiet.
                if let Some(weak) = window.imp().date_popover.take()
                    && let Some(popover) = weak.upgrade()
                {
                    popover.unparent();
                }
            }
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
        // Seconds show and tick only while the window has focus.
        self.connect_is_active_notify(|window| {
            window.refresh_timer_only();
            window.schedule_clock_tick();
        });
        self.schedule_clock_tick();
        self.sync_heartbeat();
    }

    pub(super) fn handle(&self) -> Option<TrackerHandle> {
        self.imp().handle.borrow().clone()
    }

    /// The preferred calendar-date format for displays and exports.
    pub(in crate::desktop) fn date_format(&self) -> DateFormat {
        self.imp().date_format.get()
    }

    /// Applies a date-format change, keeping the selected day and week offsets.
    pub(in crate::desktop) fn apply_date_format(&self, format: DateFormat) {
        if self.imp().date_format.get() == format {
            return;
        }
        self.imp().date_format.set(format);
        self.refresh_entries();
        self.refresh_report();
    }

    /// Totals for display: finished entries round by the preference, while
    /// the entry the active timer tracks stays exact.
    pub(super) fn entry_totals(&self) -> EntryTotals {
        EntryTotals {
            rounding: self.imp().duration_rounding.get(),
            active_entry_id: self
                .handle()
                .and_then(|handle| handle.snapshot().ok())
                .and_then(|snapshot| snapshot.state.active().and_then(|active| active.entry_id)),
        }
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
        self.sync_top_bar();
        self.schedule_clock_tick();
    }

    /// Follows a tracker change made outside the window, such as idle
    /// detection or a heartbeat. Unlike `refresh`, never opens a dialog.
    pub(super) fn follow_tracker_change(&self) {
        self.sync_top_bar();
        self.refresh_timer_only();
        self.schedule_clock_tick();
        self.sync_heartbeat();
    }

    fn sync_top_bar(&self) {
        let top_bar = self.imp().top_bar.borrow().clone();
        if let Some(top_bar) = top_bar {
            top_bar.sync();
        }
    }

    /// Runs the 30-second heartbeat only while time is counting.
    pub(super) fn sync_heartbeat(&self) {
        let Some(handle) = self.handle() else { return };
        let counting = matches!(
            handle.snapshot().map(|snapshot| snapshot.state),
            Ok(TrackerState::Running(_) | TrackerState::IdlePending(_))
        );
        let mut heartbeat = self.imp().heartbeat.borrow_mut();
        if counting {
            if heartbeat.is_none() {
                heartbeat.replace(glib::timeout_add_seconds_local(30, move || {
                    let handle = handle.clone();
                    gio::spawn_blocking(move || handle.apply(TrackerCommand::Heartbeat));
                    glib::ControlFlow::Continue
                }));
            }
        } else if let Some(source) = heartbeat.take() {
            // The source always continues, so it is still attached here.
            source.remove();
        }
    }

    /// Schedules the next clock redraw on the counter's second (focused) or
    /// minute boundary, or at midnight when nothing counts, replacing any
    /// pending one.
    pub(super) fn schedule_clock_tick(&self) {
        if let Some(pending) = self.imp().clock_tick.take() {
            pending.remove();
        }
        let weak = self.downgrade();
        let source = glib::timeout_add_local_once(self.next_clock_tick(), move || {
            let Some(window) = weak.upgrade() else { return };
            // This source is finishing; forget it so it is never removed twice.
            window.imp().clock_tick.take();
            window.tick_clock();
            window.schedule_clock_tick();
        });
        self.imp().clock_tick.replace(Some(source));
    }

    /// Redraws the live counter and rolls the day view over at midnight.
    fn tick_clock(&self) {
        let today = chrono::Local::now().date_naive().num_days_from_ce();
        if self.imp().selected_day_offset.get() == 0
            && self.imp().displayed_today_ordinal.get() != today
        {
            self.refresh_entries();
        }
        self.refresh_timer_only();
    }

    /// Lets window refreshes announce timer changes to the top bar at once.
    pub(super) fn set_top_bar(&self, top_bar: crate::desktop::top_bar::TopBar) {
        self.imp().top_bar.replace(Some(top_bar));
    }

    /// Reveals the integration banner with the given message.
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
