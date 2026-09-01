# Chapter 20 — Packaging

**Goal.** Everything a Linux desktop needs to install and show the app:
a desktop launcher, AppStream metadata, icons, the GSettings schema
installed for real, translations, a Meson build that drives Cargo, helper
scripts, and a Fedora RPM spec. The chapter ends with a staged install
that passes the standard validators. Files: `meson.build`,
`meson_options.txt`, `build-aux/*`, `po/*`, `data/*.in`, `data/icons/**`,
`scripts/*`, `packaging/fedora/*.spec`, `LICENSE`.

**You will learn**

- What each freedesktop/GNOME file is for, and how the app id ties them together.
- Cargo versus Meson: who owns what.
- `DESTDIR` staged installs, and the validators packagers run.
- gettext: `POTFILES.in`, `.pot`, `.po`, `.mo`.
- Offline builds with `cargo vendor` for RPM.

**Prerequisite.** Chapter 19 checkpoint passed; `meson`, `ninja`,
`gettext`, `desktop-file-utils`, `libappstream-glib` (or `appstream`)
installed.

---

## 20.1 Desktop file and AppStream metadata

```ini
# data/io.github.majamato.WorkTimeTracker.desktop.in
[Desktop Entry]
Name=Work Time Tracker
Comment=Track focused work locally
Exec=work-time-tracker
Icon=io.github.majamato.WorkTimeTracker
Terminal=false
Type=Application
Categories=Office;GTK;GNOME;
Keywords=time;work;project;task;report;
StartupNotify=true
```

```xml
<!-- data/io.github.majamato.WorkTimeTracker.metainfo.xml.in -->
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>io.github.majamato.WorkTimeTracker</id>
  <metadata_license>CC0-1.0</metadata_license>
  <project_license>GPL-3.0-or-later</project_license>
  <name>Work Time Tracker</name>
  <developer id="io.github.majamato"><name>majamato</name></developer>
  <summary>Private, local-first tracking for focused work</summary>
  <description>
    <p>Track projects and tasks, reconcile idle time, review weekly totals, and
    keep portable backups without creating an online account.</p>
  </description>
  <launchable type="desktop-id">io.github.majamato.WorkTimeTracker.desktop</launchable>
  <provides><binary>work-time-tracker</binary></provides>
  <url type="homepage">https://github.com/majamato/work-time-tracker</url>
  <content_rating type="oars-1.1"/>
  <releases>
    <release version="0.1.0" date="2026-08-27">
      <description><p>Initial Fedora and GNOME release.</p></description>
    </release>
  </releases>
</component>
```

Copy the two icons from the original (they are artwork, not code):

```sh
mkdir -p data/icons/hicolor/scalable/apps data/icons/hicolor/symbolic/apps
cp ../work_time_tracker/data/icons/hicolor/scalable/apps/io.github.majamato.WorkTimeTracker.svg data/icons/hicolor/scalable/apps/
cp ../work_time_tracker/data/icons/hicolor/symbolic/apps/io.github.majamato.WorkTimeTracker-symbolic.svg data/icons/hicolor/symbolic/apps/
cp ../work_time_tracker/LICENSE .
```

**What.** The *desktop entry* is what the app grid and search show and
launch. The *AppStream metainfo* is what software centres show
(description, licence, releases). Both are `.in` files: Meson merges
translations into them at build time (20.4).

**Linux — the id everywhere.** `io.github.majamato.WorkTimeTracker` is the
GApplication id (Chapter 14), the desktop file name, the icon name, the
schema id, the notification source, and the AppStream `<id>`. GNOME
matches a running window to its launcher and icon by that one string;
`launchable` ties metainfo to the desktop file. Icons go to
`hicolor/scalable/apps/<id>.svg` and the symbolic variant to
`hicolor/symbolic/apps/<id>-symbolic.svg`, and are referenced by name only.

**Linux — categories and keywords.** `Categories` decides the menu
section; `Keywords` feed search. `StartupNotify` lets the shell show a
"starting" cursor until the window maps.

## 20.2 Meson

```meson
# meson.build
project(
  'work-time-tracker',
  'rust',
  version: '0.1.0',
  license: 'GPL-3.0-or-later',
  meson_version: '>= 1.3.0',
)

gnome = import('gnome')
i18n = import('i18n')

app_id = 'io.github.majamato.WorkTimeTracker'

cargo = find_program('cargo')
glib_compile_schemas = find_program('glib-compile-schemas')

gnome.compile_resources(
  'work-time-tracker-resources',
  'data/' + app_id + '.gresource.xml',
  source_dir: 'data',
  gresource_bundle: true,
  build_by_default: true,
)

cargo_args = ['--locked', '--features', 'native-ui']
if get_option('offline')
  cargo_args += '--offline'
endif

rust_binary = custom_target(
  'work-time-tracker-rust',
  input: ['Cargo.toml', 'Cargo.lock'],
  output: 'work-time-tracker',
  command: [
    find_program('bash'),
    files('build-aux/cargo-build.sh'),
    meson.project_source_root(),
    meson.project_build_root(),
    '@OUTPUT@',
  ] + cargo_args,
  build_by_default: true,
  build_always_stale: true,
  install: true,
  install_dir: get_option('bindir'),
)

desktop = i18n.merge_file(
  input: 'data/' + app_id + '.desktop.in',
  output: app_id + '.desktop',
  po_dir: 'po',
  type: 'desktop',
  install: true,
  install_dir: get_option('datadir') / 'applications',
)

metainfo = i18n.merge_file(
  input: 'data/' + app_id + '.metainfo.xml.in',
  output: app_id + '.metainfo.xml',
  po_dir: 'po',
  type: 'xml',
  install: true,
  install_dir: get_option('datadir') / 'metainfo',
)

install_data(
  'data/' + app_id + '.gschema.xml',
  install_dir: get_option('datadir') / 'glib-2.0/schemas',
)

install_data(
  'data/icons/hicolor/scalable/apps/' + app_id + '.svg',
  install_dir: get_option('datadir') / 'icons/hicolor/scalable/apps',
)
install_data(
  'data/icons/hicolor/symbolic/apps/' + app_id + '-symbolic.svg',
  install_dir: get_option('datadir') / 'icons/hicolor/symbolic/apps',
)

install_data('LICENSE', install_dir: get_option('datadir') / 'licenses/work-time-tracker')

subdir('po')

meson.add_install_script('build-aux/post-install.py')
```

```meson
# meson_options.txt
option('cargo-home', type: 'string', value: '', description: 'Optional CARGO_HOME containing vendored sources')
option('offline', type: 'boolean', value: false, description: 'Pass --offline to Cargo')
```

```bash
#!/usr/bin/bash
# build-aux/cargo-build.sh
set -euo pipefail

source_root=$1
build_root=$2
output=$3
shift 3

export CARGO_TARGET_DIR="$build_root/cargo-target"
cargo build --manifest-path "$source_root/Cargo.toml" --release "$@"
cp "$CARGO_TARGET_DIR/release/work-time-tracker" "$output"
```

```python
#!/usr/bin/python3
# build-aux/post-install.py
"""Refresh caches for a direct install; packagers run their own scriptlets."""

import os
import subprocess


def run_if_native_root(command: list[str]) -> None:
    if not os.environ.get("DESTDIR"):
        subprocess.run(command, check=False)


prefix = os.environ.get("MESON_INSTALL_PREFIX", "/usr/local")
run_if_native_root(["glib-compile-schemas", os.path.join(prefix, "share/glib-2.0/schemas")])
run_if_native_root(["gtk4-update-icon-cache", "-qtf", os.path.join(prefix, "share/icons/hicolor")])
```

Make the scripts executable: `chmod +x build-aux/cargo-build.sh build-aux/post-install.py`.

**What.** Meson is the *installer*, not the compiler: it asks Cargo to
build the binary (through the shell script), compiles the resource bundle
independently (a check that the XML is valid), merges translations into
the two `.in` files, and installs everything into the GNU directory
layout (`bindir`, `datadir`) that every distribution understands.

**Why two build systems.** Cargo knows Rust and crates; it does not know
where GNOME expects schemas, icons or desktop files, nor how to merge
translations. Meson knows all of that and is what GNOME Builder,
Flatpak and RPM expect to find. Each does its own job;
`build-aux/cargo-build.sh` is the seam.

**Meson — pieces.** `custom_target` runs an arbitrary command and
declares its output; `build_always_stale` makes Meson always ask Cargo
(which then decides for itself whether anything changed). `i18n.merge_file`
is the translation merge. `install_data` copies files. `get_option('datadir')
/ 'applications'` joins paths. `subdir('po')` includes the translations'
own `meson.build`. The post-install script refreshes the schema and icon
caches after a *direct* install (`meson install` to `/usr/local`), but
not under `DESTDIR`, where a package's own scriptlets do it.

**Linux — `--locked`.** Meson passes `--locked` so Cargo refuses to change
`Cargo.lock`: a packaged build must use exactly the audited versions.
`--offline` (the `offline` option) is for the RPM build in 20.6.

## 20.3 Translations

```meson
# po/meson.build
i18n.gettext(
  'work-time-tracker',
  preset: 'glib',
  args: ['--from-code=UTF-8'],
)
```

```text
# po/POTFILES.in
data/io.github.majamato.WorkTimeTracker.desktop.in
data/io.github.majamato.WorkTimeTracker.metainfo.xml.in
data/io.github.majamato.WorkTimeTracker.gschema.xml
data/ui/window.ui
crates/app/src/native/window.rs
```

```text
# po/LINGUAS
en
```

The `.pot` template and the English `.po` are generated, not typed:

```sh
meson setup build-meson --buildtype=release -Doffline=false
meson compile -C build-meson work-time-tracker-pot
cd po && msginit --no-translator --locale=en_US.UTF-8 --input=work-time-tracker.pot --output=en.po && cd ..
```

**What.** gettext scans the files in `POTFILES.in` for translatable
strings (`translatable="yes"` in XML, `<summary>` in the schema, string
literals in `window.rs`), writes them to the `.pot` template, and each
language keeps a `.po` with translations that Meson compiles to `.mo` at
install time. The `glib` preset teaches `xgettext` about GTK's XML.

**Linux — why an English `.po`.** `LINGUAS` lists languages to install;
an identity English catalogue is the conventional starting point so the
pipeline is exercised before any real translation exists. The original
project's `en.po` has a hand-edited header; the generated one differs only
there.

## 20.4 Developer scripts

Copy the two scripts and make them executable — they are shell, with
dependency checks that only *report* missing Fedora packages:

```sh
cp ../work_time_tracker/scripts/build-dev.sh ../work_time_tracker/scripts/build-release.sh scripts/
chmod +x scripts/*.sh
```

Read `scripts/build-dev.sh`: it checks `cargo`, `rustc`, `cc`,
`pkg-config`, `glib-compile-resources` and the `pkg-config` versions of
GTK 4.12, libadwaita 1.5 and GIO 2.84, prints `sudo dnf install ...` for
anything missing, and runs `cargo build --workspace --locked --features
native-ui`. `build-release.sh` does the same for the Meson toolchain and
runs `meson setup`/`meson compile` into `build-release/`.

**Idiom.** A build script that never installs anything for you, only
tells you what to install, is polite: it keeps `sudo` in the human's
hands.

## 20.5 Checkpoint — a staged install

```sh
./scripts/build-dev.sh                       # Cargo, debug, with the feature
./scripts/build-release.sh                   # Meson + Cargo release into build-release/
DESTDIR="$PWD/stage" meson install -C build-release
find stage -type f | sed "s|$PWD/stage||" | sort
```

Expected tree:

```
/usr/local/bin/work-time-tracker
/usr/local/share/applications/io.github.majamato.WorkTimeTracker.desktop
/usr/local/share/glib-2.0/schemas/io.github.majamato.WorkTimeTracker.gschema.xml
/usr/local/share/icons/hicolor/scalable/apps/io.github.majamato.WorkTimeTracker.svg
/usr/local/share/icons/hicolor/symbolic/apps/io.github.majamato.WorkTimeTracker-symbolic.svg
/usr/local/share/licenses/work-time-tracker/LICENSE
/usr/local/share/locale/en/LC_MESSAGES/work-time-tracker.mo
/usr/local/share/metainfo/io.github.majamato.WorkTimeTracker.metainfo.xml
```

Validators:

```sh
desktop-file-validate stage/usr/local/share/applications/*.desktop
appstreamcli validate --no-net stage/usr/local/share/metainfo/*.xml
glib-compile-schemas --strict --dry-run stage/usr/local/share/glib-2.0/schemas
```

Expected: nothing from the first and third; `✔ Validation was successful`
from AppStream. Then run the staged binary with the staged schema:

```sh
glib-compile-schemas stage/usr/local/share/glib-2.0/schemas
GSETTINGS_SCHEMA_DIR=stage/usr/local/share/glib-2.0/schemas GSETTINGS_BACKEND=memory \
  XDG_DATA_HOME=/tmp/wtt-study XDG_CONFIG_HOME=/tmp/wtt-config \
  stage/usr/local/bin/work-time-tracker
```

**Linux — `DESTDIR`.** Installs into `stage/` as if it were `/`. This is
how packages are built: nothing touches the real system, and you can
inspect exactly what a user would get. A real install is
`sudo meson install -C build-release` (to `/usr/local`), after which the
launcher appears in the app grid and the schema is found without any
environment variables — Chapter 18's exercise 3 can be done then.

```sh
git add -A && git commit -m "Chapter 20: packaging"
```

Note `.gitignore` already excludes `build*/` and `stage/`.

## 20.6 The Fedora spec

Copy it as is; it is packaging metadata, and the interesting part is
reading it:

```sh
mkdir -p packaging/fedora && cp ../work_time_tracker/packaging/fedora/work-time-tracker.spec packaging/fedora/
```

```spec
# packaging/fedora/work-time-tracker.spec (excerpt)
BuildRequires:  cargo >= 1.85
BuildRequires:  meson >= 1.3
BuildRequires:  gtk4-devel >= 4.12
BuildRequires:  libadwaita-devel >= 1.5
...
%prep
%autosetup
tar -xf %{SOURCE1}
mkdir -p .cargo
printf '[source.crates-io]\nreplace-with = "vendored-sources"\n[source.vendored-sources]\ndirectory = "vendor"\n' > .cargo/config.toml

%build
%meson -Doffline=true
%meson_build

%check
cargo test --workspace --all-targets --offline
desktop-file-validate %{buildroot}%{_datadir}/applications/io.github.majamato.WorkTimeTracker.desktop
appstream-util validate-relax --nonet %{buildroot}%{_metainfodir}/io.github.majamato.WorkTimeTracker.metainfo.xml
```

**What.** An RPM build has no network. The spec expects two tarballs: the
source and a *vendor* archive of every crate, produced on a connected
machine with `cargo vendor --locked vendor` (155 crates for this
project). `.cargo/config.toml` redirects crates.io to that directory,
`%meson -Doffline=true` passes `--offline` through the option from 20.2,
and `%check` runs the tests and the same validators as your checkpoint.

**Linux — the pattern.** Source + lock file + vendored dependencies +
offline build is how every Rust desktop app reaches a distribution. Your
`Cargo.lock` is part of the contract.

## 20.7 Exercises

1. **Break the id.** Change `Icon=` in the desktop file to
   `io.github.majamato.WorkTimeTracke` (drop a letter), rebuild, install
   to `stage/`, run `desktop-file-validate`.

   <details><summary>Answer</summary>

   The validator passes — it checks syntax, not that the icon exists.
   AppStream's validator would also pass. Only a real install shows a
   generic icon in the app grid. The id is a convention enforced by eyes
   and tests, not tools; the constant `APP_ID` in `lib.rs` and the
   literal names in these files must agree. Revert.
   </details>

2. **See the translation pipeline.** In `window.ui`, change the *Tracker*
   page title to "Tracker!" and run
   `meson compile -C build-meson work-time-tracker-pot`, then
   `grep -n 'Tracker!' po/work-time-tracker.pot`.

   <details><summary>Answer</summary>

   The new string appears in the template with its file and line. A
   translator would now get it in their `.po`. Revert, and regenerate.
   </details>

3. **Vendor.** Run `cargo vendor --locked /tmp/wtt-vendor | head` and look
   at `/tmp/wtt-vendor`.

   <details><summary>Answer</summary>

   Cargo prints the `[source]` replacement config it wants you to use,
   and the directory holds a folder per crate — the same content the
   spec's `Source1` tarball carries. Delete it afterwards.
   </details>

## Recap

- One id ties launcher, icon, schema, notifications and metadata together.
- Cargo builds; Meson installs, merges translations, and speaks the
  distribution's language.
- `DESTDIR` staging plus the three validators is what a packager runs.
- gettext: sources listed in `POTFILES.in`, template `.pot`, per-language
  `.po`, compiled `.mo`.
- RPM builds are offline: vendor the crates, keep the lock file.

Next: **Chapter 21 — Wrap-up**, the final checklist and where to go from
here.
