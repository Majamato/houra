//! GNOME desktop presentation and session integrations.

mod application;
mod dialogs;
mod pages;
mod platform;
mod widgets;
mod window;

pub use application::run;
pub(super) use application::{load_settings, log_background_error};
