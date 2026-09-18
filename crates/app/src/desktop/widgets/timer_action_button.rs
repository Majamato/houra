use gtk::prelude::*;
use gtk::subclass::prelude::*;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct TimerActionButton;

    #[glib::object_subclass]
    impl ObjectSubclass for TimerActionButton {
        const NAME: &'static str = "HouraTimerActionButton";
        type Type = super::TimerActionButton;
        type ParentType = gtk::Button;
    }

    impl ObjectImpl for TimerActionButton {
        fn constructed(&self) {
            self.parent_constructed();
            let object = self.obj();
            object.add_css_class("suggested-action");
            object.add_css_class("timer-action");
        }
    }
    impl WidgetImpl for TimerActionButton {}
    impl ButtonImpl for TimerActionButton {}
}

glib::wrapper! {
    pub struct TimerActionButton(ObjectSubclass<imp::TimerActionButton>)
        @extends gtk::Widget, gtk::Button,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}
