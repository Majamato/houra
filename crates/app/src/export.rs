//! Weekly reports for the UI and CSV export.

use std::fs;
use std::io::Write;
use std::path::Path;

use chrono::{Local, NaiveDate, TimeZone};
use houra_core::{Activity, EntrySource, EntryTotals, Project, TimeEntry};

use crate::AppError;
use crate::date_format::{format_csv_datetime, format_date, format_date_range};
use crate::settings::DateFormat;

/// How much detail a report shows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReportMode {
    /// One row per task with its weekly total.
    #[default]
    Tasks,
    /// One row per interval with exact times.
    Full,
}

/// Options selecting what a report shows and how its dates read.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReportOptions {
    /// How much detail a report shows.
    pub mode: ReportMode,
    /// Totals for display: finished entries round, the active entry stays exact.
    pub totals: EntryTotals,
    /// How calendar dates appear in rows and CSV date values.
    pub date_format: DateFormat,
}

/// One interval clipped to the reported week.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkedInterval {
    /// Clipped start, in wall-clock milliseconds.
    pub start_ms: i64,
    /// Clipped end, in wall-clock milliseconds.
    pub end_ms: i64,
    /// How this interval came into existence.
    pub source: EntrySource,
}

impl WorkedInterval {
    /// Returns the clipped length of the interval.
    pub fn duration_ms(&self) -> i64 {
        self.end_ms - self.start_ms
    }
}

/// One task's work inside the reported week.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskReport<'a> {
    /// Task these totals belong to.
    pub entry: &'a TimeEntry,
    /// Week-clipped intervals contributing to this task.
    pub intervals: Vec<WorkedInterval>,
    /// Week-clipped total, rounded unless the active timer still tracks the
    /// task; the intervals themselves stay exact.
    pub duration_ms: i64,
    /// Local date of the earliest clipped work.
    pub first_date: NaiveDate,
    /// Local date of the latest clipped work.
    pub last_date: NaiveDate,
}

/// One report per task, using only work inside the selected half-open week.
pub fn weekly_tasks<'a>(
    entries: &'a [TimeEntry],
    week: (i64, i64),
    totals: &EntryTotals,
) -> Vec<TaskReport<'a>> {
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
            let duration_ms = totals.total_ms(entry, week.0, week.1);
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

/// Formats milliseconds as `Xh YYm`.
pub fn duration(ms: i64) -> String {
    let seconds = ms / 1_000;
    format!("{}h {:02}m", seconds / 3600, seconds / 60 % 60)
}

/// Returns the display name of an entry source.
pub fn source_name(source: EntrySource) -> &'static str {
    match source {
        EntrySource::Timer => "Timer",
        EntrySource::Manual => "Manual",
        EntrySource::IdleReassignment => "Idle reassignment",
        EntrySource::Recovery => "Recovery",
    }
}

/// Resolves an entry's project and activity names, with fallbacks.
/// Missing projects show as `(missing)`; missing activities as empty.
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

/// One human-facing report row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayRow {
    /// Task note, or `Untitled task`.
    pub title: String,
    /// Project, dates, and duration line.
    pub subtitle: String,
}

/// Builds untranslated report rows for the given week and options.
/// Tests use this; the UI uses the localized variant.
pub fn report_display_rows(
    entries: &[TimeEntry],
    projects: &[Project],
    activities: &[Activity],
    week: (i64, i64),
    options: &ReportOptions,
) -> Vec<DisplayRow> {
    let mut rows = Vec::new();
    for task in weekly_tasks(entries, week, &options.totals) {
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
        match options.mode {
            ReportMode::Tasks => rows.push(DisplayRow {
                title: title.into(),
                subtitle: format!(
                    "{details} · {} · {}",
                    format_date_range(task.first_date, task.last_date, options.date_format),
                    duration(task.duration_ms)
                ),
            }),
            ReportMode::Full => {
                for interval in task.intervals {
                    rows.push(DisplayRow {
                        title: title.into(),
                        subtitle: format!(
                            "{details} · {} – {} · {} · {}",
                            format_csv_datetime(interval.start_ms, options.date_format),
                            format_csv_datetime(interval.end_ms, options.date_format),
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

/// Human-facing report rows; dates follow the explicit options.
#[cfg(feature = "native-ui")]
pub fn localized_report_display_rows(
    entries: &[TimeEntry],
    projects: &[Project],
    activities: &[Activity],
    week: (i64, i64),
    options: &ReportOptions,
) -> Vec<DisplayRow> {
    use crate::date_format::format_ui_datetime;
    use crate::locale::{tr, trf};

    let mut rows = Vec::new();
    for task in weekly_tasks(entries, week, &options.totals) {
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
        match options.mode {
            ReportMode::Tasks => {
                let dates = format_date_range(task.first_date, task.last_date, options.date_format);
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
                                (
                                    "start",
                                    &format_ui_datetime(interval.start_ms, options.date_format),
                                ),
                                (
                                    "end",
                                    &format_ui_datetime(interval.end_ms, options.date_format),
                                ),
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
/// CSV date values follow the explicit options.
pub fn write_csv<W: Write>(
    writer: W,
    entries: &[TimeEntry],
    projects: &[Project],
    activities: &[Activity],
    week: (i64, i64),
    options: &ReportOptions,
) -> Result<(), AppError> {
    let mut csv = csv::Writer::from_writer(writer);
    match options.mode {
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
    for task in weekly_tasks(entries, week, &options.totals) {
        let (project, activity) = names(task.entry, projects, activities);
        let id = task
            .entry
            .id
            .map_or_else(String::new, |id| id.0.to_string());
        match options.mode {
            ReportMode::Tasks => csv.write_record([
                id,
                format_date(task.first_date, options.date_format),
                format_date(task.last_date, options.date_format),
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
                        format_csv_datetime(interval.start_ms, options.date_format),
                        format_csv_datetime(interval.end_ms, options.date_format),
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
    options: &ReportOptions,
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
        options,
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
