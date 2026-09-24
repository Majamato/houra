use std::collections::BTreeMap;

use chrono::{Local, NaiveDate, TimeZone};
use gtk::prelude::*;
use houra_core::{EntryId, EntrySource, TimeEntry, TrackerState};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::window::MainWindow;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReportSession {
    start_ms: i64,
    end_ms: i64,
    source: Option<EntrySource>,
    provisional: bool,
}

fn split_sessions(
    sessions: impl IntoIterator<Item = ReportSession>,
    date_at: impl Fn(i64) -> Option<NaiveDate>,
    next_midnight: impl Fn(NaiveDate) -> Option<i64>,
) -> BTreeMap<NaiveDate, Vec<ReportSession>> {
    let mut days = BTreeMap::<NaiveDate, Vec<ReportSession>>::new();
    for session in sessions {
        let mut start = session.start_ms;
        while start < session.end_ms {
            let Some(date) = date_at(start) else { break };
            let Some(midnight) = next_midnight(date) else {
                break;
            };
            if midnight <= start {
                break;
            }
            let end = session.end_ms.min(midnight);
            days.entry(date).or_default().push(ReportSession {
                start_ms: start,
                end_ms: end,
                ..session.clone()
            });
            start = end;
        }
    }
    for sessions in days.values_mut() {
        sessions.sort_by_key(|session| (session.start_ms, session.end_ms));
    }
    days
}

fn report_sessions(entry: &TimeEntry, state: &TrackerState, now_ms: i64) -> Vec<ReportSession> {
    let mut sessions = entry
        .intervals
        .iter()
        .map(|interval| ReportSession {
            start_ms: interval.start_ms,
            end_ms: interval.end_ms,
            source: Some(interval.source),
            provisional: false,
        })
        .collect::<Vec<_>>();
    if state.active().and_then(|active| active.entry_id) == entry.id && entry.id.is_some() {
        let active = state.active();
        if let Some(active) = active {
            let (end_ms, provisional) = match state {
                TrackerState::Running(_) => (now_ms, false),
                TrackerState::IdlePending(pending) => (pending.return_ms.unwrap_or(now_ms), true),
                TrackerState::RecoveryPending(pending) => (pending.proposed_end_ms, true),
                TrackerState::Stopped => (active.start_ms, false),
            };
            if end_ms > active.start_ms {
                sessions.push(ReportSession {
                    start_ms: active.start_ms,
                    end_ms,
                    source: None,
                    provisional,
                });
            }
        }
    }
    sessions
}

fn local_days(sessions: Vec<ReportSession>) -> BTreeMap<NaiveDate, Vec<ReportSession>> {
    split_sessions(
        sessions,
        |ms| {
            Local
                .timestamp_millis_opt(ms)
                .single()
                .map(|time| time.date_naive())
        },
        |date| {
            Local
                .from_local_datetime(&date.succ_opt()?.and_hms_opt(0, 0, 0)?)
                .earliest()
                .map(|time| time.timestamp_millis())
        },
    )
}

fn elapsed(ms: i64) -> String {
    let seconds = ms.max(0) / 1_000;
    let millis = ms.max(0) % 1_000;
    let base = format!(
        "{}h {:02}m {:02}s",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    );
    if millis == 0 {
        base
    } else {
        format!("{base} {millis:03}ms")
    }
}

fn timestamp(ms: i64) -> String {
    Local.timestamp_millis_opt(ms).single().map_or_else(
        || "Unknown time".into(),
        |time| time.format("%b %-d, %Y at %-I:%M:%S %p").to_string(),
    )
}

fn time_of_day(ms: i64) -> String {
    Local.timestamp_millis_opt(ms).single().map_or_else(
        || "Unknown time".into(),
        |time| time.format("%-I:%M:%S %p").to_string(),
    )
}

fn text_line(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Start)
        .wrap(true)
        .selectable(true)
        .build()
}

fn append_report(
    content: &gtk::Box,
    entry: &TimeEntry,
    state: &TrackerState,
    now_ms: i64,
    project: &str,
    activity: &str,
) {
    while let Some(child) = content.first_child() {
        content.remove(&child);
    }
    let sessions = report_sessions(entry, state, now_ms);
    let days = local_days(sessions);
    let total = days.values().flatten().fold(0_i64, |sum, segment| {
        sum.saturating_add(segment.end_ms.saturating_sub(segment.start_ms))
    });
    let note = if entry.note.is_empty() {
        "Tracked work"
    } else {
        &entry.note
    };
    let heading = text_line(note);
    heading.add_css_class("title-2");
    heading.set_selectable(false);
    content.append(&heading);
    for line in [
        format!("Project: {project}"),
        format!("Activity: {activity}"),
        format!("Created: {}", timestamp(entry.created_at_ms)),
        format!("Last updated: {}", timestamp(entry.updated_at_ms)),
        format!("Total duration: {}", elapsed(total)),
        format!(
            "Latest saved stop: {}",
            entry
                .latest_end_ms()
                .map_or_else(|| "None".into(), timestamp)
        ),
    ] {
        content.append(&text_line(&line));
    }
    if days.is_empty() {
        content.append(&text_line("No recorded sessions"));
    }
    for (day, segments) in days.iter().rev() {
        let total = segments.iter().fold(0_i64, |sum, segment| {
            sum.saturating_add(segment.end_ms.saturating_sub(segment.start_ms))
        });
        let title = text_line(&format!(
            "{} · {}",
            day.format("%A, %B %-d, %Y"),
            elapsed(total)
        ));
        title.add_css_class("heading");
        title.set_margin_top(16);
        content.append(&title);
        for segment in segments {
            let source = match segment.source {
                Some(EntrySource::Timer) => "Timer",
                Some(EntrySource::Manual) => "Manual",
                Some(EntrySource::IdleReassignment) => "Idle reassignment",
                Some(EntrySource::Recovery) => "Recovery",
                None if segment.provisional => "Current session · provisional, awaiting review",
                None => "Current session · ongoing",
            };
            let line = text_line(&format!(
                "{} – {} · {} · {source}",
                time_of_day(segment.start_ms),
                time_of_day(segment.end_ms),
                elapsed(segment.end_ms.saturating_sub(segment.start_ms))
            ));
            line.set_margin_start(12);
            content.append(&line);
        }
    }
}

impl MainWindow {
    pub(in crate::desktop) fn show_entry_report(&self, id: EntryId) {
        let Some(handle) = self.handle() else { return };
        if let Err(error) = handle.entry(id) {
            self.show_database_error(&error.to_string());
            return;
        }
        let dialog = adw::Dialog::builder()
            .title("Task report")
            .content_width(540)
            .content_height(600)
            .build();
        let toolbar = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        toolbar.add_top_bar(&header);
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_top(18)
            .margin_bottom(18)
            .margin_start(24)
            .margin_end(24)
            .build();
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&content)
            .build();
        scroll.set_focusable(true);
        toolbar.set_content(Some(&scroll));
        dialog.set_child(Some(&toolbar));
        let update = {
            let handle = handle.clone();
            let content = content.clone();
            let scroll = scroll.clone();
            move || {
                if let (Ok(entry), Ok(snapshot)) = (handle.entry(id), handle.snapshot()) {
                    let position = scroll.vadjustment().value();
                    let project = handle
                        .projects(true)
                        .ok()
                        .and_then(|items| {
                            items.into_iter().find(|item| item.id == entry.project_id)
                        })
                        .map_or_else(|| "Missing project".to_string(), |item| item.name);
                    let activity = entry
                        .activity_id
                        .and_then(|id| {
                            handle
                                .activities(true)
                                .ok()?
                                .into_iter()
                                .find(|item| item.id == id)
                        })
                        .map_or_else(|| "None".to_string(), |item| item.name);
                    append_report(
                        &content,
                        &entry,
                        &snapshot.state,
                        Local::now().timestamp_millis(),
                        &project,
                        &activity,
                    );
                    let adjustment = scroll.vadjustment();
                    glib::idle_add_local_once(move || adjustment.set_value(position));
                }
            }
        };
        update();
        let weak = dialog.downgrade();
        glib::timeout_add_seconds_local(1, move || {
            if weak.upgrade().is_none() {
                return glib::ControlFlow::Break;
            }
            update();
            glib::ControlFlow::Continue
        });
        dialog.present(Some(self));
        scroll.grab_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;
    use houra_core::{ActiveTimer, PendingIdle, PendingRecovery, ProjectId, TrackedInterval};

    #[test]
    fn report_times_are_readable_without_timezone_labels() {
        let at = Local
            .with_ymd_and_hms(2026, 9, 24, 14, 5, 6)
            .single()
            .expect("midafternoon local time should exist");
        assert_eq!(
            timestamp(at.timestamp_millis()),
            "Sep 24, 2026 at 2:05:06 PM"
        );
        assert_eq!(time_of_day(at.timestamp_millis()), "2:05:06 PM");
    }

    fn entry(intervals: Vec<TrackedInterval>) -> TimeEntry {
        TimeEntry {
            id: Some(EntryId(1)),
            project_id: ProjectId(1),
            activity_id: None,
            note: String::new(),
            intervals,
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }
    fn interval(start_ms: i64, end_ms: i64) -> TrackedInterval {
        TrackedInterval {
            id: None,
            start_ms,
            end_ms,
            source: EntrySource::Timer,
        }
    }
    #[test]
    fn multiple_sessions_split_at_midnight_and_totals_match() {
        let entry = entry(vec![
            interval(10, 20),
            interval(90, 110),
            interval(120, 130),
        ]);
        let days = split_sessions(
            report_sessions(&entry, &TrackerState::Stopped, 0),
            |ms| NaiveDate::from_ymd_opt(2026, 1, if ms < 100 { 1 } else { 2 }),
            |date| Some(if date.day() == 1 { 100 } else { 200 }),
        );
        assert_eq!(days.len(), 2);
        assert_eq!(days.values().map(|day| day.len()).sum::<usize>(), 4);
        assert_eq!(
            days.values()
                .flatten()
                .map(|part| part.end_ms - part.start_ms)
                .sum::<i64>(),
            entry.duration_ms()
        );
    }
    #[test]
    fn daylight_saving_day_uses_its_actual_midnight_boundary() {
        // A 23-hour day ends at 82,800,000 ms, not 86,400,000 ms.
        let sessions = vec![ReportSession {
            start_ms: 82_790_000,
            end_ms: 82_810_000,
            source: Some(EntrySource::Timer),
            provisional: false,
        }];
        let days = split_sessions(
            sessions,
            |ms| NaiveDate::from_ymd_opt(2026, 3, if ms < 82_800_000 { 8 } else { 9 }),
            |date| {
                Some(if date.day() == 8 {
                    82_800_000
                } else {
                    169_200_000
                })
            },
        );
        assert_eq!(
            days.values()
                .map(|day| day
                    .iter()
                    .map(|part| part.end_ms - part.start_ms)
                    .sum::<i64>())
                .collect::<Vec<_>>(),
            vec![10_000, 10_000]
        );
    }
    #[test]
    fn fall_back_day_can_last_twenty_five_hours() {
        let sessions = vec![ReportSession {
            start_ms: 89_990_000,
            end_ms: 90_010_000,
            source: Some(EntrySource::Timer),
            provisional: false,
        }];
        let days = split_sessions(
            sessions,
            |ms| NaiveDate::from_ymd_opt(2026, 11, if ms < 90_000_000 { 1 } else { 2 }),
            |date| {
                Some(if date.day() == 1 {
                    90_000_000
                } else {
                    176_400_000
                })
            },
        );
        assert_eq!(
            days.values()
                .flatten()
                .map(|part| part.end_ms - part.start_ms)
                .sum::<i64>(),
            20_000
        );
        assert_eq!(days.len(), 2);
    }
    #[test]
    fn pending_sessions_are_provisional() {
        let active = ActiveTimer {
            entry_id: Some(EntryId(1)),
            project_id: ProjectId(1),
            activity_id: None,
            note: String::new(),
            start_ms: 100,
            started_monotonic_ms: 0,
            last_heartbeat_ms: 100,
        };
        let current = entry(vec![]);
        let idle = TrackerState::IdlePending(PendingIdle {
            active: active.clone(),
            idle_start_ms: 120,
            return_ms: Some(150),
        });
        let recovery = TrackerState::RecoveryPending(PendingRecovery {
            active,
            proposed_end_ms: 140,
            unresolved_idle_start_ms: None,
        });
        assert!(report_sessions(&current, &idle, 200)[0].provisional);
        assert_eq!(report_sessions(&current, &idle, 200)[0].end_ms, 150);
        assert!(report_sessions(&current, &recovery, 200)[0].provisional);
        assert_eq!(report_sessions(&current, &recovery, 200)[0].end_ms, 140);
    }
    #[test]
    fn current_session_becomes_saved_after_stop() {
        let active = ActiveTimer {
            entry_id: Some(EntryId(1)),
            project_id: ProjectId(1),
            activity_id: None,
            note: String::new(),
            start_ms: 100,
            started_monotonic_ms: 0,
            last_heartbeat_ms: 100,
        };
        let current = entry(vec![interval(0, 50)]);
        let running = report_sessions(&current, &TrackerState::Running(active), 150);
        assert_eq!(running.len(), 2);
        assert_eq!(running[1].source, None);
        let stopped = entry(vec![interval(0, 50), interval(100, 150)]);
        assert_eq!(
            report_sessions(&stopped, &TrackerState::Stopped, 200)
                .iter()
                .map(|s| s.end_ms - s.start_ms)
                .sum::<i64>(),
            100
        );
        assert!(
            report_sessions(&stopped, &TrackerState::Stopped, 200)
                .iter()
                .all(|s| s.source.is_some())
        );
    }
}
