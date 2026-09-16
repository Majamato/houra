// crates/core/src/clock.rs
//! Time sources used by the state machine.
//!
//! Persisted timestamps use UTC wall time. Live counters use monotonic time so
//! an NTP adjustment cannot make the visible timer jump backwards.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Supplies wall-clock milliseconds and a monotonic duration.
pub trait Clock: Clone + Send + Sync + 'static {
    fn wall_time_ms(&self) -> i64;
    fn monotonic(&self) -> Duration;
}

/// The operating system's clocks.
#[derive(Clone, Debug)]
pub struct SystemClock {
    origin: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn wall_time_ms(&self) -> i64 {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis();
        i64::try_from(millis).unwrap_or(i64::MAX)
    }

    fn monotonic(&self) -> Duration {
        self.origin.elapsed()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ManualClock {
    value: Arc<Mutex<(i64, Duration)>>,
}

impl ManualClock {
    pub fn at(wall_time_ms: i64) -> Self {
        Self {
            value: Arc::new(Mutex::new((wall_time_ms, Duration::ZERO))),
        }
    }

    pub fn advance(&self, duration: Duration) {
        if let Ok(mut value) = self.value.lock() {
            value.0 = value
                .0
                .saturating_add(i64::try_from(duration.as_millis()).unwrap_or(i64::MAX));
            value.1 = value.1.saturating_add(duration);
        }
    }

    pub fn set_wall_time_ms(&self, wall_time_ms: i64) {
        if let Ok(mut value) = self.value.lock() {
            value.0 = wall_time_ms;
        }
    }
}

impl Clock for ManualClock {
    fn wall_time_ms(&self) -> i64 {
        self.value.lock().map_or(0, |value| value.0)
    }

    fn monotonic(&self) -> Duration {
        self.value.lock().map_or(Duration::ZERO, |value| value.1)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn manual_clock_starts_at_the_requested_wall_time() {
        let clock = ManualClock::at(1_000);

        assert_eq!(clock.wall_time_ms(), 1_000);
        assert_eq!(clock.monotonic(), Duration::ZERO);
    }

    #[test]
    fn advance_moves_both_timelines_and_clones_share_state() {
        let clock = ManualClock::at(1_000);
        let shared = clock.clone();

        clock.advance(Duration::from_secs(5));

        assert_eq!(shared.wall_time_ms(), 6_000);
        assert_eq!(shared.monotonic(), Duration::from_secs(5));
    }

    #[test]
    fn advances_accumulate() {
        let clock = ManualClock::at(1_000);

        clock.advance(Duration::from_millis(250));
        clock.advance(Duration::from_millis(750));

        assert_eq!(clock.wall_time_ms(), 2_000);
        assert_eq!(clock.monotonic(), Duration::from_secs(1));
    }

    #[test]
    fn sub_millisecond_advance_only_moves_monotonic_time() {
        let clock = ManualClock::at(1_000);
        let elapsed = Duration::from_micros(500);

        clock.advance(elapsed);

        assert_eq!(clock.wall_time_ms(), 1_000);
        assert_eq!(clock.monotonic(), elapsed);
    }

    #[test]
    fn setting_wall_time_does_not_move_monotonic_time() {
        let clock = ManualClock::at(1_000);
        clock.advance(Duration::from_secs(5));

        clock.set_wall_time_ms(500);

        assert_eq!(clock.wall_time_ms(), 500);
        assert_eq!(clock.monotonic(), Duration::from_secs(5));
    }

    #[test]
    fn wall_time_saturates_on_overflow() {
        let clock = ManualClock::at(i64::MAX - 1);

        clock.advance(Duration::from_millis(2));

        assert_eq!(clock.wall_time_ms(), i64::MAX);
        assert_eq!(clock.monotonic(), Duration::from_millis(2));
    }

    #[test]
    fn system_clock_wall_time_is_between_observations() {
        let clock = SystemClock::default();
        let before = unix_time_ms();

        let observed = clock.wall_time_ms();

        let after = unix_time_ms();
        assert!(
            (before..=after).contains(&observed),
            "clock returned {observed}, expected a value between {before} and {after}"
        );
    }

    #[test]
    fn system_clock_monotonic_time_does_not_go_backwards() {
        let clock = SystemClock::default();
        let first = clock.monotonic();
        let second = clock.monotonic();

        assert!(second >= first);
    }

    fn unix_time_ms() -> i64 {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis();
        i64::try_from(millis).unwrap_or(i64::MAX)
    }
}
