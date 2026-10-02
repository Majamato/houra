//! User preferences and their defaults.

use houra_core::DurationRounding;
use serde::{Deserialize, Serialize};

/// How calendar dates appear in displays, editable timestamps, and CSV date values.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DateFormat {
    /// The current system locale's date.
    #[default]
    System,
    /// Day first, as in `17/09/2026`.
    DayMonthYear,
    /// Month first, as in `09/17/2026`.
    MonthDayYear,
    /// Year first, as in `2026-09-17`.
    YearMonthDay,
}

impl DateFormat {
    /// Every format, in the order the preferences list them.
    pub const ALL: [Self; 4] = [
        Self::System,
        Self::DayMonthYear,
        Self::MonthDayYear,
        Self::YearMonthDay,
    ];

    /// Stable name used by the settings key.
    pub fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::DayMonthYear => "day-month-year",
            Self::MonthDayYear => "month-day-year",
            Self::YearMonthDay => "year-month-day",
        }
    }

    /// Reads a settings key name; `None` for unknown names.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|format| format.key() == key)
    }
}

/// The user preferences. Native builds read them from GSettings;
/// this type documents the defaults and ranges.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Preferences {
    /// Minutes of inactivity before the idle prompt; defaults to 5.
    pub idle_threshold_minutes: u32,
    /// Whether Houra starts with the desktop session; defaults to on.
    pub launch_at_login: bool,
    /// Whether Houra may show desktop notifications; defaults to on.
    pub notifications: bool,
    /// How finished entry totals round to whole minutes; defaults to up.
    pub duration_rounding: DurationRounding,
    /// How calendar dates appear; defaults to the system locale's date.
    pub date_format: DateFormat,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            idle_threshold_minutes: 5,
            launch_at_login: true,
            notifications: true,
            duration_rounding: DurationRounding::Up,
            date_format: DateFormat::System,
        }
    }
}

impl Preferences {
    /// Sets the idle threshold, clamping it to 1–120 minutes.
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
                duration_rounding: DurationRounding::Up,
                date_format: DateFormat::System,
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

    #[test]
    fn date_format_keys_round_trip() {
        assert_eq!(
            DateFormat::ALL.map(DateFormat::key),
            [
                "system",
                "day-month-year",
                "month-day-year",
                "year-month-day"
            ]
        );
        for format in DateFormat::ALL {
            assert_eq!(DateFormat::from_key(format.key()), Some(format));
        }
        for key in ["", "iso", "dd/mm/yyyy", "SYSTEM", "day_month_year"] {
            assert_eq!(DateFormat::from_key(key), None);
        }
    }

    #[test]
    fn date_format_menu_lists_every_variant_once() {
        assert_eq!(
            DateFormat::ALL,
            [
                DateFormat::System,
                DateFormat::DayMonthYear,
                DateFormat::MonthDayYear,
                DateFormat::YearMonthDay,
            ]
        );
    }

    #[test]
    fn date_format_serde_uses_stored_keys() {
        for format in DateFormat::ALL {
            let serialized = serde_json::to_string(&format)
                .unwrap_or_else(|error| panic!("DateFormat should serialize: {error}"));
            assert_eq!(serialized, format!("\"{}\"", format.key()));
            let parsed: DateFormat = serde_json::from_str(&serialized)
                .unwrap_or_else(|error| panic!("DateFormat should deserialize: {error}"));
            assert_eq!(parsed, format);
        }
    }

    #[test]
    fn preferences_default_to_system_dates() {
        assert_eq!(Preferences::default().date_format, DateFormat::System);
        assert_eq!(DateFormat::default(), DateFormat::System);
    }
}
