# Chapter 9 — SQLite store

**Goal.** `Store` opens (or creates) the database, runs the schema
migration, keeps the "was the last shutdown clean?" marker, loads the
persisted snapshot, and lists projects. Two integration tests use a real
SQLite file in a temporary directory. Files: `crates/app/src/storage.rs`,
`crates/app/tests/storage.rs`, one line in `lib.rs`.

**You will learn**

- Owning a `rusqlite::Connection`; pragmas (WAL, foreign keys, busy timeout).
- Migrations with `PRAGMA user_version` inside a transaction.
- SQL constraints and triggers as a second line of defence.
- `query_row`, `OptionalExtension`, `query_map` and collecting `Result`s.
- `map_or_else`, and `TempDir` as RAII in tests.

**Prerequisite.** Chapter 8 checkpoint passed.

---

## 9.1 Opening the database

```rust
// crates/app/src/storage.rs
//! SQLite schema and transactional persistence.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use houra_core::{Project, ProjectId, TrackerSnapshot};

use crate::error::AppError;

const SCHEMA_VERSION: i64 = 1;

/// Owns the SQLite connection. Only one `Store` exists per database, and it
/// moves into the storage thread in Chapter 13.
pub struct Store {
    connection: Connection,
    previous_shutdown_clean: bool,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| AppError::io(parent, source))?;
        }
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let mut store = Self {
            connection,
            previous_shutdown_clean: true,
        };
        store.migrate()?;
        store.ensure_general_project()?;
        store.previous_shutdown_clean = store.meta_bool("clean_shutdown")?.unwrap_or(true);
        store.set_meta("clean_shutdown", "0")?;
        Ok(store)
    }

    /// Same schema and constraints as `open`, without a file. For tests.
    pub fn open_in_memory() -> Result<Self, AppError> {
        let connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let mut store = Self {
            connection,
            previous_shutdown_clean: true,
        };
        store.migrate()?;
        store.ensure_general_project()?;
        store.set_meta("clean_shutdown", "0")?;
        Ok(store)
    }

    pub fn previous_shutdown_clean(&self) -> bool {
        self.previous_shutdown_clean
    }

    pub fn mark_clean_shutdown(&self) -> Result<(), AppError> {
        self.set_meta("clean_shutdown", "1")
    }
```

(The `impl` block stays open through 9.4.)

**What.** `open` creates the parent folder, opens the file (creating it if
absent), configures the connection, migrates the schema, guarantees the
General project, reads the previous run's shutdown marker, and immediately
writes "dirty". Only `mark_clean_shutdown` (called on orderly exit with a
stopped timer, Chapter 13) sets it back to "clean".

**Why the marker.** If the process is killed while a timer runs, the next
start finds `clean_shutdown = 0` *and* a running snapshot, and hands the
engine `unclean_shutdown = true` (Chapter 6's `restore`). Writing "dirty" at
open — not at the first change — is what makes a crash detectable.

**Why these pragmas.** WAL (write-ahead log) lets readers proceed while the
one writer commits; `foreign_keys = ON` is off by default in SQLite and must
be enabled per connection; a 5-second busy timeout waits instead of failing
if another process briefly holds the file.

**Rust — owning the connection.** `Connection` is a plain owned value, no
`Arc`, no `Mutex`. There will be exactly one owner (the storage thread), and
Rust's ownership rules — not discipline — prevent two threads from using it.

**Rust — `?` on rusqlite results.** `Connection::open(path)?` returns
`rusqlite::Error` on failure; `?` converts it to `AppError::Database` via the
`#[from]` of Chapter 8. `create_dir_all` returns `std::io::Error`, which has
no single `From`, so `map_err` builds `AppError::Io` with the path.

**Rust — `let mut store` then methods.** `migrate` takes `&mut self`, so the
binding must be `mut`; after construction the store is returned by value.

## 9.2 The schema migration

```rust
// crates/app/src/storage.rs (inside impl Store)

    fn migrate(&mut self) -> Result<(), AppError> {
        let version: i64 = self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(AppError::InvalidBackup(format!(
                "database schema {version} is newer than supported {SCHEMA_VERSION}"
            )));
        }
        if version < 1 {
            let transaction = self.connection.transaction()?;
            transaction.execute_batch(
                "
                CREATE TABLE meta (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                CREATE TABLE projects (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL COLLATE NOCASE UNIQUE CHECK(length(trim(name)) > 0),
                    color TEXT NOT NULL,
                    archived INTEGER NOT NULL DEFAULT 0 CHECK(archived IN (0, 1)),
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );
                CREATE TABLE tasks (
                    id INTEGER PRIMARY KEY,
                    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
                    name TEXT NOT NULL COLLATE NOCASE CHECK(length(trim(name)) > 0),
                    archived INTEGER NOT NULL DEFAULT 0 CHECK(archived IN (0, 1)),
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    UNIQUE(project_id, name)
                );
                CREATE TABLE entries (
                    id INTEGER PRIMARY KEY,
                    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
                    task_id INTEGER REFERENCES tasks(id) ON DELETE RESTRICT,
                    note TEXT NOT NULL DEFAULT '',
                    start_ms INTEGER NOT NULL,
                    end_ms INTEGER NOT NULL CHECK(end_ms > start_ms),
                    source TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );
                CREATE INDEX entries_interval ON entries(start_ms, end_ms);
                CREATE TRIGGER entries_no_overlap_insert BEFORE INSERT ON entries
                -- Half-open intervals overlap iff new.start < old.end and
                -- new.end > old.start; adjacent boundaries are therefore legal.
                WHEN EXISTS (
                    SELECT 1 FROM entries
                    WHERE NEW.start_ms < end_ms AND NEW.end_ms > start_ms
                ) BEGIN SELECT RAISE(ABORT, 'time entry overlaps existing entry'); END;
                CREATE TRIGGER entries_no_overlap_update BEFORE UPDATE OF start_ms, end_ms ON entries
                WHEN EXISTS (
                    SELECT 1 FROM entries
                    WHERE id != NEW.id AND NEW.start_ms < end_ms AND NEW.end_ms > start_ms
                ) BEGIN SELECT RAISE(ABORT, 'time entry overlaps existing entry'); END;
                CREATE TRIGGER entry_task_project_insert BEFORE INSERT ON entries
                WHEN NEW.task_id IS NOT NULL AND NOT EXISTS (
                    SELECT 1 FROM tasks WHERE id = NEW.task_id AND project_id = NEW.project_id
                ) BEGIN SELECT RAISE(ABORT, 'task does not belong to project'); END;
                CREATE TRIGGER entry_task_project_update BEFORE UPDATE OF project_id, task_id ON entries
                WHEN NEW.task_id IS NOT NULL AND NOT EXISTS (
                    SELECT 1 FROM tasks WHERE id = NEW.task_id AND project_id = NEW.project_id
                ) BEGIN SELECT RAISE(ABORT, 'task does not belong to project'); END;
                CREATE TABLE tracker_state (
                    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                    snapshot_json TEXT NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );
                PRAGMA user_version = 1;
                ",
            )?;
            transaction.commit()?;
        }
        Ok(())
    }

    /// Project 1, "General", always exists; UI and backups rely on it.
    fn ensure_general_project(&self) -> Result<(), AppError> {
        self.connection.execute(
            "INSERT OR IGNORE INTO projects(id, name, color, archived, created_at_ms, updated_at_ms)
             VALUES(1, 'General', '#3584e4', 0, unixepoch('subsec') * 1000, unixepoch('subsec') * 1000)",
            [],
        )?;
        Ok(())
    }
```

**What.** SQLite keeps a free integer per database, `user_version`. It is
0 for a new file. If it is 0, create every table, index and trigger and set
it to 1 — all inside one transaction. A future schema change would be
`if version < 2 { ... }` after this block. A database from a *newer* version
of the app is refused rather than guessed at.

**Why constraints in SQL too.** Chapter 10 validates in Rust before writing
so the user gets a good message. The `CHECK`s, `UNIQUE`s, foreign keys and
triggers make the database refuse bad data *even if* some future code path
forgets to validate. The overlap triggers use the same `<` rule as
`validate_no_overlaps` (Chapter 7). `ON DELETE RESTRICT` is why projects
with history are archived, never deleted.

**Why `tracker_state` has one row.** `singleton INTEGER PRIMARY KEY
CHECK(singleton = 1)`: the table can only ever hold the row with key 1. The
snapshot is stored as JSON text — the exact JSON you printed in Chapter 3.

**Rust — SQL is a string.** Rust checks the `Result`, not the SQL. A typo in
the schema is a runtime error from SQLite, which is why the migration is
covered by tests. `execute_batch` runs several statements; `execute` runs
one. `[]` is "no parameters".

**Rust — transactions and RAII.** `self.connection.transaction()?` returns a
`Transaction` that *borrows* the connection. If it goes out of scope without
`commit()`, it rolls back automatically — a `Drop` implementation does that
(Exercise 2). Nothing to remember in error paths: a `?` that returns early
drops the transaction, and the database is unchanged.

**Rust — `query_row` and closures.** `query_row(sql, params, |row| row.get(0))`
runs a query expected to return one row and maps it with the closure; `row.get(0)`
reads column 0 into the type the caller wants — here `i64`, from the
annotation on `version`.

**Rust — `format!` captures.** `{version}` and `{SCHEMA_VERSION}` read
variables and constants directly inside the string.

## 9.3 Loading the snapshot and listing projects

```rust
// crates/app/src/storage.rs (inside impl Store)

    pub fn load_snapshot(&self) -> Result<TrackerSnapshot, AppError> {
        let json: Option<String> = self
            .connection
            .query_row(
                "SELECT snapshot_json FROM tracker_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        json.map_or_else(
            || Ok(TrackerSnapshot::default()),
            |value| serde_json::from_str(&value).map_err(AppError::from),
        )
    }

    pub fn list_projects(&self, include_archived: bool) -> Result<Vec<Project>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, color, archived, created_at_ms, updated_at_ms FROM projects
             WHERE ?1 OR archived = 0 ORDER BY archived, name COLLATE NOCASE",
        )?;
        let rows = statement.query_map([include_archived], |row| {
            Ok(Project {
                id: ProjectId(row.get(0)?),
                name: row.get(1)?,
                color: row.get(2)?,
                archived: row.get(3)?,
                created_at_ms: row.get(4)?,
                updated_at_ms: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn set_project_archived(
        &self,
        id: ProjectId,
        archived: bool,
        now_ms: i64,
    ) -> Result<(), AppError> {
        if id == ProjectId(1) && archived {
            return Err(AppError::InvalidBackup("General cannot be archived".into()));
        }
        self.connection.execute(
            "UPDATE projects SET archived=?1, updated_at_ms=?2 WHERE id=?3",
            params![archived, now_ms, id.0],
        )?;
        Ok(())
    }
```

**What.** `load_snapshot` returns the stored snapshot or a default
(`Stopped`, revision 0) for a fresh database. `list_projects` reads rows
into `Project` values; `set_project_archived` flips the flag, except for
General.

**Rust — `OptionalExtension`.** `query_row` returns `Err(QueryReturnedNoRows)`
for an empty result. `.optional()` (a trait method from the imported
`OptionalExtension`) converts that specific error into `Ok(None)` and keeps
every other error. Importing a trait brings its methods into scope — that is
why the `use` line matters even though the name never appears again.

**Rust — `map_or_else`.** Two closures: the first (`|| ...`) produces the
value for `None`, the second maps `Some(value)`. `serde_json::from_str`
parses the JSON into `TrackerSnapshot` using the derives from Chapter 3;
`map_err(AppError::from)` converts the serde error explicitly because this
expression is not followed by `?`. Passing `AppError::from` as a function
value is the same as `|error| AppError::from(error)`.

**Rust — `prepare`, `query_map`, collect.** `prepare` compiles the SQL into
a statement (`mut` because executing it mutates its internal state).
`query_map` runs it and yields `Result<Project, rusqlite::Error>` per row,
lazily. `.collect::<Result<Vec<_>, _>>()` is a standard trick: collecting an
iterator of `Result`s into a `Result` of a `Vec` stops at the first error.
The `_`s let inference fill in the types.

**Rust — `params!`.** Builds the positional parameter list `?1, ?2, ?3` from
Rust values. Values are bound, never pasted into the SQL text, so user input
cannot break the query (no SQL injection). `[include_archived]` — a
one-element array — works for a single parameter.

**SQL — `?1 OR archived = 0`.** Binding a `bool` as the first parameter
turns the same statement into "all projects" or "active only".

## 9.4 Meta helpers

```rust
// crates/app/src/storage.rs (inside impl Store)

    fn meta_bool(&self, key: &str) -> Result<Option<bool>, AppError> {
        let value: Option<String> = self
            .connection
            .query_row("SELECT value FROM meta WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(value.map(|value| value == "1"))
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<(), AppError> {
        self.connection.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}
```

**What.** A tiny key/value table for flags like `clean_shutdown`. `set_meta`
is an *upsert*: insert, or update on conflict.

**Rust — `Option::map`.** `value.map(|value| value == "1")` turns
`Option<String>` into `Option<bool>` without unwrapping; `None` stays
`None`.

Register the module:

```rust
// crates/app/src/lib.rs
// ...
pub mod error;
pub mod storage;
// ...
```

## 9.5 The first storage tests

```rust
// crates/app/tests/storage.rs
use tempfile::TempDir;
use houra_core::ProjectId;
use houra::storage::Store;

fn temporary_store() -> (TempDir, Store) {
    let directory = TempDir::new().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let store = Store::open(&directory.path().join("tracker.sqlite3"))
        .unwrap_or_else(|error| panic!("store failed: {error}"));
    (directory, store)
}

#[test]
fn migration_creates_non_archivable_general_project() {
    let (_directory, store) = temporary_store();
    let projects = store
        .list_projects(false)
        .unwrap_or_else(|error| panic!("list failed: {error}"));
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "General");
    assert!(store.set_project_archived(ProjectId(1), true, 1).is_err());
}

#[test]
fn corrupt_database_returns_a_contextual_error() {
    let directory = TempDir::new().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let path = directory.path().join("corrupt.sqlite3");
    std::fs::write(&path, b"this is not sqlite")
        .unwrap_or_else(|error| panic!("fixture write failed: {error}"));
    assert!(Store::open(&path).is_err());
}
```

**What.** Real SQLite files in a throwaway directory. The first test proves
the migration ran and General is protected; the second that garbage on disk
becomes an error, not a panic.

**Rust — `TempDir` and RAII.** `TempDir::new()` creates a directory and
*deletes it when the value is dropped*, even if the test panics. The helper
returns the `TempDir` together with the `Store` so the directory outlives
the open connection — `let (_directory, store) = ...` keeps it alive until
the end of the test. Naming it `_directory` (not `_`) matters: `let _ =
...` would drop it immediately.

**Rust — byte strings.** `b"..."` is a `&[u8]`, what `fs::write` takes.

## 9.6 Checkpoint

```sh
cargo test -p houra
```

Expected:

```
     Running tests/storage.rs
test corrupt_database_returns_a_contextual_error ... ok
test migration_creates_non_archivable_general_project ... ok
test result: ok. 2 passed; ...
```

```sh
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
git add -A && git commit -m "Chapter 9: sqlite store"
```

The diff against the original `storage.rs` is large for now; Chapter 10
fills in the rest. The database file this store would create for the real
app lives at `~/.local/share/houra/tracker.sqlite3`; the tests
never touch it.

## 9.7 Exercises

1. **A newer database is refused (optional keeper).** Append to
   `tests/storage.rs`:

   ```rust
   #[test]
   fn newer_schema_is_refused() {
       let directory = TempDir::new().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
       let path = directory.path().join("future.sqlite3");
       let connection = rusqlite::Connection::open(&path)
           .unwrap_or_else(|error| panic!("open failed: {error}"));
       connection
           .pragma_update(None, "user_version", 99)
           .unwrap_or_else(|error| panic!("pragma failed: {error}"));
       let message = Store::open(&path)
           .err()
           .map(|error| error.to_string())
           .unwrap_or_default();
       assert_eq!(
           message,
           "backup validation failed: database schema 99 is newer than supported 1"
       );
   }
   ```

   <details><summary>Answer</summary>

   Passes. A test can use `rusqlite` directly because it is a dependency of
   the crate under test. The message shows the `InvalidBackup` variant's
   prefix — the original reuses that variant for a few "invalid input"
   cases rather than adding one per message.
   </details>

2. **Transactions roll back on drop.** In `migrate`, delete the line
   `transaction.commit()?;` and run the tests.

   <details><summary>Answer</summary>

   ```
   test migration_creates_non_archivable_general_project ... FAILED
   store failed: database operation failed: no such table: projects
   ```

   The schema statements ran, but the `Transaction` was dropped at the end
   of the `if` block without a commit, so SQLite rolled everything back.
   `ensure_general_project` then found no table. Put the commit back.
   </details>

3. **The marker in action (optional keeper).** Append:

   ```rust
   #[test]
   fn clean_shutdown_marker_survives_reopen() {
       let directory = TempDir::new().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
       let path = directory.path().join("tracker.sqlite3");
       let first = Store::open(&path).unwrap_or_else(|error| panic!("open failed: {error}"));
       assert!(first.previous_shutdown_clean());
       drop(first);
       let second = Store::open(&path).unwrap_or_else(|error| panic!("open failed: {error}"));
       assert!(!second.previous_shutdown_clean());
       assert!(second.mark_clean_shutdown().is_ok());
       drop(second);
       let third = Store::open(&path).unwrap_or_else(|error| panic!("open failed: {error}"));
       assert!(third.previous_shutdown_clean());
   }
   ```

   <details><summary>Answer</summary>

   Passes. A brand-new database counts as clean; a reopen without
   `mark_clean_shutdown` counts as unclean; marking fixes it. `drop(x)`
   closes the connection explicitly so the next `open` sees the committed
   state.
   </details>

## Recap

- `Store` owns the connection; pragmas are set per connection.
- `user_version` is the migration cursor; the migration runs in one
  transaction that rolls back on any error.
- Constraints and triggers in SQL duplicate the Rust rules on purpose.
- `optional()` turns "no rows" into `None`; `collect::<Result<Vec<_>, _>>`
  turns row errors into one `Result`.
- `clean_shutdown` is written dirty at open and clean only on orderly exit.

Next: **Chapter 10 — Entries, projects, tasks**, the CRUD methods and
overlap checks.
