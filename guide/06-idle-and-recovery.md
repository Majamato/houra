# Chapter 6 — Idle and recovery

**Goal.** The engine becomes complete: idle detection and the four ways to
resolve it, crash recovery, and `restore` for start-up. The remaining
example tests and two property-based tests go in. Files:
`crates/core/src/engine.rs`, `crates/core/tests/transitions.rs`,
`crates/core/tests/properties.rs`, `crates/core/Cargo.toml`.

**You will learn**

- Struct-update syntax `..value`.
- Passing ownership into helpers (`mut active: ActiveTimer`) and `&mut Vec`
  output parameters.
- `ok_or_else` + `?` to turn an `Option` into an early error.
- `restore`: turning persisted state into a question for the user.
- Property-based testing with `proptest`: strategies, hundreds of cases,
  shrinking.

**Prerequisite.** Chapter 5 checkpoint passed.

---

## 6.1 Idle detection and return

Add these arms to the `match` in `apply`, **before** the catch-all
`(state, _)` arm. The order of arms only matters for overlapping patterns;
the catch-all must stay last.

```rust
// crates/core/src/engine.rs (inside apply's match, before `(state, _) =>`)
            (TrackerState::Running(active), TrackerCommand::IdleDetected { idle_start_ms }) => {
                if idle_start_ms < active.start_ms || idle_start_ms > now {
                    return Err(DomainError::InvalidIdleStart {
                        active_start_ms: active.start_ms,
                        idle_start_ms,
                    });
                }
                TrackerState::IdlePending(PendingIdle {
                    active,
                    idle_start_ms,
                    return_ms: None,
                })
            }
            (TrackerState::IdlePending(mut pending), TrackerCommand::Heartbeat) => {
                pending.active.last_heartbeat_ms = now.max(pending.active.start_ms);
                TrackerState::IdlePending(pending)
            }
            (
                TrackerState::IdlePending(mut pending),
                TrackerCommand::UserReturned { return_ms },
            ) => {
                if return_ms < pending.idle_start_ms {
                    return Err(DomainError::InvalidReturn {
                        idle_start_ms: pending.idle_start_ms,
                        return_ms,
                    });
                }
                pending.return_ms = Some(return_ms);
                notifications.push(Notification::IdleNeedsResolution);
                TrackerState::IdlePending(pending)
            }
            (TrackerState::IdlePending(pending), TrackerCommand::ResolveIdle(decision)) => {
                let return_ms = pending.return_ms.ok_or_else(|| {
                    DomainError::InvalidState(TrackerState::IdlePending(pending.clone()))
                })?;
                resolve_idle(
                    pending,
                    decision,
                    return_ms,
                    monotonic_ms,
                    &mut completed_entries,
                    &mut notifications,
                )
            }
```

And extend the imports at the top of the file:

```rust
// crates/core/src/engine.rs
use crate::{
    ActiveTimer, Clock, DomainError, EntrySource, IdleDecision, Notification, PendingIdle,
    PendingRecovery, TimeEntry, TrackerCommand, TrackerSnapshot, TrackerState, Transition,
};
```

**What.** GNOME tells the app "the user has been idle since T" (Chapter 19);
the timer moves to `IdlePending`, keeping the active timer inside. When the
user comes back, `UserReturned` records that moment and asks the UI to show
the dialog. `ResolveIdle` applies their answer. Heartbeats keep working while
idle so recovery stays bounded.

**Why validate `idle_start_ms`.** An idle start before the timer started, or
in the future, is a bug in the caller. The engine refuses rather than
producing a nonsense entry, and the message shows both numbers.

**Why record `return_ms` separately.** The user may answer the dialog
minutes after returning. The entries must use the time they *returned*, not
the time they clicked — the tests below check exactly this.

**Rust — `ok_or_else` and `?`.** `pending.return_ms` is an `Option<i64>`.
`ok_or_else(|| error)` converts it to a `Result`, building the error lazily
only if it is `None`; `?` then either unwraps the `i64` or returns the error.
The closure captures `pending` by reference, hence `pending.clone()` inside
it — the error needs an owned state to carry.

**Rust — `if` with `||`.** Boolean *or*; like `&&`, it short-circuits.

## 6.2 Recovery

Still before the catch-all:

```rust
// crates/core/src/engine.rs (inside apply's match, before `(state, _) =>`)
            (
                TrackerState::RecoveryPending(pending),
                TrackerCommand::ResolveRecovery { end_ms, resume },
            ) => {
                if end_ms < pending.active.start_ms || end_ms > pending.active.last_heartbeat_ms {
                    return Err(DomainError::InvalidRecoveryEnd {
                        start_ms: pending.active.start_ms,
                        last_heartbeat_ms: pending.active.last_heartbeat_ms,
                        end_ms,
                    });
                }
                push_entry(
                    &mut completed_entries,
                    &pending.active,
                    pending.active.start_ms,
                    end_ms,
                    EntrySource::Recovery,
                );
                notifications.push(Notification::RecoveryResolved);
                if resume {
                    TrackerState::Running(ActiveTimer {
                        start_ms: now,
                        started_monotonic_ms: monotonic_ms,
                        last_heartbeat_ms: now,
                        ..pending.active
                    })
                } else {
                    TrackerState::Stopped
                }
            }
            (TrackerState::RecoveryPending(_), TrackerCommand::DiscardRecovery) => {
                notifications.push(Notification::RecoveryResolved);
                TrackerState::Stopped
            }
```

**What.** After a crash the user is offered an end time; they may edit it,
but never beyond the last heartbeat. The recovered interval becomes an entry
tagged `Recovery`; optionally a fresh timer starts *now* with the same
project, task and note.

**Why the heartbeat bound.** The app was alive at `last_heartbeat_ms`. It
knows nothing after that: maybe the machine slept for a week. Counting time
it cannot vouch for would silently inflate a report.

**Rust — struct-update syntax.** `ActiveTimer { start_ms: now, ..., ..pending.active }`
sets three fields explicitly and takes *all the remaining fields* from
`pending.active`. The `..` moves those fields (here `note`, a `String`) out
of `pending.active`, which is fine because `pending` is owned by this arm and
not used afterwards. Exercise 2 shows what happens without it.

**Rust — `_` in a variant pattern.** `RecoveryPending(_)` matches the
variant and ignores its payload; nothing is bound or moved.

## 6.3 Resolving idle time

Two private helpers after `push_entry` (order among free functions does not
matter to the compiler; keeping them together helps readers):

```rust
// crates/core/src/engine.rs (after the impl block)

fn resolve_idle(
    pending: PendingIdle,
    decision: IdleDecision,
    return_ms: i64,
    monotonic_ms: u64,
    completed: &mut Vec<TimeEntry>,
    notifications: &mut Vec<Notification>,
) -> TrackerState {
    match decision {
        IdleDecision::Keep => TrackerState::Running(pending.active),
        IdleDecision::DiscardAndResume => {
            push_entry(
                completed,
                &pending.active,
                pending.active.start_ms,
                pending.idle_start_ms,
                EntrySource::Timer,
            );
            TrackerState::Running(resumed_active(pending.active, return_ms, monotonic_ms))
        }
        IdleDecision::ReassignAndResume {
            project_id,
            task_id,
            note,
        } => {
            push_entry(
                completed,
                &pending.active,
                pending.active.start_ms,
                pending.idle_start_ms,
                EntrySource::Timer,
            );
            let reassigned = ActiveTimer {
                project_id,
                task_id,
                note,
                start_ms: pending.idle_start_ms,
                started_monotonic_ms: 0,
                last_heartbeat_ms: return_ms,
            };
            push_entry(
                completed,
                &reassigned,
                pending.idle_start_ms,
                return_ms,
                EntrySource::IdleReassignment,
            );
            TrackerState::Running(resumed_active(pending.active, return_ms, monotonic_ms))
        }
        IdleDecision::Stop => {
            push_entry(
                completed,
                &pending.active,
                pending.active.start_ms,
                pending.idle_start_ms,
                EntrySource::Timer,
            );
            notifications.push(Notification::TimerStopped);
            TrackerState::Stopped
        }
    }
}

fn resumed_active(mut active: ActiveTimer, return_ms: i64, monotonic_ms: u64) -> ActiveTimer {
    active.start_ms = return_ms;
    active.last_heartbeat_ms = return_ms;
    active.started_monotonic_ms = monotonic_ms;
    active
}
```

**What.** The timeline was `[start, idle_start)` focused, `[idle_start,
return)` away. Each decision partitions it differently:

| Decision | Entries produced | Then |
| --- | --- | --- |
| Keep | none | keep running as if never idle |
| DiscardAndResume | focused part | new timer from `return_ms` |
| ReassignAndResume | focused part **and** the idle part on another project | new timer from `return_ms` |
| Stop | focused part | stopped |

**Why a separate function.** The `match` in `apply` was getting long; this
decision has its own four-way `match`, so it gets its own function with the
pieces it needs passed in.

**Rust — ownership in parameters.** `pending: PendingIdle` and
`decision: IdleDecision` are taken *by value*: the caller gives them up, and
the function may move parts out (`pending.active` into `Running`, `note` into
`reassigned`). `completed: &mut Vec<TimeEntry>` is an output parameter: the
caller keeps the `Vec` and lends it for appending. `resumed_active(mut
active: ActiveTimer, ...)` takes ownership, mutates locally (`mut` on the
parameter), and hands the value back — no clone anywhere in this path.

**Rust — `push_entry(completed, ...)`.** `completed` is already a
`&mut Vec`, so it is passed on as is (Chapter 5 passed
`&mut completed_entries` because there the `Vec` was owned).

**Idiom.** Move when the callee should own the result, borrow when it only
looks. Each helper here has parameters that read like a sentence of what it
needs.

## 6.4 Restoring from disk

Add this associated function inside the `impl` block, after `new`:

```rust
// crates/core/src/engine.rs (inside impl<C: Clock> TrackerEngine<C>)

    /// Rebuilds an engine from a persisted snapshot. After an unclean
    /// shutdown a live timer becomes a recovery question for the user.
    pub fn restore(clock: C, mut snapshot: TrackerSnapshot, unclean_shutdown: bool) -> Self {
        if unclean_shutdown {
            snapshot.state = match snapshot.state {
                TrackerState::Running(active) => {
                    let proposed_end_ms = active.last_heartbeat_ms.max(active.start_ms);
                    TrackerState::RecoveryPending(PendingRecovery {
                        active,
                        proposed_end_ms,
                        unresolved_idle_start_ms: None,
                    })
                }
                TrackerState::IdlePending(pending) => {
                    let proposed_end_ms = pending
                        .active
                        .last_heartbeat_ms
                        .max(pending.active.start_ms);
                    TrackerState::RecoveryPending(PendingRecovery {
                        active: pending.active,
                        proposed_end_ms,
                        unresolved_idle_start_ms: Some(pending.idle_start_ms),
                    })
                }
                state => state,
            };
        }
        Self { clock, snapshot }
    }
```

**What.** At start-up the app loads the last snapshot from SQLite and asks
"was the previous shutdown clean?" (Chapter 9 keeps that marker). If not,
and a timer was live, the state is rewritten into `RecoveryPending` with the
heartbeat as the proposed end. A clean shutdown, or a stopped timer, restores
as is.

**Rust — `mut` parameter, `match` as assignment.** `mut snapshot` lets the
function modify its own copy of the argument. `snapshot.state = match
snapshot.state { ... }` moves the old state into the match and stores the
result back; the `state => state` arm passes unchanged states through. Since
the match consumes `snapshot.state`, each arm binds owned payloads
(`active`, `pending`) and can move them into the new variant without cloning.

## 6.5 The remaining example tests

Add to `tests/transitions.rs` (and extend the `use` line):

```rust
// crates/core/tests/transitions.rs
use work_time_core::{
    IdleDecision, ManualClock, Notification, ProjectId, TrackerCommand, TrackerEngine, TrackerState,
};

// ... start helper, the two Chapter 5 tests ...

#[test]
fn discard_idle_resumes_at_recorded_return_not_dialog_time() {
    let clock = ManualClock::at(10_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(600));
    let idle = engine.apply(TrackerCommand::IdleDetected {
        idle_start_ms: 310_000,
    });
    assert!(idle.is_ok());
    let returned = engine.apply(TrackerCommand::UserReturned { return_ms: 610_000 });
    assert!(returned.is_ok());
    clock.advance(Duration::from_secs(90));
    let result = engine.apply(TrackerCommand::ResolveIdle(IdleDecision::DiscardAndResume));
    assert!(result.is_ok());
    let transition = result.unwrap_or_else(|error| panic!("unexpected error: {error}"));
    assert_eq!(transition.completed_entries[0].start_ms, 10_000);
    assert_eq!(transition.completed_entries[0].end_ms, 310_000);
    match transition.snapshot.state {
        TrackerState::Running(active) => assert_eq!(active.start_ms, 610_000),
        state => panic!("expected running, got {state:?}"),
    }
}

#[test]
fn reassign_idle_preserves_whole_timeline() {
    let clock = ManualClock::at(1_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(10));
    assert!(
        engine
            .apply(TrackerCommand::IdleDetected {
                idle_start_ms: 6_000
            })
            .is_ok()
    );
    assert!(
        engine
            .apply(TrackerCommand::UserReturned { return_ms: 11_000 })
            .is_ok()
    );
    let result = engine.apply(TrackerCommand::ResolveIdle(
        IdleDecision::ReassignAndResume {
            project_id: ProjectId(2),
            task_id: None,
            note: "break".into(),
        },
    ));
    assert!(result.is_ok());
    let entries = result
        .unwrap_or_else(|error| panic!("unexpected error: {error}"))
        .completed_entries;
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries.iter().map(|entry| entry.duration_ms()).sum::<i64>(),
        10_000
    );
}

#[test]
fn recovery_never_includes_time_after_heartbeat() {
    let clock = ManualClock::at(5_000);
    let mut engine = TrackerEngine::new(clock.clone());
    start(&mut engine);
    clock.advance(Duration::from_secs(30));
    assert!(engine.apply(TrackerCommand::Heartbeat).is_ok());
    let snapshot = engine.snapshot().clone();
    clock.advance(Duration::from_secs(3_600));
    let mut recovered = TrackerEngine::restore(clock, snapshot, true);
    let pending = match &recovered.snapshot().state {
        TrackerState::RecoveryPending(pending) => pending,
        state => panic!("expected recovery, got {state:?}"),
    };
    assert_eq!(pending.proposed_end_ms, 35_000);
    let result = recovered.apply(TrackerCommand::ResolveRecovery {
        end_ms: 35_000,
        resume: false,
    });
    assert!(result.is_ok());
    assert_eq!(
        result
            .unwrap_or_else(|error| panic!("unexpected error: {error}"))
            .completed_entries[0]
            .end_ms,
        35_000
    );
}
```

Move `wall_clock_reversal_does_not_create_negative_entry` to the end of the
file if you want the same order as the original; order does not affect the
tests.

**What.** Three timelines with exact numbers. The first proves that the
resumed timer starts at the *recorded return* (610 s), not when the dialog
was answered (700 s). The second proves reassignment loses no time: two
entries summing to the full 10 s. The third proves recovery proposes the
heartbeat (35 s), even though an hour passed before restart.

**Rust — `match` in tests.** `match transition.snapshot.state { Running(active)
=> ..., state => panic!(...) }` is the standard way to assert on one variant
and get a readable failure for any other: `{state:?}` prints the actual
state. In the third test the match borrows (`&recovered.snapshot().state`)
because `recovered` is used again afterwards.

**Rust — `sum::<i64>()`.** `sum` is generic over the result type; the
*turbofish* `::<i64>` names it when inference cannot. `iter().map(|entry|
entry.duration_ms())` produces the `i64`s.

## 6.6 Property-based tests

```toml
# crates/core/Cargo.toml
[dev-dependencies]
proptest.workspace = true
# ... (serde_json if you kept the Chapter 3 exercise)
```

```rust
// crates/core/tests/properties.rs
use std::time::Duration;

use proptest::prelude::*;
use work_time_core::{
    IdleDecision, ManualClock, ProjectId, TrackerCommand, TrackerEngine, TrackerState,
};

proptest! {
    #[test]
    fn idle_reassignment_reconciles_the_original_timeline(
        focused_ms in 1_i64..3_600_000,
        idle_ms in 1_i64..3_600_000,
        answer_delay_ms in 0_i64..600_000,
    ) {
        let start_ms = 1_000_000_i64;
        let idle_start_ms = start_ms + focused_ms;
        let return_ms = idle_start_ms + idle_ms;
        let clock = ManualClock::at(start_ms);
        let mut engine = TrackerEngine::new(clock.clone());
        let started = engine.apply(TrackerCommand::Start {
            project_id: ProjectId(1),
            task_id: None,
            note: "focus".into(),
        });
        prop_assert!(started.is_ok());
        clock.advance(Duration::from_millis(
            u64::try_from(focused_ms + idle_ms).unwrap_or(u64::MAX),
        ));
        let idle = engine.apply(TrackerCommand::IdleDetected { idle_start_ms });
        prop_assert!(idle.is_ok());
        let returned = engine.apply(TrackerCommand::UserReturned { return_ms });
        prop_assert!(returned.is_ok());
        clock.advance(Duration::from_millis(
            u64::try_from(answer_delay_ms).unwrap_or(u64::MAX),
        ));
        let transition = engine.apply(TrackerCommand::ResolveIdle(
            IdleDecision::ReassignAndResume {
                project_id: ProjectId(2),
                task_id: None,
                note: "away".into(),
            },
        ));
        prop_assert!(transition.is_ok());
        let transition = transition.unwrap_or_else(|error| panic!("unexpected error: {error}"));
        let recorded: i64 = transition.completed_entries.iter().map(|entry| entry.duration_ms()).sum();
        prop_assert_eq!(recorded, focused_ms + idle_ms);
        prop_assert!(transition.completed_entries.iter().all(|entry| entry.duration_ms() > 0));
        match transition.snapshot.state {
            TrackerState::Running(active) => prop_assert_eq!(active.start_ms, return_ms),
            state => prop_assert!(false, "expected running, got {state:?}"),
        }
    }

    #[test]
    fn discard_idle_records_only_focused_time(
        focused_ms in 1_i64..3_600_000,
        idle_ms in 1_i64..3_600_000,
    ) {
        let start_ms = 10_000_i64;
        let idle_start_ms = start_ms + focused_ms;
        let return_ms = idle_start_ms + idle_ms;
        let clock = ManualClock::at(start_ms);
        let mut engine = TrackerEngine::new(clock.clone());
        let started = engine.apply(TrackerCommand::Start {
            project_id: ProjectId(1), task_id: None, note: String::new(),
        });
        prop_assert!(started.is_ok());
        clock.advance(Duration::from_millis(u64::try_from(focused_ms + idle_ms).unwrap_or(u64::MAX)));
        let idle = engine.apply(TrackerCommand::IdleDetected { idle_start_ms });
        prop_assert!(idle.is_ok());
        let returned = engine.apply(TrackerCommand::UserReturned { return_ms });
        prop_assert!(returned.is_ok());
        let transition = engine.apply(TrackerCommand::ResolveIdle(IdleDecision::DiscardAndResume));
        prop_assert!(transition.is_ok());
        let recorded = transition
            .unwrap_or_else(|error| panic!("unexpected error: {error}"))
            .completed_entries
            .iter()
            .map(|entry| entry.duration_ms())
            .sum::<i64>();
        prop_assert_eq!(recorded, focused_ms);
    }
}
```

**What.** Instead of one hand-picked timeline, each test runs 256 random
ones: focused for 1 ms to 1 h, idle for 1 ms to 1 h, answered after 0–10 min.
The *properties*: reassignment records exactly `focused + idle`, every entry
is non-empty, and the timer resumes at the return; discarding records
exactly `focused`.

**Why property tests here.** The example tests pin specific numbers. The
properties say what must hold for *all* numbers — the kind of statement a
reviewer wants to hear about a timeline algorithm. When a property fails,
proptest *shrinks* the input to the smallest case that still fails
(Exercise 1), which is usually the clearest bug report you will get.

**Rust — `proptest!`.** The macro turns `name in strategy` parameters into
generated inputs. `1_i64..3_600_000` is a *range strategy* (exclusive end);
the `_i64` suffix fixes the integer type. `prop_assert!` and
`prop_assert_eq!` are used instead of `assert!` so that a failure is reported
with the shrunk input rather than as a plain panic. rustfmt does not reformat
inside macros, which is why the second test has a compact one-line struct.

**Rust — `u64::try_from(...)`.** The clock advances by a `Duration`, whose
constructor takes `u64`; the test's numbers are `i64` for arithmetic with
timestamps. The fallible conversion is handled with a fallback, as in
Chapter 4.

## 6.7 Checkpoint

```sh
cargo test -p work-time-core
```

Expected (order within a file may vary):

```
     Running tests/properties.rs
test discard_idle_records_only_focused_time ... ok
test idle_reassignment_reconciles_the_original_timeline ... ok
test result: ok. 2 passed; ...
     Running tests/transitions.rs
test discard_idle_resumes_at_recorded_return_not_dialog_time ... ok
test reassign_idle_preserves_whole_timeline ... ok
test recovery_never_includes_time_after_heartbeat ... ok
test start_stop_records_exact_interval ... ok
test wall_clock_reversal_does_not_create_negative_entry ... ok
test result: ok. 5 passed; ...
```

```sh
cargo clippy -p work-time-core --all-targets -- -D warnings
cargo fmt --all -- --check
diff <(grep -v '^\s*//' ../work_time_tracker/crates/core/src/engine.rs) \
     <(grep -v '^\s*//' crates/core/src/engine.rs)
```

The diff must be empty. Same for `tests/transitions.rs` and
`tests/properties.rs`.

```sh
git add -A && git commit -m "Chapter 6: idle and recovery"
```

## 6.8 Exercises

1. **Watch proptest shrink.** In `resolve_idle`, in the `ReassignAndResume`
   arm, change the second `push_entry` call's `return_ms` to `return_ms - 1`.
   Run `cargo test -p work-time-core --test properties`.

   <details><summary>Answer</summary>

   ```
   ---- idle_reassignment_reconciles_the_original_timeline stdout ----
   proptest: Saving this and future failures in .../crates/core/tests/properties.proptest-regressions
   Test failed: assertion failed: `(left == right)`
     left: `1`,
    right: `2` at crates/core/tests/properties.rs:46.
   minimal failing input: focused_ms = 1, idle_ms = 1, answer_delay_ms = 0
           successes: 0
   test result: FAILED. 1 passed; 1 failed; ...
   ```

   The first random case failed; proptest then shrank all three inputs to
   the smallest values that still fail — 1 ms focused, 1 ms idle — where
   the off-by-one is obvious: 2 ms expected, 1 recorded. It also wrote a
   `properties.proptest-regressions` file so the same case is retried first
   next time; delete that file after reverting the bug (`rm
   crates/core/tests/*.proptest-regressions`).

   Try a subtler bug: make `push_entry` drop intervals shorter than 2 ms
   (`if end_ms - start_ms < 2`). The property tests still pass — 256 random
   draws from a 3.6-million range almost never land on 1. Random testing
   finds *common* failures; the example tests and the explicit `> 0` guard
   are still needed for the edges.
   </details>

2. **Struct update is not optional.** In the `ResolveRecovery` arm remove
   the line `..pending.active` and build.

   <details><summary>Answer</summary>

   ```
   error[E0063]: missing fields `note`, `project_id` and `task_id` in initializer of `model::ActiveTimer`
   ```

   Every field of a struct must be given a value; there are no defaults
   unless you use `..Default::default()` or another base value. `..` is how
   "the same as before, except these" is written.
   </details>

3. **Two tests worth having.** Append to `transitions.rs` (optional keepers):

   ```rust
   #[test]
   fn clean_shutdown_keeps_running_state() {
       let clock = ManualClock::at(1_000);
       let mut engine = TrackerEngine::new(clock.clone());
       start(&mut engine);
       let snapshot = engine.snapshot().clone();
       let restored = TrackerEngine::restore(clock, snapshot.clone(), false);
       assert_eq!(restored.snapshot(), &snapshot);
   }

   #[test]
   fn idle_before_start_is_rejected() {
       let clock = ManualClock::at(5_000);
       let mut engine = TrackerEngine::new(clock.clone());
       start(&mut engine);
       let error = engine
           .apply(TrackerCommand::IdleDetected { idle_start_ms: 4_000 })
           .err()
           .map(|error| error.to_string())
           .unwrap_or_default();
       assert_eq!(
           error,
           "idle start 4000 is outside the active interval beginning 5000"
       );
       assert!(matches!(engine.snapshot().state, TrackerState::Running(_)));
   }
   ```

   <details><summary>Answer</summary>

   Both pass (`7 passed`). The second uses `matches!(value, Pattern)`, a
   macro that returns `true` if the value fits the pattern — the short way to
   check a variant without binding anything. Note `assert_eq!(restored.snapshot(),
   &snapshot)`: the left side is a reference, so the right must be one too.
   </details>

## Recap

- `IdlePending` and `RecoveryPending` carry the original timer; resolving
  them partitions the timeline into entries without losing time.
- `restore` turns an unclean shutdown into a bounded recovery question.
- Ownership flows down into helpers (`pending`, `decision`) and results flow
  back up; `&mut Vec` collects outputs.
- Struct-update syntax copies the unchanged fields; `ok_or_else` + `?`
  handles the "should not happen" `None`.
- Property tests check invariants over hundreds of inputs and shrink
  failures to minimal cases; example tests still cover the edges.

Next: **Chapter 7 — Reports**, slices, iterators, `BTreeMap`, and local
midnight.
