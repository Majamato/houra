# Chapter 18 — Settings and background

**Goal.** Preferences stored in GSettings with a schema, a preferences
dialog, first-run autostart, the `--background` start, and an application
hold so the process outlives its window. Files:
`data/io.github.majamato.Houra.gschema.xml`,
`crates/app/src/native/mod.rs`, `crates/app/src/native/window.rs`,
`data/ui/window.ui`.

**You will learn**

- GSettings: schemas, keys with types and ranges, `gio::Settings`, and
  running without an installed schema.
- Command-line options on `GApplication`; `hold()` and single instances.
- The turbofish for `None::<&T>`; `is_none_or` / `is_some_and`.
- `glib::idle_add_local_once` to get back to the main loop.
- libadwaita preferences widgets that write settings on change.

**Prerequisite.** Chapter 17 checkpoint passed.

---

## 18.1 The schema

```xml
<!-- data/io.github.majamato.Houra.gschema.xml -->
<?xml version="1.0" encoding="UTF-8"?>
<schemalist gettext-domain="houra">
  <schema id="io.github.majamato.Houra" path="/io/github/majamato/Houra/">
    <key name="idle-threshold-minutes" type="u">
      <default>5</default>
      <range min="1" max="120"/>
      <summary>Idle threshold in minutes</summary>
    </key>
    <key name="launch-at-login" type="b">
      <default>true</default>
      <summary>Launch in the background when signing in</summary>
    </key>
    <key name="notifications" type="b">
      <default>true</default>
      <summary>Show reconciliation notifications</summary>
    </key>
    <key name="week-starts-monday" type="b">
      <default>true</default>
      <summary>Use Monday as the first day of reports</summary>
    </key>
    <key name="onboarding-complete" type="b">
      <default>false</default>
      <summary>Whether first-run setup has completed</summary>
    </key>
  </schema>
</schemalist>
```

**What.** The five preferences, typed (`u` = unsigned 32-bit, `b` =
boolean, GVariant type codes), with defaults and one range. The defaults
match `Preferences::default()` from Chapter 12.

**Linux — GSettings.** GNOME's preference store. Applications never write
config files; they read and write *keys* under a schema, and the `dconf`
service persists them. Schemas are installed system-wide as XML and
compiled into a binary cache (`glib-compile-schemas`), which is why a
`cargo run` build cannot find one unless you compile it yourself
(18.6). The `path` is where the values live in dconf; the `id` is looked
up by the app.

## 18.2 Loading settings and first run

Add to `native/mod.rs` (after `load_css`):

```rust
// crates/app/src/native/mod.rs
use tracing::{error, warn};
// ...

/// The GSettings object, or `None` when the schema is not installed (dev runs).
pub(crate) fn load_settings() -> Option<gio::Settings> {
    gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup(crate::APP_ID, true))
        .map(|schema| gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None))
}

fn complete_first_run(settings: &Option<gio::Settings>) {
    let Some(settings) = settings else { return };
    if settings.boolean("onboarding-complete") {
        return;
    }
    let launch_enabled = match std::env::current_exe() {
        Ok(executable) => {
            if let Err(error) = crate::autostart::set_enabled(true, &executable) {
                warn!(%error, "could not enable first-run autostart");
                false
            } else {
                true
            }
        }
        Err(error) => {
            warn!(%error, "could not locate executable for first-run autostart");
            false
        }
    };
    let _ignored = settings.set_boolean("launch-at-login", launch_enabled);
    let _ignored = settings.set_boolean("onboarding-complete", true);
}
```

**What.** `load_settings` looks the schema up *first* and returns `None`
if it is missing; `complete_first_run` enables autostart once (Chapter
12's `set_enabled`) and records that onboarding happened.

**Why look up before constructing.** `gio::Settings::new(id)` *aborts the
process* if the schema is not installed — GLib treats that as a programmer
error. Looking up through `SettingsSchemaSource` turns it into an `Option`,
so development builds run with defaults and every caller handles `None`.

**Rust — the turbofish on `None`.** `new_full(&schema, None::<&gio::SettingsBackend>, None)`:
the second parameter is generic over "anything that is a settings
backend"; a bare `None` gives inference nothing to work with (Exercise 1).

**Rust — `match` producing a value with logging in the arms.**
`launch_enabled` is `true` only if the file was written; both failure
paths log with `warn!` and evaluate to `false`. `%error` formats with
`Display`.

## 18.3 Options, hold, settings in `run`

Update `run` in `native/mod.rs`:

```rust
// crates/app/src/native/mod.rs
use std::cell::{Cell, RefCell};
// ...

pub fn run(database_path: PathBuf) -> Result<(), AppError> {
    register_resources()?;
    let service = TrackerService::start(database_path)?;
    let handle = service.handle.clone();
    let settings = load_settings();
    complete_first_run(&settings);
    let background = std::env::args().any(|argument| argument == "--background");
    let application = adw::Application::builder().application_id(APP_ID).build();
    application.add_main_option(
        "background",
        glib::Char(0),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        "Start the tracker without presenting its window",
        None,
    );
    let _application_hold = application.hold();
    let main_window: Rc<RefCell<Option<MainWindow>>> = Rc::new(RefCell::new(None));
    let suppress_first_present = Rc::new(Cell::new(background));

    application.connect_startup(|application| {
        load_css();
        application.set_accels_for_action("app.toggle-timer", &["<Control>space"]);
        application.set_accels_for_action("app.add-entry", &["<Control>n"]);
        application.set_accels_for_action("app.preferences", &["<Control>comma"]);
        application.set_accels_for_action("app.quit", &["<Control>q"]);
    });

    let activate_window = Rc::clone(&main_window);
    let activate_handle = handle.clone();
    let activate_suppression = Rc::clone(&suppress_first_present);
    application.connect_activate(move |application| {
        if activate_window.borrow().is_none() {
            let window = MainWindow::new(application, activate_handle.clone());
            window.connect_close_request(|window| {
                window.set_visible(false);
                glib::Propagation::Stop
            });
            activate_window.replace(Some(window));
        }
        let suppress_present = activate_suppression.replace(false);
        if !suppress_present && let Some(window) = activate_window.borrow().as_ref() {
            window.present();
        }
    });

    install_actions(&application, &main_window, handle, &settings);
    let _status = application.run();
    service.shutdown()
}
```

**What.** Settings load before the window exists; `--background` is
declared as a known option and detected; a *hold* keeps the app alive with
no visible window; the first activation is not presented when started in
the background — later activations (a second launch, the launcher) do
present it.

**GTK — `add_main_option`.** GApplication parses the command line. An
undeclared `--background` would be rejected ("Unknown option"); declaring
it makes it legal, and the code reads it from `std::env::args()` because
the flag only matters in this process.

**GTK — `hold()`.** GApplication quits when its last window closes *and*
nothing holds it. `hold()` returns a guard; the app runs until the guard
drops (end of `run`) or `quit()` is called. Combined with close-to-hide
(Chapter 17), the tracker keeps running after the window closes, which is
what a timer needs.

**Rust — `Rc<Cell<bool>>` and `replace`.** `activate_suppression.replace(false)`
returns the old value and stores `false`: a one-shot flag shared between
`run` and the closure. `Cell` because `bool` is `Copy`.

**Rust — `let` chains with `!`.** `if !suppress_present && let Some(window)
= ... { window.present() }` combines a boolean and a pattern.

**Rust — `std::env::args().any(..)`.** Iterates the arguments lazily and
stops at the first match.

## 18.4 The preferences action and the integration hook

In `install_actions`, add a `settings` parameter and the `preferences`
action (after `add`, before `quit`), and register it:

```rust
// crates/app/src/native/mod.rs
fn install_actions(
    application: &adw::Application,
    window: &Rc<RefCell<Option<MainWindow>>>,
    handle: crate::TrackerHandle,
    settings: &Option<gio::Settings>,
) {
    // ... toggle, add ...
    let preferences = gio::ActionEntry::builder("preferences")
        .activate({
            let window = Rc::clone(window);
            move |_: &adw::Application, _, _| {
                if let Some(window) = window.borrow().as_ref() {
                    window.show_preferences();
                }
            }
        })
        .build();
    // ... quit, backup, restore ...
    application.add_action_entries([toggle, add, preferences, backup, restore, quit]);

    let idle_threshold = settings
        .as_ref()
        .map_or(5, |settings| settings.uint("idle-threshold-minutes"))
        .clamp(1, 120);
    let notifications = settings
        .as_ref()
        .is_none_or(|settings| settings.boolean("notifications"));
    if let Err(error) = platform::start_integrations(handle, idle_threshold, notifications) {
        warn!(%error, "GNOME idle/session integration is unavailable; manual tracking remains active");
        let message = error.to_string();
        let window = Rc::clone(window);
        glib::idle_add_local_once(move || {
            if let Some(window) = window.borrow().as_ref() {
                window.show_integration_warning(&message);
            }
        });
    }
}
```

The last block calls `platform::start_integrations` and
`show_integration_warning`, which do not exist until Chapter 19. **For
this chapter, leave that block out** (everything from `let idle_threshold`
to the closing brace of the `if`), and add `mod platform;` only in Chapter
19. The checkpoint below reflects that.

Also add the menu item:

```xml
<!-- data/ui/window.ui (first section of main_menu, after Add Manual Entry) -->
      <item><attribute name="label" translatable="yes">Preferences</attribute><attribute name="action">app.preferences</attribute></item>
```

**Rust — `is_none_or`.** `settings.as_ref().is_none_or(|s| s.boolean(..))`
reads: "true if there are no settings, otherwise whatever the key says" —
the default for a missing schema in one expression. `map_or(5, ..)` is the
same idea for the threshold. Exercise 2 pins the semantics.

**Rust — `glib::idle_add_local_once`.** Runs a closure once, on the main
loop, as soon as it is idle. Used here because `install_actions` runs
before the window exists; the warning banner must wait for it.

## 18.5 Week start from settings, and the preferences dialog

In `window.rs`, `report_bounds` now reads the preference:

```rust
// crates/app/src/native/window.rs (inside report_bounds)
        let today = Local::now().date_naive();
        let starts_monday = crate::native::load_settings()
            .is_none_or(|settings| settings.boolean("week-starts-monday"));
```

And the dialog, after `show_edit_entry`:

```rust
// crates/app/src/native/window.rs (inside impl MainWindow)

    pub fn show_preferences(&self) {
        let dialog = adw::PreferencesDialog::new();
        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::builder()
            .title("Idle Detection")
            .build();
        let threshold = adw::SpinRow::with_range(1.0, 120.0, 1.0);
        threshold.set_title("Idle threshold (minutes)");
        threshold.set_subtitle("Changes take effect the next time the app starts");
        let settings = crate::native::load_settings();
        let current_threshold = settings
            .as_ref()
            .map_or(5, |settings| settings.uint("idle-threshold-minutes"));
        threshold.set_value(f64::from(current_threshold));
        threshold.connect_value_notify({
            let settings = settings.clone();
            move |row| {
                if let Some(settings) = &settings {
                    let value = row.value().round().clamp(1.0, 120.0) as u32;
                    let _ignored = settings.set_uint("idle-threshold-minutes", value);
                }
            }
        });
        group.add(&threshold);
        let launch = adw::SwitchRow::builder()
            .title("Launch at login")
            .active(
                settings
                    .as_ref()
                    .is_some_and(|settings| settings.boolean("launch-at-login")),
            )
            .build();
        launch.connect_active_notify({
            let settings = settings.clone();
            move |row| {
                if let Some(settings) = &settings {
                    let _ignored = settings.set_boolean("launch-at-login", row.is_active());
                }
                match std::env::current_exe() {
                    Ok(executable) => {
                        if let Err(error) =
                            crate::autostart::set_enabled(row.is_active(), &executable)
                        {
                            log_background_error("updating autostart", error);
                        }
                    }
                    Err(error) => log_background_error("locating executable", error),
                }
            }
        });
        group.add(&launch);
        let notifications = adw::SwitchRow::builder()
            .title("Notifications")
            .active(
                settings
                    .as_ref()
                    .is_none_or(|settings| settings.boolean("notifications")),
            )
            .build();
        notifications.connect_active_notify({
            let settings = settings.clone();
            move |row| {
                if let Some(settings) = &settings {
                    let _ignored = settings.set_boolean("notifications", row.is_active());
                }
            }
        });
        group.add(&notifications);
        let week_start = adw::SwitchRow::builder()
            .title("Weeks start on Monday")
            .active(
                settings
                    .as_ref()
                    .is_none_or(|settings| settings.boolean("week-starts-monday")),
            )
            .build();
        week_start.connect_active_notify({
            let settings = settings.clone();
            let weak = self.downgrade();
            move |row| {
                if let Some(settings) = &settings {
                    let _ignored = settings.set_boolean("week-starts-monday", row.is_active());
                }
                if let Some(window) = weak.upgrade() {
                    window.refresh_report();
                }
            }
        });
        group.add(&week_start);
        page.add(&group);
        dialog.add(&page);
        dialog.present(Some(self));
    }
```

**What.** A libadwaita preferences dialog with four rows. Each row writes
its key on change; there is no Apply button. Toggling "Launch at login"
also writes or removes the autostart file immediately.

**GTK — preferences widgets.** `PreferencesDialog` → `PreferencesPage` →
`PreferencesGroup` → rows. `SpinRow::with_range(min, max, step)` and
`SwitchRow` map directly to a `u` key with a range and `b` keys.
`connect_value_notify` / `connect_active_notify` are property-notify
signals again.

**Rust — `as u32`.** The one place the code uses an `as` cast: from `f64`
to `u32` after `round()` and `clamp(1.0, 120.0)`, so the cast cannot
truncate anything meaningful. `f64::from(u32)` in the other direction is a
lossless `From`.

**Rust — `settings.clone()` per closure.** `gio::Settings` is a GObject:
cloning bumps a refcount. Each handler gets its own handle in a block
expression, the same shape as `install_actions`.

## 18.6 Checkpoint

The schema is not installed on your system (Chapter 20 installs it), so
compile it into a directory and point GLib at it:

```sh
mkdir -p /tmp/houra-schemas
cp data/io.github.majamato.Houra.gschema.xml /tmp/houra-schemas/
glib-compile-schemas --strict /tmp/houra-schemas
cargo build --features native-ui
GSETTINGS_SCHEMA_DIR=/tmp/houra-schemas GSETTINGS_BACKEND=memory XDG_DATA_HOME=/tmp/houra-study \
  XDG_CONFIG_HOME=/tmp/houra-config cargo run --features native-ui
```

`GSETTINGS_BACKEND=memory` keeps this study build's preferences out of your
real dconf database (where the installed original stores *its*
preferences under the same schema id). `XDG_CONFIG_HOME` keeps the
autostart file away from `~/.config/autostart`.

Check: Ctrl+, opens Preferences; change the threshold and switches (they
persist for this process only, because of the memory backend). The first
run wrote `/tmp/houra-config/autostart/io.github.majamato.Houra.desktop`
— open it; it is the file from Chapter 12 with your binary's path. Run with
`--background`: no window appears, the process stays alive; run again
without the flag from another terminal: the window appears. Without
`GSETTINGS_SCHEMA_DIR`, everything still works with defaults and no
autostart file is written.

```sh
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
diff <(grep -v '^\s*//' ../work_time_tracker/crates/app/src/native/mod.rs) \
     <(grep -v '^\s*//' crates/app/src/native/mod.rs)
```

Differences left: `mod platform;`, the integration block at the end of
`install_actions` (Chapter 19), and `load_css` (this guide's addition).

```sh
git add -A && git commit -m "Chapter 18: settings and background"
```

## 18.7 Exercises

1. **Why the turbofish.** Change `None::<&gio::SettingsBackend>` to `None`
   and build.

   <details><summary>Answer</summary>

   ```
   error[E0283]: type annotations needed
       |
   176 |         .map(|schema| gio::Settings::new_full(&schema, None, None))
       |                                                        ^^^^ cannot infer type of the type parameter `T` declared on the enum `Option`
       = note: the type must implement `IsA<SettingsBackend>`
   ```

   The parameter is `Option<&impl IsA<SettingsBackend>>`; `None` alone
   does not say which `T`. Any type would do (`None::<&gio::SettingsBackend>`
   is just the obvious one). Revert.
   </details>

2. **`is_none_or` and `is_some_and` (temporary test).** Append to
   `mod.rs`:

   ```rust
   #[cfg(test)]
   mod option_predicates {
       #[test]
       fn defaults_when_settings_are_missing() {
           let missing: Option<bool> = None;
           let present = Some(false);
           assert!(missing.is_none_or(|value| value));
           assert!(!present.is_none_or(|value| value));
           assert!(!missing.is_some_and(|value| value));
           assert!(Some(true).is_some_and(|value| value));
       }
   }
   ```

   Run `cargo test --features native-ui -p houra --lib option_predicates`.

   <details><summary>Answer</summary>

   Passes. `is_none_or` defaults to *true* when absent (notifications on,
   Monday weeks); `is_some_and` defaults to *false* (the "Launch at login"
   switch shows off when no schema exists, because nothing was written).
   Choose the predicate by the default you want. Revert.
   </details>

3. **The real store.** After Chapter 20 installs the schema, run the
   installed app once and then:

   ```sh
   gsettings list-recursively io.github.majamato.Houra
   dconf dump /io/github/majamato/Houra/
   ```

   Nothing to do now — remember to come back.

## Recap

- Preferences are GSettings keys declared in a schema; the app looks the
  schema up and degrades to defaults if it is missing.
- `--background` is a declared option; `hold()` keeps the process alive
  without a window; close hides, quit ends.
- First run enables autostart and records onboarding.
- `is_none_or` / `is_some_and` encode "what if there are no settings".
- Preferences rows write keys on change; the week-start switch refreshes
  the report immediately.

Next: **Chapter 19 — D-Bus**, where GNOME tells the app that the user is
idle, the screen is locked, or the machine is about to sleep.
