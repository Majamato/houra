# Chapter 12 — Export, settings, autostart

**Goal.** Three small, independent modules: CSV export of entries, the
preferences type with its defaults, and the XDG autostart launcher. Files:
`crates/app/src/export.rs`, `crates/app/src/settings.rs`,
`crates/app/src/autostart.rs`, `lib.rs`.

**You will learn**

- Generic functions over `W: Write` — testable I/O.
- The `csv` crate and quoting.
- chrono formatting at the presentation boundary.
- `impl Default` by hand, `clamp`.
- What an XDG autostart entry is and how it is written.

**Prerequisite.** Chapter 11 checkpoint passed.

---

## 12.1 CSV export

```rust
// crates/app/src/export.rs
//! CSV export of time entries.

use std::fs;
use std::io::Write;
use std::path::Path;

use chrono::{Local, TimeZone};
use houra_core::{Project, Task, TimeEntry};

use crate::AppError;

/// Writes one CSV row per entry to any writer; times are local.
pub fn write_csv<W: Write>(
    writer: W,
    entries: &[TimeEntry],
    projects: &[Project],
    tasks: &[Task],
) -> Result<(), AppError> {
    let mut csv = csv::Writer::from_writer(writer);
    csv.write_record([
        "date",
        "start_local",
        "end_local",
        "duration_seconds",
        "project",
        "task",
        "note",
        "source",
    ])?;
    for entry in entries {
        let start = Local.timestamp_millis_opt(entry.start_ms).single();
        let end = Local.timestamp_millis_opt(entry.end_ms).single();
        let project = projects
            .iter()
            .find(|project| project.id == entry.project_id)
            .map_or("(missing)", |project| project.name.as_str());
        let task = entry
            .task_id
            .and_then(|id| tasks.iter().find(|task| task.id == id))
            .map_or("", |task| task.name.as_str());
        let date = start.map_or_else(String::new, |value| value.format("%x").to_string());
        let start_local = start.map_or_else(String::new, |value| value.to_rfc3339());
        let end_local = end.map_or_else(String::new, |value| value.to_rfc3339());
        csv.write_record([
            date,
            start_local,
            end_local,
            (entry.duration_ms() / 1_000).to_string(),
            project.to_owned(),
            task.to_owned(),
            entry.note.clone(),
            format!("{:?}", entry.source),
        ])?;
    }
    csv.flush().map_err(csv::Error::from)?;
    Ok(())
}

/// Writes the CSV atomically (temp file, fsync, rename).
pub fn write_csv_path(
    path: &Path,
    entries: &[TimeEntry],
    projects: &[Project],
    tasks: &[Task],
) -> Result<(), AppError> {
    let Some(parent) = path.parent() else {
        return Err(AppError::DataDirectoryUnavailable);
    };
    fs::create_dir_all(parent).map_err(|source| AppError::io(parent, source))?;
    let temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|source| AppError::io(parent, source))?;
    write_csv(temporary.as_file(), entries, projects, tasks)?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|source| AppError::io(path, source))?;
    temporary
        .persist(path)
        .map_err(|error| AppError::io(path, error.error))?;
    Ok(())
}
```

**What.** A header row, then one row per entry with names resolved from the
project and task lists. `write_csv_path` wraps it with the same atomic
temp-file dance as Chapter 11.

**Why names, not IDs.** A CSV is for a spreadsheet, so it carries the
project *name*. A project that has since been deleted (impossible through
the UI, but possible in principle) shows as `(missing)` rather than
breaking the export.

**Rust — `W: Write`.** The function is generic over *any* writer: a `File`,
a `&File`, a `Vec<u8>` in memory, a network socket. The test in Exercise 1
writes into a `Vec<u8>` and inspects the text — no filesystem. `use
std::io::Write` brings the trait into scope; without it the bound cannot be
named (Exercise 3).

**Rust — `find` and `map_or`.** `iter().find(|p| ...)` returns
`Option<&Project>`; `map_or("(missing)", |p| p.name.as_str())` yields a
`&str` either way, without allocating. `and_then` chains two `Option`s:
"if there is a task ID, look it up". `map_or_else(String::new, ...)` uses the
function `String::new` as the fallback producer.

**Rust — one array, one type.** `write_record([...])` takes an array whose
elements must all have the same type. The header is `[&str; 8]`; the data
row is `[String; 8]`, so the borrowed names are converted with `to_owned()`
and the note is cloned. `format!("{:?}", entry.source)` gives `Manual`,
`Timer`, … via `Debug`.

**Rust — `csv::Writer`.** Handles quoting for you: a note containing a
comma or a quote is escaped correctly (Exercise 1). `flush` returns an
`io::Error`; `map_err(csv::Error::from)` turns it into the CSV error type
that `AppError` already knows how to wrap.

**chrono.** `%x` is the locale's date format; `to_rfc3339()` is the
unambiguous `2026-08-30T17:41:33-05:00` form with the offset — the right
thing for a file that may be read on another machine.

## 12.2 Preferences

```rust
// crates/app/src/settings.rs
//! Preference values and their defaults, independent of GSettings.

use serde::{Deserialize, Serialize};

/// The user preferences. Native builds read them from GSettings (Chapter 18);
/// this type documents the defaults and ranges.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Preferences {
    pub idle_threshold_minutes: u32,
    pub launch_at_login: bool,
    pub notifications: bool,
    pub week_starts_monday: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            idle_threshold_minutes: 5,
            launch_at_login: true,
            notifications: true,
            week_starts_monday: true,
        }
    }
}

impl Preferences {
    pub fn set_idle_threshold_minutes(&mut self, minutes: u32) {
        self.idle_threshold_minutes = minutes.clamp(1, 120);
    }
}
```

**What.** The four preferences and their defaults, as plain Rust. On GNOME
the live values come from GSettings (a schema with the same defaults and
range, Chapter 18); this type keeps the rules visible and testable without
GLib.

**Rust — `impl Default` by hand.** `#[derive(Default)]` would give
`0`/`false` for every field; the real defaults are 5 minutes and `true`, so
the trait is written out. `T::default()` is then the one place to look.

**Rust — `clamp`.** `minutes.clamp(1, 120)` returns the value limited to
the range; a setter with a rule beats a public field with a comment.

## 12.3 Autostart

```rust
// crates/app/src/autostart.rs
//! XDG-compliant per-user autostart management.

use std::fs;
use std::path::{Path, PathBuf};

use directories::BaseDirs;

use crate::AppError;

const FILE_NAME: &str = "io.github.majamato.Houra.desktop";

/// `$XDG_CONFIG_HOME/autostart/<app id>.desktop`.
pub fn default_path() -> Result<PathBuf, AppError> {
    let base = BaseDirs::new().ok_or(AppError::DataDirectoryUnavailable)?;
    Ok(base.config_dir().join("autostart").join(FILE_NAME))
}

/// Writes or removes the per-user autostart launcher.
pub fn set_enabled(enabled: bool, executable: &Path) -> Result<(), AppError> {
    let path = default_path()?;
    if enabled {
        let Some(parent) = path.parent() else {
            return Err(AppError::DataDirectoryUnavailable);
        };
        fs::create_dir_all(parent).map_err(|source| AppError::io(parent, source))?;
        let escaped = executable.to_string_lossy().replace(' ', "\\ ");
        let desktop = format!(
            "[Desktop Entry]\nType=Application\nName=Houra\nExec={escaped} --background\nIcon=io.github.majamato.Houra\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n"
        );
        fs::write(&path, desktop).map_err(|source| AppError::io(&path, source))
    } else if path.exists() {
        fs::remove_file(&path).map_err(|source| AppError::io(&path, source))
    } else {
        Ok(())
    }
}
```

**What.** "Launch at login" on Linux means: a `.desktop` file in
`~/.config/autostart/`. Enabling writes one that runs this executable with
`--background` (Chapter 18); disabling removes only that file.

**Linux — Desktop Entry.** An INI-like format read by every desktop
environment. `Exec` is the command; `NoDisplay=true` hides it from app
menus (the real launcher is installed by the package, Chapter 20);
`X-GNOME-Autostart-enabled` is GNOME's own switch. Because it lives under
the user's config directory, package install/uninstall never touches it.

**Rust — `to_string_lossy`.** Paths on Linux are bytes, not necessarily
UTF-8; `to_string_lossy()` gives a `Cow<str>` (borrowed if valid, converted
if not). `.replace(' ', "\\ ")` escapes spaces for the `Exec` line.

**Rust — `if`/`else if`/`else` as one expression.** All three branches
return `Result<(), AppError>`, and the whole `if` is the function's value.
`fs::write` takes anything that can become a `&Path` for the path and
anything byte-like for the content.

Register the modules:

```rust
// crates/app/src/lib.rs
pub mod autostart;
pub mod backup;
pub mod error;
pub mod export;
pub mod settings;
pub mod storage;
```

## 12.4 Checkpoint

```sh
cargo build
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
for f in export settings autostart; do
  diff <(grep -v '^\s*//' ../work_time_tracker/crates/app/src/$f.rs) \
       <(grep -v '^\s*//' crates/app/src/$f.rs)
done
```

Diffs empty (or a leading blank line from the added doc header).

```sh
git add -A && git commit -m "Chapter 12: export, settings, autostart"
```

## 12.5 Exercises

1. **CSV into memory (optional keeper).** Append to `export.rs`:

   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;
       use houra_core::{EntryId, EntrySource, ProjectId};

       #[test]
       fn csv_has_a_header_and_one_row_per_entry() {
           let project = Project {
               id: ProjectId(1),
               name: "General".into(),
               color: "#3584e4".into(),
               archived: false,
               created_at_ms: 0,
               updated_at_ms: 0,
           };
           let entry = TimeEntry {
               id: Some(EntryId(1)),
               project_id: ProjectId(1),
               task_id: None,
               note: "said \"hi\", left".into(),
               start_ms: 1_700_000_000_000,
               end_ms: 1_700_000_090_000,
               source: EntrySource::Manual,
               created_at_ms: 0,
               updated_at_ms: 0,
           };
           let mut buffer = Vec::new();
           assert!(write_csv(&mut buffer, &[entry], &[project], &[]).is_ok());
           let text = String::from_utf8(buffer).unwrap_or_default();
           println!("{text}");
           let mut lines = text.lines();
           assert_eq!(
               lines.next(),
               Some("date,start_local,end_local,duration_seconds,project,task,note,source")
           );
           let row = lines.next().unwrap_or_default();
           assert!(row.contains(",90,General,,\"said \"\"hi\"\", left\",Manual"));
       }
   }
   ```

   Run `cargo test -p houra --lib -- --nocapture`.

   <details><summary>Answer</summary>

   Passes and prints the header plus one row. `&mut Vec<u8>` implements
   `Write`, so the generic function never knows it is not writing a file.
   The note with a comma and quotes comes out as `"said ""hi"", left"` —
   the CSV standard's escaping, done by the crate. `String::from_utf8`
   converts the bytes back; `text.lines()` iterates lines without the
   newline characters.
   </details>

2. **Defaults and clamping (optional keeper).** Append to `settings.rs`:

   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;

       #[test]
       fn idle_threshold_is_clamped_to_the_schema_range() {
           let mut preferences = Preferences::default();
           preferences.set_idle_threshold_minutes(0);
           assert_eq!(preferences.idle_threshold_minutes, 1);
           preferences.set_idle_threshold_minutes(999);
           assert_eq!(preferences.idle_threshold_minutes, 120);
           assert_eq!(Preferences::default().idle_threshold_minutes, 5);
       }
   }
   ```

   <details><summary>Answer</summary>

   Passes. Chapter 18's GSettings schema declares the same `1..120` range;
   both layers agree so a value can never be out of range no matter which
   path set it.
   </details>

3. **Bounds are required.** Change `pub fn write_csv<W: Write>(` to
   `pub fn write_csv<W>(` and build.

   <details><summary>Answer</summary>

   ```
   error[E0277]: the trait bound `W: std::io::Write` is not satisfied
      |
   19 |     let mut csv = csv::Writer::from_writer(writer);
      |                   ------------------------ ^^^^^^ the trait `std::io::Write` is not implemented for `W`
   ```

   A generic parameter without bounds is an opaque type: the function may
   move it around but cannot call anything on it. Bounds are how a generic
   function states what it needs; the compiler checks the body against the
   bound, not against the concrete types callers happen to use.
   </details>

4. **Look at a real autostart file.** After Chapter 18 runs the app for the
   first time, `cat ~/.config/autostart/io.github.majamato.Houra.desktop`.
   Nothing to do now; remember to come back.

## Recap

- `write_csv<W: Write>` is testable in memory and reusable for files; the
  `csv` crate handles quoting.
- Times are converted to local only when formatting for humans.
- `Preferences::default()` and `clamp` hold the rules that GSettings will
  mirror.
- Autostart is a `.desktop` file under `$XDG_CONFIG_HOME/autostart`, owned by
  the user, never by the package.

Next: **Chapter 13 — Actor**, the thread that owns the store and the engine,
and the handle everyone else talks to.
