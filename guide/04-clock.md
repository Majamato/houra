# Chapter 4 — Clock

**Goal.** A `Clock` trait with two implementations: `SystemClock` (the real
OS clocks) and `ManualClock` (a clock tests can move by hand). File:
`crates/core/src/clock.rs`, plus two lines in `lib.rs`.

**You will learn**

- What a trait is, how to implement it, and what the bounds after the colon promise.
- `Instant` versus `SystemTime`: monotonic versus wall-clock time.
- Shared mutable state with `Arc<Mutex<_>>` — interior mutability.
- Closures with `map_or`, and bounded integer conversion with `try_from`.

**Prerequisite.** Chapter 3 checkpoint passed.

---

## 4.1 The trait

```rust
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
```

**What.** Anything that can answer "what time is it?" (wall) and "how much
time has passed?" (monotonic) is a `Clock`.

**Why a trait at all.** The engine (Chapter 5) needs the time. If it called
`SystemTime::now()` directly, a test of "start, wait 42 seconds, stop" would
have to *sleep* 42 seconds — or could not be written. With the time source
behind a trait, the engine is generic over it: production hands it the OS
clock, tests hand it a clock they advance instantly. This is dependency
injection with no framework: just a type parameter.

**Rust — traits.** A trait lists method signatures without bodies; a type
*implements* the trait by providing the bodies (`impl Clock for X`). Code
can then be written against the trait instead of a concrete type. Dart
interfaces are the closest relative; Rust traits can also carry default
method bodies and be implemented for types you did not write.

**Rust — the bounds.** `Clock: Clone + Send + Sync + 'static` means "every
implementor must also be `Clone`, `Send`, `Sync` and `'static`". These are
*supertraits*. Each one is a promise the engine will need:

- `Clone` — the engine is cloned before every transaction (Chapter 13), so
  its clock must be cloneable.
- `Send` — the value may be moved to another thread. `Sync` — it may be
  *shared* between threads (`&T` may cross). Both are automatic for types
  made only of thread-safe parts; the compiler refuses the rest
  (Exercise 2).
- `'static` — the type holds no borrowed data that could expire; it can live
  for the whole program. Types that own everything (like these) satisfy it.

`Duration` is the standard library's length-of-time type (nanosecond
precision, never negative).

## 4.2 The real clock

```rust
// crates/core/src/clock.rs
// ...

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
```

**What.** `SystemClock` remembers the `Instant` it was created and reports
how much time has elapsed since (monotonic), and the current Unix time in
milliseconds (wall).

**Rust — `Instant` vs `SystemTime`.** Both are in `std::time`, and they are
deliberately different types. `SystemTime` is the calendar clock: it can be
converted to a date, and the OS may set it backwards. `Instant` is an opaque
stopwatch reading: it only ever increases, and it cannot be turned into a
date (Exercise 4). `origin.elapsed()` is `Instant::now() - origin`.
`UNIX_EPOCH` is the `SystemTime` for 1970-01-01T00:00:00Z.

**Rust — `Default` by hand.** `#[derive(Default)]` would need `Instant` to
have a default, and it has none (there is no "zero instant"). So the trait
is implemented manually: `SystemClock::default()` snapshots `Instant::now()`.
`Default` rather than a `new()` because generic code and derives can call
`T::default()`.

**Rust — the `unwrap_or` chain.** `duration_since` returns a `Result`
(an `Err` if the system clock is set before 1970); `unwrap_or(Duration::ZERO)`
takes the value or substitutes zero. `as_millis()` yields a `u128`;
`i64::try_from(...)` is a *fallible* conversion returning a `Result`, and
`unwrap_or(i64::MAX)` caps it. The trait method is infallible (`-> i64`), so
every failure is bounded instead of panicking — this is what the
"no `unwrap`" lint pushes you towards. (`unwrap_or` is allowed; only the
panicking `unwrap` is denied.)

**Idiom.** When a value is *almost* always fine but the API must not fail,
choose a defensible fallback and write it down (`ZERO`, `MAX`).

## 4.3 The manual clock

```rust
// crates/core/src/clock.rs
// ...

/// A thread-safe clock whose two timelines can be advanced independently.
///
/// Tests use this type to model suspend and wall-clock corrections explicitly.
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
```

**What.** A clock that starts at a chosen wall time and moves only when a
test says so. `advance` moves both timelines; `set_wall_time_ms` moves only
the wall clock — which is how a test simulates "the OS clock was set back"
(Chapter 5 has that test).

**Why `Arc<Mutex<_>>`.** The engine will own a *clone* of the clock, and the
test keeps the original to call `advance`. If cloning copied the numbers, the
engine's clone would never move. The clock must therefore be a *handle* to
shared state: cloning the handle shares the state.

**Rust — `Arc`.** `Arc<T>` (atomic reference count) is shared ownership: many
handles, one value, freed when the last handle drops. It is the thread-safe
sibling of `Rc`, which you will meet in the GTK chapters. `Arc` alone gives
*read* access only.

**Rust — `Mutex` and interior mutability.** Methods take `&self`, a shared
borrow, and shared borrows cannot mutate (Exercise 3). To change a value
through `&self`, it must be wrapped in a type that mediates access at run
time: `Mutex<T>` lets one caller at a time lock it and get a `&mut T`. This
is *interior mutability*: the outside looks immutable, the inside changes
under a lock. `lock()` returns a `Result` because a mutex becomes *poisoned*
if a thread panicked while holding it; `if let Ok(mut value) = ...` handles
the good case and silently skips a poisoned lock, acceptable in a test
helper. `value` is a *guard*: it derefs to the tuple (`value.0`, `value.1`)
and unlocks when it goes out of scope at the closing brace.

**Rust — tuples.** `(i64, Duration)` is an anonymous pair; fields are `.0`
and `.1`. Fine for two related numbers in a private field; for anything
public, use a struct with names.

**Rust — `map_or`.** `self.value.lock().map_or(0, |value| value.0)`: if the
lock is `Ok(guard)`, run the closure on it and return its result; if `Err`,
return `0`. The closure `|value| value.0` takes the guard and reads the first
field. One expression, no `match`, no panic.

**Dart.** `Arc<Mutex<T>>` has no direct equivalent because Dart isolates do
not share memory. Think of it as the one place where Rust lets two threads
touch the same value, with the compiler insisting on the lock.

## 4.4 Export it

```rust
// crates/core/src/lib.rs
// ...
mod clock;
mod error;
mod model;

pub use clock::{Clock, ManualClock, SystemClock};
pub use error::DomainError;
pub use model::*;
```

**Idiom.** Modules are listed alphabetically; re-exports name items
explicitly except for `model`, whose whole content is the crate's vocabulary.

## 4.5 Checkpoint

```sh
cargo build -p houra-core
cargo clippy -p houra-core --all-targets -- -D warnings
cargo fmt --all -- --check
diff <(grep -v '^\s*//' ../work_time_tracker/crates/core/src/clock.rs) \
     <(grep -v '^\s*//' crates/core/src/clock.rs)
```

The diff should print nothing.

```sh
git add -A && git commit -m "Chapter 4: clock"
```

## 4.6 Exercises

1. **Unit tests for the manual clock (optional keeper).** Append to
   `clock.rs`:

   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;

       #[test]
       fn advance_moves_both_timelines_and_clones_share_them() {
           let clock = ManualClock::at(1_000);
           let shared = clock.clone();
           clock.advance(Duration::from_secs(5));
           assert_eq!(shared.wall_time_ms(), 6_000);
           assert_eq!(shared.monotonic(), Duration::from_secs(5));
       }

       #[test]
       fn wall_time_can_move_without_monotonic() {
           let clock = ManualClock::at(1_000);
           clock.set_wall_time_ms(500);
           assert_eq!(clock.wall_time_ms(), 500);
           assert_eq!(clock.monotonic(), Duration::ZERO);
       }

       #[test]
       fn system_clock_is_plausible() {
           let clock = SystemClock::default();
           assert!(clock.wall_time_ms() > 1_700_000_000_000);
           assert!(clock.monotonic() < Duration::from_secs(1));
       }
   }
   ```

   Run `cargo test -p houra-core clock::`.

   <details><summary>Answer</summary>

   ```
   test clock::tests::advance_moves_both_timelines_and_clones_share_them ... ok
   test clock::tests::system_clock_is_plausible ... ok
   test clock::tests::wall_time_can_move_without_monotonic ... ok
   test result: ok. 3 passed; 0 failed; ...
   ```

   The first test is the important one: `shared` was cloned *before* the
   advance and still sees it, because both handles point at one
   `Arc<Mutex<_>>`. The argument `clock::` filters tests by name prefix.
   </details>

2. **The bounds are enforced.** Append:

   ```rust
   #[cfg(test)]
   mod not_send {
       use super::*;
       use std::cell::Cell;
       use std::rc::Rc;

       #[derive(Clone)]
       struct LocalClock {
           value: Rc<Cell<i64>>,
       }

       impl Clock for LocalClock {
           fn wall_time_ms(&self) -> i64 {
               self.value.get()
           }
           fn monotonic(&self) -> Duration {
               Duration::ZERO
           }
       }
   }
   ```

   <details><summary>Answer</summary>

   ```
   error[E0277]: `Rc<Cell<i64>>` cannot be shared between threads safely
     --> crates/core/src/clock.rs:95:20
      |
   95 |     impl Clock for LocalClock {
      |                    ^^^^^^^^^^ `Rc<Cell<i64>>` cannot be shared between threads safely
      |
      = help: within `LocalClock`, the trait `Sync` is not implemented for `Rc<Cell<i64>>`
   note: required by a bound in `clock::Clock`
      |
   10 | pub trait Clock: Clone + Send + Sync + 'static {
      |                                 ^^^^ required by this bound in `Clock`
   ```

   `Rc`/`Cell` are the single-threaded versions of `Arc`/`Mutex`: cheaper,
   but not `Send`/`Sync`, so the compiler refuses to let them implement a
   trait that promises thread safety. Later, GTK code will use `Rc` and
   `RefCell` on purpose, because GTK is single-threaded and never crosses the
   line this error guards.
   </details>

3. **`&self` cannot mutate.** Append:

   ```rust
   #[cfg(test)]
   mod no_interior {
       struct Counter {
           value: i64,
       }

       impl Counter {
           fn advance(&self) {
               self.value += 1;
           }
       }

       #[test]
       fn touch() {
           Counter { value: 0 }.advance();
       }
   }
   ```

   <details><summary>Answer</summary>

   ```
   error[E0594]: cannot assign to `self.value`, which is behind a `&` reference
      |
   92 |             self.value += 1;
      |             ^^^^^^^^^^^^^^^ `self` is a `&` reference, so it cannot be written to
      |
   help: consider changing this to be a mutable reference
      |
   91 |         fn advance(&mut self) {
   ```

   The compiler offers `&mut self`. That would be the right fix for
   `Counter` — but not for `ManualClock`, because the engine holds a *clone*
   and the test holds the original; a `&mut` on one would not affect the
   other. That is the case for `Arc<Mutex<_>>`: shared handles, mutation under
   a lock.
   </details>

4. **An `Instant` is not a date.** Append:

   ```rust
   #[cfg(test)]
   mod instants {
       use super::*;

       #[test]
       fn instant_has_no_epoch() {
           let _ = Instant::now().duration_since(UNIX_EPOCH);
       }
   }
   ```

   <details><summary>Answer</summary>

   ```
   error[E0308]: mismatched types
      |
   90 |         let _ = Instant::now().duration_since(UNIX_EPOCH);
      |                                -------------- ^^^^^^^^^^ expected `Instant`, found `SystemTime`
   ```

   `Instant::duration_since` only accepts another `Instant`. The two clocks
   are different types precisely so that nobody stores a stopwatch reading as
   a timestamp. `ActiveTimer` keeps both for the same reason (Chapter 3).
   </details>

## Recap

- `Clock` is a trait; the engine will be generic over it, so tests control time.
- `SystemClock` pairs a monotonic `Instant` with wall-clock `SystemTime`.
- `ManualClock` is a handle: `Arc` shares, `Mutex` allows mutation through `&self`.
- Bounds on a trait (`Send + Sync + 'static`) are promises the compiler checks.
- Fallible conversions get explicit fallbacks instead of panics.

Next: **Chapter 5 — Engine basics**, where commands meet state in one big
`match`.
