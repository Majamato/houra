use crate::locale::{tr, trf};
use std::time::Duration;

use chrono::{Datelike, Local, TimeZone};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::{ActivityId, ProjectId, TimeEntry, TrackerCommand, TrackerState};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::widgets::{active_entry_total_duration, counted_live_elapsed};
use crate::desktop::window::MainWindow;

impl MainWindow {
    pub(in crate::desktop) fn refresh_active_entry_duration(&self) {
        let Some(handle) = self.handle() else { return };
        let Ok(snapshot) = handle.snapshot() else {
            return;
        };
        let Some(active) = snapshot.state.active() else {
            self.clear_active_entry_duration();
            return;
        };
        let saved_ms = active
            .entry_id
            .and_then(|entry_id| handle.entry(entry_id).ok())
            .map_or(0, |entry| entry.duration_ms());
        self.imp().active_entry_id.set(active.entry_id);
        self.imp().active_entry_saved_ms.set(saved_ms);
        self.imp().active_entry_duration_cached.set(true);
    }

    fn clear_active_entry_duration(&self) {
        self.imp().active_entry_id.set(None);
        self.imp().active_entry_saved_ms.set(0);
        self.imp().active_entry_duration_cached.set(false);
    }

    pub(in crate::desktop) fn reload_projects(&self) {
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

    fn selected_activity_id(&self) -> Option<ActivityId> {
        let selected = self.imp().activity_dropdown.selected();
        if selected == 0 || selected == gtk::INVALID_LIST_POSITION {
            return None;
        }
        usize::try_from(selected.saturating_sub(1))
            .ok()
            .and_then(|index| {
                self.imp()
                    .activities
                    .borrow()
                    .get(index)
                    .map(|activity| activity.id)
            })
    }

    pub(in crate::desktop) fn reload_activities(&self) {
        let Some(handle) = self.handle() else { return };
        let selected_id = self.selected_activity_id();
        let activities = handle.activities(false).unwrap_or_default();
        let selected = selected_id
            .and_then(|id| activities.iter().position(|activity| activity.id == id))
            .and_then(|index| u32::try_from(index + 1).ok())
            .unwrap_or(0);
        let names = std::iter::once(tr("No activity").to_owned())
            .chain(activities.iter().map(|activity| activity.name.clone()))
            .collect::<Vec<_>>();
        let name_refs = names.iter().map(String::as_str).collect::<Vec<_>>();
        self.imp().updating_activity_dropdown.set(true);
        self.imp().activities.replace(activities);
        self.imp()
            .activity_dropdown
            .set_model(Some(&gtk::StringList::new(&name_refs)));
        self.imp().activity_dropdown.set_selected(selected);
        self.imp().updating_activity_dropdown.set(false);
    }

    pub(in crate::desktop) fn update_active_details(&self) {
        let Some(handle) = self.handle() else { return };
        let Ok(snapshot) = handle.snapshot() else {
            return;
        };
        if matches!(
            snapshot.state,
            TrackerState::Running(_) | TrackerState::Paused(_)
        ) && let Err(error) = handle.apply(TrackerCommand::EditActive {
            project_id: self.selected_project_id(),
            activity_id: self.selected_activity_id(),
            note: self.imp().note_entry.text().to_string(),
        }) {
            self.show_database_error(&error.to_string());
        }
    }

    /// Advances the timer one step: starts, pauses, or resumes it.
    /// Finishing stays explicit through the Finish button.
    pub fn toggle_timer(&self) {
        let Some(state) = self.timer_state_or_review() else {
            return;
        };
        let command = match state {
            TrackerState::Stopped => TrackerCommand::Start {
                project_id: self.selected_project_id(),
                activity_id: self.selected_activity_id(),
                note: self.imp().note_entry.text().to_string(),
            },
            TrackerState::Running(_) => TrackerCommand::Pause,
            TrackerState::Paused(_) => TrackerCommand::Resume,
            TrackerState::IdlePending(_) | TrackerState::RecoveryPending(_) => return,
        };
        self.apply_timer_command(command);
    }

    pub(in crate::desktop) fn start_timer_from_note(&self) {
        let Some(state) = self.timer_state_or_review() else {
            return;
        };
        if !matches!(state, TrackerState::Stopped) {
            return;
        }
        self.apply_timer_command(TrackerCommand::Start {
            project_id: self.selected_project_id(),
            activity_id: self.selected_activity_id(),
            note: self.imp().note_entry.text().to_string(),
        });
    }

    /// Pauses a running timer or resumes a paused one.
    pub fn toggle_pause(&self) {
        if let Some(state) = self.timer_state_or_review() {
            self.pause_or_resume_state(&state);
        }
    }

    /// Pauses or resumes for the top bar. Unlike `toggle_pause`, leaves
    /// pending reviews alone: the window may be hidden, so they wait for
    /// `review_pending`.
    pub(in crate::desktop) fn pause_or_resume(&self) {
        if let Some(state) = self.timer_state() {
            self.pause_or_resume_state(&state);
        }
    }

    /// Pauses when running, resumes when paused; ignores every other state.
    fn pause_or_resume_state(&self, state: &TrackerState) {
        let command = match state {
            TrackerState::Running(_) => TrackerCommand::Pause,
            TrackerState::Paused(_) => TrackerCommand::Resume,
            _ => return,
        };
        self.apply_timer_command(command);
    }

    /// Opens the review a pending idle period or interrupted timer needs,
    /// unless a dialog is already showing.
    pub(in crate::desktop) fn review_pending(&self) {
        if self.visible_dialog().is_some() {
            return;
        }
        match self.timer_state() {
            Some(TrackerState::IdlePending(_)) => self.review_idle(),
            Some(TrackerState::RecoveryPending(_)) => self.show_recovery_dialog(),
            _ => {}
        }
    }

    /// Finishes the running or paused timer, saving its time.
    pub fn finish_timer(&self) {
        let Some(state) = self.timer_state_or_review() else {
            return;
        };
        if !matches!(state, TrackerState::Running(_) | TrackerState::Paused(_)) {
            return;
        }
        self.apply_timer_command(TrackerCommand::Stop);
    }

    /// Returns the current state, routing pending reviews to their dialogs.
    fn timer_state_or_review(&self) -> Option<TrackerState> {
        match self.timer_state()? {
            TrackerState::IdlePending(_) => {
                self.review_idle();
                None
            }
            TrackerState::RecoveryPending(_) => {
                self.show_recovery_dialog();
                None
            }
            state => Some(state),
        }
    }

    /// Returns the current state, reporting a failed read.
    fn timer_state(&self) -> Option<TrackerState> {
        let handle = self.handle()?;
        match handle.snapshot() {
            Ok(snapshot) => Some(snapshot.state),
            Err(error) => {
                self.show_database_error(&error.to_string());
                None
            }
        }
    }

    fn apply_timer_command(&self, command: TrackerCommand) {
        let Some(handle) = self.handle() else { return };
        let clears_note = matches!(command, TrackerCommand::Stop);
        match handle.apply(command) {
            Ok(_) => {
                if clears_note {
                    self.imp().note_entry.set_text("");
                }
                self.refresh();
            }
            Err(error) => self.show_database_error(&error.to_string()),
        }
    }

    pub(in crate::desktop) fn continue_entry(&self, entry: &TimeEntry) {
        let Some(handle) = self.handle() else { return };
        let Some(entry_id) = entry.id else { return };
        match handle.snapshot().map(|snapshot| snapshot.state) {
            Ok(TrackerState::Stopped | TrackerState::Running(_) | TrackerState::Paused(_)) => {}
            Ok(TrackerState::IdlePending(_)) => {
                self.review_idle();
                return;
            }
            Ok(TrackerState::RecoveryPending(_)) => {
                self.show_recovery_dialog();
                return;
            }
            Err(error) => {
                self.show_database_error(&error.to_string());
                return;
            }
        }
        match handle.continue_entry(entry_id) {
            Ok(_) => self.refresh(),
            Err(error) => self.show_database_error(&error.to_string()),
        }
    }

    /// Delay before the next clock redraw: aligned to the cumulative
    /// counter's next second while time is counting in the focused window,
    /// or its next minute when unfocused, otherwise the next local midnight,
    /// when the day view rolls over.
    pub(in crate::desktop) fn next_clock_tick(&self) -> Duration {
        let Some(handle) = self.handle() else {
            return MINUTE;
        };
        match handle.snapshot().map(|snapshot| snapshot.state) {
            Ok(
                TrackerState::Running(active)
                | TrackerState::IdlePending(houra_core::PendingIdle { active, .. }),
            ) => {
                let until_next = if self.is_active() {
                    until_next_second
                } else {
                    until_next_minute
                };
                if !self.imp().active_entry_duration_cached.get()
                    || self.imp().active_entry_id.get() != active.entry_id
                {
                    self.refresh_active_entry_duration();
                }
                handle.live_elapsed().map_or(MINUTE, |elapsed| {
                    let counted = counted_live_elapsed(elapsed, active.accumulated_ms);
                    let total = active_entry_total_duration(
                        self.imp().active_entry_saved_ms.get(),
                        counted,
                    );
                    until_next(total)
                })
            }
            // At most an hour: monotonic timers stop during suspend, and time
            // zone or DST changes move midnight.
            _ => until_local_midnight(Local::now()).min(HOUR),
        }
    }

    pub(in crate::desktop) fn refresh_timer_only(&self) {
        let Some(handle) = self.handle() else { return };
        let Ok(snapshot) = handle.snapshot() else {
            return;
        };
        match snapshot.state {
            TrackerState::Running(active)
            | TrackerState::IdlePending(houra_core::PendingIdle { active, .. }) => {
                let elapsed = handle.live_elapsed().unwrap_or_else(|_| {
                    Duration::from_secs(
                        u64::try_from(
                            chrono::Utc::now()
                                .timestamp_millis()
                                .saturating_sub(active.start_ms)
                                .max(0)
                                / 1_000,
                        )
                        .unwrap_or(0),
                    )
                });
                let now = Local::now();
                let midnight_ms = now
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .and_then(|value| Local.from_local_datetime(&value).earliest())
                    .map_or(active.start_ms, |value| value.timestamp_millis());
                let live_today = u64::try_from(
                    now.timestamp_millis()
                        .saturating_sub(active.start_ms.max(midnight_ms))
                        .max(0)
                        / 1_000,
                )
                .unwrap_or(0);
                let counted = counted_live_elapsed(elapsed, active.accumulated_ms);
                self.render_active_timer(&active, counted, live_today, false);
            }
            TrackerState::Paused(paused) => {
                // The frozen segment is already banked, so the saved entry
                // includes it; nothing live counts while paused.
                self.render_active_timer(&paused.active, Duration::ZERO, 0, true);
            }
            TrackerState::RecoveryPending(_) => {
                self.imp().timer_label.set_text(tr("Review"));
                self.imp().tracking_eyebrow.set_visible(false);
                self.imp().stopped_panel.set_visible(false);
                self.imp().running_panel.set_visible(true);
                self.show_timer_actions(false);
            }
            TrackerState::Stopped => {
                self.clear_active_entry_duration();
                self.imp().stopped_panel.set_visible(true);
                self.imp().running_panel.set_visible(false);
            }
        }
    }

    /// Renders the running panel for a live or frozen timer. The big counter
    /// shows the entry's saved time plus the unsaved live interval (`counted`);
    /// `live_today` feeds the day total instead.
    fn render_active_timer(
        &self,
        active: &houra_core::ActiveTimer,
        counted: Duration,
        live_today: u64,
        paused: bool,
    ) {
        if !self.imp().active_entry_duration_cached.get()
            || self.imp().active_entry_id.get() != active.entry_id
        {
            self.refresh_active_entry_duration();
        }
        let total = active_entry_total_duration(self.imp().active_entry_saved_ms.get(), counted);
        self.imp()
            .timer_label
            .set_markup(&crate::desktop::widgets::format_clock_markup(
                total.as_secs(),
                self.is_active(),
            ));
        self.imp().tracking_eyebrow.set_visible(true);
        self.imp().stopped_panel.set_visible(false);
        self.imp().running_panel.set_visible(true);
        self.set_active_labels(active);
        self.show_timer_actions(paused);
        self.update_live_total(live_today);
    }

    /// Swaps the running panel between tracking and paused presentation.
    /// The two-button row stays put; only labels and emphasis change.
    fn show_timer_actions(&self, paused: bool) {
        self.imp().tracking_eyebrow.set_label(if paused {
            tr("PAUSED")
        } else {
            tr("TRACKING NOW")
        });
        self.imp()
            .pause_button
            .set_label(if paused { tr("Resume") } else { tr("Pause") });
        if paused {
            self.imp().pause_button.add_css_class("suggested-action");
            self.imp().stop_button.remove_css_class("suggested-action");
            self.imp().timer_label.add_css_class("timer-paused");
        } else {
            self.imp().pause_button.remove_css_class("suggested-action");
            self.imp().stop_button.add_css_class("suggested-action");
            self.imp().timer_label.remove_css_class("timer-paused");
        }
    }

    fn set_active_labels(&self, active: &houra_core::ActiveTimer) {
        self.imp()
            .active_note_label
            .set_label(if active.note.is_empty() {
                tr("Tracked work")
            } else {
                &active.note
            });
        let projects = self.imp().projects.borrow();
        let activities = self.imp().activities.borrow();
        let project = projects
            .iter()
            .find(|project| project.id == active.project_id)
            .map_or(tr("Missing project"), |project| project.name.as_str());
        let activity = active
            .activity_id
            .and_then(|id| activities.iter().find(|activity| activity.id == id))
            .map(|activity| activity.name.as_str());
        self.imp()
            .active_meta_label
            .set_label(&activity.map_or_else(
                || project.to_owned(),
                |activity| {
                    trf(
                        "{project} · {activity}",
                        &[("project", project), ("activity", activity)],
                    )
                },
            ));
    }

    fn update_live_total(&self, elapsed: u64) {
        if self.imp().selected_day_offset.get() != 0 {
            return;
        }
        let stored = self.imp().stored_day_seconds.get();
        self.imp()
            .total_value
            .set_label(&crate::desktop::widgets::format_duration(
                stored.saturating_add(elapsed),
            ));
    }

    pub(in crate::desktop) fn show_active_editor(&self) {
        let Some(handle) = self.handle() else { return };
        let Ok(snapshot) = handle.snapshot() else {
            return;
        };
        let Some(active) = snapshot.state.active().cloned() else {
            return;
        };
        let projects = self.imp().projects.borrow().clone();
        let activities = self.imp().activities.borrow().clone();
        let dialog = adw::Dialog::builder()
            .title(tr("Edit active timer"))
            .content_width(440)
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
        project.set_selected(
            projects
                .iter()
                .position(|item| item.id == active.project_id)
                .and_then(|index| u32::try_from(index).ok())
                .unwrap_or(0),
        );
        let mut activity_names = vec![tr("No activity")];
        activity_names.extend(activities.iter().map(|activity| activity.name.as_str()));
        let activity = gtk::DropDown::from_strings(&activity_names);
        activity.set_selected(
            active
                .activity_id
                .and_then(|id| activities.iter().position(|item| item.id == id))
                .and_then(|index| u32::try_from(index + 1).ok())
                .unwrap_or(0),
        );
        let note = gtk::Entry::builder()
            .text(active.note)
            .placeholder_text(tr("What are you working on?"))
            .build();
        for (label, widget) in [
            (tr("Project"), project.clone().upcast::<gtk::Widget>()),
            (tr("Activity"), activity.clone().upcast()),
            (tr("Note"), note.clone().upcast()),
        ] {
            content.append(
                &gtk::Label::builder()
                    .label(label)
                    .halign(gtk::Align::Start)
                    .build(),
            );
            content.append(&widget);
        }
        let save = gtk::Button::with_label(tr("Save changes"));
        save.add_css_class("suggested-action");
        content.append(&save);
        dialog.set_child(Some(&content));
        let weak = self.downgrade();
        let dialog_to_close = dialog.clone();
        save.connect_clicked(move |_| {
            let Some(window) = weak.upgrade() else { return };
            let project_id = usize::try_from(project.selected())
                .ok()
                .and_then(|index| projects.get(index))
                .map_or(ProjectId(1), |item| item.id);
            let activity_id = usize::try_from(activity.selected())
                .ok()
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| activities.get(index))
                .map(|item| item.id);
            match handle.apply(TrackerCommand::EditActive {
                project_id,
                activity_id,
                note: note.text().to_string(),
            }) {
                Ok(_) => {
                    dialog_to_close.close();
                    window.refresh();
                }
                Err(error) => window.show_database_error(&error.to_string()),
            }
        });
        dialog.present(Some(self));
    }

    pub(in crate::desktop) fn show_date_chooser(&self) {
        let current = self
            .imp()
            .date_popover
            .borrow()
            .as_ref()
            .and_then(glib::WeakRef::upgrade);
        if let Some(current) = current {
            current.popdown();
            return;
        }
        let today = Local::now().date_naive();
        let selected = today
            .checked_add_signed(chrono::Duration::days(i64::from(
                self.imp().selected_day_offset.get(),
            )))
            .unwrap_or(today);
        let calendar = gtk::Calendar::new();
        if let Ok(value) = glib::DateTime::new(
            &glib::TimeZone::local(),
            selected.year(),
            i32::try_from(selected.month()).unwrap_or(1),
            i32::try_from(selected.day()).unwrap_or(1),
            12,
            0,
            0.0,
        ) {
            calendar.select_day(&value);
        }
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .margin_top(14)
            .margin_bottom(14)
            .margin_start(14)
            .margin_end(14)
            .build();
        content.append(
            &gtk::Label::builder()
                .label(tr("Choose date"))
                .halign(gtk::Align::Start)
                .css_classes(["heading"])
                .build(),
        );
        content.append(&calendar);
        let hint = gtk::Label::builder()
            .label(tr("Only today and past dates can be chosen."))
            .halign(gtk::Align::Start)
            .css_classes(["dim-label", "date-chooser-hint"])
            .wrap(true)
            .build();
        content.append(&hint);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let footer = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let choose = gtk::Button::with_label(tr("Choose"));
        choose.add_css_class("suggested-action");
        let spacer = gtk::Box::builder().hexpand(true).build();
        footer.append(&spacer);
        footer.append(&choose);
        content.append(&footer);
        let sync_state = glib::clone!(
            #[weak]
            choose,
            #[weak]
            hint,
            move |calendar: &gtk::Calendar| {
                let allowed = calendar_selected_date(calendar).is_some_and(|date| {
                    crate::date_navigation::date_can_be_selected(date, Local::now().date_naive())
                });
                choose.set_sensitive(allowed);
                hint.set_visible(!allowed);
            }
        );
        sync_state(&calendar);
        calendar.connect_day_selected(sync_state);
        let popover = gtk::Popover::builder()
            .child(&content)
            .position(gtk::PositionType::Bottom)
            .autohide(true)
            .build();
        popover.set_parent(&self.imp().day_button.get());
        self.imp().date_popover.replace(Some(popover.downgrade()));
        // Weak everywhere below: the content closures must not keep the popover
        // alive, or the button warns about leftover children when torn down.
        let weak_window = self.downgrade();
        popover.connect_closed(move |popover| {
            popover.unparent();
            if let Some(window) = weak_window.upgrade() {
                window.imp().date_popover.take();
            }
        });
        let weak = self.downgrade();
        let popover_weak = popover.downgrade();
        let calendar_for_choose = calendar.clone();
        choose.connect_clicked(move |_| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            if let Some(date) = calendar_selected_date(&calendar_for_choose) {
                window.apply_chosen_date(date);
            }
            if let Some(popover) = popover_weak.upgrade() {
                popover.popdown();
            }
        });
        let weak = self.downgrade();
        let popover_weak = popover.downgrade();
        let calendar_for_double_click = calendar.clone();
        let double_click = gtk::GestureClick::new();
        double_click.set_button(1);
        double_click.connect_pressed(move |_, n_press, _, _| {
            if n_press != 2 {
                return;
            }
            let Some(window) = weak.upgrade() else {
                return;
            };
            if let Some(date) = calendar_selected_date(&calendar_for_double_click) {
                window.apply_chosen_date(date);
                if let Some(popover) = popover_weak.upgrade() {
                    popover.popdown();
                }
            }
        });
        calendar.add_controller(double_click);
        popover.popup();
    }

    fn apply_chosen_date(&self, date: chrono::NaiveDate) {
        let today = Local::now().date_naive();
        if !crate::date_navigation::date_can_be_selected(date, today) {
            return;
        }
        let offset = date.signed_duration_since(today).num_days();
        self.imp()
            .selected_day_offset
            .set(i32::try_from(offset).unwrap_or(0));
        if let Some(week_offset) = crate::date_navigation::week_offset_for_date(date, today) {
            self.imp().visible_week_offset.set(week_offset);
        }
        self.refresh_entries();
    }
}

fn calendar_selected_date(calendar: &gtk::Calendar) -> Option<chrono::NaiveDate> {
    let selected = calendar.date();
    chrono::NaiveDate::from_ymd_opt(
        selected.year(),
        u32::try_from(selected.month()).unwrap_or(1),
        u32::try_from(selected.day_of_month()).unwrap_or(1),
    )
}

const SECOND: Duration = Duration::from_secs(1);
const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(3_600);

/// Time until the cumulative entry counter reaches its next whole minute,
/// so the unfocused window flips on the counter's own boundary.
fn until_next_minute(elapsed: Duration) -> Duration {
    let minute_ms = MINUTE.as_millis();
    let into_minute = elapsed.as_millis() % minute_ms;
    Duration::from_millis(u64::try_from(minute_ms - into_minute).unwrap_or(60_000))
}

/// Time until the cumulative entry counter reaches its next whole second,
/// so the focused window's seconds flip together with the counter.
fn until_next_second(elapsed: Duration) -> Duration {
    let second_ms = SECOND.as_millis();
    let into_second = elapsed.as_millis() % second_ms;
    Duration::from_millis(u64::try_from(second_ms - into_second).unwrap_or(1_000))
}

/// Time until the next local midnight; one hour when that midnight doesn't
/// exist, as when a DST change skips it.
fn until_local_midnight(now: chrono::DateTime<Local>) -> Duration {
    now.date_naive()
        .succ_opt()
        .and_then(|tomorrow| tomorrow.and_hms_opt(0, 0, 0))
        .and_then(|midnight| Local.from_local_datetime(&midnight).earliest())
        .and_then(|midnight| (midnight - now).to_std().ok())
        .unwrap_or(HOUR)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::{Local, TimeZone};

    use super::{until_local_midnight, until_next_minute, until_next_second};
    use crate::desktop::widgets::active_entry_total_duration;

    #[test]
    fn clock_ticks_land_on_the_next_whole_minute() {
        for (elapsed_ms, delay_ms) in [
            (0, 60_000),
            (1, 59_999),
            (59_999, 1),
            (90_500, 29_500),
            (3_600_000, 60_000),
        ] {
            assert_eq!(
                until_next_minute(Duration::from_millis(elapsed_ms)),
                Duration::from_millis(delay_ms),
                "elapsed {elapsed_ms} ms"
            );
        }
    }

    #[test]
    fn focused_clock_ticks_land_on_the_next_whole_second() {
        for (elapsed_ms, delay_ms) in [
            (0, 1_000),
            (1, 999),
            (999, 1),
            (90_500, 500),
            (3_600_000, 1_000),
        ] {
            assert_eq!(
                until_next_second(Duration::from_millis(elapsed_ms)),
                Duration::from_millis(delay_ms),
                "elapsed {elapsed_ms} ms"
            );
        }
    }

    #[test]
    fn idle_clock_waits_for_the_next_local_midnight() {
        let at = |hour, minute, second| {
            Local
                .with_ymd_and_hms(2026, 1, 15, hour, minute, second)
                .single()
                .unwrap_or_else(|| panic!("{hour}:{minute}:{second} is not a single local time"))
        };
        assert_eq!(
            until_local_midnight(at(23, 59, 30)),
            Duration::from_secs(30)
        );
        assert_eq!(
            until_local_midnight(at(0, 0, 0)),
            Duration::from_secs(24 * 3_600)
        );
    }

    #[test]
    fn active_entry_total_truncates_fractional_seconds_for_display() {
        assert_eq!(
            active_entry_total_duration(3_075_500, Duration::from_millis(44_499)).as_secs(),
            3_119
        );
        assert_eq!(
            active_entry_total_duration(3_075_500, Duration::from_millis(44_500)).as_secs(),
            3_120
        );
    }

    #[test]
    fn cumulative_counter_schedules_second_and_minute_ticks() {
        let to_second = active_entry_total_duration(45_500, Duration::from_millis(250));
        assert_eq!(until_next_second(to_second), Duration::from_millis(250));
        let to_minute = active_entry_total_duration(45_500, Duration::from_millis(14_250));
        assert_eq!(until_next_minute(to_minute), Duration::from_millis(250));
    }
}
