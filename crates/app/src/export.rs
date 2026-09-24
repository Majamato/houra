use std::fs;
use std::io::Write;
use std::path::Path;

use chrono::{Local, NaiveDate, TimeZone};
use houra_core::{Activity, EntrySource, Project, TimeEntry};

use crate::AppError;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReportMode {
    #[default]
    Tasks,
    Full,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkedInterval {
    pub start_ms: i64,
    pub end_ms: i64,
    pub source: EntrySource,
}

impl WorkedInterval {
    pub fn duration_ms(&self) -> i64 {
        self.end_ms - self.start_ms
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskReport<'a> {
    pub entry: &'a TimeEntry,
    pub intervals: Vec<WorkedInterval>,
    pub duration_ms: i64,
    pub first_date: NaiveDate,
    pub last_date: NaiveDate,
}

/// One report per task, using only work inside the selected half-open week.
pub fn weekly_tasks(entries: &[TimeEntry], week: (i64, i64)) -> Vec<TaskReport<'_>> {
    entries
        .iter()
        .filter_map(|entry| {
            let intervals = entry
                .intervals
                .iter()
                .filter_map(|interval| {
                    let start_ms = interval.start_ms.max(week.0);
                    let end_ms = interval.end_ms.min(week.1);
                    (start_ms < end_ms).then_some(WorkedInterval {
                        start_ms,
                        end_ms,
                        source: interval.source,
                    })
                })
                .collect::<Vec<_>>();
            let first_ms = intervals.iter().map(|interval| interval.start_ms).min()?;
            let last_ms = intervals.iter().map(|interval| interval.end_ms - 1).max()?;
            let first_date = Local.timestamp_millis_opt(first_ms).single()?.date_naive();
            let last_date = Local.timestamp_millis_opt(last_ms).single()?.date_naive();
            let duration_ms = intervals.iter().fold(0_i64, |total, interval| {
                total.saturating_add(interval.duration_ms())
            });
            Some(TaskReport {
                entry,
                intervals,
                duration_ms,
                first_date,
                last_date,
            })
        })
        .collect()
}

pub fn date(date: NaiveDate) -> String {
    date.format("%b %-d, %Y").to_string()
}

pub fn date_range(first: NaiveDate, last: NaiveDate) -> String {
    if first == last {
        date(first)
    } else {
        format!("{} – {}", date(first), date(last))
    }
}

pub fn local_datetime(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map_or_else(String::new, |time| {
            time.format("%b %-d, %Y %-I:%M %p").to_string()
        })
}

fn seconds(ms: i64) -> String {
    if ms % 1_000 == 0 {
        (ms / 1_000).to_string()
    } else {
        format!("{}.{:03}", ms / 1_000, ms % 1_000)
    }
}

fn hours_minutes(ms: i64) -> String {
    let minutes = ms / 60_000;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

pub fn duration(ms: i64) -> String {
    let seconds = ms / 1_000;
    format!("{}h {:02}m", seconds / 3600, seconds / 60 % 60)
}

pub fn source_name(source: EntrySource) -> &'static str {
    match source {
        EntrySource::Timer => "Timer",
        EntrySource::Manual => "Manual",
        EntrySource::IdleReassignment => "Idle reassignment",
        EntrySource::Recovery => "Recovery",
    }
}

pub fn names<'a>(
    entry: &TimeEntry,
    projects: &'a [Project],
    activities: &'a [Activity],
) -> (&'a str, &'a str) {
    let project = projects
        .iter()
        .find(|project| project.id == entry.project_id)
        .map_or("(missing)", |project| project.name.as_str());
    let activity = entry
        .activity_id
        .and_then(|id| activities.iter().find(|activity| activity.id == id))
        .map_or("", |activity| activity.name.as_str());
    (project, activity)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayRow {
    pub title: String,
    pub subtitle: String,
}

pub fn report_display_rows(
    entries: &[TimeEntry],
    projects: &[Project],
    activities: &[Activity],
    week: (i64, i64),
    mode: ReportMode,
) -> Vec<DisplayRow> {
    let mut rows = Vec::new();
    for task in weekly_tasks(entries, week) {
        let (project, activity) = names(task.entry, projects, activities);
        let title = if task.entry.note.is_empty() {
            "Untitled task"
        } else {
            task.entry.note.as_str()
        };
        let details = if activity.is_empty() {
            project.to_owned()
        } else {
            format!("{project} / {activity}")
        };
        match mode {
            ReportMode::Tasks => rows.push(DisplayRow {
                title: title.into(),
                subtitle: format!(
                    "{details} · {} · {}",
                    date_range(task.first_date, task.last_date),
                    duration(task.duration_ms)
                ),
            }),
            ReportMode::Full => {
                for interval in task.intervals {
                    rows.push(DisplayRow {
                        title: title.into(),
                        subtitle: format!(
                            "{details} · {} – {} · {} · {}",
                            local_datetime(interval.start_ms),
                            local_datetime(interval.end_ms),
                            duration(interval.duration_ms()),
                            source_name(interval.source)
                        ),
                    });
                }
            }
        }
    }
    rows
}

/// Human-facing report rows. CSV output keeps its stable data representation.
#[cfg(feature = "native-ui")]
pub fn localized_report_display_rows(
    entries: &[TimeEntry],
    projects: &[Project],
    activities: &[Activity],
    week: (i64, i64),
    mode: ReportMode,
) -> Vec<DisplayRow> {
    use crate::locale::{tr, trf, ui_date, ui_datetime};

    let mut rows = Vec::new();
    for task in weekly_tasks(entries, week) {
        let (project, activity) = names(task.entry, projects, activities);
        let title = if task.entry.note.is_empty() {
            tr("Untitled task").to_owned()
        } else {
            task.entry.note.clone()
        };
        let details = if activity.is_empty() {
            project.to_owned()
        } else {
            trf(
                "{project} / {activity}",
                &[("project", project), ("activity", activity)],
            )
        };
        let duration = |ms: i64| {
            let minutes = ms.max(0) / 60_000;
            trf(
                "{hours}h {minutes}m",
                &[
                    ("hours", &(minutes / 60).to_string()),
                    ("minutes", &format!("{:02}", minutes % 60)),
                ],
            )
        };
        match mode {
            ReportMode::Tasks => {
                let first = ui_date(task.first_date, "%x");
                let last = ui_date(task.last_date, "%x");
                let dates = if task.first_date == task.last_date {
                    first
                } else {
                    format!("{first} – {last}")
                };
                rows.push(DisplayRow {
                    title,
                    subtitle: trf(
                        "{details} · {dates} · {duration}",
                        &[
                            ("details", &details),
                            ("dates", &dates),
                            ("duration", &duration(task.duration_ms)),
                        ],
                    ),
                });
            }
            ReportMode::Full => {
                for interval in task.intervals {
                    let source = match interval.source {
                        EntrySource::Timer => tr("Timer"),
                        EntrySource::Manual => tr("Manual"),
                        EntrySource::IdleReassignment => tr("Idle reassignment"),
                        EntrySource::Recovery => tr("Recovery"),
                    };
                    rows.push(DisplayRow {
                        title: title.clone(),
                        subtitle: trf(
                            "{details} · {start} – {end} · {duration} · {source}",
                            &[
                                ("details", &details),
                                ("start", &ui_datetime(interval.start_ms, "%x %X")),
                                ("end", &ui_datetime(interval.end_ms, "%x %X")),
                                ("duration", &duration(interval.duration_ms())),
                                ("source", source),
                            ],
                        ),
                    });
                }
            }
        }
    }
    rows
}

/// Writes the selected week's task totals or its individual intervals.
pub fn write_csv<W: Write>(
    writer: W,
    entries: &[TimeEntry],
    projects: &[Project],
    activities: &[Activity],
    week: (i64, i64),
    mode: ReportMode,
) -> Result<(), AppError> {
    let mut csv = csv::Writer::from_writer(writer);
    match mode {
        ReportMode::Tasks => csv.write_record([
            "entry_id",
            "first_date",
            "last_date",
            "duration_seconds",
            "duration_hh_mm",
            "project",
            "activity",
            "note",
        ])?,
        ReportMode::Full => csv.write_record([
            "entry_id",
            "start_local",
            "end_local",
            "duration_seconds",
            "duration_hh_mm",
            "project",
            "activity",
            "note",
            "source",
        ])?,
    }
    for task in weekly_tasks(entries, week) {
        let (project, activity) = names(task.entry, projects, activities);
        let id = task
            .entry
            .id
            .map_or_else(String::new, |id| id.0.to_string());
        match mode {
            ReportMode::Tasks => csv.write_record([
                id,
                date(task.first_date),
                date(task.last_date),
                seconds(task.duration_ms),
                hours_minutes(task.duration_ms),
                project.into(),
                activity.into(),
                task.entry.note.clone(),
            ])?,
            ReportMode::Full => {
                for interval in task.intervals {
                    csv.write_record([
                        id.clone(),
                        local_datetime(interval.start_ms),
                        local_datetime(interval.end_ms),
                        seconds(interval.duration_ms()),
                        hours_minutes(interval.duration_ms()),
                        project.into(),
                        activity.into(),
                        task.entry.note.clone(),
                        source_name(interval.source).into(),
                    ])?;
                }
            }
        }
    }
    csv.flush().map_err(csv::Error::from)?;
    Ok(())
}

/// Writes the CSV atomically (temp file, fsync, rename).
pub fn write_csv_path(
    path: &Path,
    entries: &[TimeEntry],
    projects: &[Project],
    activities: &[Activity],
    week: (i64, i64),
    mode: ReportMode,
) -> Result<(), AppError> {
    let Some(parent) = path.parent() else {
        return Err(AppError::DataDirectoryUnavailable);
    };
    fs::create_dir_all(parent).map_err(|source| AppError::io(parent, source))?;
    let temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|source| AppError::io(parent, source))?;
    write_csv(
        temporary.as_file(),
        entries,
        projects,
        activities,
        week,
        mode,
    )?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|source| AppError::io(path, source))?;
    temporary
        .persist(path)
        .map_err(|error| AppError::io(path, error.error))?;
    Ok(())
}
