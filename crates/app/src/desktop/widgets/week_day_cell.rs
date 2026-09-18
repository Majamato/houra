use chrono::{Datelike, NaiveDate};
use glib::subclass::InitializingObject;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

use super::format_duration;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/majamato/Houra/ui/week-day-cell.ui")]
    pub struct WeekDayCell {
        #[template_child]
        pub weekday: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub number: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub total: gtk::TemplateChild<gtk::Label>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WeekDayCell {
        const NAME: &'static str = "HouraWeekDayCell";
        type Type = super::WeekDayCell;
        type ParentType = gtk::Button;

        fn class_init(class: &mut Self::Class) {
            class.bind_template();
        }

        fn instance_init(object: &InitializingObject<Self>) {
            object.init_template();
        }
    }

    impl ObjectImpl for WeekDayCell {
        fn dispose(&self) {
            self.dispose_template();
        }
    }
    impl WidgetImpl for WeekDayCell {}
    impl ButtonImpl for WeekDayCell {}
}

glib::wrapper! {
    pub struct WeekDayCell(ObjectSubclass<imp::WeekDayCell>)
        @extends gtk::Widget, gtk::Button,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl WeekDayCell {
    pub(in crate::desktop) fn new(date: NaiveDate, total_seconds: u64, selected: bool) -> Self {
        let cell: Self = glib::Object::builder().build();
        cell.imp().weekday.set_label(&date.format("%a").to_string());
        cell.imp().number.set_label(&date.day().to_string());
        let total = if total_seconds == 0 {
            "—".to_owned()
        } else {
            format_duration(total_seconds)
        };
        cell.imp().total.set_label(&total);
        if selected {
            cell.add_css_class("selected");
        }
        cell
    }
}
