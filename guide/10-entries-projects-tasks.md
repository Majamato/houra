# Chapter 10 — Entries, projects, tasks

**Goal.** `Store` learns to write: persist a transition, add and update
entries, list entries in a window, manage tasks, archive and delete. Rust
validation runs first for good messages; SQL constraints remain the backstop.
Three more integration tests. Files: `crates/app/src/storage.rs`,
`crates/app/tests/storage.rs`.

**You will learn**

- `Transaction<'_>` and lifetimes in signatures.
- Validate-in-Rust, constrain-in-SQL, and what each layer's error looks like.
- Let-chains (`if let ... && cond`).
- Reading rows into domain types; `&'static str` mapping with an exhaustive `match`.
- `Option::map(TaskId)` — a constructor used as a function.

**Prerequisite.** Chapter 9 checkpoint passed.

---

## 10.1 Imports and the transaction boundary

Extend the imports:

```rust
// crates/app/src/storage.rs
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use work_time_core::{
    EntryId, EntrySource, Project, ProjectId, Task, TaskId, TimeEntry, TrackerSnapshot,
    TrackerState, Transition,
};

use crate::error::AppError;
```

Insert after `load_snapshot`, inside `impl Store`:

```rust
// crates/app/src/storage.rs (inside impl Store, after load_snapshot)

    /// Writes completed entries and the new snapshot in one transaction.
    pub fn persist_transition(&mut self, transition: &Transition) -> Result<(), AppError> {
        let transaction = self.connection.transaction()?;
        validate_active_references(&transaction, &transition.snapshot.state)?;
        for entry in &transition.completed_entries {
            insert_entry(&transaction, entry)?;
        }
        write_snapshot(&transaction, &transition.snapshot)?;
        transaction.commit()?;
        Ok(())
    }
```

**What.** This is the method the storage thread will call after every
accepted command (Chapter 13). Entries and snapshot go to disk together, or
not at all.

**Why one transaction.** Imagine "stop" wrote the entry, then failed to
write the `Stopped` snapshot. On restart the timer would still be running
*and* its time would already be recorded — double counting. Atomicity makes
that impossible. Exercise 4 proves it.

**Rust — `for entry in &vec`.** Iterating over a borrowed `Vec` yields
`&TimeEntry`; the vector is still usable afterwards. `for entry in vec`
(no `&`) would move and consume it.

## 10.2 Adding and updating entries

```rust
// crates/app/src/storage.rs (inside impl Store, after persist_transition)

    pub fn add_entry(&mut self, entry: &TimeEntry) -> Result<EntryId, AppError> {
        entry.validate()?;
        self.validate_against_active(entry)?;
        let transaction = self.connection.transaction()?;
        validate_entry_references(&transaction, entry)?;
        reject_entry_overlaps(&transaction, entry, None)?;
        insert_entry(&transaction, entry)?;
        let id = EntryId(transaction.last_insert_rowid());
        transaction.commit()?;
        Ok(id)
    }

    pub fn update_entry(&mut self, entry: &TimeEntry) -> Result<(), AppError> {
        entry.validate()?;
        self.validate_against_active(entry)?;
        let id = entry
            .id
            .ok_or_else(|| AppError::InvalidBackup("entry ID is required for update".into()))?;
        let transaction = self.connection.transaction()?;
        validate_entry_references(&transaction, entry)?;
        reject_entry_overlaps(&transaction, entry, Some(id))?;
        let changed = transaction.execute(
            "UPDATE entries SET project_id=?1, task_id=?2, note=?3, start_ms=?4, end_ms=?5,
             source=?6, updated_at_ms=?7 WHERE id=?8",
            params![
                entry.project_id.0,
                entry.task_id.map(|value| value.0),
                entry.note,
                entry.start_ms,
                entry.end_ms,
                source_name(entry.source),
                entry.updated_at_ms,
                id.0
            ],
        )?;
        if changed == 0 {
            return Err(AppError::InvalidBackup(format!(
                "entry {id:?} was not found"
            )));
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn list_entries(&self, start_ms: i64, end_ms: i64) -> Result<Vec<TimeEntry>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, project_id, task_id, note, start_ms, end_ms, source, created_at_ms, updated_at_ms
             FROM entries WHERE start_ms < ?2 AND end_ms > ?1 ORDER BY start_ms",
        )?;
        let rows = statement.query_map(params![start_ms, end_ms], read_entry)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn list_all_entries(&self) -> Result<Vec<TimeEntry>, AppError> {
        self.list_entries(i64::MIN, i64::MAX)
    }
```

**What.** The manual-entry and edit dialogs (Chapter 17) call these. Each
write goes: domain validation (`entry.validate()`, from Chapter 2) →
"does it collide with the running timer?" → references exist → no overlap →
SQL. `list_entries` returns every entry that *touches* the window
`[start_ms, end_ms)` — including ones that started before it — which is what
a "today" view needs (Exercise 4).

**Why two error layers.** `reject_entry_overlaps` runs *before* the insert
and names the conflicting IDs: "entry overlaps existing entries:
[EntryId(1)]". If it were removed, the trigger from Chapter 9 would still
refuse, but the message would be SQLite's: "time entry overlaps existing
entry" (Exercise 1). Rust for the user, SQL for safety.

**Rust — `Option::map` with a closure returning a field.** `entry.task_id.map(|value| value.0)`
turns `Option<TaskId>` into `Option<i64>`; `params!` binds `None` as SQL
`NULL`.

**Rust — `execute` returns the row count.** `changed == 0` means no row had
that ID. Checking it turns a silent no-op into an error.

**Rust — passing a function name.** `query_map(params, read_entry)` passes
the function defined in 10.6 where a closure is expected; any `fn` with the
right signature works.

**SQL — the window test.** `start_ms < ?2 AND end_ms > ?1` is the same
half-open overlap condition as the trigger, applied between an entry and the
requested window.

## 10.3 Projects and tasks

Insert after `list_projects`:

```rust
// crates/app/src/storage.rs (inside impl Store, after list_projects)

    pub fn list_tasks(&self, include_archived: bool) -> Result<Vec<Task>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, project_id, name, archived, created_at_ms, updated_at_ms FROM tasks
             WHERE ?1 OR archived = 0 ORDER BY project_id, archived, name COLLATE NOCASE",
        )?;
        let rows = statement.query_map([include_archived], |row| {
            Ok(Task {
                id: TaskId(row.get(0)?),
                project_id: ProjectId(row.get(1)?),
                name: row.get(2)?,
                archived: row.get(3)?,
                created_at_ms: row.get(4)?,
                updated_at_ms: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn create_project(
        &self,
        name: &str,
        color: &str,
        now_ms: i64,
    ) -> Result<ProjectId, AppError> {
        let project = Project {
            id: ProjectId(0),
            name: name.trim().to_owned(),
            color: color.to_owned(),
            archived: false,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        };
        project.validate()?;
        self.connection.execute(
            "INSERT INTO projects(name, color, archived, created_at_ms, updated_at_ms)
             VALUES(?1, ?2, 0, ?3, ?3)",
            params![project.name, project.color, now_ms],
        )?;
        Ok(ProjectId(self.connection.last_insert_rowid()))
    }

    pub fn create_task(
        &self,
        project_id: ProjectId,
        name: &str,
        now_ms: i64,
    ) -> Result<TaskId, AppError> {
        validate_project(&self.connection, project_id)?;
        let trimmed = name.trim();
        work_time_core::validate_name(trimmed)?;
        self.connection.execute(
            "INSERT INTO tasks(project_id, name, archived, created_at_ms, updated_at_ms)
             VALUES(?1, ?2, 0, ?3, ?3)",
            params![project_id.0, trimmed, now_ms],
        )?;
        Ok(TaskId(self.connection.last_insert_rowid()))
    }
```

And after `set_project_archived`:

```rust
// crates/app/src/storage.rs (inside impl Store, after set_project_archived)

    pub fn set_task_archived(
        &self,
        id: TaskId,
        archived: bool,
        now_ms: i64,
    ) -> Result<(), AppError> {
        self.connection.execute(
            "UPDATE tasks SET archived=?1, updated_at_ms=?2 WHERE id=?3",
            params![archived, now_ms, id.0],
        )?;
        Ok(())
    }

    pub fn delete_project_permanently(&self, id: ProjectId) -> Result<(), AppError> {
        if id == ProjectId(1) {
            return Err(AppError::ReferencedItem);
        }
        let references: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE project_id=?1)",
            [id.0],
            |row| row.get(0),
        )?;
        if references {
            return Err(AppError::ReferencedItem);
        }
        self.connection
            .execute("DELETE FROM projects WHERE id=?1", [id.0])?;
        Ok(())
    }

    pub fn delete_task_permanently(&self, id: TaskId) -> Result<(), AppError> {
        let references: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE task_id=?1)",
            [id.0],
            |row| row.get(0),
        )?;
        if references {
            return Err(AppError::ReferencedItem);
        }
        self.connection
            .execute("DELETE FROM tasks WHERE id=?1", [id.0])?;
        Ok(())
    }
```

**What.** Create a project or task (validated and trimmed), archive either,
and delete permanently only when nothing references it. The UI only ever
archives; the delete methods exist for the storage API and its tests.

**Why build a `Project` just to validate.** `Project::validate` checks name
and colour together; constructing the value with a placeholder ID reuses
that rule instead of duplicating it. `ProjectId(0)` is never written — the
`INSERT` omits `id` and SQLite assigns one, returned by `last_insert_rowid`.

**Rust — `&str` in, `String` stored.** `name: &str` accepts anything;
`.trim().to_owned()` produces the owned `String` the struct needs.
`to_owned()` and `to_string()` both work on `&str`; `to_owned` says "I want
an owned copy" without implying formatting.

**Rust — `&self` for writes?** `create_project` takes `&self` even though it
writes to the database: the *Rust value* is not mutated (`Connection` uses
interior mutability for its handle). `persist_transition` and `add_entry`
take `&mut self` because `transaction()` needs `&mut Connection`. The
signature tells you which methods open transactions.

**SQL — `EXISTS(SELECT 1 ...)`.** Yields 0 or 1, read into a Rust `bool`.

## 10.4 Checking against the running timer

Insert after `set_meta`, still inside the `impl`:

```rust
// crates/app/src/storage.rs (inside impl Store, after set_meta)

    fn validate_against_active(&self, entry: &TimeEntry) -> Result<(), AppError> {
        if let Some(active) = self.load_snapshot()?.state.active()
            && entry.end_ms > active.start_ms
        {
            return Err(AppError::InvalidBackup(
                "entry overlaps the active timer; stop it before editing this interval".into(),
            ));
        }
        Ok(())
    }
}
```

**What.** A manual entry must not reach into the interval the running
timer will eventually claim. Since the running timer is not an entry yet,
the trigger cannot see it; this check does.

**Rust — let-chains.** `if let Some(active) = ... && entry.end_ms >
active.start_ms` combines a pattern and a condition in one `if` (edition
2024). `active` is bound only when the pattern matches, and the second
condition can use it. Before let-chains this needed a nested `if`.

## 10.5 The free functions

After the `impl` block:

```rust
// crates/app/src/storage.rs (after impl Store)

fn validate_project(connection: &Connection, id: ProjectId) -> Result<(), AppError> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1 AND archived=0)",
        [id.0],
        |row| row.get(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(AppError::InvalidProject(id))
    }
}

fn validate_entry_references(connection: &Connection, entry: &TimeEntry) -> Result<(), AppError> {
    validate_project(connection, entry.project_id)?;
    if let Some(task_id) = entry.task_id {
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1 AND project_id=?2 AND archived=0)",
            params![task_id.0, entry.project_id.0],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(AppError::InvalidTask(task_id));
        }
    }
    Ok(())
}

fn validate_active_references(
    connection: &Connection,
    state: &TrackerState,
) -> Result<(), AppError> {
    if let Some(active) = state.active() {
        let entry = TimeEntry {
            id: None,
            project_id: active.project_id,
            task_id: active.task_id,
            note: active.note.clone(),
            start_ms: active.start_ms,
            end_ms: active.start_ms.saturating_add(1),
            source: EntrySource::Timer,
            created_at_ms: active.start_ms,
            updated_at_ms: active.start_ms,
        };
        validate_entry_references(connection, &entry)?;
        let mut statement = connection
            .prepare("SELECT id FROM entries WHERE end_ms > ?1 ORDER BY start_ms LIMIT 20")?;
        let conflicts = statement
            .query_map([active.start_ms], |row| row.get::<_, i64>(0).map(EntryId))?
            .collect::<Result<Vec<_>, _>>()?;
        if !conflicts.is_empty() {
            return Err(work_time_core::DomainError::Overlap { conflicts }.into());
        }
    }
    Ok(())
}

fn reject_entry_overlaps(
    connection: &Connection,
    entry: &TimeEntry,
    excluded_id: Option<EntryId>,
) -> Result<(), AppError> {
    let mut statement = connection.prepare(
        "SELECT id FROM entries
         WHERE ?1 < end_ms AND ?2 > start_ms AND (?3 IS NULL OR id != ?3)
         ORDER BY start_ms LIMIT 20",
    )?;
    let conflicts = statement
        .query_map(
            params![entry.start_ms, entry.end_ms, excluded_id.map(|id| id.0)],
            |row| row.get::<_, i64>(0).map(EntryId),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(work_time_core::DomainError::Overlap { conflicts }.into())
    }
}

fn insert_entry(transaction: &Transaction<'_>, entry: &TimeEntry) -> Result<(), AppError> {
    validate_entry_references(transaction, entry)?;
    transaction.execute(
        "INSERT INTO entries(project_id,task_id,note,start_ms,end_ms,source,created_at_ms,updated_at_ms)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![entry.project_id.0, entry.task_id.map(|id| id.0), entry.note, entry.start_ms,
            entry.end_ms, source_name(entry.source), entry.created_at_ms, entry.updated_at_ms],
    )?;
    Ok(())
}

fn write_snapshot(
    transaction: &Transaction<'_>,
    snapshot: &TrackerSnapshot,
) -> Result<(), AppError> {
    let json = serde_json::to_string(snapshot)?;
    let updated_at_ms = snapshot
        .state
        .active()
        .map_or(0, |active| active.last_heartbeat_ms);
    transaction.execute(
        "INSERT INTO tracker_state(singleton,snapshot_json,updated_at_ms) VALUES(1,?1,?2)
         ON CONFLICT(singleton) DO UPDATE SET snapshot_json=excluded.snapshot_json, updated_at_ms=excluded.updated_at_ms",
        params![json, updated_at_ms],
    )?;
    Ok(())
}
```

**What.** Checks and writes that several methods share. `validate_active_references`
treats the running timer as a 1 ms entry to reuse the reference check, then
refuses to *start* a timer whose start lies inside recorded history.
`write_snapshot` upserts the single JSON row.

**Rust — `Transaction<'_>`.** A `Transaction` borrows its `Connection`, so
its type carries a *lifetime*: `Transaction<'a>` lives no longer than the
connection borrowed for `'a`. In a function signature `'_` says "some
lifetime, inferred; this function does not keep the borrow". You rarely
write lifetimes by hand; when you do, it is to tell the compiler how long a
reference in a struct or return value stays valid.

**Rust — `Transaction` where `Connection` is expected.** `insert_entry`
passes its `&Transaction` to `validate_entry_references(connection:
&Connection, ...)`. That works because `Transaction` implements `Deref<Target
= Connection>`: a `&Transaction` auto-converts to `&Connection`. Statements
run inside the transaction.

**Rust — `row.get::<_, i64>(0).map(EntryId)`.** The turbofish fixes the
column type; `.map(EntryId)` wraps it using the tuple-struct constructor as a
plain function `fn(i64) -> EntryId`.

**Rust — `.into()` on an error.** `DomainError::Overlap { .. }.into()`
converts to `AppError` through the same `From` that `?` uses — handy when
the error is built in an `else` branch rather than propagated.

**Rust — `params!` across lines.** The macro takes any number of values;
rustfmt leaves the inside of macros alone, so the long lists are wrapped by
hand.

## 10.6 Row mapping and source names

```rust
// crates/app/src/storage.rs (end of file)

fn source_name(source: EntrySource) -> &'static str {
    match source {
        EntrySource::Timer => "timer",
        EntrySource::Manual => "manual",
        EntrySource::IdleReassignment => "idle_reassignment",
        EntrySource::Recovery => "recovery",
    }
}

fn read_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<TimeEntry> {
    let source: String = row.get(6)?;
    Ok(TimeEntry {
        id: Some(EntryId(row.get(0)?)),
        project_id: ProjectId(row.get(1)?),
        task_id: row.get::<_, Option<i64>>(2)?.map(TaskId),
        note: row.get(3)?,
        start_ms: row.get(4)?,
        end_ms: row.get(5)?,
        source: match source.as_str() {
            "manual" => EntrySource::Manual,
            "idle_reassignment" => EntrySource::IdleReassignment,
            "recovery" => EntrySource::Recovery,
            _ => EntrySource::Timer,
        },
        created_at_ms: row.get(7)?,
        updated_at_ms: row.get(8)?,
    })
}
```

**What.** The enum ↔ text mapping for the `source` column, and the
column-by-index reader used by `list_entries`.

**Rust — `&'static str`.** The function returns string literals, which
live in the binary forever; `'static` is the lifetime that says so. No
allocation per call.

**Rust — exhaustive in one direction.** `source_name` has no `_` arm: adding
a fifth `EntrySource` variant fails to compile here until it is mapped
(Exercise 3). `read_entry` *does* have a `_` arm, because the input is text
from disk — unknown strings must map to something rather than crash, and
`Timer` is the safe default.

**Rust — `rusqlite::Result<T>`.** A type alias for `Result<T, rusqlite::Error>`;
row readers return it because `query_map` expects that error type. `String`
then `.as_str()` for the match: you cannot match a `String` against literals
directly, but you can match its `&str` view.

## 10.7 Tests

Add the fixture and three tests to `tests/storage.rs` (extend the `use`):

```rust
// crates/app/tests/storage.rs
use tempfile::TempDir;
use work_time_core::{EntryId, EntrySource, ProjectId, TaskId, TimeEntry};
use work_time_tracker::storage::Store;

// ... temporary_store ...

fn manual(id: Option<i64>, project_id: i64, start_ms: i64, end_ms: i64) -> TimeEntry {
    TimeEntry {
        id: id.map(EntryId),
        project_id: ProjectId(project_id),
        task_id: None,
        note: "manual".into(),
        start_ms,
        end_ms,
        source: EntrySource::Manual,
        created_at_ms: end_ms,
        updated_at_ms: end_ms,
    }
}

// ... migration_creates_non_archivable_general_project ...

#[test]
fn database_constraint_rejects_overlapping_manual_entries() {
    let (_directory, mut store) = temporary_store();
    assert!(store.add_entry(&manual(None, 1, 100, 200)).is_ok());
    let overlap = store.add_entry(&manual(None, 1, 199, 300));
    assert!(overlap.is_err());
    assert!(
        overlap
            .err()
            .map(|error| error.to_string().contains("EntryId(1)"))
            .unwrap_or(false)
    );
    let entries = store
        .list_all_entries()
        .unwrap_or_else(|error| panic!("list failed: {error}"));
    assert_eq!(entries.len(), 1);
}

#[test]
fn task_must_belong_to_entry_project() {
    let (_directory, mut store) = temporary_store();
    let second = store
        .create_project("Second", "#ff0000", 1)
        .unwrap_or_else(|error| panic!("project failed: {error}"));
    let task = store
        .create_task(second, "Task", 1)
        .unwrap_or_else(|error| panic!("task failed: {error}"));
    let mut entry = manual(None, 1, 100, 200);
    entry.task_id = Some(task);
    assert!(store.add_entry(&entry).is_err());
}

// ... corrupt_database_returns_a_contextual_error ...

#[test]
fn referenced_task_is_archived_not_deleted() {
    let (_directory, mut store) = temporary_store();
    let task = store
        .create_task(ProjectId(1), "Task", 1)
        .unwrap_or_else(|error| panic!("task failed: {error}"));
    let mut entry = manual(None, 1, 100, 200);
    entry.task_id = Some(task);
    assert!(store.add_entry(&entry).is_ok());
    assert!(store.set_task_archived(task, true, 300).is_ok());
    assert!(store.delete_task_permanently(task).is_err());
    assert_eq!(task, TaskId(task.0));
}
```

**What.** Overlaps are rejected *and* the message names the existing entry;
a task from another project is refused; a task with history can be
archived but not deleted.

**Rust — `id.map(EntryId)`.** Same constructor-as-function trick as
`read_entry`, now on `Option<i64>`.

**Rust — `let mut entry = ...; entry.task_id = Some(task);`.** Fixtures
return owned values you can adjust; `mut` is needed to assign a field.

## 10.8 Checkpoint

```sh
cargo test -p work-time-tracker
```

Expected: `tests/storage.rs` — 5 passed (`corrupt_database…`,
`database_constraint…`, `migration_creates…`, `referenced_task…`,
`task_must_belong…`), plus keepers.

```sh
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
diff <(grep -v '^\s*//' ../work_time_tracker/crates/app/src/storage.rs) \
     <(grep -v '^\s*//' crates/app/src/storage.rs)
```

The diff shows only the `backup`/`restore` methods and the
`use crate::backup::...` line — Chapter 11.

```sh
git add -A && git commit -m "Chapter 10: entries, projects, tasks"
```

## 10.9 Exercises

1. **Two layers, two messages.** In `add_entry` delete the line
   `reject_entry_overlaps(&transaction, entry, None)?;` and run the tests.

   <details><summary>Answer</summary>

   ```
   test database_constraint_rejects_overlapping_manual_entries ... FAILED
   ```

   The insert is still refused — by the trigger — but the error is now
   `database operation failed: time entry overlaps existing entry` and no
   longer contains `EntryId(1)`, so the assertion on the message fails.
   Before removing the line the message was
   `entry overlaps existing entries: [EntryId(1)]`. Rust validation
   exists for the user; SQL constraints exist for the data.
   </details>

2. **SQL is checked at run time.** In `update_entry` remove the
   `entry.updated_at_ms,` line from the `params!` list (leaving `?7` in the
   SQL). Build, then add a temporary test that calls `update_entry` and
   prints the error.

   <details><summary>Answer</summary>

   It compiles. At run time:

   ```
   database operation failed: Wrong number of parameters passed to query. Got 7, needed 8
   ```

   The compiler cannot see inside the SQL string. This is the trade-off of
   `rusqlite`'s approach — and the reason every write path here has a test.
   </details>

3. **Exhaustive enum mapping.** Delete the `EntrySource::Recovery` arm in
   `source_name` and build.

   <details><summary>Answer</summary>

   ```
   error[E0004]: non-exhaustive patterns: `EntrySource::Recovery` not covered
   ```

   Compare with `read_entry`, whose `_ => EntrySource::Timer` arm accepts
   anything. Text from disk is untrusted; a Rust enum is not.
   </details>

4. **Two tests worth keeping.** Append to `tests/storage.rs`:

   ```rust
   #[test]
   fn list_entries_returns_everything_touching_the_window() {
       let (_directory, mut store) = temporary_store();
       assert!(store.add_entry(&manual(None, 1, 100, 200)).is_ok());
       assert!(store.add_entry(&manual(None, 1, 200, 300)).is_ok());
       assert!(store.add_entry(&manual(None, 1, 300, 400)).is_ok());
       let inside = store
           .list_entries(150, 250)
           .unwrap_or_else(|error| panic!("list failed: {error}"));
       assert_eq!(inside.len(), 2);
       assert_eq!(inside[0].start_ms, 100);
       assert_eq!(inside[1].start_ms, 200);
       let edge = store
           .list_entries(200, 300)
           .unwrap_or_else(|error| panic!("list failed: {error}"));
       assert_eq!(edge.len(), 1);
   }

   #[test]
   fn persist_transition_is_atomic() {
       use work_time_core::{TrackerSnapshot, Transition};

       let (_directory, mut store) = temporary_store();
       assert!(store.add_entry(&manual(None, 1, 100, 200)).is_ok());
       let transition = Transition {
           snapshot: TrackerSnapshot {
               state: work_time_core::TrackerState::Stopped,
               revision: 7,
           },
           completed_entries: vec![manual(None, 1, 500, 600), manual(None, 1, 150, 250)],
           notifications: vec![],
       };
       assert!(store.persist_transition(&transition).is_err());
       let entries = store
           .list_all_entries()
           .unwrap_or_else(|error| panic!("list failed: {error}"));
       assert_eq!(entries.len(), 1);
       let snapshot = store
           .load_snapshot()
           .unwrap_or_else(|error| panic!("load failed: {error}"));
       assert_eq!(snapshot.revision, 0);
   }
   ```

   <details><summary>Answer</summary>

   Both pass (7 total). The window test shows adjacency again: the entry
   `[300, 400)` does not touch `[200, 300)`. The atomicity test inserts a
   valid entry `[500, 600)` and then an overlapping one in the same
   transition; the trigger aborts the second insert, the whole transaction
   rolls back, and neither the first entry nor revision 7 reaches the
   database.
   </details>

## Recap

- Writes follow validate → check references → check overlaps → SQL, inside a
  transaction; the snapshot and its entries commit together.
- `Transaction<'_>` borrows the connection and derefs to it; helpers take
  `&Connection`.
- `source_name` is an exhaustive map; `read_entry` tolerates unknown text.
- Rust messages name IDs; SQL constraints catch whatever Rust misses.
- Let-chains merge a pattern and a condition into one `if`.

Next: **Chapter 11 — Backup**, a versioned JSON document and a restore
that cannot half-succeed.
