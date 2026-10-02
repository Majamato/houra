use serde::{Deserialize, Serialize};

use crate::{EntryId, TimeEntry};

const MINUTE_MS: i64 = 60_000;

/// How the total of a finished time entry rounds to whole minutes.
/// Tracked intervals always keep their exact times; only totals round.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DurationRounding {
    /// Any started minute counts in full.
    #[default]
    Up,
    /// Half a minute or more counts in full; less is dropped.
    Nearest,
    /// Only completed minutes count.
    Down,
}

impl DurationRounding {
    /// Every mode, in the order the preferences list them.
    pub const ALL: [Self; 3] = [Self::Up, Self::Nearest, Self::Down];

    /// Rounds a duration to whole minutes. Zero stays zero, and any other
    /// duration keeps at least one minute so finished work never vanishes.
    pub fn round_ms(self, duration_ms: i64) -> i64 {
        if duration_ms <= 0 {
            return 0;
        }
        let whole = duration_ms / MINUTE_MS;
        let rest = duration_ms % MINUTE_MS;
        let minutes = match self {
            Self::Up => whole + i64::from(rest > 0),
            Self::Nearest => whole + i64::from(rest >= MINUTE_MS / 2),
            Self::Down => whole,
        };
        minutes.max(1).saturating_mul(MINUTE_MS)
    }

    /// Stable name used by the settings key.
    pub fn key(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Nearest => "nearest",
            Self::Down => "down",
        }
    }

    /// Reads a settings key name; `None` for unknown names.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.key() == key)
    }
}

/// Totals time entries for display: finished entries round, while the entry
/// the active timer tracks stays exact until it is finished.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EntryTotals {
    /// How finished entries round.
    pub rounding: DurationRounding,
    /// Entry the active timer tracks, running or paused.
    pub active_entry_id: Option<EntryId>,
}

impl EntryTotals {
    /// Returns the entry's time inside `[start_ms, end_ms)`, rounded unless
    /// the entry is still being tracked.
    pub fn total_ms(&self, entry: &TimeEntry, start_ms: i64, end_ms: i64) -> i64 {
        let clipped = entry.duration_between_ms(start_ms, end_ms);
        if self.is_active(entry) {
            clipped
        } else {
            self.rounding.round_ms(clipped)
        }
    }

    /// Whether the active timer still tracks this entry.
    pub fn is_active(&self, entry: &TimeEntry) -> bool {
        entry.id.is_some() && entry.id == self.active_entry_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntrySource, ProjectId, TrackedInterval};

    #[test]
    fn rounds_to_whole_minutes_by_mode() {
        use DurationRounding::{Down, Nearest, Up};
        for (duration_ms, up, nearest, down) in [
            (0, 0, 0, 0),
            (-5, 0, 0, 0),
            (1, 1, 1, 1),
            (40_000, 1, 1, 1),
            (60_000, 1, 1, 1),
            (70_000, 2, 1, 1),
            (89_999, 2, 1, 1),
            (90_000, 2, 2, 1),
            (119_999, 2, 2, 1),
            (5_430_000, 91, 91, 90),
        ] {
            for (mode, minutes) in [(Up, up), (Nearest, nearest), (Down, down)] {
                assert_eq!(
                    mode.round_ms(duration_ms),
                    minutes * 60_000,
                    "{mode:?} {duration_ms} ms"
                );
            }
        }
        assert_eq!(Up.round_ms(i64::MAX), i64::MAX);
    }

    #[test]
    fn keys_round_trip() {
        for mode in DurationRounding::ALL {
            assert_eq!(DurationRounding::from_key(mode.key()), Some(mode));
        }
        assert_eq!(DurationRounding::from_key("sideways"), None);
        assert_eq!(DurationRounding::default(), DurationRounding::Up);
    }

    #[test]
    fn only_finished_entries_round() {
        let entry = |id| TimeEntry {
            id,
            project_id: ProjectId(1),
            activity_id: None,
            note: String::new(),
            intervals: vec![TrackedInterval {
                id: None,
                start_ms: 0,
                end_ms: 70_000,
                source: EntrySource::Timer,
            }],
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        let totals = EntryTotals {
            rounding: DurationRounding::Up,
            active_entry_id: Some(EntryId(1)),
        };
        assert_eq!(
            totals.total_ms(&entry(Some(EntryId(1))), 0, i64::MAX),
            70_000
        );
        assert_eq!(
            totals.total_ms(&entry(Some(EntryId(2))), 0, i64::MAX),
            120_000
        );
        assert_eq!(totals.total_ms(&entry(None), 0, i64::MAX), 120_000);
        assert_eq!(totals.total_ms(&entry(Some(EntryId(2))), 0, 30_000), 60_000);
        assert_eq!(totals.total_ms(&entry(Some(EntryId(2))), 80_000, 90_000), 0);
    }
}
