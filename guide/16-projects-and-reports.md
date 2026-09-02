# Chapter 16 — Projects and reports

**Goal.** Two more pages in the view stack. *Projects*: every project with
its tasks, add/archive buttons, and a shared "enter a name" dialog.
*Reports*: the weekly grouping from Chapter 7 rendered as rows, week
navigation, and CSV export through a file dialog. Files: `data/ui/window.ui`,
`crates/app/src/native/window.rs`.

**You will learn**

- Building widgets in Rust with builders, `add_suffix`, `valign`.
- A generic method with a `where` clause and a `'static` closure bound.
- `Fn` versus `FnOnce`, and why a signal handler needs `Fn`.
- Asynchronous `gtk::FileDialog` callbacks that own their data.
- `Option` + `?` inside a function returning `Option`.
- `Result` combinator chains (`map_err`, `and_then`) in callbacks.

**Prerequisite.** Chapter 15 checkpoint passed.

---

## 16.1 Two more pages

In `window.ui`, add two `AdwViewStackPage` children to `view_stack`, after
the tracker page (inside the same `<object class="AdwViewStack">`):

```xml
<!-- data/ui/window.ui (inside AdwViewStack, after the tracker page) -->
                <child>
                  <object class="AdwViewStackPage">
                    <property name="name">reports</property>
                    <property name="title" translatable="yes">Reports</property>
                    <property name="icon-name">x-office-spreadsheet-symbolic</property>
                    <property name="child">
                      <object class="GtkScrolledWindow">
                        <property name="hscrollbar-policy">never</property>
                        <property name="child">
                          <object class="AdwClamp">
                            <property name="maximum-size">620</property>
                            <property name="margin-top">24</property><property name="margin-bottom">24</property><property name="margin-start">18</property><property name="margin-end">18</property>
                            <property name="child">
                              <object class="GtkBox">
                                <property name="orientation">vertical</property><property name="spacing">12</property>
                                <child><object class="GtkLabel"><property name="label" translatable="yes">Weekly Report</property><property name="halign">start</property><style><class name="title-1"/></style></object></child>
                                <child>
                                  <object class="GtkBox">
                                    <property name="spacing">6</property>
                                    <child><object class="GtkButton" id="report_previous_button"><property name="icon-name">go-previous-symbolic</property><property name="tooltip-text" translatable="yes">Previous week</property></object></child>
                                    <child><object class="GtkLabel" id="report_week_label"><property name="hexpand">true</property><style><class name="heading"/></style></object></child>
                                    <child><object class="GtkButton" id="report_next_button"><property name="icon-name">go-next-symbolic</property><property name="tooltip-text" translatable="yes">Next week</property></object></child>
                                  </object>
                                </child>
                                <child><object class="GtkButton" id="export_csv_button"><property name="label" translatable="yes">Export CSV</property><property name="halign">start</property></object></child>
                                <child><object class="GtkBox" id="report_box"><property name="orientation">vertical</property><property name="spacing">6</property></object></child>
                              </object>
                            </property>
                          </object>
                        </property>
                      </object>
                    </property>
                  </object>
                </child>
                <child>
                  <object class="AdwViewStackPage">
                    <property name="name">projects</property>
                    <property name="title" translatable="yes">Projects</property>
                    <property name="icon-name">folder-symbolic</property>
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
                                <property name="spacing">12</property>
                                <child><object class="GtkLabel"><property name="label" translatable="yes">Projects and Tasks</property><property name="halign">start</property><style><class name="title-1"/></style></object></child>
                                <child><object class="GtkButton" id="add_project_button"><property name="label" translatable="yes">Add Project</property><property name="halign">start</property><style><class name="suggested-action"/></style></object></child>
                                <child><object class="GtkBox" id="projects_box"><property name="orientation">vertical</property><property name="spacing">6</property></object></child>
                              </object>
                            </property>
                          </object>
                        </property>
                      </object>
                    </property>
                  </object>
                </child>
```

**What.** The same scrolled-clamp-column recipe as the tracker page. The
view switcher in the header now shows three tabs automatically — the
switcher reads the stack's pages; nothing else changes.

## 16.2 New template children and setup

```rust
// crates/app/src/native/window.rs
use chrono::{Datelike, Local, NaiveDate, TimeZone};
// ...

    pub struct MainWindow {
        // ... the Chapter 15 fields ...
        #[template_child]
        pub add_project_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub projects_box: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub report_box: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub report_week_label: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub report_previous_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub report_next_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub export_csv_button: gtk::TemplateChild<gtk::Button>,
        pub handle: RefCell<Option<TrackerHandle>>,
        pub projects: RefCell<Vec<Project>>,
        pub tasks: RefCell<Vec<Task>>,
        pub report_week_offset: Cell<i32>,
        pub selected_day_offset: Cell<i32>,
    }
```

(Put the seven `#[template_child]` fields between `day_next_button` and
`handle` to match the original's order.)

In `setup`, add the two initial renders after `reload_tasks()` and the four
handlers after the note handler:

```rust
// crates/app/src/native/window.rs (inside setup)
        self.reload_projects();
        self.reload_tasks();
        self.refresh_projects_page();
        self.refresh_report();
        self.refresh();
        // ... start_button, project_dropdown, task_dropdown, note_entry handlers ...
        self.imp().add_project_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.show_new_project()
        ));
        self.imp()
            .report_previous_button
            .connect_clicked(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| {
                    window
                        .imp()
                        .report_week_offset
                        .set(window.imp().report_week_offset.get().saturating_sub(1));
                    window.refresh_report();
                }
            ));
        self.imp().report_next_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window
                    .imp()
                    .report_week_offset
                    .set(window.imp().report_week_offset.get().saturating_add(1));
                window.refresh_report();
            }
        ));
        self.imp().export_csv_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.export_report_csv()
        ));
        // ... day_previous_button, day_next_button, the two timers ...
```

## 16.3 The projects page

Add after `update_active_details`:

```rust
// crates/app/src/native/window.rs (inside impl MainWindow)

    fn refresh_projects_page(&self) {
        let Some(handle) = self.handle() else { return };
        while let Some(child) = self.imp().projects_box.first_child() {
            self.imp().projects_box.remove(&child);
        }
        let projects = match handle.projects(true) {
            Ok(projects) => projects,
            Err(error) => {
                self.show_database_error(&error.to_string());
                return;
            }
        };
        let tasks = handle.tasks(true).unwrap_or_default();
        for project in projects {
            let row = adw::ActionRow::builder()
                .title(&project.name)
                .subtitle(if project.archived {
                    "Archived"
                } else {
                    &project.color
                })
                .build();
            let add_task = gtk::Button::builder()
                .icon_name("list-add-symbolic")
                .tooltip_text("Add task")
                .valign(gtk::Align::Center)
                .build();
            add_task.connect_clicked(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| window.show_new_task(project.id)
            ));
            row.add_suffix(&add_task);
            if project.id != ProjectId(1) {
                let archive = gtk::Button::builder()
                    .icon_name(if project.archived {
                        "view-refresh-symbolic"
                    } else {
                        "user-trash-symbolic"
                    })
                    .tooltip_text(if project.archived {
                        "Restore"
                    } else {
                        "Archive"
                    })
                    .valign(gtk::Align::Center)
                    .build();
                archive.connect_clicked(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    #[strong]
                    handle,
                    move |_| {
                        let now = chrono::Utc::now().timestamp_millis();
                        if let Err(error) =
                            handle.set_project_archived(project.id, !project.archived, now)
                        {
                            window.show_database_error(&error.to_string());
                        }
                        window.reload_projects();
                        window.refresh_projects_page();
                    }
                ));
                row.add_suffix(&archive);
            }
            self.imp().projects_box.append(&row);
            for task in tasks.iter().filter(|task| task.project_id == project.id) {
                let task_row = adw::ActionRow::builder()
                    .title(format!("↳ {}", task.name))
                    .subtitle(if task.archived {
                        "Archived task"
                    } else {
                        "Task"
                    })
                    .build();
                let archive = gtk::Button::builder()
                    .icon_name(if task.archived {
                        "view-refresh-symbolic"
                    } else {
                        "user-trash-symbolic"
                    })
                    .tooltip_text(if task.archived { "Restore" } else { "Archive" })
                    .valign(gtk::Align::Center)
                    .build();
                let task_id = task.id;
                let archived = task.archived;
                archive.connect_clicked(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    #[strong]
                    handle,
                    move |_| {
                        let now = chrono::Utc::now().timestamp_millis();
                        if let Err(error) = handle.set_task_archived(task_id, !archived, now) {
                            window.show_database_error(&error.to_string());
                        }
                        window.refresh_projects_page();
                    }
                ));
                task_row.add_suffix(&archive);
                self.imp().projects_box.append(&task_row);
            }
        }
    }
```

**What.** Clear the box; for each project (archived included) build a row
with an "add task" button and — except for General — an archive/restore
button; then a row per task under it. Every click sends a command through
the handle and rebuilds the page.

**GTK — builders and suffixes.** `gtk::Button::builder().icon_name(..)
.tooltip_text(..).valign(gtk::Align::Center).build()` sets properties before
construction. `row.add_suffix(&button)` places the button at the row's
end. `Align::Center` keeps a small button vertically centred in a taller
row.

**Rust — captures in `glib::clone!`.** `#[weak(rename_to = window)] self`
plus `#[strong] handle`: the handle is cheap to clone and must outlive the
closure, so it is captured strongly; the window weakly, as always. Values
like `project.id` (a `Copy` newtype) and `!project.archived` are moved into
the closure by `move` — note `let task_id = task.id;` pulls the `Copy`
fields out first because `task` is only a borrow from `tasks.iter()`.

**Rust — `for project in projects` (owned) vs `tasks.iter()` (borrowed).**
Projects are consumed one by one — each row's closure moves `project.id` and
`project.archived` out. Tasks are iterated by reference because the list
is reused for every project.

**Rust — `&project.name` into `.title(...)`.** The builder takes anything
`Into<GString>`; a `&String` works, and so does the `format!` result for
`"↳ {}"`.

## 16.4 The weekly report

```rust
// crates/app/src/native/window.rs (inside impl MainWindow, after refresh_projects_page)

    fn report_bounds(&self) -> Option<(chrono::DateTime<Local>, chrono::DateTime<Local>)> {
        let today = Local::now().date_naive();
        let starts_monday = true;
        let days_from_start = if starts_monday {
            today.weekday().num_days_from_monday()
        } else {
            today.weekday().num_days_from_sunday()
        };
        let week_start =
            today.checked_sub_signed(chrono::Duration::days(i64::from(days_from_start)))?;
        let start_date = week_start.checked_add_signed(chrono::Duration::weeks(i64::from(
            self.imp().report_week_offset.get(),
        )))?;
        let start = Local
            .from_local_datetime(&start_date.and_hms_opt(0, 0, 0)?)
            .earliest()?;
        let end_date = start_date.checked_add_signed(chrono::Duration::weeks(1))?;
        let end = Local
            .from_local_datetime(&end_date.and_hms_opt(0, 0, 0)?)
            .earliest()?;
        Some((start, end))
    }

    fn refresh_report(&self) {
        let Some(handle) = self.handle() else { return };
        let Some((start, end)) = self.report_bounds() else {
            return;
        };
        self.imp().report_week_label.set_label(&format!(
            "{} – {}",
            start.format("%x"),
            end.date_naive()
                .pred_opt()
                .map_or_else(String::new, |date| date.format("%x").to_string())
        ));
        while let Some(child) = self.imp().report_box.first_child() {
            self.imp().report_box.remove(&child);
        }
        let entries = match handle.entries(start.timestamp_millis(), end.timestamp_millis()) {
            Ok(entries) => entries,
            Err(error) => {
                self.show_database_error(&error.to_string());
                return;
            }
        };
        let projects = handle.projects(true).unwrap_or_default();
        let tasks = handle.tasks(true).unwrap_or_default();
        let rows = houra_core::group_entries(&entries);
        if rows.is_empty() {
            self.imp()
                .report_box
                .append(&gtk::Label::new(Some("No tracked time this week")));
            return;
        }
        for row in rows {
            let date = NaiveDate::from_yo_opt(row.bucket.local_year, row.bucket.local_ordinal)
                .map_or_else(
                    || "Unknown day".into(),
                    |date| date.format("%A, %x").to_string(),
                );
            let project = projects
                .iter()
                .find(|project| project.id == row.bucket.project_id)
                .map_or("Missing project", |project| project.name.as_str());
            let task = row
                .bucket
                .task_id
                .and_then(|id| tasks.iter().find(|task| task.id == id))
                .map(|task| format!(" / {}", task.name))
                .unwrap_or_default();
            let seconds = row.duration_ms / 1_000;
            let report_row = adw::ActionRow::builder()
                .title(format!("{project}{task}"))
                .subtitle(format!(
                    "{date} · {}h {:02}m",
                    seconds / 3600,
                    (seconds / 60) % 60
                ))
                .build();
            self.imp().report_box.append(&report_row);
        }
    }
```

**What.** `report_bounds` computes the local week `[Monday 00:00, next
Monday 00:00)` shifted by the offset; `refresh_report` fetches the entries
in that window, groups them with the core's `group_entries`, and renders
one row per (day, project, task). The week-start preference is hard-coded
to Monday until Chapter 18 reads it from GSettings.

**Rust — `?` on `Option`.** Inside a function returning `Option`, `?` on
an `Option` returns `None` early. Five chrono steps each may fail on
absurd dates; `?` keeps the happy path readable, and `refresh_report`'s
`let Some(..) = .. else { return }` handles the `None`.

**Rust — the core does the work.** The window only resolves names and
formats; `group_entries` (pure, tested in Chapter 7) owns the day-splitting
logic. That is the layering paying off.

**chrono.** `from_yo_opt(year, ordinal)` reverses the bucket key;
`pred_opt()` is yesterday, used to print an inclusive end date.

## 16.5 CSV export through a file dialog

```rust
// crates/app/src/native/window.rs (inside impl MainWindow, after refresh_report)

    fn export_report_csv(&self) {
        let Some(handle) = self.handle() else { return };
        let Some((start, end)) = self.report_bounds() else {
            return;
        };
        let chooser = gtk::FileDialog::builder()
            .title("Export Weekly CSV")
            .initial_name(format!("houra-{}.csv", start.format("%Y-%m-%d")))
            .build();
        let weak = self.downgrade();
        chooser.save(Some(self), None::<&gio::Cancellable>, move |result| {
            let Some(window) = weak.upgrade() else { return };
            let result = result
                .map_err(|error| crate::AppError::InvalidBackup(error.to_string()))
                .and_then(|file| {
                    let path = file.path().ok_or_else(|| {
                        crate::AppError::InvalidBackup("CSV export requires a local file".into())
                    })?;
                    let entries =
                        handle.entries(start.timestamp_millis(), end.timestamp_millis())?;
                    let projects = handle.projects(true)?;
                    let tasks = handle.tasks(true)?;
                    crate::export::write_csv_path(&path, &entries, &projects, &tasks)
                });
            if let Err(error) = result {
                window.show_database_error(&error.to_string());
            }
        });
    }
```

**What.** Open a save dialog; when the user picks a file (later, on the
main loop), fetch the week's data and write the CSV from Chapter 12.

**GTK — asynchronous dialogs.** `chooser.save(parent, cancellable,
callback)` returns immediately; the callback runs when the dialog closes.
Everything the callback needs — the handle, the bounds, a weak window — is
moved into it, because by then `export_report_csv`'s stack frame is long
gone. A cancelled dialog arrives as an `Err`, which becomes a dialog
message (harmless; the original does not special-case it).

**Rust — combinator chain.** `result.map_err(..).and_then(|file| { ...?;
...?; write })` converts GLib's error into `AppError`, then runs a block
where `?` works because the closure returns `Result<(), AppError>`. One
`if let Err` at the end reports any failure in the chain.

**Rust — `None::<&gio::Cancellable>`.** The turbofish tells the compiler
which `Option` type `None` is — a pattern you will see throughout gtk-rs
APIs that accept optional objects.

## 16.6 One dialog for two purposes

```rust
// crates/app/src/native/window.rs (inside impl MainWindow, after export_report_csv)

    fn show_new_project(&self) {
        self.show_name_dialog("New Project", move |handle, name, now| {
            handle
                .create_project(name, "#3584e4".into(), now)
                .map(|_| ())
        });
    }

    fn show_new_task(&self, project_id: ProjectId) {
        self.show_name_dialog("New Task", move |handle, name, now| {
            handle.create_task(project_id, name, now).map(|_| ())
        });
    }

    fn show_name_dialog<F>(&self, title: &str, save: F)
    where
        F: Fn(&TrackerHandle, String, i64) -> Result<(), crate::AppError> + 'static,
    {
        let Some(handle) = self.handle() else { return };
        let dialog = adw::AlertDialog::builder().heading(title).build();
        let entry = gtk::Entry::builder()
            .placeholder_text("Name")
            .activates_default(true)
            .build();
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", "Cancel"), ("save", "Save")]);
        dialog.set_default_response(Some("save"));
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        let weak = self.downgrade();
        dialog.connect_response(Some("save"), move |_, _| {
            let result = save(
                &handle,
                entry.text().to_string(),
                chrono::Utc::now().timestamp_millis(),
            );
            if let Some(window) = weak.upgrade() {
                if let Err(error) = result {
                    window.show_database_error(&error.to_string());
                }
                window.reload_projects();
                window.refresh_projects_page();
            }
        });
        dialog.present(Some(self));
    }
```

**What.** "New project" and "new task" differ only in what happens with
the typed name. `show_name_dialog` builds the dialog once and takes the
difference as a closure.

**Rust — generic method with `where`.** `fn show_name_dialog<F>(&self,
title: &str, save: F) where F: Fn(&TrackerHandle, String, i64) ->
Result<(), AppError> + 'static`: the `where` clause spells out the closure
type in full. `Fn` (not `FnOnce`) because the dialog's response signal may
fire more than once, so the closure must be callable repeatedly (Exercise
2). `'static` because the dialog keeps it after this method returns
(Exercise 1). `show_new_task`'s closure captures `project_id` by `move` —
a `Copy` value it owns, so it is `'static`.

**Rust — `.map(|_| ())`.** `create_project` returns `Result<ProjectId, _>`;
the dialog does not care about the ID, so the success value is mapped to
`()` to fit the shared signature.

**GTK — `AlertDialog` with an extra child.** `set_extra_child` embeds any
widget; `activates_default(true)` makes Enter in the entry trigger the
default response; `ResponseAppearance::Suggested` colours the Save button.

## 16.7 Checkpoint

```sh
cargo build --features native-ui
XDG_DATA_HOME=/tmp/houra-study cargo run --features native-ui
```

Three tabs. *Projects*: General with an add-task button; add a project
and a task, archive and restore them (General cannot be archived — the
button is absent). *Tracker*: the dropdowns now list what you created.
Track some time, then *Reports*: rows grouped by day and project, week
navigation, and *Export CSV* writes a file you can open in a spreadsheet.

```sh
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
git add -A && git commit -m "Chapter 16: projects and reports"
```

## 16.8 Exercises

1. **`'static` on a stored closure.** Remove `+ 'static` from the `where`
   clause of `show_name_dialog` and build.

   <details><summary>Answer</summary>

   ```
   error[E0310]: the parameter type `F` may not live long enough
       |
   530 | /         dialog.connect_response(Some("save"), move |_, _| {
       | |          the parameter type `F` must be valid for the static lifetime...
       |            ...so that the type `F` will meet its required lifetime bounds
   ```

   `connect_response` stores the handler inside the dialog, which lives as
   long as GTK decides. A closure that might borrow something from the
   caller's stack cannot be stored, so GTK's API demands `'static`, and the
   generic bound must promise it. Revert.
   </details>

2. **`Fn` versus `FnOnce`.** Change `F: Fn(...)` to `F: FnOnce(...)` and
   build.

   <details><summary>Answer</summary>

   ```
   error[E0507]: cannot move out of `save`, a captured variable in an `Fn` closure
       |
   531 |             let result = save(
       |                          ^^^^ `save` is moved here
   help: `Fn` and `FnMut` closures require captured values to be able to be consumed multiple times, but `FnOnce` closures may consume them only once
   ```

   The response handler itself is an `Fn` closure (GTK may call it many
   times), so anything it calls must also be callable many times. The
   three closure traits form a ladder: `FnOnce` (consumes captures, once),
   `FnMut` (mutates captures), `Fn` (only reads). Chapter 13's
   `request<T>` could use `FnOnce` because it called `build` exactly once;
   here the bound has to be `Fn`. Revert.
   </details>

3. **A row per day.** Track two short intervals today with different
   notes, then look at *Reports*. Then, in Rust, remove the
   `while let Some(child) = ... remove(&child)` loop from `refresh_report`,
   run, and click ▶ ◀.

   <details><summary>Answer</summary>

   One row (same day, same project, same task — the note is not part of
   the bucket). Without the clearing loop, every navigation appends a new
   copy of the rows under the old ones: GTK does not replace children for
   you. Revert.
   </details>

## Recap

- Data-dependent UI is built in Rust with builders; static structure stays
  in XML.
- One generic method with a `Fn + 'static` closure serves two dialogs.
- File dialogs are asynchronous; callbacks own everything they need and
  reach the window through a weak reference.
- The report page renders the core's `group_entries` output; the window
  adds names and formatting only.

Next: **Chapter 17 — Dialogs and actions**, manual and edit entries, idle
and recovery resolution, quit confirmation, backup/restore, and the
application actions and shortcuts that trigger them.
