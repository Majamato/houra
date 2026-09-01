# Chapter 13 — Actor

**Goal.** One thread owns the `Store` and the `TrackerEngine`. Everyone else
holds a `TrackerHandle` and sends requests over a channel, getting replies
back over another. With this chapter the headless application is complete:
every non-GTK file matches the original. Files: `crates/app/src/actor.rs`,
`crates/app/tests/storage.rs`, `lib.rs`.

**You will learn**

- OS threads with `std::thread`, and `move` closures.
- `std::sync::mpsc` channels, and one-shot reply channels.
- `Send` as a compile-time boundary; why the GTK side never touches SQLite.
- `JoinHandle`, `Option::take`, and a generic `request<T>(impl FnOnce)`.
- `Box` to shrink an enum; `while let`; `matches!`.
- The actor pattern and clone-before-commit.

**Prerequisite.** Chapter 12 checkpoint passed.

---

## 13.1 Requests and replies

```rust
// crates/app/src/actor.rs
//! Dedicated storage and state-machine thread.
//!
//! `rusqlite::Connection` and `TrackerEngine` never cross this thread boundary.
//! Callers exchange owned commands and immutable results over channels, keeping
//! GTK's main loop responsive and making ownership visible in the types.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use work_time_core::{
    Project, ProjectId, SystemClock, Task, TaskId, TimeEntry, TrackerCommand, TrackerEngine,
    TrackerSnapshot, Transition,
};

use crate::AppError;
use crate::backup::BackupDocument;
use crate::storage::Store;

/// One-shot channel the worker answers on.
type Reply<T> = Sender<Result<T, AppError>>;

/// Everything the worker can be asked. Each variant carries its reply channel.
enum Request {
    Apply(TrackerCommand, Reply<Transition>),
    Snapshot(Reply<TrackerSnapshot>),
    LiveElapsed(Reply<std::time::Duration>),
    Entries(i64, i64, Reply<Vec<TimeEntry>>),
    AddEntry(TimeEntry, Reply<work_time_core::EntryId>),
    UpdateEntry(TimeEntry, Reply<()>),
    Projects(bool, Reply<Vec<Project>>),
    Tasks(bool, Reply<Vec<Task>>),
    CreateProject(String, String, i64, Reply<ProjectId>),
    CreateTask(ProjectId, String, i64, Reply<TaskId>),
    ArchiveProject(ProjectId, bool, i64, Reply<()>),
    ArchiveTask(TaskId, bool, i64, Reply<()>),
    Backup(i64, Reply<BackupDocument>),
    Restore(Box<BackupDocument>, Reply<()>),
    Shutdown(Reply<()>),
}
```

**What.** The complete menu of things the worker thread does. Every variant
owns its arguments *and* a `Sender` on which exactly one reply will be sent.

**Why an actor.** SQLite connections are not meant to be shared across
threads, GTK widgets must stay on the main thread, and the engine must
process commands one at a time. Giving all three to one thread that
processes a queue solves all of it: no locks, no "who holds the mutex"
questions, and the GTK main loop never blocks on disk I/O longer than a
channel round trip. The test at the end proves two racing `Start` commands
are serialised.

**Rust — type alias.** `type Reply<T> = Sender<Result<T, AppError>>;` is a
name, not a new type. Read inside-out: a channel sender that carries a
`Result`.

**Rust — private enum, tuple-like variants.** `Request` is not `pub`:
nothing outside this file can construct one. The only public way in is
`TrackerHandle`'s methods, which keeps every request well-formed.
Tuple-like variants are fine here because each is constructed in exactly
one place, right below.

**Rust — `Box<BackupDocument>`.** An enum is as large as its largest
variant. `BackupDocument` holds three `Vec`s and a snapshot — 224 bytes —
which would make every queued `Request` that big. `Box` puts the document
on the heap and stores an 8-byte pointer in the variant (Exercise 2 prints
the numbers).

## 13.2 The handle

```rust
// crates/app/src/actor.rs
// ...

/// Cheap, cloneable, `Send` client of the storage thread.
#[derive(Clone)]
pub struct TrackerHandle {
    sender: Sender<Request>,
}

impl TrackerHandle {
    pub fn apply(&self, command: TrackerCommand) -> Result<Transition, AppError> {
        self.request(|reply| Request::Apply(command, reply))
    }

    pub fn snapshot(&self) -> Result<TrackerSnapshot, AppError> {
        self.request(Request::Snapshot)
    }

    pub fn live_elapsed(&self) -> Result<std::time::Duration, AppError> {
        self.request(Request::LiveElapsed)
    }

    pub fn entries(&self, start_ms: i64, end_ms: i64) -> Result<Vec<TimeEntry>, AppError> {
        self.request(|reply| Request::Entries(start_ms, end_ms, reply))
    }

    pub fn add_entry(&self, entry: TimeEntry) -> Result<work_time_core::EntryId, AppError> {
        self.request(|reply| Request::AddEntry(entry, reply))
    }

    pub fn update_entry(&self, entry: TimeEntry) -> Result<(), AppError> {
        self.request(|reply| Request::UpdateEntry(entry, reply))
    }

    pub fn projects(&self, include_archived: bool) -> Result<Vec<Project>, AppError> {
        self.request(|reply| Request::Projects(include_archived, reply))
    }

    pub fn tasks(&self, include_archived: bool) -> Result<Vec<Task>, AppError> {
        self.request(|reply| Request::Tasks(include_archived, reply))
    }

    pub fn create_project(
        &self,
        name: String,
        color: String,
        now_ms: i64,
    ) -> Result<ProjectId, AppError> {
        self.request(|reply| Request::CreateProject(name, color, now_ms, reply))
    }

    pub fn create_task(
        &self,
        project_id: ProjectId,
        name: String,
        now_ms: i64,
    ) -> Result<TaskId, AppError> {
        self.request(|reply| Request::CreateTask(project_id, name, now_ms, reply))
    }

    pub fn set_project_archived(
        &self,
        id: ProjectId,
        archived: bool,
        now_ms: i64,
    ) -> Result<(), AppError> {
        self.request(|reply| Request::ArchiveProject(id, archived, now_ms, reply))
    }

    pub fn set_task_archived(
        &self,
        id: TaskId,
        archived: bool,
        now_ms: i64,
    ) -> Result<(), AppError> {
        self.request(|reply| Request::ArchiveTask(id, archived, now_ms, reply))
    }

    pub fn backup(&self, exported_at_ms: i64) -> Result<BackupDocument, AppError> {
        self.request(|reply| Request::Backup(exported_at_ms, reply))
    }

    pub fn restore(&self, document: BackupDocument) -> Result<(), AppError> {
        self.request(|reply| Request::Restore(Box::new(document), reply))
    }

    pub fn shutdown(&self) -> Result<(), AppError> {
        self.request(Request::Shutdown)
    }

    fn request<T>(&self, build: impl FnOnce(Reply<T>) -> Request) -> Result<T, AppError> {
        let (sender, receiver) = mpsc::channel();
        self.sender
            .send(build(sender))
            .map_err(|_| AppError::WorkerStopped)?;
        receiver.recv().map_err(|_| AppError::WorkerStopped)?
    }
}
```

**What.** A public method per request. Each one is a one-liner delegating
to `request`, which creates a fresh reply channel, wraps it into a
`Request` using the closure, sends it, and blocks until the answer arrives.

**Why the handle takes owned values.** `create_project(name: String, ...)`
rather than `&str`: the string is going to *another thread*, which may
outlive the caller's borrow. Sending owned data is what makes this safe —
the compiler would refuse a `&str` here because the request must be
`'static`.

**Rust — channels.** `mpsc::channel()` returns a `(Sender, Receiver)` pair:
*multi-producer, single-consumer*. Cloning a `Sender` adds a producer; the
`Receiver` stays unique. Sending moves the value into the queue. `recv()`
blocks until a message arrives or every `Sender` has been dropped, in which
case it returns `Err` — that is how a dead worker is detected
(`WorkerStopped`, Exercise 4).

**Rust — `request<T>` with `impl FnOnce`.** The method is generic over the
reply type `T` and takes *any closure* that, given a `Reply<T>`, builds a
`Request`. `FnOnce` is the loosest closure bound: the closure may consume
what it captured (it moves `command`, `entry`, `name` into the variant) and
is called exactly once. `T` is inferred from each public method's return
type. Where a variant has *only* a reply — `Request::Snapshot` — the variant
constructor itself is passed as the closure.

**Rust — `map_err(|_| ...)`.** `send` fails only if the receiver is gone;
its error contains the unsent value, which is not useful here, so `_`
discards it and the app-level error replaces it. The final `?` unwraps the
outer `Result` from `recv`, leaving the inner `Result<T, AppError>` as the
return value.

**Rust — `#[derive(Clone)]` on the handle.** Cloning a handle clones the
`Sender`. `Sender` is `Send`, so handles can be moved to any thread — the
GTK thread, the D-Bus callbacks, a test's spawned thread.

## 13.3 The service

```rust
// crates/app/src/actor.rs
// ...

/// Owns the storage thread; `shutdown` joins it.
pub struct TrackerService {
    pub handle: TrackerHandle,
    join: Option<JoinHandle<()>>,
}

impl TrackerService {
    pub fn start(path: PathBuf) -> Result<Self, AppError> {
        let mut store = Store::open(&path)?;
        let snapshot = store.load_snapshot()?;
        let unclean = !store.previous_shutdown_clean() && snapshot.state.active().is_some();
        let engine = TrackerEngine::restore(SystemClock::default(), snapshot, unclean);
        let (sender, receiver) = mpsc::channel();
        let join = thread::Builder::new()
            .name("work-time-storage".into())
            .spawn(move || worker_loop(&mut store, engine, receiver))
            .map_err(|source| AppError::io("storage worker", source))?;
        Ok(Self {
            handle: TrackerHandle { sender },
            join: Some(join),
        })
    }

    /// Asks the worker to finish, then waits for the thread to end.
    pub fn shutdown(mut self) -> Result<(), AppError> {
        let result = self.handle.shutdown();
        if let Some(join) = self.join.take() {
            join.join().map_err(|_| AppError::WorkerStopped)?;
        }
        result
    }
}
```

**What.** `start` opens the store and restores the engine *on the calling
thread*, so a bad database path is an ordinary error returned to `main`,
then spawns the worker and hands it the store, engine and receiver.
`shutdown` sends the last request and waits for the thread.

**Why restore before spawning.** Errors inside a spawned thread are awkward
to report; errors before it are just `?`. Everything that can fail at
start-up fails here.

**Rust — `thread::Builder` and `move`.** `spawn(move || ...)` runs the
closure on a new OS thread. `move` transfers ownership of `store`, `engine`
and `receiver` into the closure; without it, the closure would *borrow*
them from `start`'s stack frame, which ends before the thread does
(Exercise 1). This is the moment `Store` crosses to its owner-thread — and
the last time any other thread can name it. `.name(...)` labels the thread
in debuggers and `journalctl`.

**Rust — `JoinHandle` and `Option::take`.** Joining a thread consumes the
handle, but `shutdown` only has `&mut self`-style access to the field
through `mut self`. `self.join.take()` swaps the `Option` to `None` and
returns the handle by value, so the join happens exactly once. `join()`
returns `Err` only if the thread panicked.

**Rust — `shutdown(mut self)`.** Taking `self` by value means the service
cannot be used after shutdown — the compiler enforces "shut down once".

## 13.4 The worker loop

```rust
// crates/app/src/actor.rs
// ...

fn worker_loop(
    store: &mut Store,
    mut engine: TrackerEngine<SystemClock>,
    receiver: Receiver<Request>,
) {
    while let Ok(request) = receiver.recv() {
        let should_stop = matches!(request, Request::Shutdown(_));
        match request {
            Request::Apply(command, reply) => {
                let mut candidate = engine.clone();
                let result =
                    candidate
                        .apply(command)
                        .map_err(AppError::from)
                        .and_then(|transition| {
                            store.persist_transition(&transition)?;
                            engine = candidate;
                            Ok(transition)
                        });
                let _ignored = reply.send(result);
            }
            Request::Snapshot(reply) => {
                let _ignored = reply.send(Ok(engine.snapshot().clone()));
            }
            Request::LiveElapsed(reply) => {
                let _ignored = reply.send(Ok(engine.live_elapsed()));
            }
            Request::Entries(start, end, reply) => {
                let _ignored = reply.send(store.list_entries(start, end));
            }
            Request::AddEntry(entry, reply) => {
                let _ignored = reply.send(store.add_entry(&entry));
            }
            Request::UpdateEntry(entry, reply) => {
                let _ignored = reply.send(store.update_entry(&entry));
            }
            Request::Projects(include_archived, reply) => {
                let _ignored = reply.send(store.list_projects(include_archived));
            }
            Request::Tasks(include_archived, reply) => {
                let _ignored = reply.send(store.list_tasks(include_archived));
            }
            Request::CreateProject(name, color, now, reply) => {
                let _ignored = reply.send(store.create_project(&name, &color, now));
            }
            Request::CreateTask(project_id, name, now, reply) => {
                let _ignored = reply.send(store.create_task(project_id, &name, now));
            }
            Request::ArchiveProject(id, archived, now, reply) => {
                let _ignored = reply.send(store.set_project_archived(id, archived, now));
            }
            Request::ArchiveTask(id, archived, now, reply) => {
                let _ignored = reply.send(store.set_task_archived(id, archived, now));
            }
            Request::Backup(now, reply) => {
                let _ignored = reply.send(store.backup(now));
            }
            Request::Restore(document, reply) => {
                let result = store.restore(&document).and_then(|()| {
                    let snapshot = store.load_snapshot()?;
                    engine = TrackerEngine::restore(SystemClock::default(), snapshot, false);
                    Ok(())
                });
                let _ignored = reply.send(result);
            }
            Request::Shutdown(reply) => {
                let result = if engine.snapshot().state.active().is_some() {
                    Ok(())
                } else {
                    store.mark_clean_shutdown()
                };
                let _ignored = reply.send(result);
            }
        }
        if should_stop {
            break;
        }
    }
}
```

**What.** Receive, dispatch, reply, repeat. The `Apply` arm is the heart of
the application: clone the engine, apply the command to the clone, persist
the transition, and *only then* replace the live engine with the clone.
`Shutdown` marks the database clean only if no timer is running.

**Why clone before commit.** If `persist_transition` fails (disk full,
constraint), the live `engine` was never touched: the UI keeps showing the
state that is actually on disk. Cloning an engine is cheap — a small clock
handle and one snapshot.

**Why a running timer keeps the marker dirty.** Logging out shuts the app
down cleanly *while a timer runs*. If that marked the database clean, the
next start would resume the timer as if the hours in between were work.
Leaving it dirty routes the next start through recovery, bounded by the
last heartbeat (Exercise 3).

**Rust — `while let Ok(request) = receiver.recv()`.** Loop until the
channel closes. Every `Sender` dropping — every handle gone — ends the loop
naturally; `Shutdown` ends it explicitly with `break`.

**Rust — `matches!` before `match`.** `matches!(request, Request::Shutdown(_))`
tests the variant without moving anything; the `match` below then *moves*
the payloads out of `request`, so the check has to come first.

**Rust — `and_then` with a closure that mutates.** `.and_then(|transition|
{ ...; engine = candidate; Ok(transition) })` runs only on `Ok`; inside, `?`
works because the closure returns a `Result`. The closure assigns to
`engine`, which it captures by mutable reference — allowed because
`worker_loop` owns `engine` (`mut engine`). `map_err(AppError::from)` first
lifts the `DomainError` so both branches share one error type.

**Rust — `let _ignored = reply.send(...)`.** `send` fails if the caller
already gave up waiting (its `Receiver` was dropped). The worker has nothing
to do about that, so the `Result` is bound to a named `_` variable — the
lint-clean way to say "I know, and I don't care".

**Rust — `&mut Store` parameter.** The worker receives `&mut store` from the
closure that owns it; the store lives exactly as long as the thread.

Register:

```rust
// crates/app/src/lib.rs
pub mod actor;
pub mod autostart;
// ...
pub use actor::{TrackerHandle, TrackerService};
pub use error::AppError;
```

## 13.5 The concurrency test

Append to `tests/storage.rs` (extend the `use` lines):

```rust
// crates/app/tests/storage.rs
use tempfile::TempDir;
use work_time_core::{EntryId, EntrySource, ProjectId, TaskId, TimeEntry, TrackerSnapshot};
use work_time_tracker::TrackerService;
use work_time_tracker::backup::BackupDocument;
use work_time_tracker::storage::Store;

// ...

#[test]
fn concurrent_start_commands_are_serialized() {
    let directory = TempDir::new().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let service = TrackerService::start(directory.path().join("actor.sqlite3"))
        .unwrap_or_else(|error| panic!("service failed: {error}"));
    let first = service.handle.clone();
    let second = service.handle.clone();
    let command = || work_time_core::TrackerCommand::Start {
        project_id: ProjectId(1),
        task_id: None,
        note: "concurrent".into(),
    };
    let first_join = std::thread::spawn(move || first.apply(command()));
    let second_join = std::thread::spawn(move || second.apply(command()));
    let first_result = first_join
        .join()
        .unwrap_or_else(|_| panic!("first client panicked"));
    let second_result = second_join
        .join()
        .unwrap_or_else(|_| panic!("second client panicked"));
    assert_ne!(first_result.is_ok(), second_result.is_ok());
    assert!(
        service
            .handle
            .apply(work_time_core::TrackerCommand::Stop)
            .is_ok()
    );
    assert!(service.shutdown().is_ok());
}
```

**What.** Two threads race to start the timer. Exactly one succeeds; the
other reaches the engine when it is already `Running` and gets
`InvalidState`. Then the test stops the timer and shuts down cleanly.

**Rust — closures that build values.** `let command = || TrackerCommand::Start
{ ... }` is a closure with no parameters, called twice to get two owned
commands (a `TrackerCommand` is not `Copy`, so one value could not be sent
twice). It captures nothing, so it is `Copy` itself and can be used in both
spawned closures.

**Rust — `thread::spawn` returns the closure's value.** `first.apply(...)`
is the closure's last expression; `join()` yields `Result<that, panic>`.
`assert_ne!` checks the two outcomes differ.

## 13.6 Checkpoint — end of Part 2

```sh
cargo test --workspace
```

Expected: 19 tests passing across `properties` (2), `report` (3),
`transitions` (5), `storage` (9), plus any keepers.

```sh
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
for f in src/actor.rs src/autostart.rs src/backup.rs src/error.rs src/export.rs \
         src/main.rs src/settings.rs src/storage.rs tests/storage.rs; do
  echo "== $f"
  diff <(grep -v '^\s*//' ../work_time_tracker/crates/app/$f) \
       <(grep -v '^\s*//' crates/app/$f)
done
```

All empty. `lib.rs` differs by the two `native` lines Chapter 14 adds.

```sh
git add -A && git commit -m "Chapter 13: actor (headless app complete)"
```

## 13.7 Exercises

1. **Why `move`.** Remove the word `move` from `.spawn(move || ...)` and
   build.

   <details><summary>Answer</summary>

   ```
   error[E0373]: closure may outlive the current function, but it borrows `store`, which is owned by the current function
       |
   153 |             .spawn(|| worker_loop(&mut store, engine, receiver))
       |                    ^^                  ----- `store` is borrowed here
       |                    |
       |                    may outlive borrowed value `store`
   note: function requires argument type to outlive `'static`
   ```

   A thread may run forever; a stack frame does not. `spawn` therefore
   requires a `'static` closure — one that borrows nothing from the caller.
   `move` makes the closure own its captures. This is the same `'static`
   from the `Clock` bounds in Chapter 4.
   </details>

2. **Why `Box`.** Append to `actor.rs`:

   ```rust
   #[cfg(test)]
   mod sizes {
       use super::*;

       #[test]
       fn show_sizes() {
           println!(
               "Request={} BackupDocument={} Box<BackupDocument>={}",
               std::mem::size_of::<Request>(),
               std::mem::size_of::<BackupDocument>(),
               std::mem::size_of::<Box<BackupDocument>>()
           );
           panic!("show");
       }
   }
   ```

   Run `cargo test -p work-time-tracker --lib show_sizes`. Then change the
   variant to `Restore(BackupDocument, Reply<()>)` (and `Box::new(document)`
   to `document` in the handle) and run again.

   <details><summary>Answer</summary>

   ```
   Request=128 BackupDocument=224 Box<BackupDocument>=8      # boxed
   Request=240 BackupDocument=224 Box<BackupDocument>=8      # unboxed
   ```

   Without the box, every `Request` — even `Snapshot` — takes 240 bytes on
   the channel because the enum must fit its largest variant. Boxing the
   one rare, big payload keeps the common ones small. Revert.
   </details>

3. **Recovery after a "clean" exit with a running timer (optional
   keeper).** Append to `tests/storage.rs`:

   ```rust
   #[test]
   fn shutdown_with_a_running_timer_leads_to_recovery_on_restart() {
       use work_time_core::{TrackerCommand, TrackerState};

       let directory = TempDir::new().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
       let path = directory.path().join("actor.sqlite3");
       let service = TrackerService::start(path.clone())
           .unwrap_or_else(|error| panic!("service failed: {error}"));
       assert!(
           service
               .handle
               .apply(TrackerCommand::Start {
                   project_id: ProjectId(1),
                   task_id: None,
                   note: "late".into(),
               })
               .is_ok()
       );
       assert!(service.shutdown().is_ok());

       let restarted = TrackerService::start(path)
           .unwrap_or_else(|error| panic!("service failed: {error}"));
       let snapshot = restarted
           .handle
           .snapshot()
           .unwrap_or_else(|error| panic!("snapshot failed: {error}"));
       assert!(matches!(snapshot.state, TrackerState::RecoveryPending(_)));
       assert!(restarted.shutdown().is_ok());
   }
   ```

   <details><summary>Answer</summary>

   Passes. The whole chain from Chapters 6, 9 and 13 in one test: shutdown
   leaves the marker dirty because a timer ran; `start` reads the dirty
   marker and a `Running` snapshot; `TrackerEngine::restore` turns it into
   `RecoveryPending`.
   </details>

4. **A dead worker is an error, not a hang (optional keeper).** Append:

   ```rust
   #[test]
   fn handle_after_shutdown_reports_worker_stopped() {
       let directory = TempDir::new().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
       let service = TrackerService::start(directory.path().join("actor.sqlite3"))
           .unwrap_or_else(|error| panic!("service failed: {error}"));
       let handle = service.handle.clone();
       assert!(service.shutdown().is_ok());
       let message = handle.snapshot().err().map(|error| error.to_string()).unwrap_or_default();
       assert_eq!(message, "the storage worker stopped unexpectedly");
   }
   ```

   <details><summary>Answer</summary>

   Passes. After the worker loop `break`s, its `Receiver` is dropped; the
   next `send` from any surviving handle fails, and `request` maps that to
   `WorkerStopped` instead of blocking forever.
   </details>

## Recap

- The storage thread owns `Store` and `TrackerEngine`; nothing else can
  touch them — `move` hands them over once.
- `TrackerHandle` is a cloneable `Sender`; each request carries its own
  reply channel; `request<T>` is one generic function behind fifteen
  methods.
- `Apply` works on a clone and installs it only after the transaction
  commits.
- A dropped channel end is how "the worker is gone" is detected.
- Part 2 is done: the app is complete except for its window.

Next: **Chapter 14 — First GTK window**, where the build script, GResources
and a GObject subclass put a button on screen.
