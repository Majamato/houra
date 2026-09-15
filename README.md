# Houra

Houra is a GNOME time tracker written in Rust with GTK 4 and libadwaita.
It tracks time by project and task, handles idle time and interrupted timers,
and exports CSV reports and JSON backups. Data is stored locally in SQLite.

## Project layout

```text
crates/
  core/                 Domain types, timer engine, and report calculations
    src/
    tests/              Domain integration and property tests
  app/                  Desktop application and application services
    src/
      main.rs           Executable entry point and data path
      lib.rs            Module declarations and public exports
      tracker_service.rs  Background worker and its client handle
      storage/          SQLite connection, queries, and migrations
      desktop/          GTK application, pages, and dialogs
      backup.rs         JSON backup format, validation, and file I/O
      export.rs         CSV generation and file I/O
      settings.rs       Preference values and defaults
      autostart.rs      Login autostart file management
      error.rs          Application errors
    tests/              Storage integration tests
data/                   UI templates, CSS, icons, and GNOME metadata
po/                     Translation catalogues and source-file list
scripts/                Developer-facing build commands
build-aux/              Helpers called by the Meson build and installer
packaging/fedora/       RPM package definition
```

The workspace contains two crates. `houra-core` has no GTK or SQLite dependency.
The `houra` application crate depends on it and supplies storage and the desktop
interface. Each crate's `Cargo.toml` declares its dependencies; the root manifest
contains shared versions and workspace settings.

`target/`, `build-meson/`, `build-release/`, and `stage/` are generated output
directories ignored by Git.

## Finding the right file

Desktop code lives in `crates/app/src/desktop/`:

- `application.rs` starts the application, loads resources and settings, and
  registers application actions and shortcuts.
- `window.rs` defines the main window, connects its controls, and coordinates
  refreshes. Its GTK template is `data/ui/window.ui`.
- `pages/tracker.rs` handles timer controls and project/task selection.
- `pages/entries.rs` presents the selected day's entries.
- `pages/projects.rs` presents project and task management.
- `pages/reports.rs` presents weekly reports and the CSV export chooser.
- `dialogs/` contains entry editing, name prompts, preferences, idle and recovery
  decisions, backup/restore, and quit confirmation.
- `platform.rs` connects GNOME idle/session events and systemd sleep events.

The page and dialog modules contain focused `impl MainWindow` blocks. Rust allows
one type's methods to live in several modules. These modules share the existing
window state; they are not independent widget classes. Methods needed by other
desktop modules have visibility restricted to the desktop module.

SQLite code lives in `crates/app/src/storage/`. `mod.rs` defines `Store` and opens
the connection. Private modules group migrations, project/task queries, entry
queries, snapshots, and backup restoration. Callers still use `Store` without
needing to know which file implements each method. `storage/backup.rs` handles
database operations; the application's `backup.rs` handles the backup document.

Use a file for a coherent subject and a directory when that subject needs several
files. Related small types can share a file, as `Project` and `Task` do in
`core/src/work.rs`. There is no one-type-per-file requirement. Keep module names
in `snake_case`, and prefer names that describe the feature or responsibility.

When adding or moving UI resources, update `data/io.github.majamato.Houra.gresource.xml`
and the resource inputs in `crates/app/build.rs`. Update `po/POTFILES.in` when
adding or moving files that contain translatable UI text.

## Build and run

Use a recent stable Rust toolchain. The desktop build also needs a C compiler,
`pkg-config`, GTK 4, libadwaita, and GLib development tools. The build scripts
check dependencies and print missing Fedora packages.

For an incremental development build:

```sh
./scripts/build-dev.sh
./target/debug/houra
```

The equivalent Cargo build is:

```sh
cargo build --workspace --locked --features native-ui
```

The `native-ui` feature enables the desktop modules. Without it, the application
library and its tests build without GTK, and the executable prints a message
explaining how to enable the UI.

Cargo builds Rust code and embeds the UI resources. Meson also prepares the
desktop entry, settings schema, icons, and translations for installation:

```sh
./scripts/build-release.sh
DESTDIR="$PWD/stage" meson install -C build-release
```

The release binary is `build-release/houra`. Set `HOURA_BUILD_DIR` to choose
another build directory, and use that directory in the install command too.
The older `WORK_TIME_BUILD_DIR` variable remains supported as a fallback.
Staging prepares an installation tree; it does not install into the running
desktop session. A Cargo-only build uses the settings schema if it is already
installed. To exercise preferences during development, install the schema or
point `GSETTINGS_SCHEMA_DIR` at a directory containing its compiled version.

The database is `$XDG_DATA_HOME/houra/tracker.sqlite3`, normally
`~/.local/share/houra/tracker.sqlite3`.

## Checks

```sh
cargo fmt --all -- --check
cargo test --workspace --all-targets --locked
cargo test --workspace --all-targets --locked --features native-ui
cargo clippy --workspace --all-targets --locked --features native-ui -- -D warnings
```

The desktop-enabled tests also compile the GTK modules, but do not automate GUI
interaction. For UI changes, exercise the affected pages and dialogs in a GNOME
session as well.
