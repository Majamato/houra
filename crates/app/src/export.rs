use std::fs;
use std::io::Write;
use std::path::Path;

use chrono::{Local, TimeZone};
use houra_core::{Project, Task, TimeEntry};

use crate::AppError;

/// Writes one CSV row per entry to any writer; times are local.
pub fn write_csv<W: Write>(
    writer: W,
    entries: &[TimeEntry],
    projects: &[Project],
    tasks: &[Task],
) -> Result<(), AppError> {
    let mut csv = csv::Writer::from_writer(writer);
    csv.write_record([
        "date",
        "start_local",
        "end_local",
        "duration_seconds",
        "project",
        "task",
        "note",
        "source",
    ])?;
    for entry in entries {
        let start = Local.timestamp_millis_opt(entry.start_ms).single();
        let end = Local.timestamp_millis_opt(entry.end_ms).single();
        let project = projects
            .iter()
            .find(|project| project.id == entry.project_id)
            .map_or("(missing)", |project| project.name.as_str());
        let task = entry
            .task_id
            .and_then(|id| tasks.iter().find(|task| task.id == id))
            .map_or("", |task| task.name.as_str());
        let date = start.map_or_else(String::new, |value| value.format("%x").to_string());
        let start_local = start.map_or_else(String::new, |value| value.to_rfc3339());
        let end_local = end.map_or_else(String::new, |value| value.to_rfc3339());
        csv.write_record([
            date,
            start_local,
            end_local,
            (entry.duration_ms() / 1_000).to_string(),
            project.to_owned(),
            task.to_owned(),
            entry.note.clone(),
            format!("{:?}", entry.source),
        ])?;
    }
    csv.flush().map_err(csv::Error::from)?;
    Ok(())
}

/// Writes the CSV atomically (temp file, fsync, rename).
pub fn write_csv_path(
    path: &Path,
    entries: &[TimeEntry],
    projects: &[Project],
    tasks: &[Task],
) -> Result<(), AppError> {
    let Some(parent) = path.parent() else {
        return Err(AppError::DataDirectoryUnavailable);
    };
    fs::create_dir_all(parent).map_err(|source| AppError::io(parent, source))?;
    let temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|source| AppError::io(parent, source))?;
    write_csv(temporary.as_file(), entries, projects, tasks)?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|source| AppError::io(path, source))?;
    temporary
        .persist(path)
        .map_err(|error| AppError::io(path, error.error))?;
    Ok(())
}
