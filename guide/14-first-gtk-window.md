# Chapter 14 — First GTK window

**Goal.** The application opens a libadwaita window with a timer label and a
Start/Stop button wired to the storage thread. Along the way: a Cargo build
script that compiles a GResource, a UI file, and a GObject subclass in Rust.
Files: `crates/app/build.rs`, `data/io.github.majamato.WorkTimeTracker.gresource.xml`,
`data/ui/window.ui`, `data/style.css`, `crates/app/src/native/mod.rs`,
`crates/app/src/native/window.rs`, `lib.rs`.

**You will learn**

- Build scripts (`build.rs`), `OUT_DIR`, `include_bytes!`, and GResources.
- The GLib main loop, `GApplication`, signals and `connect_*`.
- GObject subclassing in gtk-rs: `mod imp`, `glib::wrapper!`, `ObjectSubclass`,
  `CompositeTemplate`, `TemplateChild`.
- `Rc`/`RefCell` for single-threaded shared state, and `glib::clone!` with
  weak references.
- Why widgets are not `Send`, and what that means for the actor.

**Prerequisite.** Chapter 13 checkpoint passed; `pkg-config --modversion
gtk4 libadwaita-1` reports at least 4.12 and 1.5.

---

## 14.1 The build script and the resource bundle

```rust
// crates/app/build.rs
//! Bundles `data/ui/window.ui` and `data/style.css` into a GResource file in
//! `OUT_DIR` when the `native-ui` feature is enabled.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../../data/io.github.majamato.WorkTimeTracker.gresource.xml");
    println!("cargo:rerun-if-changed=../../data/ui/window.ui");
    if env::var_os("CARGO_FEATURE_NATIVE_UI").is_none() {
        return;
    }
    let out_dir = env::var_os("OUT_DIR").map(PathBuf::from);
    let Some(out_dir) = out_dir else {
        panic!("Cargo did not provide OUT_DIR");
    };
    let status = Command::new("glib-compile-resources")
        .arg("--target")
        .arg(out_dir.join("work-time-tracker.gresource"))
        .arg("--sourcedir")
        .arg("../../data")
        .arg("../../data/io.github.majamato.WorkTimeTracker.gresource.xml")
        .status();
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => panic!("glib-compile-resources exited with {status}"),
        Err(error) => panic!("could not start glib-compile-resources: {error}"),
    }
}
```

```xml
<!-- data/io.github.majamato.WorkTimeTracker.gresource.xml -->
<?xml version="1.0" encoding="UTF-8"?>
<gresources>
  <gresource prefix="/io/github/majamato/WorkTimeTracker">
    <file preprocess="xml-stripblanks">ui/window.ui</file>
    <file>style.css</file>
  </gresource>
</gresources>
```

```css
/* data/style.css */
/* Equal-width digits stop the timer label from shifting each second. */
.timer {
  font-variant-numeric: tabular-nums;
}
```

**What.** A *build script* is a Rust program Cargo compiles and runs before
compiling the crate. This one calls `glib-compile-resources`, which packs
the UI file and the CSS into one binary blob under Cargo's output directory.
The application then embeds that blob (14.3), so the installed binary has
no loose UI files to find at run time.

**Rust — build scripts.** A file named `build.rs` in the package root is
picked up automatically. It talks to Cargo by printing directives:
`cargo:rerun-if-changed=path` tells Cargo to rerun the script only when
that file changes. Cargo passes feature flags as environment variables
(`CARGO_FEATURE_NATIVE_UI`), so the script can skip the GLib tooling for
headless builds. `OUT_DIR` is the per-crate scratch directory Cargo
provides.

**Rust — `Command`.** `std::process::Command` builds and runs a child
process; `.status()` waits and returns its exit status. A build script
*may* panic — a missing tool is a build error, not an application error —
and the `match` with a guard (`Ok(status) if status.success()`) separates
"ran and succeeded", "ran and failed", and "could not start".

**Linux — GResource.** GLib's answer to "where are my assets?": files are
compiled into the executable and addressed by a URI-like path,
`/io/github/majamato/WorkTimeTracker/ui/window.ui`. `xml-stripblanks`
removes whitespace from the XML at compile time.

**GTK — CSS.** GTK styles widgets with a CSS dialect. `.timer` is a style
class; `tabular-nums` gives every digit the same width so `00:00:09` →
`00:00:10` does not shift the text. (The original bundles this file but
never loads it; 14.3 loads it — a small, deliberate improvement.)

## 14.2 The UI file

```xml
<!-- data/ui/window.ui -->
<?xml version="1.0" encoding="UTF-8"?>
<interface>
  <requires lib="gtk" version="4.12"/>
  <requires lib="Adw" version="1.5"/>
  <template class="WorkTimeTrackerWindow" parent="AdwApplicationWindow">
    <property name="title" translatable="yes">Work Time Tracker</property>
    <property name="default-width">720</property>
    <property name="default-height">680</property>
    <property name="width-request">360</property>
    <child>
      <object class="AdwToolbarView">
        <child type="top">
          <object class="AdwHeaderBar"/>
        </child>
        <property name="content">
          <object class="AdwClamp">
            <property name="maximum-size">620</property>
            <property name="margin-top">24</property>
            <property name="margin-bottom">24</property>
            <property name="margin-start">18</property>
            <property name="margin-end">18</property>
            <property name="child">
              <object class="GtkBox">
                <property name="orientation">vertical</property>
                <property name="spacing">18</property>
                <child>
                  <object class="GtkLabel" id="timer_label">
                    <property name="label">00:00:00</property>
                    <style><class name="timer"/><class name="title-1"/></style>
                  </object>
                </child>
                <child>
                  <object class="GtkButton" id="start_button">
                    <property name="label" translatable="yes">Start</property>
                    <property name="height-request">44</property>
                    <style><class name="suggested-action"/><class name="pill"/></style>
                  </object>
                </child>
              </object>
            </property>
          </object>
        </property>
      </object>
    </child>
  </template>
</interface>
```

**What.** A declarative widget tree, read by GtkBuilder. This chapter's
version is the skeleton: a header bar, a clamp that keeps content at a
readable width, a vertical box with the timer label and one button. Chapter
15 grows it into the full Tracker page.

**GTK — templates.** `<template class="WorkTimeTrackerWindow"
parent="AdwApplicationWindow">` declares a *composite widget*: a new class
whose instances are built from this XML. The class name must equal the Rust
subclass's `NAME` (14.4), and every `id` you want to touch from Rust must
match a `#[template_child]` field — a contract checked at run time
(Exercise 1). `translatable="yes"` marks strings for gettext (Chapter 20).
`<style><class name="title-1"/></style>` applies libadwaita's typography
classes; `pill` and `suggested-action` are its button styles.

**GTK — libadwaita.** GNOME's widget library on top of GTK 4: header bars,
clamps, dialogs, view switchers, plus the platform's look, dark mode and
responsive behaviour. `Adw*` classes come from it; `Gtk*` from GTK.

## 14.3 The application

```rust
// crates/app/src/native/mod.rs
//! GTK application lifecycle and GNOME integrations.

mod window;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gio::prelude::*;
use gtk::prelude::*;
use libadwaita as adw;

use crate::{APP_ID, AppError, TrackerService};
use window::MainWindow;

/// Runs the GTK application until the main loop exits, then stops the
/// storage thread.
pub fn run(database_path: PathBuf) -> Result<(), AppError> {
    register_resources()?;
    let service = TrackerService::start(database_path)?;
    let handle = service.handle.clone();
    let application = adw::Application::builder().application_id(APP_ID).build();
    let main_window: Rc<RefCell<Option<MainWindow>>> = Rc::new(RefCell::new(None));

    application.connect_startup(|_| load_css());

    let activate_window = Rc::clone(&main_window);
    application.connect_activate(move |application| {
        if activate_window.borrow().is_none() {
            let window = MainWindow::new(application, handle.clone());
            activate_window.replace(Some(window));
        }
        if let Some(window) = activate_window.borrow().as_ref() {
            window.present();
        }
    });

    let _status = application.run();
    service.shutdown()
}

fn register_resources() -> Result<(), AppError> {
    let bytes = glib::Bytes::from_static(include_bytes!(concat!(
        env!("OUT_DIR"),
        "/work-time-tracker.gresource"
    )));
    let resource = gio::Resource::from_data(&bytes).map_err(|error| {
        AppError::InvalidBackup(format!("could not load application resources: {error}"))
    })?;
    gio::resources_register(&resource);
    Ok(())
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_resource("/io/github/majamato/WorkTimeTracker/style.css");
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
```

And in `lib.rs`:

```rust
// crates/app/src/lib.rs
// ...
pub mod storage;

#[cfg(feature = "native-ui")]
pub mod native;
// ...
```

**What.** Register the embedded resources, start the storage thread, create
the application object, connect two signals, run the main loop, and — when
the loop ends — shut the storage thread down. `main.rs`'s feature-gated `run`
from Chapter 8 calls this; `cargo run --features native-ui` now compiles.

**GTK — the main loop.** `application.run()` does not return until the
application quits. Everything after that is *event-driven*: GTK calls your
closures when a signal fires (a button click, a timer, a D-Bus message). All
of it happens on this one thread. The storage thread is the only other
thread, and the only way to reach it is the handle — a channel round trip
that takes microseconds.

**GTK — signals.** `connect_startup` runs once when the app starts;
`connect_activate` runs each time it is *activated* — launched, or launched
again while running (GApplication is single-instance per `APP_ID`: a second
launch just re-activates the first process). That is why the window is
created lazily and stored: the second activation must present the existing
window, not build another.

**Rust — `Rc<RefCell<Option<MainWindow>>>`.** Read inside-out: an optional
window, in a cell that checks borrows at run time, shared by reference
count. `Rc` is `Arc`'s single-threaded sibling (cheaper, not `Send`);
`RefCell` is `Mutex`'s (no locking, panics on a conflicting borrow instead
of blocking). They fit GTK exactly: one thread, callbacks that need shared
mutable access. `Rc::clone(&main_window)` makes another handle to the same
cell for the closure to own (`move`). `.borrow()` gives a read guard;
`.replace(Some(window))` swaps the content.

**Rust — `include_bytes!` + `env!` + `concat!`.** All three run at compile
time: `env!("OUT_DIR")` reads Cargo's variable, `concat!` joins the path,
`include_bytes!` embeds the file as a `&'static [u8]`. The resource is inside
the executable; `glib::Bytes::from_static` wraps it without copying.

**Rust — `use libadwaita as adw;`.** Renames a crate on import; the short
alias is the gtk-rs convention. The `prelude::*` imports bring the trait
methods (`connect_activate`, `present`, …) into scope — without them, the
methods do not exist on the types.

**GTK — CSS provider.** `load_css` reads the CSS from the resource and
attaches it to the display at application priority. It runs in `startup`,
when a display exists.

## 14.4 The window subclass

```rust
// crates/app/src/native/window.rs
use std::cell::RefCell;

use glib::subclass::InitializingObject;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use libadwaita::subclass::prelude::*;
use work_time_core::{ProjectId, TrackerCommand, TrackerState};

use crate::TrackerHandle;

mod imp {
    use super::*;

    /// Private state of the window: template children plus Rust fields.
    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/majamato/WorkTimeTracker/ui/window.ui")]
    pub struct MainWindow {
        #[template_child]
        pub timer_label: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub start_button: gtk::TemplateChild<gtk::Button>,
        pub handle: RefCell<Option<TrackerHandle>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MainWindow {
        const NAME: &'static str = "WorkTimeTrackerWindow";
        type Type = super::MainWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(class: &mut Self::Class) {
            class.bind_template();
        }

        fn instance_init(object: &InitializingObject<Self>) {
            object.init_template();
        }
    }

    impl ObjectImpl for MainWindow {}
    impl WidgetImpl for MainWindow {}
    impl WindowImpl for MainWindow {}
    impl ApplicationWindowImpl for MainWindow {}
    impl AdwApplicationWindowImpl for MainWindow {}
}

glib::wrapper! {
    pub struct MainWindow(ObjectSubclass<imp::MainWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}
```

**What.** This is how a GTK class is written in Rust. It has two halves:

- `imp::MainWindow` — the *implementation struct*. Its fields are the state:
  typed handles to the widgets declared in the XML (`TemplateChild<…>`), and
  ordinary Rust data (`handle`).
- `MainWindow` (from `glib::wrapper!`) — the *public object*. It is a
  reference-counted GObject; cloning it bumps a refcount. All methods you
  call from outside live on this type, and it reaches the implementation
  with `.imp()`.

**GTK — GObject.** GTK is written in C with its own object system
(classes, inheritance, properties, signals, refcounting). gtk-rs maps that
onto Rust: `@extends` lists the parent classes (a `MainWindow` *is a*
`Widget`, `Window`, …) and `@implements` the interfaces; that is what makes
`window.present()` — a `Window` method — available. The `ObjectSubclass`
impl gives the class its C-level name and parent, and the five empty
`impl XxxImpl for MainWindow {}` blocks say "no overrides" for each level
of the hierarchy.

**GTK — templates in two steps.** `class_init` runs once per class and
binds the XML template to it; `instance_init` runs per window and
instantiates the XML children, filling every `TemplateChild`. Forget either
and GTK fails at run time (Exercise 2). `#[derive(CompositeTemplate)]` with
`#[template(resource = ...)]` generates the binding code from the resource
path — this is where the gresource prefix and file name meet.

**Rust — `mod imp { use super::*; }`.** An inline module inside the file;
`super::*` imports the parent module's names into it. It exists to keep the
implementation struct's name separate from the public wrapper's.

**Rust — `RefCell<Option<TrackerHandle>>`.** GObject methods receive
`&self`, so any field that changes after construction needs interior
mutability. `Default` is derived (required by `CompositeTemplate`), so the
handle starts as `None` and is set in `new`.

**Dart.** A GObject is closer to a Flutter *controller* than to a widget
value: it has identity, is shared by reference, and outlives the frame.

## 14.5 Behaviour

```rust
// crates/app/src/native/window.rs
// ...

impl MainWindow {
    pub fn new(application: &adw::Application, handle: TrackerHandle) -> Self {
        let window: Self = glib::Object::builder()
            .property("application", application)
            .build();
        window.imp().handle.replace(Some(handle));
        window.setup();
        window
    }

    fn setup(&self) {
        self.refresh_timer_only();
        self.imp().start_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.toggle_timer()
        ));
        let weak = self.downgrade();
        glib::timeout_add_seconds_local(1, move || {
            let Some(window) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            window.refresh_timer_only();
            glib::ControlFlow::Continue
        });
    }

    fn handle(&self) -> Option<TrackerHandle> {
        self.imp().handle.borrow().clone()
    }

    pub fn toggle_timer(&self) {
        let Some(handle) = self.handle() else { return };
        let state = match handle.snapshot() {
            Ok(snapshot) => snapshot.state,
            Err(error) => {
                self.show_database_error(&error.to_string());
                return;
            }
        };
        let command = match state {
            TrackerState::Stopped => TrackerCommand::Start {
                project_id: ProjectId(1),
                task_id: None,
                note: String::new(),
            },
            TrackerState::Running(_) => TrackerCommand::Stop,
            TrackerState::IdlePending(_) | TrackerState::RecoveryPending(_) => return,
        };
        match handle.apply(command) {
            Ok(_) => self.refresh_timer_only(),
            Err(error) => self.show_database_error(&error.to_string()),
        }
    }

    fn refresh_timer_only(&self) {
        let Some(handle) = self.handle() else { return };
        let Ok(snapshot) = handle.snapshot() else {
            return;
        };
        match snapshot.state {
            TrackerState::Running(active)
            | TrackerState::IdlePending(work_time_core::PendingIdle { active, .. }) => {
                let elapsed = handle.live_elapsed().map_or_else(
                    |_| {
                        u64::try_from(
                            chrono::Utc::now()
                                .timestamp_millis()
                                .saturating_sub(active.start_ms)
                                .max(0)
                                / 1_000,
                        )
                        .unwrap_or(0)
                    },
                    |duration| duration.as_secs(),
                );
                self.imp().timer_label.set_label(&format!(
                    "{:02}:{:02}:{:02}",
                    elapsed / 3600,
                    (elapsed / 60) % 60,
                    elapsed % 60
                ));
                self.imp().start_button.set_label("Stop");
                self.imp().start_button.add_css_class("destructive-action");
            }
            TrackerState::RecoveryPending(_) => {
                self.imp().timer_label.set_label("Recovery needed");
                self.imp().start_button.set_label("Review");
            }
            TrackerState::Stopped => {
                self.imp().timer_label.set_label("00:00:00");
                self.imp().start_button.set_label("Start");
                self.imp()
                    .start_button
                    .remove_css_class("destructive-action");
                self.imp().start_button.add_css_class("suggested-action");
            }
        }
    }

    fn show_database_error(&self, message: &str) {
        let dialog = adw::AlertDialog::builder()
            .heading("Could not save the change")
            .body(message)
            .build();
        dialog.add_response("close", "Close");
        dialog.present(Some(self));
    }
}
```

**What.** `new` constructs the GObject with its `application` property,
stores the handle, and calls `setup`, which renders once, connects the
button, and starts a one-second refresh. `toggle_timer` reads the *actual*
state from the actor and sends `Start` (for project General, until Chapter
15 adds the selectors) or `Stop`; the pending states are left for Chapter
17's dialogs. `refresh_timer_only` renders the label and button from the
state — never from the button's current text.

**Why render from state.** The button's meaning is derived from the
snapshot every time. If the timer was started from a keyboard shortcut, a
D-Bus event, or another process activation, the next refresh shows the
truth. There is no local "is running" flag to drift.

**GTK — `glib::Object::builder()`.** GObjects are constructed through
*properties*; `application` is one, and setting it registers the window
with the app. The `let window: Self` annotation tells the builder which
class to build.

**GTK — `connect_clicked` and `glib::clone!`.** The closure will be held by
the button for as long as the button lives — and the button lives inside
the window. If the closure captured the window *strongly*, you would have
window → button → closure → window: a reference cycle that never frees.
`glib::clone!` with `#[weak(rename_to = window)] self` captures a *weak*
reference; when the closure runs it upgrades it, and if the window is gone
the closure simply does nothing. Compare Exercise 4.

**GTK — timeouts.** `glib::timeout_add_seconds_local(1, closure)` runs the
closure on the main loop every second until it returns
`ControlFlow::Break`. `_local` means it may capture non-`Send` values
(widgets) because it runs on this thread. The explicit `downgrade()` /
`upgrade()` does by hand what `clone!` does with attributes.

**GTK — `map_or_else` fallback.** If the actor cannot answer
`live_elapsed` (it should always be able to), the wall clock is used so the
display still moves. Belt and braces, bounded to zero.

**GTK — `adw::AlertDialog`.** libadwaita's modal message box; `present(Some(self))`
attaches it to this window. Errors from the actor become a dialog and
nothing else — the clone-before-commit design (Chapter 13) means the state
on screen is still the true state.

## 14.6 Checkpoint

```sh
cargo build --features native-ui
XDG_DATA_HOME=/tmp/wtt-study cargo run --features native-ui
```

A window titled "Work Time Tracker" opens with `00:00:00` and a Start
button. Click Start: the label counts up and the button turns red and says
Stop. Click Stop: back to `00:00:00`. Close the window: the process exits
(close-to-hide comes in Chapter 17). No `Gtk-CRITICAL` lines on the
terminal.

The `XDG_DATA_HOME` override keeps this study build's database away from
the real app's `~/.local/share/work-time-tracker/`. Use it for every manual
run until the end (or set it once in your shell for the study folder).

```sh
cargo test --workspace                                   # still 19 (no display needed)
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
git add -A && git commit -m "Chapter 14: first GTK window"
```

`build.rs` and the gresource XML already match the original. `window.ui`,
`mod.rs` and `window.rs` are partial and grow over Chapters 15–19.

## 14.7 Exercises

1. **The template contract.** In `window.ui` change `id="timer_label"` to
   `id="timer"`, rebuild, run.

   <details><summary>Answer</summary>

   ```
   (work-time-tracker:184677): Gtk-CRITICAL **: Unable to retrieve child object 'timer_label' from class template for type 'WorkTimeTrackerWindow' while building a 'WorkTimeTrackerWindow'

   thread 'main' panicked at .../gtk4-0.10.3/src/subclass/widget.rs:1273:17:
   Failed to retrieve template child. Please check that all fields of type `GtkLabel` have been bound and have a #[template_child] attribute.
   ```

   It compiles — the XML is data — and fails the moment the window is
   built. XML ids and `#[template_child]` field names are a run-time
   contract, one of the few in this project the compiler cannot check.
   Revert.
   </details>

2. **Both init steps matter.** Replace `instance_init` with an empty body
   (`fn instance_init(_object: &InitializingObject<Self>) {}`), rebuild,
   run.

   <details><summary>Answer</summary>

   Same panic: `Failed to retrieve template child`. `class_init` bound the
   template to the class, but no instance ever *built* its children.
   Revert.
   </details>

3. **Widgets stay on the main thread.** Append to `window.rs`:

   ```rust
   #[cfg(test)]
   mod threads {
       #[test]
       fn widgets_cannot_cross_threads() {
           let label = gtk::Label::new(None);
           std::thread::spawn(move || label.set_label("hi"));
       }
   }
   ```

   Run `cargo test --features native-ui -p work-time-tracker --lib`.

   <details><summary>Answer</summary>

   ```
   error[E0277]: `*mut c_void` cannot be sent between threads safely
       |
   171 |         std::thread::spawn(move || label.set_label("hi"));
       |         ------------------ ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `*mut c_void` cannot be sent between threads safely
       = help: the trait `Send` is not implemented for `*mut c_void`
   note: required because it appears within the type `gtk4::Label`
   ```

   A GTK widget wraps a raw C pointer that is not `Send`. Moving one to a
   thread is a compile error — the same `Send` boundary that lets
   `TrackerHandle` cross, and the reason the actor exchanges plain domain
   values and never widgets. Revert.
   </details>

4. **Weak versus strong.** Change `#[weak(rename_to = window)]` to
   `#[strong(rename_to = window)]` in the button handler and build.

   <details><summary>Answer</summary>

   It compiles and even works — but the window now owns a button that owns
   a closure that owns the window. Closing the window no longer frees it
   (a leak you cannot see in a short run). The rule in gtk-rs code:
   capture `self`/widgets weakly in signal handlers; capture data (handles,
   IDs, strings) strongly. Revert.
   </details>

5. **Inspector.** Run with `GTK_DEBUG=interactive` and explore the widget
   tree, CSS classes and the accessibility tab. No changes to make.

## Recap

- `build.rs` compiles the GResource; `include_bytes!` embeds it; the UI
  file is addressed by resource path.
- `adw::Application` owns the main loop; `startup`/`activate` are signals;
  a second launch re-activates the single instance.
- A GTK class in Rust is an `imp` struct plus a `glib::wrapper!` object;
  templates bind XML ids to `TemplateChild` fields at run time.
- `Rc`/`RefCell` share state on the one GTK thread; signal handlers capture
  the window weakly.
- The UI renders from the actor's snapshot; widgets never cross threads.

Next: **Chapter 15 — Tracker page**, project and task selectors, notes,
the day's entries, and the heartbeat.
