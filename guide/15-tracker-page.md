# Chapter 15 — Tracker page

**Goal.** The Tracker page as the user sees it: project and task dropdowns,
a note field, the day's entries with previous/next navigation, a header
view switcher (with one page for now), the one-second display refresh, and
the 30-second heartbeat. Files: `data/ui/window.ui`,
`crates/app/src/native/window.rs`.

**You will learn**

- `gtk::StringList` models next to a typed `Vec` — why the string is not the identity.
- `TemplateChild` for many widgets, `Cell<i32>` for a counter.
- Rebuilding a widget subtree; `first_child()` loops.
- `gio::spawn_blocking` and the `Send` boundary between GTK and the actor.
- `matches!`, or-patterns in `match`, and local-day → UTC window math.
- `RefCell` panics at run time; `Cell` for `Copy` values.

**Prerequisite.** Chapter 14 checkpoint passed.

---

## 15.1 The UI: a view stack with one page

Replace `window.ui`:

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
          <object class="AdwHeaderBar">
            <property name="title-widget">
              <object class="AdwViewSwitcher">
                <property name="stack">view_stack</property>
                <property name="policy">wide</property>
              </object>
            </property>
          </object>
        </child>
        <property name="content">
          <object class="GtkBox">
            <property name="orientation">vertical</property>
            <child>
              <object class="AdwViewStack" id="view_stack">
                <property name="vexpand">true</property>
                <child>
                  <object class="AdwViewStackPage">
                    <property name="name">tracker</property>
                    <property name="title" translatable="yes">Tracker</property>
                    <property name="icon-name">alarm-symbolic</property>
                    <property name="child">
                      <object class="GtkScrolledWindow">
                        <property name="hscrollbar-policy">never</property>
                        <property name="child">
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
                                  <object class="GtkGrid">
                                    <property name="column-spacing">12</property>
                                    <property name="row-spacing">12</property>
                                    <child>
                                      <object class="GtkLabel"><property name="label" translatable="yes">Project</property><property name="halign">start</property><layout><property name="column">0</property><property name="row">0</property></layout></object>
                                    </child>
                                    <child>
                                      <object class="GtkDropDown" id="project_dropdown"><property name="hexpand">true</property><layout><property name="column">1</property><property name="row">0</property></layout></object>
                                    </child>
                                    <child>
                                      <object class="GtkLabel"><property name="label" translatable="yes">Task</property><property name="halign">start</property><layout><property name="column">0</property><property name="row">1</property></layout></object>
                                    </child>
                                    <child>
                                      <object class="GtkDropDown" id="task_dropdown"><property name="hexpand">true</property><layout><property name="column">1</property><property name="row">1</property></layout></object>
                                    </child>
                                    <child>
                                      <object class="GtkLabel"><property name="label" translatable="yes">Note</property><property name="halign">start</property><layout><property name="column">0</property><property name="row">2</property></layout></object>
                                    </child>
                                    <child>
                                      <object class="GtkEntry" id="note_entry"><property name="placeholder-text" translatable="yes">What are you working on?</property><layout><property name="column">1</property><property name="row">2</property></layout></object>
                                    </child>
                                  </object>
                                </child>
                                <child>
                                  <object class="GtkButton" id="start_button"><property name="label" translatable="yes">Start</property><property name="height-request">44</property><style><class name="suggested-action"/><class name="pill"/></style></object>
                                </child>
                                <child><object class="GtkSeparator"/></child>
                                <child>
                                  <object class="GtkBox">
                                    <property name="spacing">6</property>
                                    <child><object class="GtkButton" id="day_previous_button"><property name="icon-name">go-previous-symbolic</property><property name="tooltip-text" translatable="yes">Previous day</property></object></child>
                                    <child><object class="GtkLabel" id="day_label"><property name="hexpand">true</property><style><class name="title-2"/></style></object></child>
                                    <child><object class="GtkButton" id="day_next_button"><property name="icon-name">go-next-symbolic</property><property name="tooltip-text" translatable="yes">Next day</property></object></child>
                                  </object>
                                </child>
                                <child><object class="GtkBox" id="entries_box"><property name="orientation">vertical</property><property name="spacing">6</property></object></child>
                              </object>
                            </property>
                          </object>
                        </property>
                      </object>
                    </property>
                  </object>
                </child>
              </object>
            </child>
          </object>
        </property>
      </object>
    </child>
  </template>
</interface>
```

**What.** The header now holds an `AdwViewSwitcher` bound to an
`AdwViewStack`; the stack has one page, *Tracker* (Chapter 16 adds two
more). The page is a scrolled, clamped column: timer, a 3×2 grid of
labels and inputs, the Start button, a separator, day navigation, and an
empty `entries_box` that Rust fills.

**GTK — layout.** `GtkGrid` positions children with `<layout>` properties
(column/row). `hexpand` lets a child take the remaining width. `GtkBox` is
the vertical/horizontal stack. `GtkScrolledWindow` adds scrolling when the
content is taller than the window; `hscrollbar-policy: never` forbids
horizontal scrolling so the clamp does the wrapping. Icon names
(`go-previous-symbolic`) come from the icon theme.

**GTK — dynamic versus declared.** Widgets that always exist are declared
in XML; rows that depend on data (`entries_box` children) are created in
Rust. Both are ordinary GTK objects once built.

## 15.2 More template children and state

```rust
// crates/app/src/native/window.rs
use std::cell::{Cell, RefCell};

use chrono::{Local, TimeZone};
use glib::subclass::InitializingObject;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use libadwaita::subclass::prelude::*;
use work_time_core::{Project, ProjectId, Task, TrackerCommand, TrackerState};

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
        #[template_child]
        pub project_dropdown: gtk::TemplateChild<gtk::DropDown>,
        #[template_child]
        pub task_dropdown: gtk::TemplateChild<gtk::DropDown>,
        #[template_child]
        pub note_entry: gtk::TemplateChild<gtk::Entry>,
        #[template_child]
        pub entries_box: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub day_label: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub day_previous_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub day_next_button: gtk::TemplateChild<gtk::Button>,
        pub handle: RefCell<Option<TrackerHandle>>,
        pub projects: RefCell<Vec<Project>>,
        pub tasks: RefCell<Vec<Task>>,
        pub selected_day_offset: Cell<i32>,
    }

    // ... ObjectSubclass and the five Impl blocks unchanged ...
}
```

**What.** One `TemplateChild` per XML id the code touches, plus the
current lists of projects and tasks (for mapping dropdown rows back to
IDs) and which day is shown (0 = today, −1 = yesterday, …).

**Rust — `Cell<T>` vs `RefCell<T>`.** Both give interior mutability.
`Cell` is for `Copy` values: `get()` copies out, `set()` copies in, no
borrow can ever be outstanding. `RefCell` is for anything else and checks
at run time that no `borrow_mut` overlaps a `borrow` (Exercise 3). Use the
simplest one that fits: `Cell<i32>` for the offset, `RefCell<Vec<_>>` for
the lists.

## 15.3 Setup: signals and timers

Replace `setup`:

```rust
// crates/app/src/native/window.rs (inside impl MainWindow)

    fn setup(&self) {
        self.imp()
            .timer_label
            .update_property(&[gtk::accessible::Property::Label("Elapsed tracked time")]);
        self.reload_projects();
        self.reload_tasks();
        self.refresh();
        self.imp().start_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.toggle_timer()
        ));
        self.imp()
            .project_dropdown
            .connect_selected_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| {
                    window.reload_tasks();
                    window.update_active_details();
                }
            ));
        self.imp()
            .task_dropdown
            .connect_selected_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| window.update_active_details()
            ));
        self.imp().note_entry.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.update_active_details()
        ));
        self.imp().day_previous_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window
                    .imp()
                    .selected_day_offset
                    .set(window.imp().selected_day_offset.get().saturating_sub(1));
                window.refresh_entries();
            }
        ));
        self.imp().day_next_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window
                    .imp()
                    .selected_day_offset
                    .set(window.imp().selected_day_offset.get().saturating_add(1));
                window.refresh_entries();
            }
        ));
        let weak = self.downgrade();
        glib::timeout_add_seconds_local(1, move || {
            let Some(window) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            window.refresh_timer_only();
            glib::ControlFlow::Continue
        });
        let heartbeat = self.handle();
        glib::timeout_add_seconds_local(30, move || {
            if let Some(handle) = &heartbeat {
                let handle = handle.clone();
                gio::spawn_blocking(move || handle.apply(TrackerCommand::Heartbeat));
            }
            glib::ControlFlow::Continue
        });
    }
```

**What.** Initial render, then one handler per control, then two timers.
Changing the project reloads the task list (tasks belong to a project) and,
if a timer is running, edits it live; changing the task or note edits the
running timer too. The day buttons move the offset and refresh the list.

**Why a heartbeat every 30 s.** It updates `last_heartbeat_ms` on disk
(Chapter 5/9) so a crash can be recovered up to the last half-minute. It is
the only periodic write, and it goes through the actor like everything
else.

**GTK — `spawn_blocking`.** `handle.apply(...)` blocks until SQLite commits
— usually a millisecond, occasionally more. Doing that on the main loop
would freeze drawing for that long every 30 s. `gio::spawn_blocking` runs
the closure on a thread pool. The closure must therefore be `Send`: it may
capture the handle (which is `Send`) but not the window (Exercise 2). The
result is ignored: a failed heartbeat is not something the user can act on.

**GTK — `_local` timeouts.** `timeout_add_seconds_local` accepts non-`Send`
closures because it promises to run them on the main thread. The non-local
`timeout_add_seconds` requires `Send` and rejects the window (Exercise 1).

**GTK — `connect_selected_notify`.** GObject *properties* emit a `notify`
signal when they change; `selected` is a property of `DropDown`, and gtk-rs
generates one `connect_<property>_notify` method per property.

**GTK — accessibility.** `update_property(&[Property::Label(...)])` gives
the timer label a spoken name for screen readers; the digits alone say
nothing.

## 15.4 Selectors and their models

```rust
// crates/app/src/native/window.rs (inside impl MainWindow, after handle())

    fn reload_projects(&self) {
        let Some(handle) = self.handle() else { return };
        match handle.projects(false) {
            Ok(projects) => {
                let names: Vec<&str> = projects
                    .iter()
                    .map(|project| project.name.as_str())
                    .collect();
                self.imp()
                    .project_dropdown
                    .set_model(Some(&gtk::StringList::new(&names)));
                self.imp().projects.replace(projects);
            }
            Err(error) => self.show_database_error(&error.to_string()),
        }
    }

    fn selected_project_id(&self) -> ProjectId {
        let index = usize::try_from(self.imp().project_dropdown.selected()).unwrap_or(0);
        self.imp()
            .projects
            .borrow()
            .get(index)
            .map_or(ProjectId(1), |project| project.id)
    }

    fn selected_task_id(&self) -> Option<work_time_core::TaskId> {
        let selected = self.imp().task_dropdown.selected();
        if selected == 0 || selected == gtk::INVALID_LIST_POSITION {
            return None;
        }
        usize::try_from(selected.saturating_sub(1))
            .ok()
            .and_then(|index| self.imp().tasks.borrow().get(index).map(|task| task.id))
    }

    fn reload_tasks(&self) {
        let Some(handle) = self.handle() else { return };
        let project_id = self.selected_project_id();
        let tasks = handle
            .tasks(false)
            .unwrap_or_default()
            .into_iter()
            .filter(|task| task.project_id == project_id)
            .collect::<Vec<_>>();
        let mut names = vec!["No task"];
        names.extend(tasks.iter().map(|task| task.name.as_str()));
        self.imp()
            .task_dropdown
            .set_model(Some(&gtk::StringList::new(&names)));
        self.imp().tasks.replace(tasks);
    }

    fn update_active_details(&self) {
        let Some(handle) = self.handle() else { return };
        let Ok(snapshot) = handle.snapshot() else {
            return;
        };
        if matches!(snapshot.state, TrackerState::Running(_)) {
            let result = handle.apply(TrackerCommand::EditActive {
                project_id: self.selected_project_id(),
                task_id: self.selected_task_id(),
                note: self.imp().note_entry.text().to_string(),
            });
            if let Err(error) = result {
                self.show_database_error(&error.to_string());
            }
        }
    }
```

**What.** A `DropDown` displays a *model* of strings; the Rust side keeps
the parallel `Vec<Project>` so row *n* maps back to a typed `ProjectId`.
The task dropdown has an extra first row, "No task", so its index is
shifted by one. `update_active_details` sends `EditActive` only while
running — when stopped, the selectors are simply the inputs for the next
`Start`.

**Why not put the ID in the string.** A dropdown row is display text; two
projects could share a name, and parsing IDs out of labels is fragile. The
typed `Vec` is the source of truth; the `StringList` is a view.

**GTK — `StringList`.** A list model of strings (`&[&str]` in). Setting a
new model resets the selection to row 0. `selected()` returns a `u32`;
`INVALID_LIST_POSITION` means nothing is selected (empty model).

**Rust — `Vec<&str>` from `Vec<Project>`.** The names borrow from
`projects`, which is still alive while the model is built; then `projects`
is moved into the `RefCell` with `replace`. The borrow checker is happy
because `names` is not used after the move.

**Rust — `usize::try_from(u32)`.** Indexing needs `usize`; the conversion
cannot actually fail on 64-bit but the API is fallible, so `unwrap_or(0)`
or `.ok()` handle it without a panic. `.get(index)` returns `None` for an
out-of-range index instead of panicking like `[index]` would.

**Rust — `unwrap_or_default()` on a `Result<Vec<_>, _>`.** An error
listing tasks becomes an empty list; the dropdown shows only "No task". A
softer failure than a dialog for something that refreshes constantly.

## 15.5 Toggle and refresh

Replace `toggle_timer` and add `refresh`:

```rust
// crates/app/src/native/window.rs (inside impl MainWindow)

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
                project_id: self.selected_project_id(),
                task_id: self.selected_task_id(),
                note: self.imp().note_entry.text().to_string(),
            },
            TrackerState::Running(_) => TrackerCommand::Stop,
            TrackerState::IdlePending(_) | TrackerState::RecoveryPending(_) => return,
        };
        match handle.apply(command) {
            Ok(_) => self.refresh(),
            Err(error) => self.show_database_error(&error.to_string()),
        }
    }

    fn refresh(&self) {
        self.refresh_timer_only();
        self.refresh_entries();
    }
```

(`refresh_timer_only` is unchanged from Chapter 14.)

**What.** `Start` now carries the selected project, task and note. After
any accepted command, a full refresh re-renders the timer and the day's
list (a stop just produced a new entry). The pending states still do
nothing here; Chapter 17 opens dialogs for them.

**Rust — or-patterns.** `A | B => ...` matches either variant in one arm.
`TrackerState::Running(active) | TrackerState::IdlePending(PendingIdle {
active, .. })` in `refresh_timer_only` goes further: both alternatives bind
a variable named `active` of the same type, so the arm body can use it.

**Rust — `text().to_string()`.** `Entry::text()` returns a `GString`
(GLib's string type); `to_string()` makes an owned Rust `String` for the
command.

## 15.6 The day's entries

```rust
// crates/app/src/native/window.rs (inside impl MainWindow, before show_database_error)

    fn refresh_entries(&self) {
        let Some(handle) = self.handle() else { return };
        while let Some(child) = self.imp().entries_box.first_child() {
            self.imp().entries_box.remove(&child);
        }
        let Some(date) = Local::now()
            .date_naive()
            .checked_add_signed(chrono::Duration::days(i64::from(
                self.imp().selected_day_offset.get(),
            )))
        else {
            return;
        };
        let day_label = if self.imp().selected_day_offset.get() == 0 {
            "Today".to_owned()
        } else {
            date.format("%A, %x").to_string()
        };
        self.imp().day_label.set_label(&day_label);
        let Some(start) = date
            .and_hms_opt(0, 0, 0)
            .and_then(|value| Local.from_local_datetime(&value).earliest())
        else {
            return;
        };
        let Some(next_date) = date.succ_opt() else {
            return;
        };
        let Some(end) = next_date
            .and_hms_opt(0, 0, 0)
            .and_then(|value| Local.from_local_datetime(&value).earliest())
        else {
            return;
        };
        match handle.entries(start.timestamp_millis(), end.timestamp_millis()) {
            Ok(entries) if entries.is_empty() => {
                let label = gtk::Label::new(Some("No entries for today"));
                label.add_css_class("dim-label");
                self.imp().entries_box.append(&label);
            }
            Ok(entries) => {
                for entry in entries {
                    let duration = entry.duration_ms() / 1_000;
                    let row = adw::ActionRow::builder()
                        .title(if entry.note.is_empty() {
                            "Tracked work"
                        } else {
                            &entry.note
                        })
                        .subtitle(format!(
                            "{}h {:02}m {:02}s",
                            duration / 3600,
                            (duration / 60) % 60,
                            duration % 60
                        ))
                        .build();
                    self.imp().entries_box.append(&row);
                }
            }
            Err(error) => self.show_database_error(&error.to_string()),
        }
    }
```

**What.** Empty the box, compute the selected local day as a UTC
millisecond window, ask the actor for the entries touching it, and append
one `AdwActionRow` per entry (or a dim placeholder).

**Why compute the window in local time.** "Today" is a local calendar day
that starts at local midnight, which is *not* a round number in UTC and is
not always 24 hours long. The same chrono steps as Chapter 7's
`next_local_midnight`: date → midnight → attach zone → epoch millis.

**GTK — rebuilding children.** `while let Some(child) = box.first_child()
{ box.remove(&child) }` is the idiom for clearing a `GtkBox`. For a handful
of rows, rebuilding is simpler than diffing; lists with thousands of rows
would use `ListView` with a model instead.

**Rust — `Ok(entries) if entries.is_empty()`.** A match guard on a bound
value: the first arm takes empty results, the second the rest. Order of
arms matters when guards overlap.

**Rust — `i64::from(i32)`.** Widening conversions that cannot fail use
`From`; narrowing ones use `try_from`. `chrono::Duration::days` takes an
`i64`.

**GTK — `AdwActionRow`.** A libadwaita list row with title and subtitle,
built with the builder pattern (`::builder()...build()`), like every GTK
object. Chapter 17 makes the rows clickable.

## 15.7 Checkpoint

```sh
cargo build --features native-ui
XDG_DATA_HOME=/tmp/wtt-study cargo run --features native-ui
```

You should see: a header with a *Tracker* switcher; the project dropdown
showing *General*; task dropdown *No task*; the note entry; Start; *Today*
with "No entries for today". Start, wait a few seconds, Stop: an entry row
appears with its duration. Type a note while running, stop: the row's title
is the note. Use ◀ ▶ to move between days.

```sh
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo test --workspace
git add -A && git commit -m "Chapter 15: tracker page"
```

## 15.8 Exercises

1. **Only the main thread may hold widgets.** Change the one-second
   `glib::timeout_add_seconds_local(1, ...)` to `glib::timeout_add_seconds(1,
   ...)` and build.

   <details><summary>Answer</summary>

   ```
   error[E0277]: `std::cell::RefCell<std::option::Option<actor::TrackerHandle>>` cannot be shared between threads safely
       |
   141 |           glib::timeout_add_seconds(1, move || {
       |           -------------------------    ^ required by a bound introduced by this call
   ```

   The non-local variant may run on any thread, so its closure must be
   `Send`. The weak reference to the window is not, because the window's
   `imp` holds `RefCell`s (and widgets). The error names the first
   non-`Send` field it finds. Revert.
   </details>

2. **Nor may the thread pool.** Inside the heartbeat closure, capture
   `self.clone()` as `window` and call `window.refresh_timer_only()` inside
   the `spawn_blocking` closure. Build.

   <details><summary>Answer</summary>

   Same `E0277`: `spawn_blocking` requires `Send + 'static`, and a window
   is neither shareable across threads nor allowed there. The correct
   pattern is the one in the code: do the blocking work with the `Send`
   handle, and touch widgets only back on the main loop (Chapter 18 shows
   `glib::idle_add_local_once` for that). Revert.
   </details>

3. **`RefCell` checks at run time.** Append to `window.rs`:

   ```rust
   #[cfg(test)]
   mod borrows {
       use std::cell::RefCell;

       #[test]
       fn refcell_checks_at_run_time() {
           let names = RefCell::new(vec!["General".to_owned()]);
           let reading = names.borrow();
           names.replace(vec![]);
           assert_eq!(reading.len(), 1);
       }
   }
   ```

   Run `cargo test --features native-ui -p work-time-tracker --lib refcell`.

   <details><summary>Answer</summary>

   ```
   thread '...refcell_checks_at_run_time' panicked at crates/app/src/native/window.rs:387:15:
   RefCell already borrowed
   ```

   It compiles: `RefCell` moves the borrow rules from compile time to run
   time. Holding `reading` (a shared borrow) while `replace` needs an
   exclusive one panics. This is why the window code keeps borrows short —
   `self.imp().projects.borrow().get(index).map_or(...)` ends the borrow in
   the same expression — and why `handle()` clones the handle out instead
   of returning a guard. Revert.
   </details>

4. **Recovery, observed.** Run the app, Start, then close the window (the
   process exits, Chapter 17 changes that). Run it again.

   <details><summary>Answer</summary>

   The label reads *Recovery needed* and the button *Review*, and clicking
   does nothing yet. The chain: `service.shutdown()` with a running timer
   kept the marker dirty (Chapter 13) → `restore` produced
   `RecoveryPending` (Chapter 6) → `refresh_timer_only` rendered it.
   Chapter 17 adds the dialog that resolves it. To get unstuck now, delete
   `/tmp/wtt-study/work-time-tracker/`.
   </details>

## Recap

- Dropdowns show a `StringList`; a parallel `Vec` maps rows to IDs.
- Every control change becomes a command or a refresh; nothing is cached
  in widgets.
- Blocking actor calls from timers go through `spawn_blocking` with a
  `Send` handle; widgets never leave the main thread.
- The day list is rebuilt from a local-day UTC window each time.
- `Cell` for `Copy` state, `RefCell` for the rest — with short borrows.

Next: **Chapter 16 — Projects and reports**, two more pages, a generic
name dialog, and CSV export through a file dialog.
