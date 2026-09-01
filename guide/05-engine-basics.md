# Chapter 5 — Engine basics

**Goal.** The first version of `TrackerEngine`: it can start, stop, edit and
heartbeat a timer, and rejects everything else. Two integration tests prove
it with a manual clock. Files: `crates/core/src/engine.rs`,
`crates/core/tests/transitions.rs`, one line in `lib.rs`.

**You will learn**

- Generic structs with trait bounds, and static dispatch.
- `&mut self`, and why the state is cloned before matching.
- A transition table as a `match` on a tuple, with payloads moved out of enums.
- `let ... else`, `return` inside a `match` arm, `mut` bindings in patterns.
- Integration tests in `tests/`, `assert!`/`assert_eq!`, and how to unwrap
  under the "no `unwrap`" lint.

**Prerequisite.** Chapter 4 checkpoint passed.

---

## 5.1 The struct and its constructor

```rust
// crates/core/src/engine.rs
//! Deterministic timer state transitions.
//!
//! `TrackerEngine` is cloned before a storage transaction. The clone is only
//! installed after SQLite commits, so memory and disk cannot disagree when an
//! I/O error occurs.

use std::time::Duration;

use crate::{
    ActiveTimer, Clock, DomainError, EntrySource, Notification, TimeEntry, TrackerCommand,
    TrackerSnapshot, TrackerState, Transition,
};

/// The timer state machine. `C` is the time source.
#[derive(Clone, Debug)]
pub struct TrackerEngine<C> {
    clock: C,
    snapshot: TrackerSnapshot,
}

impl<C: Clock> TrackerEngine<C> {
    pub fn new(clock: C) -> Self {
        Self {
            clock,
            snapshot: TrackerSnapshot::default(),
        }
    }

    pub fn snapshot(&self) -> &TrackerSnapshot {
        &self.snapshot
    }
```

(The `impl` block stays open; the next sections add methods inside it.)

**What.** The engine owns a clock and the current snapshot. Both fields are
private — nothing outside this file can set the state directly; the only way
in is `apply`.

**Rust — generics.** `TrackerEngine<C>` is a *generic* struct: `C` is a type
parameter chosen by whoever creates the engine. `impl<C: Clock>
TrackerEngine<C>` says "these methods exist for every `C` that implements
`Clock`". The compiler generates one concrete copy of the code per `C`
actually used (`TrackerEngine<SystemClock>` in the app,
`TrackerEngine<ManualClock>` in tests); calls go straight to the right
method with no runtime lookup. That is *static dispatch* — generics without a
cost.

**Rust — `Self` in generic impls.** Inside the block, `Self` means
`TrackerEngine<C>`. `Self { clock, snapshot: ... }` uses the field-init
shorthand for `clock: clock`.

**Rust — returning `&TrackerSnapshot`.** `snapshot()` lends the caller a view
of the state, like `active()` did in Chapter 3. Callers that need to keep it
call `.clone()` on the result.

**Dart.** `TrackerEngine<C extends Clock>`; Dart's generics are checked at
run time and erased, Rust's produce specialised code.

## 5.2 The live counter

```rust
// crates/core/src/engine.rs (inside impl<C: Clock> TrackerEngine<C>)

    /// Time shown by the live counter; zero when nothing is running.
    pub fn live_elapsed(&self) -> Duration {
        let Some(active) = self.snapshot.state.active() else {
            return Duration::ZERO;
        };
        let started = Duration::from_millis(active.started_monotonic_ms);
        self.clock.monotonic().saturating_sub(started)
    }
```

**What.** How long the current timer has been running, measured on the
monotonic clock. The window will call this once per second.

**Rust — `let ... else`.** `let Some(active) = expr else { return ...; };`
means: if `expr` is `Some`, bind `active` and continue; otherwise run the
`else` block, which *must* leave the function (return, break, panic). It
avoids a nested `match` when only one shape matters. `active` is a
`&ActiveTimer` and stays available for the rest of the function.

**Rust — `saturating_sub` on `Duration`.** If the clock somehow reads before
the start, the result is zero rather than a panic ("attempt to subtract with
overflow"). Same discipline as the integers in Chapter 2.

## 5.3 `apply`: the transition table

```rust
// crates/core/src/engine.rs (inside impl<C: Clock> TrackerEngine<C>)

    /// Applies one command. On error the engine is unchanged.
    pub fn apply(&mut self, command: TrackerCommand) -> Result<Transition, DomainError> {
        let now = self.clock.wall_time_ms();
        let monotonic_ms = u64::try_from(self.clock.monotonic().as_millis()).unwrap_or(u64::MAX);
        let mut completed_entries = Vec::new();
        let mut notifications = Vec::new();

        let next_state = match (self.snapshot.state.clone(), command) {
            (
                TrackerState::Stopped,
                TrackerCommand::Start {
                    project_id,
                    task_id,
                    note,
                },
            ) => {
                notifications.push(Notification::TimerStarted);
                TrackerState::Running(ActiveTimer {
                    project_id,
                    task_id,
                    note,
                    start_ms: now,
                    started_monotonic_ms: monotonic_ms,
                    last_heartbeat_ms: now,
                })
            }
            (TrackerState::Running(active), TrackerCommand::Stop) => {
                push_entry(
                    &mut completed_entries,
                    &active,
                    active.start_ms,
                    now,
                    EntrySource::Timer,
                );
                notifications.push(Notification::TimerStopped);
                TrackerState::Stopped
            }
            (
                TrackerState::Running(mut active),
                TrackerCommand::EditActive {
                    project_id,
                    task_id,
                    note,
                },
            ) => {
                active.project_id = project_id;
                active.task_id = task_id;
                active.note = note;
                active.last_heartbeat_ms = now.max(active.start_ms);
                TrackerState::Running(active)
            }
            (TrackerState::Running(mut active), TrackerCommand::Heartbeat) => {
                active.last_heartbeat_ms = now.max(active.start_ms);
                TrackerState::Running(active)
            }
            (state, _) => return Err(DomainError::InvalidState(state)),
        };

        self.snapshot.state = next_state;
        self.snapshot.revision = self.snapshot.revision.saturating_add(1);
        Ok(Transition {
            snapshot: self.snapshot.clone(),
            completed_entries,
            notifications,
        })
    }
}
```

(The closing `}` ends the `impl` block.)

**What.** Read the time once, then match on the pair *(current state,
command)*. Each arm computes the next state and may push entries and
notifications. Chapter 3's diagram is this `match`: four arrows now, the
rest in Chapter 6; anything not listed falls to the last arm and is an
`InvalidState` error. Only after a successful arm is the snapshot replaced
and the revision bumped.

**Why read `now` once.** Every timestamp in a transition comes from the same
instant, so a stop entry's `end_ms` and the new snapshot agree exactly.

**Why `Start` records two clocks.** `start_ms: now` goes to disk;
`started_monotonic_ms` feeds `live_elapsed`; `last_heartbeat_ms` starts equal
to the start so recovery is safe even if the first heartbeat never happens.

**Why `now.max(active.start_ms)`.** If the wall clock was set backwards
while running, a heartbeat must not record an "alive" instant before the
start; clamping keeps `[start, heartbeat]` a valid interval.

**Rust — `&mut self`.** An exclusive borrow: while `apply` runs, nothing else
can even read the engine. The compiler enforces single-writer at compile
time, which is what makes "the engine is unchanged on error" a guarantee
rather than a hope.

**Rust — `let mut`.** Bindings are immutable unless declared `mut`. The two
`Vec`s are pushed into, so they are `mut`; `now` and `monotonic_ms` never
change, so they are not. The compiler warns about a `mut` that is never
needed.

**Rust — matching on a tuple.** `(self.snapshot.state.clone(), command)`
builds a temporary pair, and each arm is a pair of patterns. Patterns nest:
`TrackerCommand::Start { project_id, task_id, note }` matches the variant
*and* binds its three fields to local variables in one go (field-name
shorthand for `project_id: project_id`). `(state, _)` binds whatever state
was found and ignores the command with `_`.

**Rust — moving out of an enum.** `command` is owned by `apply` (it was
passed by value), so matching *moves* its fields out: `note` becomes an owned
`String` that goes straight into the new `ActiveTimer` — no copy. The state,
however, lives inside `self.snapshot` behind `&mut self`, and you cannot
move out of a borrow (Exercise 2). Hence `.clone()`: the match works on a
copy, and the original stays intact until the arm has succeeded. The cost is
one small clone per command; the benefit is that an error leaves `self`
untouched.

**Rust — `mut` in a pattern.** `TrackerState::Running(mut active)` binds
`active` as mutable so the arm can update its fields and put it back
(Exercise 4 shows the error without it).

**Rust — `return` inside `match`.** The last arm returns from the whole
function; the other arms produce a `TrackerState` that becomes `next_state`.
An arm that returns has type `!` ("never"), which fits any expected type, so
the `match` still type-checks.

**Rust — `Vec::new()` type inference.** `completed_entries` is created
without naming its element type; the first `push` of a `TimeEntry` decides
it. Explicit types are needed only when inference has nothing to go on.

**Idiom.** One `match` that reads like the specification, no `if state ==
... { if command == ... }` ladders. When someone asks "what happens on
`Heartbeat` while idle?", the answer is one line in this table — or its
absence.

## 5.4 The helper that creates entries

```rust
// crates/core/src/engine.rs (after the impl block)

fn push_entry(
    completed: &mut Vec<TimeEntry>,
    active: &ActiveTimer,
    start_ms: i64,
    end_ms: i64,
    source: EntrySource,
) {
    if end_ms <= start_ms {
        return;
    }
    completed.push(TimeEntry {
        id: None,
        project_id: active.project_id,
        task_id: active.task_id,
        note: active.note.clone(),
        start_ms,
        end_ms,
        source,
        created_at_ms: end_ms,
        updated_at_ms: end_ms,
    });
}
```

**What.** Turns an active timer plus an interval into a `TimeEntry` and
appends it — unless the interval is empty, in which case nothing is recorded.
The second test below depends on that guard.

**Why drop empty intervals here.** A zero-length entry carries no
information, and the database (Chapter 9) rejects `end_ms <= start_ms`
outright. Filtering here keeps the engine's output always storable.

**Rust — private free function.** No `pub`, so it is invisible outside
`engine.rs`. It takes `&mut Vec<TimeEntry>` (it appends) and `&ActiveTimer`
(it only reads). `active.note.clone()` is required: `active` is borrowed, the
new entry must *own* its note. The `Copy` fields (`project_id`, `task_id`)
copy silently.

**Idiom.** Small private helpers with borrowed parameters keep the big
`match` readable. Prefer `&T` and `&mut T` parameters over ownership unless
the function needs to keep the value.

Register the module:

```rust
// crates/core/src/lib.rs
// ...
mod clock;
mod engine;
mod error;
mod model;

pub use clock::{Clock, ManualClock, SystemClock};
pub use engine::TrackerEngine;
pub use error::DomainError;
pub use model::*;
```

## 5.5 The first integration tests

```rust
// crates/core/tests/transitions.rs
use std::time::Duration;

use work_time_core::{ManualClock, Notification, ProjectId, TrackerCommand, TrackerEngine};

fn start(engine: &mut TrackerEngine<ManualClock>) {
    let result = engine.apply(TrackerCommand::Start {
        project_id: ProjectId(1),
        task_id: None,
        note: "design".into(),
    });
    assert!(result.is_ok());
}

#[test]
fn start_stop_records_exact_interval() {
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(42));
    let result = engine.apply(TrackerCommand::Stop);
    assert!(result.is_ok());
    let transition = result.unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(transition.completed_entries[0].duration_ms(), 42_000);
    assert_eq!(transition.notifications, vec![Notification::TimerStopped]);
}

#[test]
fn wall_clock_reversal_does_not_create_negative_entry() {
    let clock = ManualClock::at(20_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.set_wall_time_ms(10_000);
    let result = engine.apply(TrackerCommand::Stop);
    assert!(result.is_ok());
    assert!(
        result
            .unwrap_or_else(|error| panic!("unexpected error: {error}"))
            .completed_entries
            .is_empty()
    );
}
```

**What.** Two timelines. First: start at t=1 s, advance 42 s, stop — the entry
is exactly 42 000 ms and the UI is told the timer stopped. Second: the wall
clock is set *back* 10 s while running, then stop — no entry is produced
instead of a negative one.

**Why a manual clock.** No sleeping: `clock.advance(...)` moves time for the
engine, because the engine holds a clone of the same `Arc<Mutex<_>>`. The
test runs in microseconds and gives the same result on every machine.

**Rust — integration tests.** Files in `tests/` are compiled as separate
crates that can only see the library's *public* API through
`use work_time_core::...` — exactly what a user of the crate sees. Each `fn`
marked `#[test]` runs in its own thread; a panic fails that test only.
Contrast with `#[cfg(test)] mod tests` inside a source file, which can test
private items.

**Rust — the concrete type.** `&mut TrackerEngine<ManualClock>` names the
generic parameter; the helper takes an exclusive borrow because `apply`
needs `&mut self`.

**Rust — `assert!` and `assert_eq!`.** `assert!(cond)` panics if false.
`assert_eq!(a, b)` panics with both values printed (Exercise 1). `vec![...]`
builds a `Vec` literal for comparison; `Notification` derives `PartialEq`
and `Debug` so the comparison compiles and the failure message is readable.

**Rust — unwrapping under the lint.** `unwrap()` is denied in this crate,
tests included. `unwrap_or_else(|error| panic!("unexpected error: {error}"))`
does the same job with an explicit message that includes the error's
`Display` text. The `assert!(result.is_ok())` just before makes the intent
clear at a glance.

**Rust — `"design".into()`.** `into()` converts one type to another via the
`Into` trait; here `&str` → `String`, inferred from the field type.

## 5.6 Checkpoint

```sh
cargo test -p work-time-core
```

Expected (plus the Chapter 2/3/4 unit tests if you kept them):

```
     Running tests/transitions.rs (target/debug/deps/transitions-...)
test start_stop_records_exact_interval ... ok
test wall_clock_reversal_does_not_create_negative_entry ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```sh
cargo clippy -p work-time-core --all-targets -- -D warnings
cargo fmt --all -- --check
git add -A && git commit -m "Chapter 5: engine basics"
```

The diff against the original `engine.rs` will show the missing idle and
recovery arms, `restore`, `resolve_idle` and `resumed_active` — all of
Chapter 6.

## 5.7 Exercises

1. **Read a failing assertion.** In `start_stop_records_exact_interval`
   change `42_000` to `42` and run `cargo test -p work-time-core --test
   transitions`.

   <details><summary>Answer</summary>

   ```
   ---- start_stop_records_exact_interval stdout ----

   thread 'start_stop_records_exact_interval' panicked at crates/core/tests/transitions.rs:23:5:
   assertion `left == right` failed
     left: 42000
    right: 42

   failures:
       start_stop_records_exact_interval

   test result: FAILED. 1 passed; 1 failed; ...
   ```

   `left` is the first argument, `right` the second. Put the actual value
   first and the expected value second and the message reads naturally.
   `--test transitions` runs only that file.
   </details>

2. **You cannot move out of a borrow.** Remove `.clone()` from
   `match (self.snapshot.state.clone(), command)` and build.

   <details><summary>Answer</summary>

   ```
   error[E0507]: cannot move out of `self.snapshot.state` which is behind a mutable reference
      |
   49 |         let next_state = match (self.snapshot.state, command) {
      |                                 ^^^^^^^^^^^^^^^^^^^ move occurs because `self.snapshot.state` has type `model::TrackerState`, which does not implement the `Copy` trait
      |
   help: consider cloning the value if the performance cost is acceptable
   ```

   Building the tuple would *move* the state out of `self`, leaving the
   engine with a hole in it — Rust forbids that through a reference. There
   is a zero-copy alternative (`std::mem::take`, which swaps in a default),
   but then an error in the middle of the match would leave the engine
   `Stopped`. The clone is the price of "unchanged on error".
   </details>

3. **See `InvalidState`.** Append to `transitions.rs`:

   ```rust
   #[test]
   fn stop_while_stopped_is_rejected() {
       let mut engine = TrackerEngine::new(ManualClock::at(0));
       let error = engine.apply(TrackerCommand::Stop).err();
       println!("{}", error.map(|error| error.to_string()).unwrap_or_default());
       assert_eq!(engine.snapshot().revision, 0);
       panic!("show me");
   }
   ```

   Run `cargo test -p work-time-core --test transitions stop_while`.

   <details><summary>Answer</summary>

   ```
   the tracker is not in the required state (current state: Stopped)
   ```

   The catch-all arm fired, the message comes from `thiserror`, and
   `revision` is still 0: the rejected command changed nothing. `.err()`
   turns a `Result` into an `Option` of its error; `.map(...)` formats it;
   `unwrap_or_default()` gives an empty string if there was no error.
   </details>

4. **`mut` in patterns.** In the `Heartbeat` arm change `Running(mut
   active)` to `Running(active)` and build.

   <details><summary>Answer</summary>

   ```
   error[E0594]: cannot assign to `active.last_heartbeat_ms`, as `active` is not declared as mutable
      |
   94 |                 active.last_heartbeat_ms = now.max(active.start_ms);
   help: consider changing this to be mutable
      |
   93 |             (TrackerState::Running(mut active), TrackerCommand::Heartbeat) => {
   ```

   Bindings created by a pattern follow the same rule as `let`: immutable
   unless marked `mut`. The `Stop` arm binds `active` without `mut` because
   it only reads it.
   </details>

## Recap

- `TrackerEngine<C: Clock>` is generic over its time source; tests use
  `ManualClock`.
- `apply` reads the time once, matches `(state, command)`, and only commits
  the new snapshot after an arm succeeds — errors leave the engine untouched.
- Payloads move out of the owned `command`; the state is cloned because it
  is behind `&mut self`.
- `push_entry` discards empty intervals so every produced entry is storable.
- Integration tests live in `tests/`, see only the public API, and never
  sleep.

Next: **Chapter 6 — Idle and recovery**, the remaining arms, crash restore,
and property-based tests.
