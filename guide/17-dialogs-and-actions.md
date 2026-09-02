# Chapter 17 — Dialogs and actions

**Goal.** Every remaining dialog — manual entry, edit entry, idle
resolution (with reassignment), crash recovery, quit confirmation, backup
and restore — plus the application *actions* that trigger them from a menu
and keyboard shortcuts. Closing the window now hides it. After this chapter
`window.rs` lacks only the preferences dialog and the integration banner.
Files: `data/ui/window.ui`, `crates/app/src/native/window.rs`,
`crates/app/src/native/mod.rs`.

**You will learn**

- `GAction`/`ActionEntry`, menus in XML, and accelerators.
- Closures that own data across dialog lifetimes; `#[strong]` captures.
- Parsing local date-times with `.single()`; `bool::then_some().flatten()`.
- `let (Some(a), Some(b)) = (x, y) else { ... }`.
- `upcast`, `adw::Dialog`, response signals, destructive appearance.
- Close-to-hide and the quit path with a running timer.

**Prerequisite.** Chapter 16 checkpoint passed.

---

## 17.1 Menu and menu button

In `window.ui`, add a menu button to the header bar (after the
`title-widget` property, inside `AdwHeaderBar`) and a menu model after the
closing `</template>` tag:

```xml
<!-- data/ui/window.ui (inside AdwHeaderBar) -->
            <child type="end">
              <object class="GtkMenuButton">
                <property name="icon-name">open-menu-symbolic</property>
                <property name="tooltip-text" translatable="yes">Main Menu</property>
                <property name="menu-model">main_menu</property>
              </object>
            </child>
```

```xml
<!-- data/ui/window.ui (after </template>, before </interface>) -->
  <menu id="main_menu">
    <section>
      <item><attribute name="label" translatable="yes">Add Manual Entry</attribute><attribute name="action">app.add-entry</attribute></item>
      <item><attribute name="label" translatable="yes">Back Up Data</attribute><attribute name="action">app.backup</attribute></item>
      <item><attribute name="label" translatable="yes">Restore Data</attribute><attribute name="action">app.restore</attribute></item>
    </section>
    <section>
      <item><attribute name="label" translatable="yes">Quit</attribute><attribute name="action">app.quit</attribute></item>
    </section>
  </menu>
```

(Chapter 18 adds a *Preferences* item.)

**GTK — actions.** A menu item does not call Rust directly; it activates a
named *action* (`app.add-entry`). Actions live on the application (`app.`)
or window (`win.`) and are also what keyboard accelerators bind to. Widget
focus, menus and shortcuts all converge on one name, and Rust registers
one handler per name (17.6).

## 17.2 Manual entry

New imports at the top of `window.rs`:

```rust
// crates/app/src/native/window.rs
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, TimeZone};
// ...
use houra_core::{
    EntrySource, Project, ProjectId, Task, TimeEntry, TrackerCommand, TrackerState,
};

use crate::{TrackerHandle, native::log_background_error};
```

Add after `refresh_entries`:

```rust
// crates/app/src/native/window.rs (inside impl MainWindow)

    pub fn show_manual_entry(&self) {
        let Some(handle) = self.handle() else { return };
        let projects = self.imp().projects.borrow().clone();
        let dialog = adw::Dialog::builder()
            .title("Manual Entry")
            .content_width(480)
            .content_height(400)
            .build();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .build();
        let project_names: Vec<&str> = projects
            .iter()
            .map(|project| project.name.as_str())
            .collect();
        let project = gtk::DropDown::from_strings(&project_names);
        let note = gtk::Entry::builder()
            .placeholder_text("Optional note")
            .build();
        let end_local = Local::now();
        let start_local = end_local - chrono::Duration::hours(1);
        let start = gtk::Entry::builder()
            .text(start_local.format("%Y-%m-%d %H:%M:%S").to_string())
            .build();
        let end = gtk::Entry::builder()
            .text(end_local.format("%Y-%m-%d %H:%M:%S").to_string())
            .build();
        for (label_text, widget) in [
            ("Project", project.clone().upcast::<gtk::Widget>()),
            ("Note", note.clone().upcast()),
            ("Start (local)", start.clone().upcast()),
            ("End (local)", end.clone().upcast()),
        ] {
            let label = gtk::Label::builder()
                .label(label_text)
                .halign(gtk::Align::Start)
                .build();
            content.append(&label);
            content.append(&widget);
        }
        let save = gtk::Button::with_label("Save Entry");
        save.add_css_class("suggested-action");
        content.append(&save);
        dialog.set_child(Some(&content));
        let weak = self.downgrade();
        let dialog_for_save = dialog.clone();
        save.connect_clicked(move |_| {
            let parse_local = |text: &str| {
                NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S")
                    .ok()
                    .and_then(|value| Local.from_local_datetime(&value).single())
                    .map(|value| value.timestamp_millis())
            };
            let Some(start_ms) = parse_local(&start.text()) else {
                if let Some(window) = weak.upgrade() {
                    window.show_database_error(
                        "Start must use YYYY-MM-DD HH:MM:SS and identify one local time.",
                    );
                }
                return;
            };
            let Some(end_ms) = parse_local(&end.text()) else {
                if let Some(window) = weak.upgrade() {
                    window.show_database_error(
                        "End must use YYYY-MM-DD HH:MM:SS and identify one local time.",
                    );
                }
                return;
            };
            let index = usize::try_from(project.selected()).unwrap_or(0);
            let project_id = projects
                .get(index)
                .map_or(ProjectId(1), |project| project.id);
            let now = chrono::Utc::now().timestamp_millis();
            let entry = TimeEntry {
                id: None,
                project_id,
                task_id: None,
                note: note.text().to_string(),
                start_ms,
                end_ms,
                source: EntrySource::Manual,
                created_at_ms: now,
                updated_at_ms: now,
            };
            match handle.add_entry(entry) {
                Ok(_) => {
                    dialog_for_save.close();
                    if let Some(window) = weak.upgrade() {
                        window.refresh();
                    }
                }
                Err(error) => {
                    if let Some(window) = weak.upgrade() {
                        window.show_database_error(&error.to_string());
                    }
                }
            }
        });
        dialog.present(Some(self));
    }
```

**What.** A form built entirely in Rust: project dropdown, note, start and
end as text (defaulting to "the last hour"), a Save button. Saving parses
the times, builds a `TimeEntry` with `source: Manual`, and sends
`add_entry`; the store's validation (Chapter 10) does the rest.

**Why parse with `.single()`.** A local wall-clock time can be ambiguous
(the repeated hour when DST ends) or nonexistent (the skipped hour when it
starts). `from_local_datetime(..).single()` returns `None` in both cases,
and the dialog says so instead of guessing.

**GTK — `upcast`.** The `for` loop needs one element type; `DropDown`,
`Entry` are different types, so each is *upcast* to their common parent
`gtk::Widget`. The first `upcast::<gtk::Widget>()` fixes the array's type;
the rest infer it. `.clone()` before `upcast` because the originals are
still needed in the closure.

**Rust — closures capturing widgets.** `save.connect_clicked(move |_| ..)`
moves `start`, `end`, `note`, `project`, `projects`, `handle`, `weak` and
`dialog_for_save` into the handler — the form's whole state lives in the
closure. Widgets are refcounted, so a clone (`dialog.clone()`) is another
reference to the same dialog, cheap and correct.

**Rust — a closure inside a closure.** `parse_local` is a local helper
closure used twice; it borrows nothing, so no `move`.

**Rust — `let Some(x) = .. else { ...; return; }`.** Two early exits with
different messages. The `else` block runs a side effect (show a dialog)
before diverging.

## 17.3 Editing an entry

```rust
// crates/app/src/native/window.rs (inside impl MainWindow, after show_manual_entry)

    fn show_edit_entry(&self, existing: TimeEntry) {
        let Some(handle) = self.handle() else { return };
        let projects = self.imp().projects.borrow().clone();
        let dialog = adw::Dialog::builder()
            .title("Edit Entry")
            .content_width(480)
            .content_height(400)
            .build();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .build();
        let project_names = projects
            .iter()
            .map(|project| project.name.as_str())
            .collect::<Vec<_>>();
        let project = gtk::DropDown::from_strings(&project_names);
        if let Some(index) = projects
            .iter()
            .position(|project| project.id == existing.project_id)
            .and_then(|index| u32::try_from(index).ok())
        {
            project.set_selected(index);
        }
        let note = gtk::Entry::builder().text(&existing.note).build();
        let format_time = |timestamp| {
            Local
                .timestamp_millis_opt(timestamp)
                .single()
                .map_or_else(String::new, |value| {
                    value.format("%Y-%m-%d %H:%M:%S").to_string()
                })
        };
        let start = gtk::Entry::builder()
            .text(format_time(existing.start_ms))
            .build();
        let end = gtk::Entry::builder()
            .text(format_time(existing.end_ms))
            .build();
        for (label_text, widget) in [
            ("Project", project.clone().upcast::<gtk::Widget>()),
            ("Note", note.clone().upcast()),
            ("Start (local)", start.clone().upcast()),
            ("End (local)", end.clone().upcast()),
        ] {
            content.append(
                &gtk::Label::builder()
                    .label(label_text)
                    .halign(gtk::Align::Start)
                    .build(),
            );
            content.append(&widget);
        }
        let save = gtk::Button::with_label("Save Changes");
        save.add_css_class("suggested-action");
        content.append(&save);
        dialog.set_child(Some(&content));
        let dialog_for_save = dialog.clone();
        let weak = self.downgrade();
        save.connect_clicked(move |_| {
            let parse = |entry: &gtk::Entry| {
                NaiveDateTime::parse_from_str(&entry.text(), "%Y-%m-%d %H:%M:%S")
                    .ok()
                    .and_then(|value| Local.from_local_datetime(&value).single())
                    .map(|value| value.timestamp_millis())
            };
            let (Some(start_ms), Some(end_ms)) = (parse(&start), parse(&end)) else {
                if let Some(window) = weak.upgrade() {
                    window.show_database_error(
                        "Times must use YYYY-MM-DD HH:MM:SS and identify one local time.",
                    );
                }
                return;
            };
            let index = usize::try_from(project.selected()).unwrap_or(0);
            let project_id = projects
                .get(index)
                .map_or(existing.project_id, |project| project.id);
            let task_id = (project_id == existing.project_id)
                .then_some(existing.task_id)
                .flatten();
            let updated = TimeEntry {
                project_id,
                task_id,
                note: note.text().to_string(),
                start_ms,
                end_ms,
                updated_at_ms: chrono::Utc::now().timestamp_millis(),
                ..existing.clone()
            };
            match handle.update_entry(updated) {
                Ok(()) => {
                    dialog_for_save.close();
                    if let Some(window) = weak.upgrade() {
                        window.refresh();
                        window.refresh_report();
                    }
                }
                Err(error) => {
                    if let Some(window) = weak.upgrade() {
                        window.show_database_error(&error.to_string());
                    }
                }
            }
        });
        dialog.present(Some(self));
    }
```

And make the rows on the tracker page open it. In `refresh_entries`,
after `.build();` of the row:

```rust
// crates/app/src/native/window.rs (inside refresh_entries, Ok(entries) arm)
                        .build();
                    row.set_activatable(true);
                    row.connect_activated(glib::clone!(
                        #[weak(rename_to = window)]
                        self,
                        #[strong]
                        entry,
                        move |_| window.show_edit_entry(entry.clone())
                    ));
                    self.imp().entries_box.append(&row);
```

**What.** The same form pre-filled from an existing entry; saving builds an
updated copy and sends `update_entry`. Rows become activatable and open the
editor with their entry.

**Rust — tuple `let ... else`.** `let (Some(start_ms), Some(end_ms)) =
(parse(&start), parse(&end)) else { .. }` checks two `Option`s at once with
one message.

**Rust — `then_some` + `flatten`.** `(project_id == existing.project_id)
.then_some(existing.task_id)` is `Some(Option<TaskId>)` if the project is
unchanged and `None` otherwise; `.flatten()` collapses to `Option<TaskId>`.
Meaning: keep the task only if the project did not change, because tasks
belong to projects (Exercise 2 tries it in isolation).

**Rust — `..existing.clone()`.** Struct update from a *clone*, because
`existing` is captured by the `Fn` closure and cannot be moved out of it
(Exercise 1 shows that error with `document`). The `id`, `source` and
`created_at_ms` come from the original entry.

**Rust — `position` and `u32::try_from`.** `iter().position(..)` gives a
`usize` index; `DropDown::set_selected` wants `u32`.

**GTK — `#[strong] entry`.** Each row's closure owns its own copy of the
`TimeEntry` (strong capture) and clones it again when the editor opens,
because the handler may run more than once.

## 17.4 Idle resolution and recovery

```rust
// crates/app/src/native/window.rs (inside impl MainWindow, after show_edit_entry)

    fn show_idle_dialog(&self) {
        let dialog = adw::AlertDialog::builder()
            .heading("You were away")
            .body("How should the idle interval be counted?")
            .build();
        dialog.add_responses(&[
            ("keep", "Keep"),
            ("discard", "Discard and Resume"),
            ("reassign", "Reassign and Resume"),
            ("stop", "Stop"),
        ]);
        dialog.set_default_response(Some("discard"));
        let handle = self.handle();
        let weak = self.downgrade();
        dialog.connect_response(None, move |_, response| {
            if response == "reassign" {
                if let Some(window) = weak.upgrade() {
                    window.show_idle_reassign();
                }
                return;
            }
            let decision = match response {
                "keep" => houra_core::IdleDecision::Keep,
                "stop" => houra_core::IdleDecision::Stop,
                _ => houra_core::IdleDecision::DiscardAndResume,
            };
            if let Some(handle) = &handle
                && let Err(error) = handle.apply(TrackerCommand::ResolveIdle(decision))
            {
                log_background_error("resolving idle time", error);
            }
            if let Some(window) = weak.upgrade() {
                window.refresh();
            }
        });
        dialog.present(Some(self));
    }

    fn show_idle_reassign(&self) {
        let Some(handle) = self.handle() else { return };
        let projects = self.imp().projects.borrow().clone();
        let names = projects
            .iter()
            .map(|project| project.name.as_str())
            .collect::<Vec<_>>();
        let dropdown = gtk::DropDown::from_strings(&names);
        let dialog = adw::AlertDialog::builder()
            .heading("Reassign idle interval")
            .body("Choose the project that should receive the time you were away.")
            .build();
        dialog.set_extra_child(Some(&dropdown));
        dialog.add_responses(&[("cancel", "Cancel"), ("reassign", "Reassign")]);
        dialog.set_default_response(Some("reassign"));
        let weak = self.downgrade();
        dialog.connect_response(Some("reassign"), move |_, _| {
            let index = usize::try_from(dropdown.selected()).unwrap_or(0);
            let project_id = projects
                .get(index)
                .map_or(ProjectId(1), |project| project.id);
            let result = handle.apply(TrackerCommand::ResolveIdle(
                houra_core::IdleDecision::ReassignAndResume {
                    project_id,
                    task_id: None,
                    note: "Idle time".into(),
                },
            ));
            if let Some(window) = weak.upgrade() {
                if let Err(error) = result {
                    window.show_database_error(&error.to_string());
                }
                window.refresh();
            }
        });
        dialog.present(Some(self));
    }

    fn show_recovery_dialog(&self) {
        let Some(handle) = self.handle() else { return };
        let Ok(snapshot) = handle.snapshot() else {
            return;
        };
        let TrackerState::RecoveryPending(pending) = snapshot.state else {
            return;
        };
        let dialog = adw::AlertDialog::builder()
            .heading("Recover interrupted timer?")
            .body(if pending.unresolved_idle_start_ms.is_some() {
                "The app stopped during idle reconciliation. Edit the proposed end, then keep or discard it."
            } else {
                "Only time up to the last saved heartbeat is proposed. You may edit that end time."
            })
            .build();
        let proposed_end = Local
            .timestamp_millis_opt(pending.proposed_end_ms)
            .single()
            .map_or_else(String::new, |value| {
                value.format("%Y-%m-%d %H:%M:%S").to_string()
            });
        let end_entry = gtk::Entry::builder()
            .text(proposed_end)
            .placeholder_text("YYYY-MM-DD HH:MM:SS")
            .build();
        dialog.set_extra_child(Some(&end_entry));
        dialog.add_responses(&[
            ("resume", "Keep and Resume"),
            ("stop", "Keep and Stop"),
            ("discard", "Discard"),
        ]);
        dialog.set_default_response(Some("stop"));
        let weak = self.downgrade();
        dialog.connect_response(None, move |_, response| {
            let result = if response == "discard" {
                handle.apply(TrackerCommand::DiscardRecovery)
            } else {
                let end_ms = NaiveDateTime::parse_from_str(&end_entry.text(), "%Y-%m-%d %H:%M:%S")
                    .ok()
                    .and_then(|value| Local.from_local_datetime(&value).single())
                    .map_or(pending.proposed_end_ms, |value| value.timestamp_millis());
                handle.apply(TrackerCommand::ResolveRecovery {
                    end_ms,
                    resume: response == "resume",
                })
            };
            if let Err(error) = result {
                log_background_error("recovering timer", error);
            }
            if let Some(window) = weak.upgrade() {
                window.refresh();
            }
        });
        dialog.present(Some(self));
    }
```

Wire them in. In `toggle_timer`, replace the pending arm:

```rust
// crates/app/src/native/window.rs (inside toggle_timer)
            TrackerState::Running(_) => TrackerCommand::Stop,
            TrackerState::IdlePending(_) => {
                self.show_idle_dialog();
                return;
            }
            TrackerState::RecoveryPending(_) => {
                self.show_recovery_dialog();
                return;
            }
        };
```

And in `refresh`, open a pending dialog automatically:

```rust
// crates/app/src/native/window.rs (inside impl MainWindow)
    fn refresh(&self) {
        self.refresh_timer_only();
        self.refresh_entries();
        if let Some(handle) = self.handle()
            && let Ok(snapshot) = handle.snapshot()
        {
            match snapshot.state {
                TrackerState::IdlePending(ref pending) if pending.return_ms.is_some() => {
                    self.show_idle_dialog();
                }
                TrackerState::RecoveryPending(_) => self.show_recovery_dialog(),
                _ => {}
            }
        }
    }
```

**What.** The four idle decisions from Chapter 3 become four buttons;
*Reassign* opens a second dialog with a project chooser. Recovery proposes
the heartbeat end (editable, bounded by the engine), with resume, stop, or
discard. Both dialogs open from the Start button when the state is pending,
and from any `refresh` once the user is back (`return_ms.is_some()`).

**Why parse failures fall back to the proposal.** A typo in the end field
must not lose the recovery; `map_or(pending.proposed_end_ms, ..)` keeps the
safe value, and the engine still rejects anything past the heartbeat.

**Rust — `ref` in a pattern with a guard.** `TrackerState::IdlePending(ref
pending) if pending.return_ms.is_some()` borrows the payload instead of
moving it out of `snapshot.state`, which the guard then reads.

**Rust — `let TrackerState::RecoveryPending(pending) = snapshot.state else`.**
`let ... else` works with any refutable pattern, not only `Some`/`Ok`.

**Rust — `connect_response(None, ..)`.** With `None`, the handler receives
every response id as a `&str` and matches on it; with `Some("reassign")`
it runs for that one. String matching on response ids is the libadwaita
convention.

**Rust — `log_background_error`.** A crate-private helper (17.6) that
logs through `tracing`; used where a dialog would be overkill.

## 17.5 Quit, backup, restore

```rust
// crates/app/src/native/window.rs (inside impl MainWindow)

    pub fn confirm_quit(&self) {
        let dialog = adw::AlertDialog::builder()
            .heading("A timer is still running")
            .body("Stop the timer and quit, or keep Houra running in the background.")
            .build();
        dialog.add_responses(&[("cancel", "Keep Running"), ("quit", "Stop and Quit")]);
        dialog.set_response_appearance("quit", adw::ResponseAppearance::Destructive);
        let handle = self.handle();
        let application = self.application();
        dialog.connect_response(Some("quit"), move |_, _| {
            if let Some(handle) = &handle {
                let _ignored = handle.apply(TrackerCommand::Stop);
            }
            if let Some(application) = &application {
                application.quit();
            }
        });
        dialog.present(Some(self));
    }

    pub fn backup_data(&self) {
        let Some(handle) = self.handle() else { return };
        let chooser = gtk::FileDialog::builder()
            .title("Back Up Houra")
            .initial_name(format!(
                "houra-backup-{}.json",
                Local::now().format("%Y-%m-%d")
            ))
            .build();
        let weak = self.downgrade();
        chooser.save(Some(self), None::<&gio::Cancellable>, move |result| {
            let Some(window) = weak.upgrade() else { return };
            let result = result
                .map_err(|error| crate::AppError::InvalidBackup(error.to_string()))
                .and_then(|file| {
                    let path = file.path().ok_or_else(|| {
                        crate::AppError::InvalidBackup("backup requires a local file".into())
                    })?;
                    let document = handle.backup(chrono::Utc::now().timestamp_millis())?;
                    document.write_to_path(&path)
                });
            if let Err(error) = result {
                window.show_database_error(&error.to_string());
            }
        });
    }

    pub fn restore_data(&self) {
        let Some(handle) = self.handle() else { return };
        if handle
            .snapshot()
            .is_ok_and(|snapshot| snapshot.state.active().is_some())
        {
            self.show_database_error("Stop the timer before restoring a backup.");
            return;
        }
        let chooser = gtk::FileDialog::builder()
            .title("Choose a Houra Backup")
            .build();
        let weak = self.downgrade();
        chooser.open(Some(self), None::<&gio::Cancellable>, move |result| {
            let Some(window) = weak.upgrade() else { return };
            let document = result
                .map_err(|error| crate::AppError::InvalidBackup(error.to_string()))
                .and_then(|file| {
                    let path = file.path().ok_or_else(|| {
                        crate::AppError::InvalidBackup("restore requires a local file".into())
                    })?;
                    crate::backup::BackupDocument::read_from_path(&path)
                });
            match document {
                Ok(document) => window.confirm_restore(handle.clone(), document),
                Err(error) => window.show_database_error(&error.to_string()),
            }
        });
    }

    fn confirm_restore(&self, handle: TrackerHandle, document: crate::backup::BackupDocument) {
        let dialog = adw::AlertDialog::builder()
            .heading("Replace all local data?")
            .body("The validated backup will replace projects, tasks, and entries. This cannot be undone.")
            .build();
        dialog.add_responses(&[("cancel", "Cancel"), ("restore", "Replace Data")]);
        dialog.set_response_appearance("restore", adw::ResponseAppearance::Destructive);
        let weak = self.downgrade();
        dialog.connect_response(Some("restore"), move |_, _| {
            let result = handle.restore(document.clone());
            if let Some(window) = weak.upgrade() {
                if let Err(error) = result {
                    window.show_database_error(&error.to_string());
                }
                window.reload_projects();
                window.reload_tasks();
                window.refresh();
                window.refresh_projects_page();
                window.refresh_report();
            }
        });
        dialog.present(Some(self));
    }
```

Place `backup_data`, `restore_data` and `confirm_restore` after
`export_report_csv`, and `confirm_quit` before `show_database_error`, to
match the original's order.

**What.** Quit with a running timer asks first, then stops and quits
(so the clean-shutdown marker is written). Backup picks a destination and
writes the validated document (Chapter 11). Restore refuses while running,
picks a file, parses and validates it, and asks once more before replacing
everything.

**Rust — `is_ok_and`.** `Result::is_ok_and(|v| cond)` — true only if `Ok`
and the predicate holds. Cousins: `is_some_and`, `is_none_or` (Chapter 18).

**Rust — `document.clone()` inside the handler.** The handler is `Fn`; it
cannot give away the document it owns, so it clones for the call
(Exercise 1). A backup is a few kilobytes; the clarity is worth it.

**GTK — `Destructive` appearance.** Colours the button red. It signals
consequence; the safety is `Store::restore`'s transaction.

**GTK — `self.application()`.** A window knows its application (the
property set in `new`); `quit()` ends the main loop, after which `run` in
`mod.rs` shuts down the actor.

## 17.6 Actions, accelerators, close-to-hide

Replace `native/mod.rs`:

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
use tracing::error;

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

    application.connect_startup(|application| {
        load_css();
        application.set_accels_for_action("app.toggle-timer", &["<Control>space"]);
        application.set_accels_for_action("app.add-entry", &["<Control>n"]);
        application.set_accels_for_action("app.quit", &["<Control>q"]);
    });

    let activate_window = Rc::clone(&main_window);
    let activate_handle = handle.clone();
    application.connect_activate(move |application| {
        if activate_window.borrow().is_none() {
            let window = MainWindow::new(application, activate_handle.clone());
            window.connect_close_request(|window| {
                window.set_visible(false);
                glib::Propagation::Stop
            });
            activate_window.replace(Some(window));
        }
        if let Some(window) = activate_window.borrow().as_ref() {
            window.present();
        }
    });

    install_actions(&application, &main_window, handle);
    let _status = application.run();
    service.shutdown()
}

// ... register_resources and load_css unchanged ...

fn install_actions(
    application: &adw::Application,
    window: &Rc<RefCell<Option<MainWindow>>>,
    handle: crate::TrackerHandle,
) {
    let toggle = gio::ActionEntry::builder("toggle-timer")
        .activate({
            let window = Rc::clone(window);
            move |_: &adw::Application, _, _| {
                if let Some(window) = window.borrow().as_ref() {
                    window.present();
                    window.toggle_timer();
                }
            }
        })
        .build();
    let add = gio::ActionEntry::builder("add-entry")
        .activate({
            let window = Rc::clone(window);
            move |_: &adw::Application, _, _| {
                if let Some(window) = window.borrow().as_ref() {
                    window.show_manual_entry();
                }
            }
        })
        .build();
    let quit = gio::ActionEntry::builder("quit")
        .activate({
            let window = Rc::clone(window);
            let handle = handle.clone();
            move |application: &adw::Application, _, _| {
                let active = handle
                    .snapshot()
                    .map(|snapshot| snapshot.state.active().is_some())
                    .unwrap_or(false);
                if active {
                    if let Some(window) = window.borrow().as_ref() {
                        window.confirm_quit();
                    }
                } else {
                    application.quit();
                }
            }
        })
        .build();
    let backup = gio::ActionEntry::builder("backup")
        .activate({
            let window = Rc::clone(window);
            move |_: &adw::Application, _, _| {
                if let Some(window) = window.borrow().as_ref() {
                    window.backup_data();
                }
            }
        })
        .build();
    let restore = gio::ActionEntry::builder("restore")
        .activate({
            let window = Rc::clone(window);
            move |_: &adw::Application, _, _| {
                if let Some(window) = window.borrow().as_ref() {
                    window.restore_data();
                }
            }
        })
        .build();
    application.add_action_entries([toggle, add, backup, restore, quit]);
}

pub(crate) fn log_background_error(context: &'static str, error: impl std::fmt::Display) {
    error!(%error, %context, "background operation failed");
}
```

**What.** Five application actions, three keyboard accelerators, and a
close handler that hides the window instead of destroying it. The Quit
action either quits at once or asks (17.5) when a timer runs.

**Why close hides.** A time tracker should keep counting — and keep
detecting idle time (Chapter 19) — after its window is closed. Hiding keeps
the process (and the window object) alive; only *Quit* ends it. Chapter 18
adds the explicit `hold` and `--background` start.

**GTK — `ActionEntry`.** `builder("name").activate(closure).build()`
creates an action whose handler receives `(application, action, parameter)`;
the unused ones are `_`. The type annotation `_: &adw::Application` is the
one hint inference needs. `add_action_entries([...])` registers all five;
names match the `app.*` strings in the XML menu and in
`set_accels_for_action`. A menu item whose action is missing shows greyed
out — which is why the *Preferences* item waits for Chapter 18.

**GTK — `connect_close_request`.** Returning `Propagation::Stop` tells GTK
the request was handled; the default handler (destroy) does not run.

**Rust — block expressions as arguments.** `.activate({ let window =
Rc::clone(window); move |..| .. })`: a block that clones what the closure
needs and then evaluates to the closure. This keeps each closure's captures
next to it without a `clone!` macro.

**Rust — `pub(crate)`.** Visible anywhere in this crate, not outside;
`window.rs` imports `native::log_background_error`. `impl Display` as a
parameter accepts any error type.

## 17.7 Checkpoint

```sh
cargo build --features native-ui
XDG_DATA_HOME=/tmp/houra-study cargo run --features native-ui
```

Try: the ☰ menu; Ctrl+N opens *Manual Entry*; save an entry, click its
row, edit it. Ctrl+Space toggles the timer. Close the window: the process
keeps running (the terminal does not return); a second `cargo run` presents
the same window (single instance). Ctrl+Q quits — or asks, if running.
*Back Up Data* writes a JSON file; *Restore Data* refuses while running,
otherwise asks before replacing.

Recovery: Start a timer, then from another terminal `pkill -9 -f
houra`. Run again: the recovery dialog opens with the last
heartbeat as the proposed end.

```sh
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
diff <(grep -v '^\s*//' ../work_time_tracker/crates/app/src/native/window.rs) \
     <(grep -v '^\s*//' crates/app/src/native/window.rs)
```

Remaining differences: the `integration_banner` field and
`show_integration_warning` (Chapter 19), `show_preferences` and the
`load_settings` line in `report_bounds` (Chapter 18).

```sh
git add -A && git commit -m "Chapter 17: dialogs and actions"
```

## 17.8 Exercises

1. **`Fn` closures cannot give away their captures.** In `confirm_restore`,
   change `handle.restore(document.clone())` to `handle.restore(document)`
   and build.

   <details><summary>Answer</summary>

   ```
   error[E0507]: cannot move out of `document`, a captured variable in an `Fn` closure
       |
   569 |             let result = handle.restore(document);
       |                                         ^^^^^^^^ `document` is moved here
   help: `Fn` and `FnMut` closures require captured values to be able to be consumed multiple times, but `FnOnce` closures may consume them only once
   ```

   Same rule as Chapter 16's exercise 2, from the other side: the handler
   may run again, so it must keep its document. Revert.
   </details>

2. **`then_some` and `flatten` (temporary test).** Append to `window.rs`:

   ```rust
   #[cfg(test)]
   mod options {
       #[test]
       fn then_some_and_flatten() {
           let task: Option<i32> = Some(7);
           assert_eq!(true.then_some(task).flatten(), Some(7));
           assert_eq!(false.then_some(task).flatten(), None);
           assert_eq!(true.then_some(None::<i32>).flatten(), None);
       }
   }
   ```

   Run `cargo test --features native-ui -p houra --lib then_some`.

   <details><summary>Answer</summary>

   Passes. `bool::then_some(v)` is `if cond { Some(v) } else { None }`;
   `flatten` turns `Option<Option<T>>` into `Option<T>`. Together: "keep
   the task only if the project is unchanged". Revert.
   </details>

3. **A dead shortcut.** Change `"app.toggle-timer"` in
   `set_accels_for_action` to `"app.toggle-timers"`, run, press
   Ctrl+Space.

   <details><summary>Answer</summary>

   Nothing happens and nothing is reported: action names are strings
   resolved at run time, like template ids. Keep names in one place and
   test shortcuts by hand after changing them. Revert.
   </details>

## Recap

- Menus and shortcuts activate named application actions; Rust registers
  one `ActionEntry` per name.
- Dialogs own their form state inside `Fn` closures, clone what they must
  give away, and reach the window weakly.
- Local date-time parsing uses `.single()` to refuse ambiguous times;
  failures fall back to safe proposals.
- Pending states open their dialogs from the Start button and from every
  refresh.
- Closing hides; quitting stops the timer first so the shutdown is clean.

Next: **Chapter 18 — Settings and background**, GSettings, the preferences
dialog, first-run autostart, and starting hidden with `--background`.
