use crate::locale::{tr, trf, trn};
use std::sync::LazyLock;

use chrono::{Local, TimeZone};
use glib::subclass::{InitializingObject, Signal};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::{Activity, Project, TimeEntry};

use super::format_duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::desktop) enum EntryTrackingState {
    Inactive,
    Tracking,
    ReviewRequired,
}

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/majamato/Houra/ui/entry-row.ui")]
    pub struct EntryRow {
        #[template_child]
        pub dot: gtk::TemplateChild<gtk::DrawingArea>,
        #[template_child]
        pub edit_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub title: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub subtitle: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub duration: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub report_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub tracking_status: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub continue_button: gtk::TemplateChild<gtk::Button>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EntryRow {
        const NAME: &'static str = "HouraEntryRow";
        type Type = super::EntryRow;
        type ParentType = gtk::Box;

        fn class_init(class: &mut Self::Class) {
            class.bind_template();
        }

        fn instance_init(object: &InitializingObject<Self>) {
            object.init_template();
        }
    }

    impl ObjectImpl for EntryRow {
        fn constructed(&self) {
            self.parent_constructed();
            let object = self.obj();
            self.edit_button.connect_clicked(glib::clone!(
                #[weak]
                object,
                move |_| object.emit_by_name::<()>("edit-requested", &[])
            ));
            self.continue_button.connect_clicked(glib::clone!(
                #[weak]
                object,
                move |_| object.emit_by_name::<()>("continue-requested", &[])
            ));
            self.report_button
                .update_property(&[gtk::accessible::Property::Label(tr("View task report"))]);
            self.report_button.connect_clicked(glib::clone!(
                #[weak]
                object,
                move |_| object.emit_by_name::<()>("report-requested", &[])
            ));
        }

        fn dispose(&self) {
            self.dispose_template();
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> = LazyLock::new(|| {
                vec![
                    Signal::builder("edit-requested").build(),
                    Signal::builder("continue-requested").build(),
                    Signal::builder("report-requested").build(),
                ]
            });
            SIGNALS.as_ref()
        }
    }
    impl WidgetImpl for EntryRow {}
    impl BoxImpl for EntryRow {}
}

glib::wrapper! {
    pub struct EntryRow(ObjectSubclass<imp::EntryRow>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl EntryRow {
    pub(in crate::desktop) fn new(
        entry: &TimeEntry,
        project: Option<&Project>,
        activity: Option<&Activity>,
        day_start_ms: i64,
        day_end_ms: i64,
        live_ms: i64,
        tracking: EntryTrackingState,
    ) -> Self {
        let row: Self = glib::Object::builder().build();
        let project_name = project.map_or(tr("Missing project"), |item| item.name.as_str());
        let visible = entry
            .intervals
            .iter()
            .filter(|interval| interval.start_ms < day_end_ms && interval.end_ms > day_start_ms)
            .collect::<Vec<_>>();
        let interval_count = visible.len() + usize::from(live_ms > 0);
        let times = if interval_count > 1 {
            trn(
                "{count} interval",
                "{count} intervals",
                interval_count as u32,
            )
            .replace("{count}", &interval_count.to_string())
        } else {
            match visible.as_slice() {
                [interval] => match (
                    Local
                        .timestamp_millis_opt(interval.start_ms.max(day_start_ms))
                        .single(),
                    Local
                        .timestamp_millis_opt(interval.end_ms.min(day_end_ms))
                        .single(),
                ) {
                    (Some(start), Some(end)) => {
                        format!("{}–{}", start.format("%H:%M"), end.format("%H:%M"))
                    }
                    _ => String::new(),
                },
                _ if live_ms > 0 => tr("Current session").into(),
                _ => String::new(),
            }
        };
        let metadata = activity.map_or_else(
            || {
                trf(
                    "{project} · {times}",
                    &[("project", project_name), ("times", &times)],
                )
            },
            |item| {
                trf(
                    "{project} · {activity} · {times}",
                    &[
                        ("project", project_name),
                        ("activity", &item.name),
                        ("times", &times),
                    ],
                )
            },
        );

        row.imp().title.set_label(if entry.note.is_empty() {
            tr("Tracked work")
        } else {
            &entry.note
        });
        row.imp().subtitle.set_label(&metadata);
        let stored_ms = visible.iter().fold(0_i64, |total, interval| {
            total.saturating_add(
                interval
                    .end_ms
                    .min(day_end_ms)
                    .saturating_sub(interval.start_ms.max(day_start_ms))
                    .max(0),
            )
        });
        let duration = u64::try_from(stored_ms.saturating_add(live_ms).max(0) / 1_000).unwrap_or(0);
        row.imp().duration.set_label(&format_duration(duration));
        match tracking {
            EntryTrackingState::Inactive => row.imp().continue_button.set_label(tr("Continue")),
            EntryTrackingState::Tracking => {
                row.imp().continue_button.set_visible(false);
                row.imp()
                    .tracking_status
                    .set_label(tr("Currently tracking"));
                row.imp().tracking_status.set_visible(true);
            }
            EntryTrackingState::ReviewRequired => {
                row.imp().continue_button.set_visible(false);
                row.imp().tracking_status.set_label(tr("Review required"));
                row.imp().tracking_status.set_visible(true);
            }
        }

        let color = project
            .and_then(|item| gtk::gdk::RGBA::parse(&item.color).ok())
            .unwrap_or_else(|| gtk::gdk::RGBA::new(0.5, 0.7, 0.95, 1.0));
        row.imp()
            .dot
            .set_draw_func(move |_, context, width, height| {
                context.set_source_rgba(
                    f64::from(color.red()),
                    f64::from(color.green()),
                    f64::from(color.blue()),
                    1.0,
                );
                context.arc(
                    f64::from(width) / 2.0,
                    f64::from(height) / 2.0,
                    4.5,
                    0.0,
                    std::f64::consts::TAU,
                );
                let _ignored = context.fill();
            });
        row
    }

    pub(in crate::desktop) fn connect_edit_requested<F: Fn(&Self) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "edit-requested",
            false,
            glib::closure_local!(move |row: Self| callback(&row)),
        )
    }

    pub(in crate::desktop) fn connect_continue_requested<F: Fn(&Self) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "continue-requested",
            false,
            glib::closure_local!(move |row: Self| callback(&row)),
        )
    }

    pub(in crate::desktop) fn connect_report_requested<F: Fn(&Self) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "report-requested",
            false,
            glib::closure_local!(move |row: Self| callback(&row)),
        )
    }
}
