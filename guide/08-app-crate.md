# Chapter 8 — App crate

**Goal.** The application crate gets its shape: a library with an
application-level error type and constants, and a `main.rs` that sets up
logging, finds the database path, and runs. The GTK feature flag is declared
but not yet used. Files: `crates/app/Cargo.toml`, `crates/app/src/lib.rs`,
`crates/app/src/error.rs`, `crates/app/src/main.rs`.

**You will learn**

- A library and a binary in one package, and why.
- Cargo features and `#[cfg(feature = "...")]`.
- Error conversion: `#[from]`, `#[source]`, `transparent`, and how `?` uses `From`.
- `impl Into<PathBuf>` parameters.
- `tracing` for logs, `RUST_LOG`, and process exit codes.
- XDG directories with the `directories` crate.

**Prerequisite.** Chapter 7 checkpoint passed (core complete).

---

## 8.1 Dependencies and the feature flag

```toml
# crates/app/Cargo.toml
[package]
name = "work-time-tracker"
description = "A local-first GNOME work time tracker"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
default-run = "work-time-tracker"

[features]
default = []
native-ui = ["dep:gio", "dep:glib", "dep:gtk", "dep:libadwaita"]

[dependencies]
chrono.workspace = true
csv.workspace = true
directories.workspace = true
rusqlite.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
work-time-core = { path = "../core" }
gio = { workspace = true, optional = true }
glib = { workspace = true, optional = true }
gtk = { workspace = true, optional = true }
libadwaita = { workspace = true, optional = true }
tempfile.workspace = true

[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
```

**What.** Every non-GTK dependency the app will need, opted in from the
workspace catalog. The four GTK crates are `optional`, and the feature
`native-ui` turns them on together.

**Why a feature.** With `cargo build` (no feature) the crate compiles
without GTK: SQLite, backups and the storage thread can be tested on a
server or in CI with no display. `cargo build --features native-ui` adds the
window and platform code. Meson (Chapter 20) always passes the feature.

**Rust — features.** A feature is a named switch in `Cargo.toml`. `dep:gtk`
means "enable the optional dependency `gtk`". In code, `#[cfg(feature =
"native-ui")]` includes an item only when the switch is on. The compiler
checks feature names against the manifest, so a typo in a `cfg` is a
warning. `default = []` means no feature is on unless asked.

**Rust — the first build.** `rusqlite` with the `bundled` feature compiles
SQLite's C source once (about a minute). After that, incremental builds are
fast again.

## 8.2 The library root

```rust
// crates/app/src/lib.rs
//! Application services for Work Time Tracker.
//!
//! The storage actor is available without GTK, which lets database and backup
//! behavior run in CI or on a server. `native-ui` adds the GNOME presentation
//! and platform integrations.

pub mod error;

pub use error::AppError;

/// Reverse-DNS application ID shared by GApplication, the desktop file,
/// icons, GSettings, resources, notifications, and package metadata.
pub const APP_ID: &str = "io.github.majamato.WorkTimeTracker";
pub const APP_NAME: &str = "Work Time Tracker";
```

**What.** The package now has *both* `lib.rs` and `main.rs`. Cargo compiles
them as two crates: a library named `work_time_tracker` and a binary that
depends on it. Each later chapter adds a `pub mod` line here.

**Why split lib and bin.** Integration tests (Chapter 9 onwards) can only
`use` a library. Keeping `main.rs` tiny and everything else in the library
makes the whole application testable without launching it.

**Rust — `pub mod` vs `mod`.** The core crate kept modules private and
re-exported chosen items. Here modules are `pub`: `work_time_tracker::storage::Store`
is a fine path for an application's internal layers, and tests need to
reach them.

**Rust — `const`.** A compile-time constant; the type must be written.
`&str` literals live in the binary for the whole program. The reverse-DNS
ID is the one string GNOME uses to tie together everything about this app
(Chapters 14, 18, 20).

## 8.3 The application error

```rust
// crates/app/src/error.rs
use std::path::PathBuf;

use thiserror::Error;
use work_time_core::DomainError;

/// Everything that can go wrong at the application boundary: rejected domain
/// operations plus database, serialization, filesystem, and worker failures.
#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("database operation failed: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("JSON operation failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("CSV export failed: {0}")]
    Csv(#[from] csv::Error),
    #[error("I/O operation failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("application data directory is unavailable")]
    DataDirectoryUnavailable,
    #[error("the storage worker stopped unexpectedly")]
    WorkerStopped,
    #[error("backup version {found} is unsupported; expected {expected}")]
    UnsupportedBackupVersion { found: u32, expected: u32 },
    #[error("restore requires a stopped timer")]
    RestoreWhileActive,
    #[error("backup validation failed: {0}")]
    InvalidBackup(String),
    #[error("project {0:?} does not exist or is archived")]
    InvalidProject(work_time_core::ProjectId),
    #[error("task {0:?} does not exist, is archived, or belongs to another project")]
    InvalidTask(work_time_core::TaskId),
    #[error("project/task cannot be permanently deleted because history references it")]
    ReferencedItem,
}

impl AppError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
```

**What.** The one error type every function in this crate returns. Four
variants *wrap* errors from other crates (`DomainError`, SQLite, JSON, CSV);
the rest are this crate's own failures. Some variants will only be used in
later chapters — that is fine for `pub` items (the compiler only warns about
unused *private* code).

**Why one type per layer.** A GTK dialog receives an `AppError` and shows
`error.to_string()`; it never needs to know whether SQLite or the domain
rejected the change. Callers that *do* care can `match` on the variant.

**Rust — `#[from]`.** `Database(#[from] rusqlite::Error)` makes thiserror
generate `impl From<rusqlite::Error> for AppError`. The `?` operator calls
`From::from` on the error before returning it, so a function returning
`Result<_, AppError>` can write `connection.execute(...)?` on a
`Result<_, rusqlite::Error>` and the conversion happens silently. Remove the
attribute and `?` stops compiling (Exercise 3).

**Rust — `transparent`.** `#[error(transparent)]` on `Domain` says: my
message *is* the inner error's message, add nothing. A rejected name reads
"name must contain a non-whitespace character", not "domain error: name
must …" (Exercise 2).

**Rust — `#[source]`.** Marks the field holding the underlying error so
tools can walk the chain (`error.source()`). `Io` is a struct-like variant
with a hand-written constructor because it has two fields and no single
`From` makes sense — an I/O error alone does not know which path failed.

**Rust — `impl Into<PathBuf>`.** An *anonymous generic* parameter: the
function accepts any type that can convert into a `PathBuf` (`&Path`,
`PathBuf`, `&str`, `String`) and calls `.into()` itself. Callers write
`AppError::io(parent, source)` with whatever they have. `PathBuf` is the
owned path type; `Path` is its borrowed form, like `String` and `str`.

**Rust — `Debug` only.** `AppError` derives `Debug` but not `Clone` or
`PartialEq`: `std::io::Error` and `rusqlite::Error` are neither, so the enum
cannot be. Tests compare messages instead.

## 8.4 `main`

```rust
// crates/app/src/main.rs
use std::path::PathBuf;

use directories::BaseDirs;
use tracing_subscriber::EnvFilter;
use work_time_tracker::{APP_NAME, AppError};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();
    if let Err(error) = run() {
        tracing::error!(error = %error, "application terminated");
        eprintln!("{APP_NAME}: {error}");
        std::process::exit(1);
    }
}

/// `$XDG_DATA_HOME/work-time-tracker/tracker.sqlite3`, usually
/// `~/.local/share/work-time-tracker/tracker.sqlite3`.
fn data_path() -> Result<PathBuf, AppError> {
    let base = BaseDirs::new().ok_or(AppError::DataDirectoryUnavailable)?;
    Ok(base
        .data_local_dir()
        .join("work-time-tracker")
        .join("tracker.sqlite3"))
}

#[cfg(feature = "native-ui")]
fn run() -> Result<(), AppError> {
    work_time_tracker::native::run(data_path()?)
}

#[cfg(not(feature = "native-ui"))]
fn run() -> Result<(), AppError> {
    let _path = data_path()?;
    eprintln!(
        "This build contains the tested storage engine but not the GNOME UI. \
         Rebuild with `--features native-ui` (Meson does this automatically)."
    );
    Ok(())
}
```

**What.** `main` installs the logger, calls `run`, and turns an error into a
log line, a message on stderr, and exit code 1. `data_path` computes where
the database lives. Exactly one `run` exists per build: the feature-gated
one calls the GTK module (which does not exist until Chapter 14 — so
`--features native-ui` will not compile before then; that is expected), the
other prints a note.

**Why `main` is small.** Rust's `main` cannot easily return a rich error
with a good message, and an error at this level is fatal anyway. Keeping
all fallible work in `run() -> Result` means `main` is the *only* place
that decides what "fatal" looks like.

**Rust — `if let Err(error) = run()`.** Pattern-matches only the `Err`
case; on `Ok(())` nothing happens and `main` returns normally (exit code 0).

**Rust — `tracing`.** `tracing::error!(error = %error, "message")` records a
structured event: a message plus a field named `error`, formatted with
`Display` (`%`). The *subscriber* installed at the top decides what to print;
`EnvFilter::from_default_env()` reads `RUST_LOG` (`RUST_LOG=debug`,
`RUST_LOG=error`, unset = nothing). `eprintln!` is `println!` to stderr.

**Rust — `std::process::exit(1)`.** Ends the process immediately with that
code; destructors do not run, which is why it is called only after
everything else in `main` is done.

**Linux — XDG.** `BaseDirs` implements the freedesktop base-directory
rules: `data_local_dir()` is `$XDG_DATA_HOME` or `~/.local/share`. Apps
never hard-code `~/.something`. `.join` builds paths with the right
separator. `BaseDirs::new()` returns `None` only if no home directory can be
determined at all.

**Rust — `\` in a string literal.** A backslash at the end of a line
continues the literal on the next line, skipping the leading whitespace.

## 8.5 Checkpoint

```sh
cargo run
```

Expected (after a one-time longer compile for SQLite):

```
     Running `target/debug/work-time-tracker`
This build contains the tested storage engine but not the GNOME UI. Rebuild with `--features native-ui` (Meson does this automatically).
```

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
diff <(grep -v '^\s*//' ../work_time_tracker/crates/app/src/error.rs) \
     <(grep -v '^\s*//' crates/app/src/error.rs)
diff <(grep -v '^\s*//' ../work_time_tracker/crates/app/src/main.rs) \
     <(grep -v '^\s*//' crates/app/src/main.rs)
```

Both diffs empty (or a blank line). `lib.rs` will keep growing.

```sh
git add -A && git commit -m "Chapter 8: app crate"
```

## 8.6 Exercises

1. **Force the error path.** In `data_path`, replace `BaseDirs::new()` with
   `None::<BaseDirs>` and run:

   ```sh
   cargo run; echo "status=$status"          # fish
   RUST_LOG=error ./target/debug/work-time-tracker
   ```

   <details><summary>Answer</summary>

   ```
   Work Time Tracker: application data directory is unavailable
   status=1
   2026-08-30T22:41:33.277705Z ERROR application terminated error=application data directory is unavailable
   Work Time Tracker: application data directory is unavailable
   ```

   The first run shows only the `eprintln!` line and exit code 1. With
   `RUST_LOG=error` the tracing subscriber also prints the structured event,
   with a timestamp and the `error=` field. `None::<BaseDirs>` is the
   turbofish again: `None` needs to know which `Option` it is.
   </details>

2. **Conversion and transparency (optional keeper).** Append to `error.rs`:

   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;

       fn rejects() -> Result<(), AppError> {
           work_time_core::validate_name("   ")?;
           Ok(())
       }

       #[test]
       fn domain_errors_convert_and_stay_transparent() {
           let error = rejects().err().map(|error| error.to_string()).unwrap_or_default();
           assert_eq!(error, "name must contain a non-whitespace character");
       }

       #[test]
       fn io_errors_name_the_path() {
           let missing = std::fs::read("/definitely/missing").err();
           let Some(source) = missing else { return };
           let error = AppError::io("/definitely/missing", source);
           println!("{error}");
           assert!(error.to_string().starts_with("I/O operation failed for /definitely/missing:"));
       }
   }
   ```

   Run `cargo test -p work-time-tracker --lib -- --nocapture`.

   <details><summary>Answer</summary>

   ```
   I/O operation failed for /definitely/missing: No such file or directory (os error 2)
   test error::tests::domain_errors_convert_and_stay_transparent ... ok
   test error::tests::io_errors_name_the_path ... ok
   ```

   `rejects` returns `AppError` but the `?` is applied to a
   `Result<(), DomainError>` — `From` did the conversion. The message is
   the domain message, untouched, because of `transparent`. `--lib` runs
   only the library's unit tests.
   </details>

3. **Remove a `#[from]`.** With the tests from exercise 2 in place, delete
   `#[from]` from the `Domain` variant and run the tests.

   <details><summary>Answer</summary>

   ```
   error[E0277]: `?` couldn't convert the error to `error::AppError`
      |
   56 |         work_time_core::validate_name("   ")?;
      |         ------------------------------------^ the trait `From<DomainError>` is not implemented for `error::AppError`
   note: `error::AppError` needs to implement `From<DomainError>`
   ```

   `?` is sugar for "on `Err(e)`, `return Err(From::from(e))`". No `From`,
   no `?`. Put the attribute back (and decide whether to keep the tests).
   </details>

## Recap

- One package, two crates: a testable library and a thin binary.
- `native-ui` is a Cargo feature; without it the app has no GTK at all.
- `AppError` wraps lower-level errors with `#[from]`, so `?` converts them;
  `transparent` keeps domain messages clean.
- `main` logs, prints, and exits 1; all real work returns `Result`.
- The database path follows XDG through `directories`.

Next: **Chapter 9 — SQLite store**, the schema, migrations, and the
clean-shutdown marker.
