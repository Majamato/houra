use serde::{Deserialize, Serialize};

/// The user preferences. Native builds read them from GSettings (Chapter 18);
/// this type documents the defaults and ranges.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Preferences {
    pub idle_threshold_minutes: u32,
    pub launch_at_login: bool,
    pub notifications: bool,
    pub week_starts_monday: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            idle_threshold_minutes: 5,
            launch_at_login: true,
            notifications: true,
            week_starts_monday: true,
        }
    }
}

impl Preferences {
    pub fn set_idle_threshold_minutes(&mut self, minutes: u32) {
        self.idle_threshold_minutes = minutes.clamp(1, 120);
    }
}
