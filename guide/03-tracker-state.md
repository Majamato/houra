# Chapter 3 — Tracker state

**Goal.** `model.rs` gets the types that describe *what the timer is doing*
and *what can happen to it*: `ActiveTimer`, the two "pending" records, the
`TrackerState` enum, `TrackerSnapshot`, and the vocabulary of change —
`TrackerCommand`, `IdleDecision`, `Notification`, `Transition`. The deferred
`DomainError::InvalidState` variant is added. No behaviour yet; that is
Chapter 5. Files: `crates/core/src/model.rs`, `crates/core/src/error.rs`.

**You will learn**

- Enums whose variants carry data, and why one enum beats several booleans.
- Exhaustive `match`, and returning a borrow (`Option<&T>`) out of `&self`.
- Struct-like versus tuple-like variants, and `#[default]` on enums.
- What serde's `tag`/`content` attributes do to the JSON.
- The difference between wall-clock and monotonic time, as fields.

**Prerequisite.** Chapter 2 checkpoint passed.

---

## 3.0 The state machine you are about to type

Read this once; it is what the rest of the chapter encodes.

```
                 Start                     Stop
   Stopped  ─────────────►  Running  ──────────────►  Stopped
      ▲                     │  ▲  │
      │                     │  │  └── EditActive / Heartbeat (stays Running)
      │          IdleDetected  │
      │                     ▼  │ ResolveIdle(Keep / DiscardAndResume / ReassignAndResume)
      │                IdlePending ── UserReturned (stays IdlePending, return_ms now Some)
      │                     │
      └───────── ResolveIdle(Stop)

   Running or IdlePending  ── app dies ──►  RecoveryPending (decided at next start-up)
   RecoveryPending ── ResolveRecovery{resume:true} ──► Running
   RecoveryPending ── ResolveRecovery{resume:false} / DiscardRecovery ──► Stopped
```

Four states, nine commands. Every arrow above is one arm of a `match` in
Chapter 5; every arrow *not* above is an error, `InvalidState`.

## 3.1 The active timer and the two pending records

Insert the following in `model.rs` **between** the end of `impl TimeEntry
{ ... }` and `pub fn validate_name` (keeping the validation functions at the
bottom matches the original file's layout, which makes diffs easy).

```rust
// crates/core/src/model.rs
// ... impl TimeEntry { ... }

/// The interval currently being tracked.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ActiveTimer {
    pub project_id: ProjectId,
    pub task_id: Option<TaskId>,
    pub note: String,
    /// Wall-clock start, stored on disk.
    pub start_ms: i64,
    /// Monotonic start, used only for the live display.
    pub started_monotonic_ms: u64,
    /// Last instant the running timer was known to be alive.
    pub last_heartbeat_ms: i64,
}

/// A running timer whose user went away; waiting for their decision.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PendingIdle {
    pub active: ActiveTimer,
    pub idle_start_ms: i64,
    /// Set once the user is back; `None` while they are still away.
    pub return_ms: Option<i64>,
}

/// A timer that was running when the app stopped uncleanly.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PendingRecovery {
    pub active: ActiveTimer,
    pub proposed_end_ms: i64,
    pub unresolved_idle_start_ms: Option<i64>,
}

// ... pub fn validate_name ...
```

**What.** `ActiveTimer` is a `TimeEntry` that has not ended yet: same
project/task/note, a start, but no end. `PendingIdle` wraps an active timer
plus the moment the user went idle and (later) the moment they came back.
`PendingRecovery` wraps an active timer plus the end time the app will
*propose* after a crash.

**Why three time fields on `ActiveTimer`.**

- `start_ms` — real-world UTC milliseconds. It goes to disk and into reports.
- `started_monotonic_ms` — from a clock that only moves forward and restarts
  at zero with each process. It is used for the live `00:12:34` display, so a
  system clock correction (NTP, time-zone change) cannot make the counter jump
  backwards. It is meaningless after a restart, which is fine: the display is
  recomputed from `start_ms` then.
- `last_heartbeat_ms` — updated every 30 seconds while running. If the
  machine loses power, recovery proposes *this* as the end, never "now",
  because nothing is known about the time in between.

The two `Option<i64>` fields encode a fact about the flow: `return_ms` is
`None` until the user is back; `unresolved_idle_start_ms` is `Some` only when
the crash happened while an idle question was already open.

**Rust — `u64`.** Unsigned 64-bit: a monotonic duration is never negative,
so the type says so. `i64` stays for wall time because dates before 1970 are
representable (and SQLite stores signed integers).

**Idiom.** Make the type say what the field means. `Option` for "not known
yet", unsigned for "cannot be negative", a doc comment for the unit.

## 3.2 One enum for the state

```rust
// crates/core/src/model.rs
// ...

/// Exactly one of these holds at any time.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TrackerState {
    #[default]
    Stopped,
    Running(ActiveTimer),
    IdlePending(PendingIdle),
    RecoveryPending(PendingRecovery),
}

impl TrackerState {
    /// The timer being tracked, if any state carries one.
    pub fn active(&self) -> Option<&ActiveTimer> {
        match self {
            Self::Running(active) => Some(active),
            Self::IdlePending(pending) => Some(&pending.active),
            Self::RecoveryPending(pending) => Some(&pending.active),
            Self::Stopped => None,
        }
    }
}

/// The persisted tracker state plus a counter that grows with every accepted command.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct TrackerSnapshot {
    pub state: TrackerState,
    pub revision: u64,
}
```

**What.** The tracker is in exactly one of four states, and three of them
carry data. `active()` answers "is a timer running underneath?" for any
state. `TrackerSnapshot` is what gets saved: the state plus a revision number.

**Why one enum.** The tempting alternative is a struct with
`running: bool`, `idle: bool`, `recovering: bool`, `active: Option<ActiveTimer>`.
That struct can be `running == false` with `active == Some(...)`, or idle and
recovering at once. Every function would have to check for those nonsense
combinations. With the enum they cannot be written at all: `Stopped` has no
timer, `Running` must have one. This is the Rust habit called *make invalid
states unrepresentable*, and it is the reason the state machine in Chapter 5
needs no defensive checks.

**Rust — enums with data.** `Running(ActiveTimer)` is a *tuple-like* variant
with one unnamed field; construct it with `TrackerState::Running(timer)`. A
`Stopped` variant with no data is *unit-like*. `#[default]` marks which
variant `TrackerState::default()` produces — it can only be a unit-like one.

**Rust — `match` is exhaustive.** `match self { ... }` must list every
variant or the code does not compile (Exercise 1 shows the message). Inside
an `impl`, `Self::Running(active)` is short for `TrackerState::Running(active)`.
Because `self` is a `&TrackerState`, matching binds `active` as a
`&ActiveTimer` automatically; `&pending.active` borrows a field of the borrowed
`PendingIdle`.

**Rust — returning a borrow.** The return type `Option<&ActiveTimer>` says
the caller gets a *reference into `self`*, not a copy. Nothing is cloned. The
compiler then makes sure the caller cannot keep that reference after the state
it points into is gone (Exercise 2). Where the original code would have needed
documentation ("do not use after…"), Rust has a type.

**Rust — `serde(tag, content)`.** By default serde writes an enum with data
as `{"Running": {...}}`. `tag = "kind", content = "value"` changes it to
`{"kind": "running", "value": {...}}` — an explicit discriminator field that
tools and humans can read, and one that stays stable if variants are renamed
in Rust. `rename_all = "snake_case"` lowercases the variant names. Exercise 4
prints it.

**Dart.** This is a `sealed class` hierarchy with an exhaustive `switch`. The
difference: Rust enums are plain values, not objects, and matching moves or
borrows the payload rather than downcasting.

## 3.3 The deferred error variant

Now that `TrackerState` exists, add the variant Chapter 2 left out. It goes
first in the enum:

```rust
// crates/core/src/error.rs
use thiserror::Error;

use crate::{EntryId, ProjectId, TaskId, TrackerState};

/// A rejected domain operation. Infrastructure errors belong in the app crate.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("the tracker is not in the required state (current state: {0:?})")]
    InvalidState(TrackerState),
    #[error("end time {end_ms} must be after start time {start_ms}")]
    InvalidInterval { start_ms: i64, end_ms: i64 },
    // ...
}
```

**What.** The error returned for every arrow that is *not* in the diagram —
`Stop` while stopped, `Start` while running, `ResolveIdle` while nothing is
pending. It carries the state that was found so the message can show it.

**Rust — `{0:?}`.** In a tuple-like variant the fields are numbered; `{0}`
is the first one. `:?` asks for `Debug` formatting, since `TrackerState` has
no `Display` (and should not: it is data, not a message).

```sh
cargo build -p work-time-core
```

## 3.4 Commands, decisions, notifications, transitions

Still above `validate_name`, after `TrackerSnapshot`:

```rust
// crates/core/src/model.rs
// ...

/// How the user wants an idle interval to be counted.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum IdleDecision {
    Keep,
    DiscardAndResume,
    ReassignAndResume {
        project_id: ProjectId,
        task_id: Option<TaskId>,
        note: String,
    },
    Stop,
}

/// A request to change the tracker state.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum TrackerCommand {
    Start {
        project_id: ProjectId,
        task_id: Option<TaskId>,
        note: String,
    },
    Stop,
    EditActive {
        project_id: ProjectId,
        task_id: Option<TaskId>,
        note: String,
    },
    IdleDetected {
        idle_start_ms: i64,
    },
    UserReturned {
        return_ms: i64,
    },
    ResolveIdle(IdleDecision),
    ResolveRecovery {
        end_ms: i64,
        resume: bool,
    },
    DiscardRecovery,
    Heartbeat,
}

/// Something the interface may want to tell the user after a transition.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum Notification {
    TimerStarted,
    TimerStopped,
    IdleNeedsResolution,
    RecoveryNeedsResolution,
    RecoveryResolved,
}

/// The result of applying one command.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Transition {
    pub snapshot: TrackerSnapshot,
    pub completed_entries: Vec<TimeEntry>,
    pub notifications: Vec<Notification>,
}
```

**What.** `TrackerCommand` is everything the outside world can ask of the
timer — from a button, a keyboard shortcut, a D-Bus signal, or a 30-second
timer. `IdleDecision` is the answer to the "you were away" dialog. Applying
a command yields a `Transition`: the new snapshot, zero or more entries that
became complete, and notifications the UI may show.

**Why commands are data.** The GTK window will not call `engine.stop()`; it
will build `TrackerCommand::Stop` and send it to another thread (Chapter 13).
A command that is a plain value can be sent through a channel, logged, stored
in a test, and serialised. That is why it derives `Serialize` even though the
program never writes commands to disk — the derive is cheap, and tests can
print them.

**Why `Transition` carries entries.** Stopping a timer produces one entry;
resolving idle time by *reassigning* produces two (focused time, then the
idle interval on another project). The engine returns them in a `Vec`; the
storage layer writes them and the snapshot in one transaction (Chapter 9), so
memory and disk never disagree.

**Rust — mixed variant shapes.** `TrackerCommand` mixes unit-like (`Stop`),
struct-like (`Start { ... }`), and tuple-like (`ResolveIdle(IdleDecision)`)
variants — an enum can nest another enum. Use struct-like variants whenever a
variant has more than one field of the same type; `IdleDetected { idle_start_ms }`
cannot be confused with `UserReturned { return_ms }` even though both are one
`i64`.

**Rust — `Vec<T>`.** An owned, growable array. `Vec::new()` is empty and
allocates nothing until the first push. `completed_entries` is usually empty
or has one element; a `Vec` costs nothing extra for that.

**Idiom.** Inputs and outputs of a core module are plain data types with no
behaviour attached (`Command` in, `Transition` out). Behaviour lives in one
place — the engine — which makes it testable by constructing values.

## 3.5 Checkpoint

```sh
cargo build -p work-time-core
cargo clippy -p work-time-core --all-targets -- -D warnings
cargo fmt --all -- --check
```

All clean. Then compare with the original:

```sh
diff <(grep -v '^\s*//' ../work_time_tracker/crates/core/src/model.rs) \
     <(grep -v '^\s*//' crates/core/src/model.rs)
```

Expected: only the position of `use crate::DomainError;` (the original puts
it before `use serde`; rustfmt accepts both). If you kept the unit tests from
Chapter 2 exercise 5, they also show up at the bottom. Nothing else.

```sh
git add -A && git commit -m "Chapter 3: tracker state"
```

## 3.6 Exercises

Revert each with `git checkout -- .` unless told otherwise.

1. **`match` must be complete.** Delete the line `Self::Stopped => None,`
   in `TrackerState::active` and build.

   <details><summary>Answer</summary>

   ```
   error[E0004]: non-exhaustive patterns: `&model::TrackerState::Stopped` not covered
      --> crates/core/src/model.rs:163:15
       |
   163 |         match self {
       |               ^^^^ pattern `&model::TrackerState::Stopped` not covered
   ```

   The compiler names the missing variant. This is why adding a fifth state
   later would be safe: every `match` on `TrackerState` in the program stops
   compiling until it handles the new case. A `_ => ...` catch-all arm would
   silence this, which is exactly why the code avoids one here.
   </details>

2. **A borrow cannot outlive its owner.** Append:

   ```rust
   #[cfg(test)]
   mod borrows {
       use super::*;

       #[test]
       fn borrow_cannot_outlive_owner() {
           let snapshot = TrackerSnapshot::default();
           let active = snapshot.state.active();
           drop(snapshot);
           println!("{active:?}");
       }
   }
   ```

   <details><summary>Answer</summary>

   ```
   error[E0505]: cannot move out of `snapshot` because it is borrowed
       |
   266 |         let snapshot = TrackerSnapshot::default();
       |             -------- binding `snapshot` declared here
   267 |         let active = snapshot.state.active();
       |                      -------------- borrow of `snapshot.state` occurs here
   268 |         drop(snapshot);
       |              ^^^^^^^^ move out of `snapshot` occurs here
   269 |         println!("{active:?}");
       |                    ------ borrow later used here
   ```

   `active()` returned a reference *into* `snapshot`. `drop(snapshot)` would
   free the memory it points to, and the later `println!` would read freed
   memory — in C that is a use-after-free bug; here it is a compile error.
   The compiler tracks how long every borrow is used ("borrow later used
   here"); that tracking is the *borrow checker*. Remove the `println!` line
   and it compiles: a borrow that is never used again may end before the drop.
   </details>

3. **Enums move too.** Append:

   ```rust
   #[cfg(test)]
   mod moves {
       use super::*;

       #[test]
       fn enums_move_too() {
           let first = Notification::TimerStarted;
           let second = first;
           assert_eq!(first, second);
       }
   }
   ```

   Run `cargo test -p work-time-core`. Then add `Copy` to `Notification`'s
   derive list and run again.

   <details><summary>Answer</summary>

   ```
   error[E0382]: borrow of moved value: `first`
       |
   266 |         let first = Notification::TimerStarted;
       |             ----- move occurs because `first` has type `model::Notification`, which does not implement the `Copy` trait
   267 |         let second = first;
       |                      ----- value moved here
   268 |         assert_eq!(first, second);
       |         ^^^^^^^^^^^^^^^^^^^^^^^^^ value borrowed here after move
   ```

   With `Copy` derived: `test model::moves::enums_move_too ... ok`.

   Move-by-default applies to every type, even a data-less enum. `Copy` is
   opt-in and only allowed for types whose bits can be duplicated safely (no
   `String`, no `Vec`). `Notification` *could* be `Copy`; the original does
   not bother because it is only ever pushed into a `Vec`. `EntrySource` and
   the ID types are `Copy` because they are passed around constantly.
   </details>

4. **See the JSON (optional keeper).** Add a dev-dependency:

   ```toml
   # crates/core/Cargo.toml
   [dev-dependencies]
   serde_json.workspace = true
   ```

   and append:

   ```rust
   #[cfg(test)]
   mod json {
       use super::*;

       #[test]
       fn state_round_trips_through_json() {
           let state = TrackerState::Running(ActiveTimer {
               project_id: ProjectId(1),
               task_id: None,
               note: "design".into(),
               start_ms: 1_000,
               started_monotonic_ms: 0,
               last_heartbeat_ms: 1_000,
           });
           let json = serde_json::to_string(&state).unwrap_or_default();
           println!("{json}");
           let back: TrackerState = serde_json::from_str(&json).unwrap_or_default();
           assert_eq!(back, state);

           let stopped = serde_json::to_string(&TrackerState::Stopped).unwrap_or_default();
           println!("{stopped}");
           let decision = serde_json::to_string(&IdleDecision::ReassignAndResume {
               project_id: ProjectId(2),
               task_id: Some(TaskId(9)),
               note: String::new(),
           })
           .unwrap_or_default();
           println!("{decision}");
       }
   }
   ```

   Run `cargo test -p work-time-core json -- --nocapture`.

   <details><summary>Answer</summary>

   ```
   {"kind":"running","value":{"project_id":1,"task_id":null,"note":"design","start_ms":1000,"started_monotonic_ms":0,"last_heartbeat_ms":1000}}
   {"kind":"stopped"}
   {"action":"reassign_and_resume","project_id":2,"task_id":9,"note":""}
   test model::json::state_round_trips_through_json ... ok
   ```

   Three things to see: `tag`/`content` put the variant name in `"kind"` and
   the payload in `"value"`; `#[serde(transparent)]` on the IDs makes
   `project_id` a bare `1`; `IdleDecision` uses `tag` without `content`, so
   the fields sit next to `"action"`. `null` is `None`. This exact shape is
   what Chapter 9 stores in the `tracker_state` table, which is why the
   attributes matter: changing them would make old databases unreadable.

   `unwrap_or_default()` is used instead of `unwrap()` because the lint denies
   `unwrap` even in tests; on failure `assert_eq!` still catches the mismatch.
   `-- --nocapture` tells the test harness to show `println!` output from
   passing tests. Keeping this test is fine: the original crate lists
   `serde_json` as a dev-dependency too.
   </details>

## Recap

- Four states in one enum; three carry data; impossible combinations cannot
  be written.
- `match` is exhaustive; `active()` hands out a borrow the compiler polices.
- Commands, decisions, notifications and transitions are plain data; the
  engine (Chapter 5) is the only place with behaviour.
- `serde` attributes decide the on-disk JSON; the ID newtypes disappear into
  bare numbers.
- `start_ms` for history, `started_monotonic_ms` for the live counter,
  `last_heartbeat_ms` for crash recovery.

Next: **Chapter 4 — Clock**, a trait that lets tests move time without
waiting.
