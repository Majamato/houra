# Chapter 21 — Wrap-up

**Goal.** Confirm the rebuilt project is complete and clean, look back at
what the code taught, and pick a direction for what comes next. No new
files.

**Prerequisite.** Chapter 20 checkpoint passed.

---

## 21.1 The final checklist

From the project root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --workspace --release --all-features
./scripts/build-release.sh
DESTDIR="$PWD/stage" meson install -C build-release
desktop-file-validate stage/usr/local/share/applications/*.desktop
appstreamcli validate --no-net stage/usr/local/share/metainfo/*.xml
glib-compile-schemas --strict --dry-run stage/usr/local/share/glib-2.0/schemas
```

Expected: no output from `fmt` and `clippy` except `Finished`; 19 tests
(plus every keeper you chose) passing; a release binary; a staged tree
under `stage/usr/local/` with the binary, desktop file, metainfo, schema,
icons, licence and `.mo` catalogue; all three validators silent or
"successful".

Then the diff of the whole tree against the original, ignoring comments
and blank lines:

```sh
for f in $(cd ../work_time_tracker && find crates data meson.build meson_options.txt \
    build-aux po/meson.build po/POTFILES.in po/LINGUAS scripts packaging rustfmt.toml \
    -type f | grep -v README | sort); do
  n=$(diff <(grep -vE '^\s*(//|#|<!--|--)' ../work_time_tracker/$f | sed '/^\s*$/d') \
           <(grep -vE '^\s*(//|#|<!--|--)' $f | sed '/^\s*$/d') | grep -cE '^[<>]')
  [ "$n" != "0" ] && echo "$f: $n differing lines"
done
```

Expected differences, all deliberate:

| File | Difference |
| --- | --- |
| `crates/app/src/native/mod.rs` | `load_css()` (the original never loads `style.css`) |
| `crates/core/Cargo.toml` | `serde_json` dev-dependency only if you kept Chapter 3's JSON test |
| `Cargo.lock` | dependency patch versions resolved on your date |
| any file with exercise keepers | the tests you chose to keep |

Everything else — every Rust file, the SQL, the UI, the schema, the
build system — is the original program, typed by you.

## 21.2 What the program taught

Look back through the concept index in `00-outline.md`. Grouped by theme,
this is what you now have hands-on experience with:

**Rust, the language.** Ownership and borrowing (`String`/`&str`, moves,
`&self`/`&mut self`, `Rc`/`Arc`), enums as sum types with exhaustive
`match`, `Option`/`Result` with `?`, traits and generics with bounds
(`Clock`, `TrackerEngine<C>`, `W: Write`, `F: Fn + 'static`), closures and
the `Fn`/`FnMut`/`FnOnce` ladder, iterators and combinators, interior
mutability (`Cell`, `RefCell`, `Mutex`, atomics), lifetimes where they
matter (`Transaction<'_>`, `&'static str`), modules and visibility,
macros (`macro_rules!`, derives, `glib::clone!`), and edition-2024 syntax
(`let ... else`, let-chains).

**Rust, the practice.** Workspaces and features, lints that forbid
`unsafe` and deny `unwrap`, typed errors per layer with `thiserror`,
unit/integration/property tests with a manual clock, atomic file writes,
build scripts, `tracing` for logs, and reading compiler errors as
information rather than obstacles.

**The design.** A pure core with the rules; services around it; one thread
owning the database and the state machine; the UI rendering snapshots and
sending commands; platform events as producers, never owners;
clone-before-commit so memory and disk agree; validation in Rust, safety
in SQL; recovery bounded by a heartbeat.

**GNOME and Linux.** GObject subclassing, templates and resources,
libadwaita widgets and dialogs, actions and shortcuts, GSettings, D-Bus
proxies and signals, XDG directories, autostart, desktop files, AppStream,
icons, Meson, gettext, and an RPM spec that builds offline.

## 21.3 Reading the original now

Open `../work_time_tracker/PROJECT_TOUR.md` and the files under `docs/`.
They were slow to read before because every sentence referred to code you
had not built. Read them once more now: they should read as a summary of
decisions you already understand — and where they explain something this
guide skipped, that is the next thing to learn.

## 21.4 Where to go next

Pick one; each is a few hours and builds on what is here.

1. **A change that crosses every layer.** Add a "billable" flag to
   entries: `TimeEntry` field (Chapter 2), a migration to schema version 2
   (Chapter 9 — `if version < 2`), `read_entry`/`insert_entry` (Chapter
   10), the backup format version (Chapter 11), a switch in the entry
   dialogs (Chapter 17), a column in the CSV (Chapter 12). Every `match`
   and constructor the compiler complains about is a place the flag must
   flow through — that is the exhaustiveness working for you.
2. **A second UI.** A terminal client (`crates/cli`) that uses only
   `work-time-core` and `work-time-tracker`'s `TrackerService`: start,
   stop, list today's entries. It proves the layering: no GTK, no change
   to the library.
3. **Async.** Replace the blocking `TrackerHandle` calls in GTK callbacks
   with `glib::spawn_future_local` and an async channel, so the UI never
   waits on SQLite at all.
4. **Property tests for storage.** A proptest that generates random
   non-overlapping intervals, inserts them in random order, and checks
   `list_entries` windows — the same idea as Chapter 6, one layer down.
5. **Domain docs.** Write a `CONTEXT.md` for the study project with the
   glossary from `00-outline.md` and an ADR for the actor decision
   (`docs/adr/0001-single-storage-thread.md`): what was decided, why, and
   what it rules out. Future changes get judged against it.

## 21.5 Keeping the habit

The loop that produced this project — type a small piece, build, read the
error, fix, test, commit — is the same loop for anything you write in Rust
next. Keep the lint settings from Chapter 1 in every new project; they are
most of what "good Rust" means in practice. And keep `cargo clippy -- -D
warnings` as the checkpoint: a warning-free build is a habit, not a goal.

Done. Tick the last box in `00-outline.md`.
