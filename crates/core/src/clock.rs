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
