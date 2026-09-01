# Chapter 11 — Backup

**Goal.** A versioned JSON backup of the whole database, validated on the
way in and out, written atomically, and restored inside one transaction.
`storage.rs` becomes identical to the original. Files:
`crates/app/src/backup.rs`, `crates/app/src/storage.rs`,
`crates/app/tests/storage.rs`, `lib.rs`.

**You will learn**

- serde for real: `deny_unknown_fields`, versioned formats, `to_writer_pretty`.
- Atomic file replacement: temp file → `sync_all` → rename.
- Validation order for a document with cross-references.
- `for x in &vec`, `iter().any(...)`, and `Box` around large payloads (preview).

**Prerequisite.** Chapter 10 checkpoint passed.

---

## 11.1 The document

```rust
// crates/app/src/backup.rs
//! Versioned, whole-database backup format.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use work_time_core::{Project, Task, TimeEntry, TrackerSnapshot, validate_no_overlaps};

use crate::AppError;

pub const BACKUP_VERSION: u32 = 1;

/// A complete copy of the database as one JSON document.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackupDocument {
    pub format: String,
    pub version: u32,
    pub exported_at_ms: i64,
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub entries: Vec<TimeEntry>,
    pub tracker: TrackerSnapshot,
}
```

**What.** Everything in the database, plus a format marker and a version.
The domain types from Chapter 2–3 already derive `Serialize`/`Deserialize`,
so this struct is the whole format definition.

**Why a marker and a version.** A user may pick the wrong file in the
restore dialog. `format` catches "this is not our JSON at all"; `version`
lets a future app refuse (or convert) an older document deliberately
instead of misreading it.

**Rust — `deny_unknown_fields`.** By default serde ignores JSON keys it
does not know. For a backup that is dangerous: a misspelled key would be
silently dropped and the data lost. With this attribute, unknown keys are
an error that names the key (Exercise 1).

## 11.2 Reading and writing files

```rust
// crates/app/src/backup.rs
// ...

impl BackupDocument {
    /// Parses and validates; an internally inconsistent backup is rejected here.
    pub fn read_from_path(path: &Path) -> Result<Self, AppError> {
        let bytes = fs::read(path).map_err(|source| AppError::io(path, source))?;
        let document: Self = serde_json::from_slice(&bytes)?;
        document.validate()?;
        Ok(document)
    }

    /// Writes atomically: temp file, fsync, then rename into place.
    pub fn write_to_path(&self, path: &Path) -> Result<(), AppError> {
        self.validate()?;
        let Some(parent) = path.parent() else {
            return Err(AppError::DataDirectoryUnavailable);
        };
        fs::create_dir_all(parent).map_err(|source| AppError::io(parent, source))?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .map_err(|source| AppError::io(parent, source))?;
        serde_json::to_writer_pretty(temporary.as_file_mut(), self)?;
        temporary
            .as_file_mut()
            .sync_all()
            .map_err(|source| AppError::io(path, source))?;
        temporary
            .persist(path)
            .map_err(|error| AppError::io(path, error.error))?;
        Ok(())
    }
```

(The `impl` stays open for `validate`.)

**What.** Reading is parse-then-validate, in one function, so no caller can
get an unvalidated document. Writing goes to a temporary file *in the same
directory*, forces it to disk, then renames it over the destination.

**Why atomic writes.** If the process dies half-way through writing
`backup.json` directly, the user has a truncated file and no backup. With
temp+fsync+rename, the destination either still has the old content or has
the complete new content — rename is atomic on Linux filesystems when
source and destination are on the same filesystem, which is why the temp
file is created next to the target.

**Rust — `fs::read` and `from_slice`.** `fs::read` returns `Vec<u8>`;
`serde_json::from_slice(&bytes)` parses from a byte slice. The `?` on the
parse converts `serde_json::Error` via `#[from]`. The `let document: Self`
annotation tells serde what to produce.

**Rust — `NamedTempFile`.** From the `tempfile` crate: a file with a random
name that is deleted on drop *unless* `persist` succeeds. `as_file_mut()`
gives the underlying `&mut File`, which implements `Write`, so
`to_writer_pretty` streams straight into it. `persist` returns a special
error type wrapping both the temp file and the `io::Error`; `error.error`
extracts the latter.

**Rust — `let ... else` for `Option`.** A path with no parent (`/`) cannot
host a temp file; the early return uses an existing error variant rather
than adding one.

## 11.3 Validation

```rust
// crates/app/src/backup.rs (inside impl BackupDocument)

    pub fn validate(&self) -> Result<(), AppError> {
        if self.format != "work-time-tracker-backup" {
            return Err(AppError::InvalidBackup("unknown format marker".into()));
        }
        if self.version != BACKUP_VERSION {
            return Err(AppError::UnsupportedBackupVersion {
                found: self.version,
                expected: BACKUP_VERSION,
            });
        }
        if self.tracker.state.active().is_some() {
            return Err(AppError::InvalidBackup(
                "backups must not contain an active timer".into(),
            ));
        }
        if !self
            .projects
            .iter()
            .any(|project| project.id == work_time_core::ProjectId(1))
        {
            return Err(AppError::InvalidBackup(
                "the required General project is missing".into(),
            ));
        }
        for project in &self.projects {
            project.validate()?;
        }
        for task in &self.tasks {
            task.validate()?;
            if !self
                .projects
                .iter()
                .any(|project| project.id == task.project_id)
            {
                return Err(AppError::InvalidBackup(format!(
                    "task {:?} references a missing project",
                    task.id
                )));
            }
        }
        for entry in &self.entries {
            entry.validate()?;
            if !self
                .projects
                .iter()
                .any(|project| project.id == entry.project_id)
            {
                return Err(AppError::InvalidBackup(format!(
                    "entry {:?} references a missing project",
                    entry.id
                )));
            }
            if let Some(task_id) = entry.task_id {
                let task_matches = self
                    .tasks
                    .iter()
                    .any(|task| task.id == task_id && task.project_id == entry.project_id);
                if !task_matches {
                    return Err(AppError::InvalidBackup(format!(
                        "entry {:?} has a missing or foreign task",
                        entry.id
                    )));
                }
            }
        }
        validate_no_overlaps(&self.entries)?;
        Ok(())
    }
}
```

**What.** From the outside in: envelope (format, version), tracker state
(a backup with a running timer makes no sense), the required General
project, each project and task on its own, each entry's references, and
finally overlaps across all entries — Chapter 7's function, finally used.

**Why validate before SQL.** `restore` deletes everything first. Every
check that can run on the document alone runs here, so a bad file is
refused before the transaction opens, with a message about *the file*
rather than a constraint name from SQLite.

**Rust — `iter().any(...)`.** Returns `true` as soon as the closure is true
for one element; `!` negates. `for project in &self.projects` borrows each
element so `self` stays usable. `format!("{:?}", task.id)` prints the
newtype with `Debug` — `TaskId(7)` — which is what a user reading an error
about their own file can search for.

## 11.4 Backup and restore in the store

Add the import and two methods to `storage.rs`, after
`delete_task_permanently`:

```rust
// crates/app/src/storage.rs
use crate::backup::{BACKUP_VERSION, BackupDocument};
use crate::error::AppError;

// ... inside impl Store, after delete_task_permanently:

    pub fn backup(&self, exported_at_ms: i64) -> Result<BackupDocument, AppError> {
        Ok(BackupDocument {
            format: "work-time-tracker-backup".into(),
            version: BACKUP_VERSION,
            exported_at_ms,
            projects: self.list_projects(true)?,
            tasks: self.list_tasks(true)?,
            entries: self.list_all_entries()?,
            tracker: self.load_snapshot()?,
        })
    }

    /// Replaces every table from a validated backup inside one transaction.
    pub fn restore(&mut self, document: &BackupDocument) -> Result<(), AppError> {
        document.validate()?;
        if self.load_snapshot()?.state.active().is_some() {
            return Err(AppError::RestoreWhileActive);
        }
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM tracker_state", [])?;
        transaction.execute("DELETE FROM entries", [])?;
        transaction.execute("DELETE FROM tasks", [])?;
        transaction.execute("DELETE FROM projects", [])?;
        for project in &document.projects {
            transaction.execute(
                "INSERT INTO projects(id,name,color,archived,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6)",
                params![project.id.0, project.name, project.color, project.archived, project.created_at_ms, project.updated_at_ms],
            )?;
        }
        for task in &document.tasks {
            transaction.execute(
                "INSERT INTO tasks(id,project_id,name,archived,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6)",
                params![task.id.0, task.project_id.0, task.name, task.archived, task.created_at_ms, task.updated_at_ms],
            )?;
        }
        for entry in &document.entries {
            transaction.execute(
                "INSERT INTO entries(id,project_id,task_id,note,start_ms,end_ms,source,created_at_ms,updated_at_ms)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![entry.id.map(|id| id.0), entry.project_id.0, entry.task_id.map(|id| id.0), entry.note,
                    entry.start_ms, entry.end_ms, source_name(entry.source), entry.created_at_ms, entry.updated_at_ms],
            )?;
        }
        write_snapshot(&transaction, &document.tracker)?;
        transaction.execute(
            "INSERT INTO meta(key,value) VALUES('last_restore_ms',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [document.exported_at_ms.to_string()],
        )?;
        transaction.commit()?;
        Ok(())
    }
```

Also register the module:

```rust
// crates/app/src/lib.rs
pub mod backup;
pub mod error;
pub mod storage;
```

**What.** `backup` assembles the document from the list methods (archived
included). `restore` validates, refuses while a timer runs, then deletes
in dependency order (entries reference tasks and projects) and re-inserts
with the *original IDs*, so references inside the document stay valid.

**Why the deletes are safe.** They run inside the transaction. If any
insert fails — a trigger fires, a foreign key is violated — the commit never
happens and the old data is still there (the test
`invalid_restore_is_transactionally_rejected` checks this, and Exercise 2
makes the triggers do the catching).

**Rust — `to_string()` on an integer.** `params!` also accepts a one-element
array of `String`; `exported_at_ms.to_string()` is the `Display` output.

## 11.5 Tests

Add to `tests/storage.rs` (extend the `use` lines):

```rust
// crates/app/tests/storage.rs
use tempfile::TempDir;
use work_time_core::{EntryId, EntrySource, ProjectId, TaskId, TimeEntry, TrackerSnapshot};
use work_time_tracker::backup::BackupDocument;
use work_time_tracker::storage::Store;

// ... existing helpers and tests (insert these after task_must_belong_to_entry_project) ...

#[test]
fn backup_round_trip_preserves_data() {
    let (_first_dir, mut first) = temporary_store();
    assert!(first.add_entry(&manual(None, 1, 100, 200)).is_ok());
    let backup = first
        .backup(1_000)
        .unwrap_or_else(|error| panic!("backup failed: {error}"));

    let (_second_dir, mut second) = temporary_store();
    assert!(second.restore(&backup).is_ok());
    let restored = second
        .list_all_entries()
        .unwrap_or_else(|error| panic!("list failed: {error}"));
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].start_ms, 100);
}

#[test]
fn backup_file_round_trip_is_versioned_and_validated() {
    let (directory, mut store) = temporary_store();
    assert!(store.add_entry(&manual(None, 1, 100, 200)).is_ok());
    let backup = store
        .backup(1_000)
        .unwrap_or_else(|error| panic!("backup failed: {error}"));
    let path = directory.path().join("backup.json");
    assert!(backup.write_to_path(&path).is_ok());
    let read = BackupDocument::read_from_path(&path)
        .unwrap_or_else(|error| panic!("read failed: {error}"));
    assert_eq!(read.version, 1);
    assert_eq!(read.entries.len(), 1);
}

#[test]
fn invalid_restore_is_transactionally_rejected() {
    let (_directory, mut store) = temporary_store();
    assert!(store.add_entry(&manual(None, 1, 100, 200)).is_ok());
    let invalid = BackupDocument {
        format: "work-time-tracker-backup".into(),
        version: 1,
        exported_at_ms: 1,
        projects: store
            .list_projects(true)
            .unwrap_or_else(|error| panic!("projects failed: {error}")),
        tasks: vec![],
        entries: vec![manual(Some(10), 1, 100, 200), manual(Some(11), 1, 150, 250)],
        tracker: TrackerSnapshot::default(),
    };
    assert!(store.restore(&invalid).is_err());
    let original = store
        .list_all_entries()
        .unwrap_or_else(|error| panic!("list failed: {error}"));
    assert_eq!(original.len(), 1);
}
```

**What.** Database → document → another database; document → file →
document; and a document with overlapping entries is refused and the
original data survives.

## 11.6 Checkpoint

```sh
cargo test -p work-time-tracker
```

Expected: `tests/storage.rs` — 8 passed (plus keepers).

```sh
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
diff <(grep -v '^\s*//' ../work_time_tracker/crates/app/src/backup.rs) \
     <(grep -v '^\s*//' crates/app/src/backup.rs)
diff <(grep -v '^\s*//' ../work_time_tracker/crates/app/src/storage.rs) \
     <(grep -v '^\s*//' crates/app/src/storage.rs)
```

Both empty.

```sh
git add -A && git commit -m "Chapter 11: backup"
```

## 11.7 Exercises

1. **What serde says.** Append to `tests/storage.rs` (temporary):

   ```rust
   #[test]
   fn unknown_fields_and_versions_are_rejected() {
       let directory = TempDir::new().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
       let path = directory.path().join("bad.json");
       let extra = r#"{"format":"work-time-tracker-backup","version":1,"exported_at_ms":1,
           "projects":[],"tasks":[],"entries":[],"tracker":{"state":{"kind":"stopped"},"revision":0},
           "comment":"hello"}"#;
       std::fs::write(&path, extra).unwrap_or_else(|error| panic!("write failed: {error}"));
       let message = BackupDocument::read_from_path(&path)
           .err()
           .map(|error| error.to_string())
           .unwrap_or_default();
       println!("EXTRA: {message}");
       let future = r#"{"format":"work-time-tracker-backup","version":2,"exported_at_ms":1,
           "projects":[],"tasks":[],"entries":[],"tracker":{"state":{"kind":"stopped"},"revision":0}}"#;
       std::fs::write(&path, future).unwrap_or_else(|error| panic!("write failed: {error}"));
       let message = BackupDocument::read_from_path(&path)
           .err()
           .map(|error| error.to_string())
           .unwrap_or_default();
       println!("FUTURE: {message}");
       panic!("show");
   }
   ```

   Run `cargo test -p work-time-tracker --test storage unknown_fields`.

   <details><summary>Answer</summary>

   ```
   EXTRA: JSON operation failed: unknown field `comment`, expected one of `format`, `version`, `exported_at_ms`, `projects`, `tasks`, `entries`, `tracker` at line 3 column 17
   FUTURE: backup version 2 is unsupported; expected 1
   ```

   The first error is serde's (wrapped by `AppError::Json`, with line and
   column); the second is ours, from `validate`. `r#"..."#` is a *raw
   string*: no escaping needed for the inner quotes.
   </details>

2. **The triggers still catch it.** In `Store::restore` delete the line
   `document.validate()?;` and run
   `cargo test -p work-time-tracker --test storage invalid_restore`.

   <details><summary>Answer</summary>

   Still passes. The overlap trigger from Chapter 9 aborts the second
   insert, the transaction rolls back, and the original entry survives.
   The validation exists for the *message*, not for safety. Put it back.
   </details>

3. **Backups never contain a running timer (optional keeper).** Append:

   ```rust
   #[test]
   fn backup_refuses_an_active_timer() {
       use work_time_core::{ActiveTimer, TrackerSnapshot, TrackerState, Transition};

       let (_directory, mut store) = temporary_store();
       let running = Transition {
           snapshot: TrackerSnapshot {
               state: TrackerState::Running(ActiveTimer {
                   project_id: ProjectId(1),
                   task_id: None,
                   note: String::new(),
                   start_ms: 1_000,
                   started_monotonic_ms: 0,
                   last_heartbeat_ms: 1_000,
               }),
               revision: 1,
           },
           completed_entries: vec![],
           notifications: vec![],
       };
       assert!(store.persist_transition(&running).is_ok());
       let document = store
           .backup(2_000)
           .unwrap_or_else(|error| panic!("backup failed: {error}"));
       let message = document
           .validate()
           .err()
           .map(|error| error.to_string())
           .unwrap_or_default();
       assert_eq!(message, "backup validation failed: backups must not contain an active timer");
       let (_other_dir, mut other) = temporary_store();
       assert!(other.restore(&document).is_err());
   }
   ```

   <details><summary>Answer</summary>

   Passes. `Store::backup` itself does not validate — it just reads — so
   the document can be built, but neither `write_to_path` nor `restore`
   accepts it. The UI (Chapter 17) shows exactly this message if you back up while a
   timer runs — stop it first.
   </details>

4. **Look at a backup.** Append a temporary test that writes one and prints
   the file:

   ```rust
   #[test]
   fn show_backup_json() {
       let (directory, mut store) = temporary_store();
       assert!(store.add_entry(&manual(None, 1, 100, 200)).is_ok());
       let path = directory.path().join("backup.json");
       let backup = store.backup(1_000).unwrap_or_else(|error| panic!("backup failed: {error}"));
       assert!(backup.write_to_path(&path).is_ok());
       println!("{}", std::fs::read_to_string(&path).unwrap_or_default());
       panic!("show");
   }
   ```

   <details><summary>Answer</summary>

   ```json
   {
     "format": "work-time-tracker-backup",
     "version": 1,
     "exported_at_ms": 1000,
     "projects": [
       {
         "id": 1,
         "name": "General",
         "color": "#3584e4",
         "archived": false,
         "created_at_ms": 1788129927729,
         "updated_at_ms": 1788129927729
       }
     ],
     "tasks": [],
     "entries": [
       {
         "id": 1,
         "project_id": 1,
         "task_id": null,
         "note": "manual",
         "start_ms": 100,
         "end_ms": 200,
         "source": "manual",
         "created_at_ms": 200,
         "updated_at_ms": 200
       }
     ],
     "tracker": {
       "state": {
         "kind": "stopped"
       },
       "revision": 0
     }
   }
   ```

   `to_writer_pretty` indents; every attribute from Chapters 2–3 shows up:
   transparent IDs, snake-case sources, the tagged state. This is the file
   a user can keep, inspect, or hand-edit — hence `deny_unknown_fields`.
   </details>

## Recap

- `BackupDocument` is the format; serde derives do the encoding, and one
  `validate` guards both directions.
- Write temp → fsync → rename so a crash never leaves a half-written file.
- Restore validates first, then replaces all tables inside one
  transaction; failure leaves the old data untouched.
- `deny_unknown_fields` turns typos into errors instead of data loss.

Next: **Chapter 12 — Export, settings, autostart**, three small modules
that round out the services layer.
