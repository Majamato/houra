use chrono::{Local, NaiveDate, TimeZone};
use houra::{
    AppError,
    export::{ReportMode, weekly_tasks, write_csv, write_csv_path},
};
use houra_core::{
    Activity, ActivityId, EntryId, EntrySource, Project, ProjectId, TimeEntry, TrackedInterval,
};

fn at(day: u32, hour: u32, minute: u32) -> i64 {
    Local
        .with_ymd_and_hms(2026, 9, day, hour, minute, 0)
        .single()
        .unwrap_or_else(|| panic!("test local date should exist"))
        .timestamp_millis()
}

fn week() -> (i64, i64) {
    (at(21, 0, 0), at(28, 0, 0))
}

fn project() -> Project {
    Project {
        id: ProjectId(1),
        name: "Work, \"one\"".into(),
        color: "#123456".into(),
        archived: true,
        created_at_ms: 0,
        updated_at_ms: 0,
    }
}

fn activity() -> Activity {
    Activity {
        id: ActivityId(1),
        name: "line\nbreak".into(),
        archived: true,
        created_at_ms: 0,
        updated_at_ms: 0,
    }
}

fn entry() -> TimeEntry {
    TimeEntry {
        id: Some(EntryId(42)),
        project_id: ProjectId(1),
        activity_id: Some(ActivityId(1)),
        note: "comma, quote\" newline\n".into(),
        intervals: vec![
            TrackedInterval {
                id: None,
                start_ms: at(20, 23, 30),
                end_ms: at(21, 1, 0),
                source: EntrySource::Timer,
            },
            TrackedInterval {
                id: None,
                start_ms: at(24, 14, 5),
                end_ms: at(24, 15, 5),
                source: EntrySource::Manual,
            },
            TrackedInterval {
                id: None,
                start_ms: at(27, 23, 0),
                end_ms: at(28, 1, 0),
                source: EntrySource::Recovery,
            },
            TrackedInterval {
                id: None,
                start_ms: at(28, 2, 0),
                end_ms: at(28, 3, 0),
                source: EntrySource::IdleReassignment,
            },
        ],
        created_at_ms: 0,
        updated_at_ms: 0,
    }
}

fn rows(
    entries: &[TimeEntry],
    mode: ReportMode,
) -> Result<(csv::StringRecord, Vec<csv::StringRecord>), Box<dyn std::error::Error>> {
    let mut output = Vec::new();
    write_csv(
        &mut output,
        entries,
        &[project()],
        &[activity()],
        week(),
        mode,
    )?;
    let mut reader = csv::Reader::from_reader(output.as_slice());
    let headers = reader.headers()?.clone();
    let rows = reader.records().collect::<Result<Vec<_>, _>>()?;
    Ok((headers, rows))
}

#[test]
fn task_summary_sums_clipped_intervals_and_preserves_task_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let first = entry();
    let mut second = first.clone();
    second.id = Some(EntryId(43));
    second.intervals = vec![TrackedInterval {
        id: None,
        start_ms: at(24, 10, 0),
        end_ms: at(24, 11, 0),
        source: EntrySource::Timer,
    }];
    let entries = [first, second];
    let tasks = weekly_tasks(&entries, week());
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].intervals.len(), 3);
    assert_eq!(tasks[0].duration_ms, 10_800_000);
    assert_eq!(
        tasks[0].first_date,
        NaiveDate::from_ymd_opt(2026, 9, 21).unwrap_or_else(|| panic!("test date should exist"))
    );
    assert_eq!(
        tasks[0].last_date,
        NaiveDate::from_ymd_opt(2026, 9, 27).unwrap_or_else(|| panic!("test date should exist"))
    );
    let (headers, rows) = rows(&entries, ReportMode::Tasks)?;
    assert_eq!(
        headers,
        csv::StringRecord::from(vec![
            "entry_id",
            "first_date",
            "last_date",
            "duration_seconds",
            "duration_hh_mm",
            "project",
            "activity",
            "note"
        ])
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(&rows[0][0], "42");
    assert_eq!(&rows[0][1], "Sep 21, 2026");
    assert_eq!(&rows[0][2], "Sep 27, 2026");
    assert_eq!(&rows[0][3], "10800");
    assert_eq!(&rows[0][4], "03:00");
    assert_eq!(&rows[0][5], project().name);
    assert_eq!(&rows[0][6], activity().name);
    assert_eq!(&rows[0][7], entries[0].note);
    assert_eq!(&rows[1][4], "01:00");
    assert_eq!(&rows[1][1], "Sep 24, 2026");
    assert_eq!(&rows[1][2], "Sep 24, 2026");
    Ok(())
}

#[test]
fn full_report_lists_clipped_intervals_and_sources() -> Result<(), Box<dyn std::error::Error>> {
    let (headers, rows) = rows(&[entry()], ReportMode::Full)?;
    assert_eq!(
        headers,
        csv::StringRecord::from(vec![
            "entry_id",
            "start_local",
            "end_local",
            "duration_seconds",
            "duration_hh_mm",
            "project",
            "activity",
            "note",
            "source"
        ])
    );
    assert_eq!(rows.len(), 3);
    assert_eq!(&rows[0][1], "Sep 21, 2026 12:00 AM");
    assert_eq!(&rows[0][2], "Sep 21, 2026 1:00 AM");
    assert_eq!(&rows[0][3], "3600");
    assert_eq!(&rows[0][4], "01:00");
    assert_eq!(&rows[0][8], "Timer");
    assert_eq!(&rows[1][1], "Sep 24, 2026 2:05 PM");
    assert_eq!(&rows[1][8], "Manual");
    assert_eq!(&rows[2][2], "Sep 28, 2026 12:00 AM");
    assert_eq!(&rows[2][8], "Recovery");
    assert_eq!(
        rows.iter()
            .map(|row| row[3]
                .parse::<i64>()
                .unwrap_or_else(|error| panic!("test duration should parse: {error}")))
            .sum::<i64>(),
        10800
    );
    Ok(())
}

#[test]
fn missing_names_and_outside_entries() -> Result<(), Box<dyn std::error::Error>> {
    let mut missing = entry();
    missing.project_id = ProjectId(99);
    missing.activity_id = Some(ActivityId(99));
    missing.intervals.truncate(1);
    let mut outside = missing.clone();
    outside.intervals[0].start_ms = at(28, 2, 0);
    outside.intervals[0].end_ms = at(28, 3, 0);
    for mode in [ReportMode::Tasks, ReportMode::Full] {
        let (_, rows) = rows(&[missing.clone(), outside.clone()], mode)?;
        assert_eq!(rows.len(), 1);
        assert_eq!(&rows[0][5], "(missing)");
        assert_eq!(&rows[0][6], "");
    }
    Ok(())
}

#[test]
fn writer_errors_are_propagated_in_both_modes() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("broken"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("broken"))
        }
    }
    for mode in [ReportMode::Tasks, ReportMode::Full] {
        assert!(
            matches!(write_csv(Broken, &[], &[], &[], week(), mode), Err(AppError::Csv(error)) if error.is_io_error())
        );
    }
}

#[test]
fn file_export_replaces_existing_file_and_reports_path_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("nested/export.csv");
    for mode in [ReportMode::Tasks, ReportMode::Full] {
        write_csv_path(&path, &[], &[], &[], week(), mode)?;
        let expected = std::fs::read(&path)?;
        std::fs::write(&path, b"old")?;
        write_csv_path(&path, &[], &[], &[], week(), mode)?;
        assert_eq!(std::fs::read(&path)?, expected);
        assert!(
            matches!(write_csv_path(directory.path(), &[], &[], &[], week(), mode), Err(AppError::Io { path, .. }) if path == directory.path())
        );
        assert!(matches!(
            write_csv_path(&path.join("child"), &[], &[], &[], week(), mode),
            Err(AppError::Io { .. })
        ));
    }
    Ok(())
}

#[test]
fn report_mode_changes_display_rows_and_csv_together() -> Result<(), Box<dyn std::error::Error>> {
    use houra::export::report_display_rows;
    let entries = [entry()];
    let projects = [project()];
    let activities = [activity()];
    let summary = report_display_rows(&entries, &projects, &activities, week(), ReportMode::Tasks);
    let full = report_display_rows(&entries, &projects, &activities, week(), ReportMode::Full);
    assert_eq!(summary.len(), rows(&entries, ReportMode::Tasks)?.1.len());
    assert_eq!(full.len(), rows(&entries, ReportMode::Full)?.1.len());
    assert_eq!(summary.len(), 1);
    assert_eq!(full.len(), 3);
    assert_eq!(summary[0].title, entries[0].note);
    assert!(summary[0].subtitle.contains("Sep 21, 2026 – Sep 27, 2026"));
    assert!(summary[0].subtitle.contains("3h 00m"));
    assert!(full[1].subtitle.contains("Sep 24, 2026 2:05 PM"));
    assert!(full[1].subtitle.contains("Manual"));
    Ok(())
}

#[test]
fn millisecond_durations_remain_additive_across_modes() -> Result<(), Box<dyn std::error::Error>> {
    let mut task = entry();
    task.intervals = vec![
        TrackedInterval {
            id: None,
            start_ms: at(24, 10, 0),
            end_ms: at(24, 10, 0) + 501,
            source: EntrySource::Timer,
        },
        TrackedInterval {
            id: None,
            start_ms: at(24, 11, 0),
            end_ms: at(24, 11, 0) + 502,
            source: EntrySource::Timer,
        },
    ];
    let (_, summary) = rows(&[task.clone()], ReportMode::Tasks)?;
    let (_, full) = rows(&[task], ReportMode::Full)?;
    assert_eq!(&summary[0][3], "1.003");
    assert_eq!(&summary[0][4], "00:00");
    assert_eq!(&full[0][3], "0.501");
    assert_eq!(&full[0][4], "00:00");
    assert_eq!(&full[1][3], "0.502");
    assert_eq!(&full[1][4], "00:00");
    Ok(())
}

#[test]
fn full_csv_keeps_every_supported_source() -> Result<(), Box<dyn std::error::Error>> {
    let mut task = entry();
    task.intervals = [
        EntrySource::Timer,
        EntrySource::Manual,
        EntrySource::IdleReassignment,
        EntrySource::Recovery,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, source)| TrackedInterval {
        id: None,
        start_ms: at(24, 10 + index as u32, 0),
        end_ms: at(24, 10 + index as u32, 1),
        source,
    })
    .collect();
    let (_, rows) = rows(&[task], ReportMode::Full)?;
    assert_eq!(
        rows.iter().map(|row| row[8].to_owned()).collect::<Vec<_>>(),
        ["Timer", "Manual", "Idle reassignment", "Recovery"]
    );
    Ok(())
}
