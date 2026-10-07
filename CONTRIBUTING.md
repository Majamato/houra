# Contributing to Houra

Houra is a GNOME time tracker written in Rust with GTK 4 and libadwaita. This guide
covers reporting bugs, building from source, and finding your way around the code.
The terms used in code and documentation (project, activity, time entry, tracked
interval, active timer) are defined in [CONTEXT.md](CONTEXT.md).

## Reporting bugs

Open an issue at <https://github.com/Majamato/houra/issues> and include:

- your distribution, GNOME and Houra versions (`cat /etc/os-release`,
  `gnome-shell --version` and `houra --version`), and whether you use Wayland or X11;
- how you installed Houra: COPR, the Ubuntu PPA, the `.deb` from GitHub, the AUR,
  the install script (for your user or with `--system`), or a source build. If you
  are unsure, `which -a houra` shows where it is installed;
- what you did, what you expected, and what happened instead;
- the logs. Quit Houra (Ctrl+Q), start it from a terminal with `RUST_LOG=info houra`,
  reproduce the problem, and copy what it prints. Also include the lines from
  `journalctl --user -o cat -b` around the time of the problem; top-bar problems
  appear there as GNOME Shell messages.

Houra is used daily only on Fedora 44 with GNOME 50 on Wayland. CI installs every
package on the other supported distributions, but without a GNOME session, so reports
from them and from GNOME 49 and 51 are especially useful. Say which parts work and
which don't.

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
data/                   UI templates, CSS, icons, GNOME metadata, and screenshots
po/                     Translation catalogues and source-file list
scripts/                Build, release and packaging commands, and install.sh for users
build-aux/              Helpers called by the Meson build and installer
packaging/fedora/       RPM package definition
packaging/debian/       Debian and Ubuntu package definition (copied to debian/)
packaging/arch/         AUR houra-bin package template
.copr/                  Source RPM build for Fedora COPR
.github/workflows/      CI, package builds and tests, and release automation
shell-extension/        GNOME Shell extension that shows the active timer in the top bar
docs/                   Release checklist
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
- `pages/tracker.rs` handles timer controls and project/activity selection.
- `pages/entries.rs` presents the selected day's time entries.
- `pages/projects.rs` presents project and activity management.
- `pages/reports.rs` presents weekly reports and the CSV export chooser.
- `dialogs/` contains time entry editing, name prompts, preferences, idle and
  recovery decisions, backup/restore, and quit confirmation.
- `platform.rs` connects GNOME idle/session events and systemd sleep events.
- `top_bar/` publishes the active timer on D-Bus for the top-bar extension and
  keeps that extension enabled.

The page and dialog modules contain focused `impl MainWindow` blocks. Rust allows
one type's methods to live in several modules. These modules share the existing
window state; they are not independent widget classes. Methods needed by other
desktop modules have visibility restricted to the desktop module.

SQLite code lives in `crates/app/src/storage/`. `mod.rs` defines `Store` and opens
the connection. Private modules group migrations, project/activity queries, entry
queries, snapshots, and backup restoration. Callers still use `Store` without
needing to know which file implements each method. `storage/backup.rs` handles
database operations; the application's `backup.rs` handles the backup document.

Use a file for a coherent subject and a directory when that subject needs several
files. Related small types can share a file, as `Project` and `Activity` do in
`core/src/work.rs`. There is no one-type-per-file requirement. Keep module names
in `snake_case`, and prefer names that describe the feature or responsibility.

When adding or moving UI resources, update `data/io.github.majamato.Houra.gresource.xml`
and the resource inputs in `crates/app/build.rs`. Update `po/POTFILES.in` when
adding or moving files that contain translatable UI text.

## Build and run

Use a recent stable Rust toolchain. The desktop build also needs a C compiler,
`pkg-config`, GTK 4, libadwaita, and GLib development tools. The build scripts
check dependencies and print missing Fedora packages.

Houra has two identities. The stable app (`io.github.majamato.Houra`) is what
users install. The development variant (`io.github.majamato.Houra.Devel`,
shown as Houra Dev) runs alongside it with a separate database, separate
preferences, its own launcher and autostart entry, and its own top-bar
extension. The identities are defined in `data/app-variants.json` and selected
by the explicit `dev-app` Cargo feature; debug or release optimization never
decides which database the app uses.

```sh
# Build development artifacts without installing or starting the app.
./scripts/build-dev.sh

# Build, install dev desktop assets, and run Houra Dev.
./scripts/build-and-run.sh dev

# Build and run an optimized development variant.
./scripts/build-and-run.sh dev --release

# Install existing development artifacts without starting the app.
./scripts/install-dev.sh

# Remove Houra Dev's desktop assets (--purge-data also removes its database).
./scripts/uninstall-dev.sh

# Explicitly build and run the production variant.
./scripts/build-and-run.sh release
```

Warning: `build-and-run.sh release` runs the production identity, so it is
not isolated from an installed Houra. It uses the production database,
settings, autostart entry and bus name; it rewrites the production autostart
entry to point at the local build; and a branch with a newer database schema
would lock the packaged app out of its database. The script refuses to run
while a packaged Houra launcher exists; pass `release --replace-packaged` to
override it, or use `dev` for isolated development.

Houra Dev is single-instance: if it is already running, `build-and-run.sh dev`
only raises the old window and the rebuilt binary exits, so quit Houra Dev first.

`build-dev.sh` is build-only: it compiles with `native-ui,dev-app` into the
isolated `target/dev` directory and prepares the dev settings schema, launcher,
icons, and extension beside the binary. Debug builds land at
`target/dev/debug/houra`, optimized dev builds (`build-dev.sh --release`) at
`target/dev/release/houra`. `build-and-run.sh dev` additionally installs only
the dev launcher, icons, and extension, then starts the matching dev binary.

The equivalent direct Cargo builds are:

```sh
cargo build --workspace --locked --features native-ui,dev-app
cargo build --workspace --locked --features native-ui
```

Build with `native-ui,dev-app`: plain `native-ui` builds are the production
app. They use the production database, settings and autostart entry, and if
the packaged Houra is running they only raise its window and exit. The
`native-ui` feature enables the desktop modules. Without it, the application
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
The release script also installs or updates your local app launcher and icons.
Each changed icon gets a new filename so the desktop can replace its cached image.
The script also refreshes the icon in an existing local top-bar extension. Log out
and back in to make GNOME Shell reload that icon.
Quit and reopen Houra after building to run the new binary. To refresh just the
launcher and icons from an existing release build, run:

```sh
./scripts/install-desktop.py
```

Set `HOURA_INSTALL_DESKTOP=0` when building only for staging or packaging.
When the variable is unset and a packaged Houra launcher exists, the script
skips the local launcher install; set `HOURA_INSTALL_DESKTOP=1` to install it
anyway.
The local launcher takes precedence over an installed package's launcher. Remove
`~/.local/share/applications/io.github.majamato.Houra.desktop` when switching to
the packaged app, or the equivalent file under `$XDG_DATA_HOME`.

Staging prepares an installation tree; it does not install into the running
desktop session. A Cargo-only stable build uses the settings schema if it is
already installed. To exercise preferences during development, install the
schema or point `GSETTINGS_SCHEMA_DIR` at a directory containing its compiled
version. Development builds prepare and find their own schema automatically.

The production database is `$XDG_DATA_HOME/houra/houra.sqlite3`, normally
`~/.local/share/houra/houra.sqlite3`. The development database is
`$XDG_DATA_HOME/houra-dev/houra.sqlite3`. Houra Dev never copies, migrates, or
opens the production database; for realistic data, export a JSON backup from
the production app and import it explicitly into Houra Dev.

## Top bar

While Houra runs, its GNOME Shell extension (`shell-extension/`) shows Houra in the
top bar: the active timer with a pause/resume button, or just Houra's icon when
nothing is tracked. Clicking outside the button brings the Houra window to the front.
The extension talks to the `io.github.majamato.Houra.ActiveTimer` interface that the
app exports on its own bus name. The contract lives in
`data/dbus/io.github.majamato.Houra.ActiveTimer.xml`, and
`shell-extension/activeTimer.js` must embed an identical copy; a test enforces it.

Meson installs the extension to `$datadir/gnome-shell/extensions/houra@majamato.github.io`.
Houra adds it to GNOME Shell's `enabled-extensions` setting when it starts. The
development extension has its own UUID, `houra-dev@majamato.github.io`, its own
bus target, GObject type, CSS classes, and translation domain, so both
extensions run side by side. For a development copy in your home directory:

```sh
./scripts/install-shell-extension.sh
```

This installs the prepared dev extension by default; pass `release` to install
the stable sources instead. `./scripts/build-and-run.sh dev` installs the dev
extension automatically.

A copy in your home directory takes precedence over the one a package installs, so
remove `~/.local/share/gnome-shell/extensions/houra@majamato.github.io` before
testing an installed package.

On Wayland, GNOME Shell loads new or changed extension code only at login, so log
out and back in after installing or changing it. Read the Shell's log with
`journalctl --user -f -o cat /usr/bin/gnome-shell`. For faster iteration, a nested
Shell (`dbus-run-session gnome-shell --devkit --wayland`) needs the `mutter-devkit`
package at the same version as `mutter`. With Dash to Panel, the element appears in
its right section.

## Translations

Houra includes English, Spanish, Brazilian Portuguese, French, Simplified
Chinese, Japanese, German, Korean, Italian, and Russian catalogues. Translatable
text comes from the files listed in `po/POTFILES.in`: the UI templates, the Rust
sources (through `tr`, `trf`, and `trn`), the desktop entry, the metainfo, the
settings schema, and the top-bar extension.

After changing translatable text, regenerate the template and merge it into every
catalogue:

```sh
meson setup build-meson   # once
meson compile -C build-meson houra-pot houra-update-po
```

Then translate every new or fuzzy entry, remove the `#, fuzzy` flags, and drop
obsolete `#~` entries (`msgattrib --no-obsolete -o po/xx.po po/xx.po`).
`python3 scripts/check-translations.py` fails on fuzzy, untranslated, or missing
entries and on mismatched `{placeholder}` names.

To add a language, add its code to `po/LINGUAS` and create its catalogue with
`msginit -i po/houra.pot -o po/xx.po -l xx`.

## Checks

```sh
cargo fmt --all -- --check
cargo test --workspace --all-targets --locked
cargo test --workspace --all-targets --locked --features dev-app
cargo test --workspace --all-targets --locked --features native-ui
cargo test --workspace --all-targets --locked --features native-ui,dev-app
cargo test -p houra --lib --release --locked --features dev-app
cargo clippy --workspace --all-targets --locked --features native-ui -- -D warnings
cargo clippy --workspace --all-targets --locked --features native-ui,dev-app -- -D warnings
python3 scripts/check-translations.py
python3 -B -m unittest discover -s scripts/tests
gjs -m shell-extension/tests/format.test.js
dbus-run-session -- gjs -m shell-extension/tests/activeTimer.test.js
dev_artifacts=$(mktemp -d)
python3 -B scripts/prepare-dev.py --output-dir "$dev_artifacts"
dbus-run-session -- gjs -m shell-extension/tests/activeTimer.test.js "$dev_artifacts/shell-extension"
gjs -m shell-extension/tests/identity.test.js "$dev_artifacts/shell-extension"
shellcheck scripts/*.sh build-aux/*.sh
```

CI runs these on Fedora 44 for every push and pull request. It also builds every
package and installs it on each supported distribution in containers; see
[Packages](#packages).

The desktop-enabled tests also compile the GTK modules, but do not automate GUI
interaction. For UI changes, exercise the affected pages and dialogs in a GNOME
session as well.

To check the installable metadata, build with Meson and validate the generated files:

```sh
meson compile -C build-meson
appstreamcli validate --no-net build-meson/io.github.majamato.Houra.metainfo.xml
desktop-file-validate build-meson/io.github.majamato.Houra.desktop
```

Tests follow the [Rust Book's organization guidance](https://doc.rust-lang.org/book/ch11-03-test-organization.html).
Focused tests live beside their implementation in `#[cfg(test)] mod tests`.
Core integration tests cover timer workflows in `tests/transition.rs` and
cross-module properties in `tests/properties.rs`. Application integration tests
are split into `storage.rs`, `backup.rs`, `export.rs`, and `tracker_service.rs`.
They use public APIs; reusable fixtures live in each crate's `tests/common/mod.rs`.

Run a focused suite with:

```sh
cargo test -p houra-core --lib
cargo test -p houra-core --test transition
cargo test -p houra-core --test properties
cargo test -p houra --test storage
cargo test -p houra --test backup
cargo test -p houra --test export
cargo test -p houra --test tracker_service
cargo test -p houra --lib
cargo test --workspace --doc --locked
cargo test --workspace --doc --locked --features native-ui
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Storage and filesystem tests use temporary directories or in-memory SQLite.
Autostart tests pass a temporary launcher path and never change login settings.
Timer tests use `ManualClock`; concurrent service clients synchronize with a
barrier and join their threads. Calendar tests launch child test processes with
`TZ=UTC` and `TZ=America/New_York`, leaving the parent environment unchanged.

## Packages

Every package builds from the two release tarballs that
`scripts/make-release-tarballs.sh` creates: the source and the vendored crates, so no
build needs network access.

| Package | Built by | Built on |
|---|---|---|
| Fedora RPM (COPR) | `.copr/Makefile` and `packaging/fedora/houra.spec` | COPR |
| `houra-X.Y.Z-x86_64-linux.tar.xz`, used by `install.sh` and the AUR | `scripts/make-binary-tarball.sh` | Debian 13 with Rust 1.88, the oldest supported base (glibc 2.41) |
| `.deb` for Debian | `scripts/make-deb.sh --binary` | Debian testing |
| Ubuntu PPA source packages | `scripts/make-deb.sh --series CODENAME` | Launchpad |
| AUR `houra-bin` | `scripts/make-aur-package.sh` | Arch Linux |

`.github/workflows/packages.yml` builds all of them and tests each one in containers:
`scripts/smoke-test-install.sh` runs `install.sh` on Fedora 43–rawhide, Ubuntu 26.04
and 26.10, Debian testing and sid, Arch and openSUSE Tumbleweed, and the `.deb` and
AUR packages are installed where they belong. To run the same check locally, build
the tarballs and run, for example:

```sh
podman run --rm -v "$PWD/scripts:/scripts:z,ro" -v "$PWD/dist:/dist:z,ro" \
    docker.io/library/archlinux:latest bash -c \
    'pacman -Syu --noconfirm gtk4 libadwaita desktop-file-utils xz && /scripts/smoke-test-install.sh /dist'
```

`houra --version` prints the version without opening a window or using D-Bus. The
installer and the package tests use it to check that the binary's libraries resolve.

## Releasing

[docs/RELEASING.md](docs/RELEASING.md) is the release checklist: version bump, tag,
the release workflow, the PPA upload and the COPR build.
