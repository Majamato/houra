# Houra — rebuild guide

You already have a working Houra in `../work_time_tracker`. This
guide rebuilds it from an empty folder, one chapter at a time, so that at the
end you have the same working program **and** you understand why every piece
is written the way it is.

The guide gives you the code. Your job is to type it, run the checkpoint,
read the explanations, and do the small experiments. Nothing here is a puzzle;
the learning comes from doing the steps in order and watching the compiler.

## Decisions

Answers from the interview that shaped this guide. A later session continues
from these without asking again.

- Learn: basic/intermediate Rust and how a native GNOME (GTK4/libadwaita) app is built.
- Mode: rebuild of `../work_time_tracker` (the original stays untouched).
- Level at start: almost no Rust; Flutter/Dart background.
- Prose: English. Order: bottom-up, runnable at every chapter.
- Code: full code in explained chunks; built code keeps minimal, idiomatic comments.
- Exercises: 2–4 short experiments per chapter, answers collapsed.
- Delivery: all 21 chapters were written at once (the user asked for that after
  Chapters 1–2); progress is tracked in the list below. Each chapter's
  checkpoint and exercise outputs were verified in a scratch build before
  the chapter was written.

## How to use this guide

1. One chapter per sitting. Chapters are 30–90 minutes.
2. Type the code from the chunks (copying is allowed, but typing makes you
   read every token, which is the point).
3. Run the **Checkpoint** at the end of the chapter. It must pass before you
   continue. If it does not, compare with the original using the `diff`
   command in *Conventions* below.
4. Commit: `git add -A && git commit -m "Chapter N"`.
5. Do the **Exercises**. They are experiments: change something, read what
   the compiler or the tests say, then throw the change away with
   `git checkout -- .` so your code stays identical to the guide.
6. Tick the chapter in the *Progress* list below and open the next one.

## Before you start

You already built the original on this machine, so the toolchain is there.
Check anyway:

```sh
rustc --version      # 1.85 or newer (the guide was written with 1.98)
cargo --version
cargo clippy --version
cargo fmt --version
pkg-config --modversion gtk4 libadwaita-1   # needed from Chapter 14 on
```

Editor: anything with **rust-analyzer** (VS Code, Zed, Helix, Neovim). It shows
types on hover and explains errors inline; that is half of learning Rust.

Create the project folder next to the original and put it under git from the
first minute:

```sh
mkdir -p ~/Develop/Personal/Houra
cd ~/Develop/Personal/Houra
git init
```

The `guide/` folder (this file) lives inside that project. Commit it too.

## Conventions

- Every code block starts with a comment naming the file it belongs to, for
  example `// crates/core/src/model.rs`. A block that contains `// ...` means
  "the rest of the file stays as it is".
- After each chunk you will find short notes:
  - **What** — what the code does.
  - **Why** — the design reason in this application.
  - **Rust** — the language feature being used, explained the first time it
    appears and only referenced afterwards.
  - **Idiom** — how experienced Rust programmers write this (naming, error
    handling, ownership choices). These are the "good Rust" guidelines.
  - **Dart** — a comparison with Flutter/Dart, only where it helps.
- Commands are shown for `fish`/`bash` from the project root.
- The study code uses `///` documentation comments on public items and very
  few `//` comments. The explanations live here, not in the code.
- From Chapter 14 on, run the study build with an isolated environment so it
  never touches the real app's data, preferences or autostart file:

  ```sh
  set -x XDG_DATA_HOME /tmp/houra-study          # fish; bash: export XDG_DATA_HOME=/tmp/houra-study
  set -x XDG_CONFIG_HOME /tmp/houra-config
  set -x GSETTINGS_BACKEND memory
  set -x GSETTINGS_SCHEMA_DIR /tmp/houra-schemas  # after Chapter 18 compiles the schema there
  ```
- Comparing your file with the original, ignoring its teaching comments:

  ```sh
  diff <(grep -v '^\s*//' ../work_time_tracker/crates/core/src/model.rs) \
       <(grep -v '^\s*//' crates/core/src/model.rs)
  ```

  Small differences in comments, wording of doc comments, or blank lines are
  fine. Differences in code are not.

## Chapter map

The order is bottom-up: pure Rust first, then storage and threads, then the
GTK interface, then Linux integration, then packaging. Every chapter ends with
something that compiles and runs.

**Part 1 — Pure Rust: the domain (no GTK, no database)**

| # | Chapter | You build | You learn |
| --- | --- | --- | --- |
| 1 | Workspace and first run | Cargo workspace, two crates, hello-world binary | package/crate/workspace, editions, Cargo commands, lints |
| 2 | Domain model | IDs, `Project`, `Task`, `TimeEntry`, `DomainError`, validation | structs, enums, `Option`/`Result`, `?`, derives, traits, modules, macros |
| 3 | Tracker state | `TrackerState`, commands, decisions, transitions | enums with data, exhaustive `match`, "invalid states cannot exist" |
| 4 | Clock | `Clock` trait, `SystemClock`, `ManualClock` | traits, generics bounds, `Arc<Mutex<_>>`, monotonic vs wall time |
| 5 | Engine basics | `TrackerEngine`: start/stop/edit/heartbeat, first tests | generics, `&mut self`, match on tuples, integration tests |
| 6 | Idle and recovery | The rest of the state machine, property tests | struct update syntax, ownership transfer, proptest |
| 7 | Reports | Overlap validation, grouping by local day | slices, iterators, `BTreeMap`, chrono, DST-safe dates |

**Part 2 — Application services (SQLite, files, threads; still no GTK)**

| # | Chapter | You build | You learn |
| --- | --- | --- | --- |
| 8 | App crate | `AppError`, `main.rs`, data directory, feature flag | lib + bin, Cargo features, `#[cfg]`, error conversion, tracing, XDG |
| 9 | SQLite store | Schema, migrations, snapshot persistence | rusqlite, transactions, `PRAGMA user_version`, WAL, lifetimes |
| 10 | Entries, projects, tasks | CRUD, overlap rejection, archiving | `params!`, `query_map`, collecting `Result`s, let-chains |
| 11 | Backup | Versioned JSON backup and transactional restore | serde in depth, atomic file writes |
| 12 | Export, settings, autostart | CSV export, preferences, XDG autostart | generic writers, `Default`, chrono formatting |
| 13 | Actor | Storage thread, `TrackerHandle`, `TrackerService` | threads, channels, `Send`, `move`, `FnOnce`, the actor pattern |

**Part 3 — GTK4 / libadwaita interface**

| # | Chapter | You build | You learn |
| --- | --- | --- | --- |
| 14 | First GTK window | build script, GResource, `MainWindow` subclass | build.rs, GObject subclassing, templates, main loop, `Rc`/`RefCell` |
| 15 | Tracker page | timer, dropdowns, day list, heartbeat | models, timeouts, weak references, `spawn_blocking` |
| 16 | Projects and reports | two more pages, name dialog, CSV export | dynamic widgets, `Fn + 'static` bounds, async file dialogs |
| 17 | Dialogs and actions | manual/edit entry, idle, recovery, quit, backup/restore, menu, shortcuts | closures owning data, GAction, close-to-hide |
| 18 | Settings and background | GSettings schema, preferences, first-run autostart, `--background`, hold | schemas, `is_none_or`, single-instance apps |

**Part 4 — Native Linux integration**

| # | Chapter | You build | You learn |
| --- | --- | --- | --- |
| 19 | D-Bus | idle monitor, screensaver, logind suspend | D-Bus concepts, `GDBusProxy`, GVariant, atomics, file descriptors |

**Part 5 — Packaging and wrap-up**

| # | Chapter | You build | You learn |
| --- | --- | --- | --- |
| 20 | Packaging | desktop file, AppStream, icons, Meson, gettext, RPM spec | freedesktop standards, Cargo vs Meson, staged installs |
| 21 | Wrap-up | Final checks | fmt/clippy/test/release, concept index, where to go next |

## Progress

- [ ] Chapter 1 — Workspace and first run
- [ ] Chapter 2 — Domain model
- [ ] Chapter 3 — Tracker state
- [ ] Chapter 4 — Clock
- [ ] Chapter 5 — Engine basics
- [ ] Chapter 6 — Idle and recovery
- [ ] Chapter 7 — Reports
- [ ] Chapter 8 — App crate
- [ ] Chapter 9 — SQLite store
- [ ] Chapter 10 — Entries, projects, tasks
- [ ] Chapter 11 — Backup
- [ ] Chapter 12 — Export, settings, autostart
- [ ] Chapter 13 — Actor
- [ ] Chapter 14 — First GTK window
- [ ] Chapter 15 — Tracker page
- [ ] Chapter 16 — Projects and reports
- [ ] Chapter 17 — Dialogs and actions
- [ ] Chapter 18 — Settings and background
- [ ] Chapter 19 — D-Bus
- [ ] Chapter 20 — Packaging
- [ ] Chapter 21 — Wrap-up

## Concept index

"Where" is chapter.section.

| Concept | Where |
| --- | --- |
| Package, crate, workspace; virtual manifest; `[workspace.package]` / `[workspace.dependencies]` | 1.1 |
| Editions, MSRV; `[profile.release]` | 1.1 |
| `[lints]`: `unsafe_code`, `unwrap_used`, `expect_used` | 1.3 |
| Library crate vs binary crate; `fn main`, `println!` | 1.3, 1.4 |
| `cargo build/run/test/fmt/clippy`; `Cargo.lock` | 1.6 |
| `use`, `::` paths; `mod` and files; `pub`; `pub use` re-exports | 2.1 |
| `macro_rules!`; tuple structs / newtypes; `const fn` | 2.1 |
| `#[derive]` and the standard traits; implementing `Display` by hand | 2.1 |
| Enums; struct-like variants; `thiserror` `#[error]` | 2.2 |
| `String` vs `&str`; `&self`; `Option`; `Result` and `?`; early `return`; `if` as expression | 2.3 |
| Iterator adapters (`bytes`, `skip`, `all`), closures; saturating arithmetic | 2.3 |
| `#[cfg(test)]`, `#[test]`, `assert!`/`assert_eq!`; moves; `#[must_use]` | 2.5 |
| Enums with data; make invalid states unrepresentable; `#[default]` | 3.2 |
| Exhaustive `match`; returning `Option<&T>`; borrow checker basics | 3.2, 3.6 |
| serde `tag`/`content`/`rename_all`/`transparent` | 3.2, 3.6 |
| Tuple-like vs unit-like vs struct-like variants; `Vec<T>` | 3.4 |
| Traits, supertraits (`Clone + Send + Sync + 'static`), `impl Trait for Type` | 4.1 |
| `Instant` vs `SystemTime`, `Duration`; `Default` by hand; bounded conversions | 4.2 |
| `Arc<Mutex<_>>`, interior mutability, lock poisoning, guards; `map_or` | 4.3 |
| Generic structs and `impl<C: Clock>`; static dispatch | 5.1 |
| `let ... else` | 5.2 |
| `&mut self`; `let mut`; match on tuples; moving out of enums; `mut` in patterns; `return` in an arm | 5.3 |
| Private helpers with borrowed parameters | 5.4 |
| Integration tests in `tests/`; `unwrap_or_else(|e| panic!)`; `.into()` | 5.5 |
| `ok_or_else` + `?`; `\|\|` | 6.1 |
| Struct-update syntax `..value`; `_` in variant patterns | 6.2 |
| Ownership into helpers; `&mut Vec` out-params | 6.3 |
| `mut` parameters; `match` as assignment | 6.4 |
| `match` in tests with `panic!("{state:?}")`; turbofish `sum::<i64>()` | 6.5 |
| Property-based testing: `proptest!`, strategies, shrinking, `prop_assert!` | 6.6, 6.8 |
| Slices `&[T]`; `Vec<&T>`; `sort_by_key`, `windows`, `dedup`; `if let Some(..)` | 7.1 |
| Derived `Ord` ordering; `usize` | 7.2 |
| `BTreeMap` and the entry API; `while` + `break` in `let ... else`; `into_iter().map().collect()` | 7.3 |
| chrono: `Local`, `DateTime`, `NaiveDate`, DST-safe midnight | 7.3 |
| Arrays vs slices; type annotation vs turbofish | 7.4 |
| Cargo features, `#[cfg(feature)]`, optional dependencies | 8.1 |
| `pub mod`; `const` | 8.2 |
| `#[from]`, `#[source]`, `transparent`; `?` and `From`; `impl Into<PathBuf>`; `PathBuf`/`Path` | 8.3 |
| `if let Err`; `tracing` and `RUST_LOG`; `process::exit`; XDG dirs | 8.4 |
| Owning a `rusqlite::Connection`; pragmas; `map_err` for `io::Error` | 9.1 |
| `PRAGMA user_version` migrations; transactions and RAII rollback; `query_row`; SQL constraints/triggers | 9.2 |
| `OptionalExtension`; `map_or_else`; `prepare`/`query_map`/`collect::<Result<Vec<_>,_>>`; `params!` | 9.3 |
| `Option::map`; `TempDir` RAII; byte strings | 9.4, 9.5 |
| `for x in &vec`; `execute` row count; passing `fn` items as closures | 10.1, 10.2 |
| `&str` in, `String` stored; `&self` vs `&mut self` for DB writes | 10.3 |
| Let-chains | 10.4 |
| `Transaction<'_>` lifetimes; `Deref` to `Connection`; `.map(EntryId)`; `.into()` on errors | 10.5 |
| `&'static str`; exhaustive vs `_` arms; `rusqlite::Result`; matching `String` via `as_str()` | 10.6 |
| serde `deny_unknown_fields`; versioned formats | 11.1 |
| Atomic writes: `NamedTempFile`, `sync_all`, `persist`; `fs::read`, `from_slice` | 11.2 |
| `iter().any()`; validation order | 11.3 |
| Raw strings `r#"..."#` | 11.7 |
| Generic `W: Write`; `find`/`map_or`/`and_then`; arrays of one type; `csv::Writer` | 12.1 |
| `impl Default` by hand; `clamp` | 12.2 |
| `to_string_lossy`; `if`/`else if` as expression; XDG autostart desktop entries | 12.3 |
| Type aliases; private enums; `Box` to shrink an enum | 13.1 |
| `mpsc` channels; one-shot replies; generic `request<T>(impl FnOnce)`; `map_err(\|_\|)`; `#[derive(Clone)]` handles | 13.2 |
| `thread::Builder`, `move` closures, `'static`; `JoinHandle`; `Option::take`; `self` by value | 13.3 |
| `while let`; `matches!`; `and_then` with mutation; `let _ignored =` | 13.4 |
| Closures that build values; `thread::spawn` return values; `assert_ne!` | 13.5 |
| Build scripts, `cargo:rerun-if-changed`, `OUT_DIR`; `Command`; GResource; GTK CSS | 14.1 |
| UI templates, `translatable="yes"`, libadwaita | 14.2 |
| GLib main loop; `startup`/`activate`; `Rc<RefCell<Option<_>>>`; `include_bytes!`/`env!`/`concat!`; `use x as y`; preludes | 14.3 |
| GObject subclassing: `mod imp`, `glib::wrapper!`, `ObjectSubclass`, `CompositeTemplate`, `TemplateChild` | 14.4 |
| `Object::builder()`; `connect_clicked`; `glib::clone!` weak captures; `timeout_add_seconds_local`; `AlertDialog` | 14.5 |
| Widgets are not `Send` | 14.7 |
| `Cell<T>` vs `RefCell<T>` | 15.2 |
| `spawn_blocking`; `_local` timeouts; property-notify signals; accessibility labels | 15.3 |
| `StringList` models + typed `Vec`; `usize::try_from(u32)`; `.get(i)`; `unwrap_or_default` | 15.4 |
| Or-patterns; `GString::to_string()` | 15.5 |
| Rebuilding children; match guards; `i64::from(i32)`; `ActionRow` | 15.6 |
| Builders, `add_suffix`, `valign`; `#[strong]` captures; owned vs borrowed iteration | 16.3 |
| `?` on `Option`; core does the work | 16.4 |
| Async `FileDialog` callbacks; `map_err`/`and_then` chains; `None::<&T>` | 16.5 |
| Generic method with `where`; `Fn + 'static`; `.map(\|_\| ())` | 16.6, 16.8 |
| Actions from menus | 17.1 |
| `.single()` for local times; `upcast`; closures owning widgets; nested closures | 17.2 |
| Tuple `let ... else`; `then_some().flatten()`; `..existing.clone()`; `position` | 17.3 |
| `ref` in patterns with guards; `let Variant(x) = v else`; `connect_response(None, ..)` | 17.4 |
| `is_ok_and`; cloning inside `Fn` handlers; `Destructive` appearance; `self.application()` | 17.5 |
| `ActionEntry`, accelerators, `connect_close_request`; block expressions as arguments; `pub(crate)` | 17.6 |
| GSettings schemas and types | 18.1 |
| `SettingsSchemaSource` lookup; turbofish `None::<&T>`; `match` with logging arms | 18.2 |
| `add_main_option`; `hold()`; `Rc<Cell<bool>>`; `std::env::args().any` | 18.3 |
| `is_none_or` / `is_some_and`; `glib::idle_add_local_once` | 18.4 |
| Preferences widgets; `as u32` after clamp; `f64::from` | 18.5 |
| D-Bus vocabulary; `busctl` | 19.0 |
| `DBusProxy::for_bus_sync`, `call_sync`, `connect_g_signal`; GVariant tuples; `AtomicU32` + `Ordering::Relaxed`; keeping proxies alive | 19.2 |
| Delay inhibitors; `OwnedFd` and `Drop`; `let Ok(x) = .. else` | 19.3 |
| fds over D-Bus; `gio::Notification` | 19.4 |
| Desktop entries, AppStream, icon names, the app id everywhere | 20.1 |
| Meson vs Cargo; `custom_target`, `i18n.merge_file`, `install_data`; `--locked` | 20.2 |
| gettext: `POTFILES.in`, `.pot`, `.po`, `.mo` | 20.3 |
| `DESTDIR` staged installs; validators | 20.5 |
| `cargo vendor`; offline RPM builds | 20.6 |

## Glossary

The application's own vocabulary. Use these words in code and commit messages.

- **Project / Task** — a task belongs to exactly one project. Project 1,
  "General", always exists and cannot be archived.
- **Time entry** — a completed, half-open interval `[start_ms, end_ms)`
  attributed to a project (and optionally a task). Entries never overlap.
- **Active timer** — the interval currently being tracked; it becomes an
  entry when it stops.
- **Tracker state** — exactly one of: *Stopped*, *Running*, *Idle pending*
  (the user went away; waiting for a decision), *Recovery pending* (the app
  died while running; waiting for a decision).
- **Command** — a request to change the tracker state (start, stop, idle
  detected, resolve idle, …). Applying a command produces a **transition**.
- **Transition** — the result of a command: the new **snapshot**, the entries
  completed by it, and notifications to show.
- **Snapshot** — the tracker state plus a **revision** counter that grows by
  one per accepted command.
- **Heartbeat** — every 30 seconds the running timer records "I was alive at
  this instant". After a crash, recovery never proposes time beyond the last
  heartbeat.
- **Wall time** — real-world UTC milliseconds since 1970; stored on disk.
- **Monotonic time** — a counter that only moves forward; used for the live
  timer display so an OS clock adjustment cannot make it jump.

## Deviations from the original

All deliberate; the final diff (Chapter 21) shows exactly these.

- `DomainError::InvalidState` is added in Chapter 3, when `TrackerState` exists.
- Core lists `serde_json` as a dev-dependency only if you keep Chapter 3,
  exercise 4.
- Optional unit tests offered as "keepers" in exercises (Chapters 2, 4, 6, 8,
  9, 10, 11, 12, 13); the original keeps all tests under `tests/`.
- `native/mod.rs` has `load_css()`: the original bundles `style.css` but never
  loads it, so its tabular-digits rule had no effect.
- `report_bounds` hard-codes Monday in Chapter 16 and reads GSettings from
  Chapter 18; Chapter 18 defers the platform-integration block to Chapter 19.
- `crates/app/Cargo.toml` omits the original's empty `[build-dependencies]`
  section.
- `use` line order in `model.rs`/`error.rs` differs (rustfmt accepts both).
- `po/en.po` is generated with `msginit` rather than copied; the header differs.
