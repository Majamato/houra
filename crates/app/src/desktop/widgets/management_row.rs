use std::{cell::Cell, sync::LazyLock};

use glib::subclass::{InitializingObject, Signal};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use libadwaita::subclass::prelude::*;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/majamato/Houra/ui/management-row.ui")]
    pub struct ManagementRow {
        #[template_child]
        pub archive_button: gtk::TemplateChild<gtk::Button>,
        pub archived: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ManagementRow {
        const NAME: &'static str = "HouraManagementRow";
        type Type = super::ManagementRow;
        type ParentType = adw::ActionRow;

        fn class_init(class: &mut Self::Class) {
            class.bind_template();
        }

        fn instance_init(object: &InitializingObject<Self>) {
            object.init_template();
        }
    }

    impl ObjectImpl for ManagementRow {
        fn constructed(&self) {
            self.parent_constructed();
            let object = self.obj();
            self.archive_button.connect_clicked(glib::clone!(
                #[weak]
                object,
                move |_| {
                    let target = !object.imp().archived.get();
                    object.emit_by_name::<()>("archive-toggle-requested", &[&target]);
                }
            ));
        }

        fn dispose(&self) {
            self.dispose_template();
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> = LazyLock::new(|| {
                vec![
                    Signal::builder("archive-toggle-requested")
                        .param_types([bool::static_type()])
                        .build(),
                ]
            });
            SIGNALS.as_ref()
        }
    }
    impl WidgetImpl for ManagementRow {}
    impl ListBoxRowImpl for ManagementRow {}
    impl PreferencesRowImpl for ManagementRow {}
    impl ActionRowImpl for ManagementRow {}
}

glib::wrapper! {
    pub struct ManagementRow(ObjectSubclass<imp::ManagementRow>)
        @extends gtk::Widget, gtk::ListBoxRow, adw::PreferencesRow, adw::ActionRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl ManagementRow {
    pub(in crate::desktop) fn new(
        name: &str,
        active_subtitle: &str,
        archived: bool,
        archive_allowed: bool,
    ) -> Self {
        let row: Self = glib::Object::builder().build();
        row.set_title(name);
        row.set_subtitle(if archived {
            "Archived"
        } else {
            active_subtitle
        });
        row.imp().archived.set(archived);
        row.imp().archive_button.set_visible(archive_allowed);
        row.imp().archive_button.set_icon_name(if archived {
            "view-refresh-symbolic"
        } else {
            "user-trash-symbolic"
        });
        row.imp().archive_button.set_tooltip_text(Some(if archived {
            "Restore"
        } else {
            "Archive"
        }));
        row
    }

    pub(in crate::desktop) fn connect_archive_toggle_requested<F: Fn(&Self, bool) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "archive-toggle-requested",
            false,
            glib::closure_local!(move |row: Self, archived: bool| callback(&row, archived)),
        )
    }
}
