use serde::{Deserialize, Serialize};

/// The user preferences. Native builds read them from GSettings;
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_threshold_clamping() {
        let mut preferences = Preferences::default();
        assert_eq!(
            preferences,
            Preferences {
                idle_threshold_minutes: 5,
                launch_at_login: true,
                notifications: true,
                week_starts_monday: true
            }
        );
        for (input, expected) in [
            (0, 1),
            (1, 1),
            (60, 60),
            (120, 120),
            (121, 120),
            (u32::MAX, 120),
        ] {
            preferences.set_idle_threshold_minutes(input);
            assert_eq!(
                preferences,
                Preferences {
                    idle_threshold_minutes: expected,
                    ..Preferences::default()
                }
            );
        }
    }
}
