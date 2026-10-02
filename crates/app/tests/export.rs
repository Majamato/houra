use chrono::{Local, NaiveDate, TimeZone};
use houra::{
    AppError, DateFormat,
    export::{ReportMode, ReportOptions, weekly_tasks, write_csv, write_csv_path},
};
use houra_core::{
    Activity, ActivityId, DurationRounding, EntryId, EntrySource, EntryTotals, Project, ProjectId,
    TimeEntry, TrackedInterval,
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

/// Explicit fixture dates keep expectations independent of the host locale.
fn options(mode: ReportMode) -> ReportOptions {
    ReportOptions {
        mode,
        totals: EntryTotals::default(),
        date_format: DateFormat::YearMonthDay,
    }
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

type CsvRows = (csv::StringRecord, Vec<csv::StringRecord>);

fn rows(entries: &[TimeEntry], mode: ReportMode) -> Result<CsvRows, Box<dyn std::error::Error>> {
    rows_with(entries, mode, &EntryTotals::default())
}

fn rows_with(
    entries: &[TimeEntry],
    mode: ReportMode,
    totals: &EntryTotals,
) -> Result<CsvRows, Box<dyn std::error::Error>> {
    rows_with_options(
        entries,
        &ReportOptions {
            mode,
            totals: *totals,
            date_format: DateFormat::YearMonthDay,
        },
    )
}

fn rows_with_options(
    entries: &[TimeEntry],
    options: &ReportOptions,
) -> Result<CsvRows, Box<dyn std::error::Error>> {
    let mut output = Vec::new();
    write_csv(
        &mut output,
        entries,
        &[project()],
        &[activity()],
        week(),
        options,
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
    let tasks = weekly_tasks(&entries, week(), &EntryTotals::default());
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
    assert_eq!(&rows[0][1], "2026-09-21");
    assert_eq!(&rows[0][2], "2026-09-27");
    assert_eq!(&rows[0][3], "10800");
    assert_eq!(&rows[0][4], "03:00");
    assert_eq!(&rows[0][5], project().name);
    assert_eq!(&rows[0][6], activity().name);
    assert_eq!(&rows[0][7], entries[0].note);
    assert_eq!(&rows[1][4], "01:00");
    assert_eq!(&rows[1][1], "2026-09-24");
    assert_eq!(&rows[1][2], "2026-09-24");
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
    assert_eq!(&rows[0][1], "2026-09-21 12:00 AM");
    assert_eq!(&rows[0][2], "2026-09-21 1:00 AM");
    assert_eq!(&rows[0][3], "3600");
    assert_eq!(&rows[0][4], "01:00");
    assert_eq!(&rows[0][8], "Timer");
    assert_eq!(&rows[1][1], "2026-09-24 2:05 PM");
    assert_eq!(&rows[1][8], "Manual");
    assert_eq!(&rows[2][2], "2026-09-28 12:00 AM");
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
fn csv_and_subtitles_follow_every_explicit_format() -> Result<(), Box<dyn std::error::Error>> {
    use houra::export::report_display_rows;
    let entries = [entry()];
    let mut single = entry();
    single.intervals = vec![TrackedInterval {
        id: None,
        start_ms: at(24, 10, 0),
        end_ms: at(24, 11, 0),
        source: EntrySource::Timer,
    }];
    let singles = [single];
    let projects = [project()];
    let activities = [activity()];
    for (date_format, first, last, clipped_end, single_day) in [
        (
            DateFormat::DayMonthYear,
            "21/09/2026",
            "27/09/2026",
            "28/09/2026",
            "24/09/2026",
        ),
        (
            DateFormat::MonthDayYear,
            "09/21/2026",
            "09/27/2026",
            "09/28/2026",
            "09/24/2026",
        ),
        (
            DateFormat::YearMonthDay,
            "2026-09-21",
            "2026-09-27",
            "2026-09-28",
            "2026-09-24",
        ),
    ] {
        for mode in [ReportMode::Tasks, ReportMode::Full] {
            let option_set = ReportOptions {
                mode,
                totals: EntryTotals::default(),
                date_format,
            };
            let (_, csv_rows) = rows_with_options(&entries, &option_set)?;
            let display =
                report_display_rows(&entries, &projects, &activities, week(), &option_set);
            match mode {
                ReportMode::Tasks => {
                    assert_eq!(&csv_rows[0][1], first, "{date_format:?}");
                    assert_eq!(&csv_rows[0][2], last, "{date_format:?}");
                    assert_eq!(display.len(), 1);
                    assert!(
                        display[0].subtitle.contains(&format!("{first} – {last}")),
                        "{date_format:?}: {}",
                        display[0].subtitle
                    );
                }
                ReportMode::Full => {
                    assert!(
                        csv_rows[0][1].starts_with(first),
                        "{date_format:?}: {}",
                        &csv_rows[0][1]
                    );
                    assert!(
                        csv_rows[2][2].starts_with(clipped_end),
                        "{date_format:?}: {}",
                        &csv_rows[2][2]
                    );
                    assert_eq!(display.len(), 3);
                    assert!(
                        display[0].subtitle.contains(first),
                        "{date_format:?}: {}",
                        display[0].subtitle
                    );
                    assert!(
                        display[2].subtitle.contains(last),
                        "{date_format:?}: {}",
                        display[2].subtitle
                    );
                }
            }
        }
        let single_tasks = ReportOptions {
            mode: ReportMode::Tasks,
            totals: EntryTotals::default(),
            date_format,
        };
        let (_, csv_rows) = rows_with_options(&singles, &single_tasks)?;
        assert_eq!(&csv_rows[0][1], single_day, "{date_format:?}");
        assert_eq!(&csv_rows[0][2], single_day, "{date_format:?}");
        let display = report_display_rows(&singles, &projects, &activities, week(), &single_tasks);
        assert_eq!(display.len(), 1);
        assert!(
            display[0].subtitle.contains(single_day) && !display[0].subtitle.contains('–'),
            "{date_format:?}: {}",
            display[0].subtitle
        );
        let single_full = ReportOptions {
            mode: ReportMode::Full,
            totals: EntryTotals::default(),
            date_format,
        };
        let (_, csv_rows) = rows_with_options(&singles, &single_full)?;
        assert_eq!(
            &csv_rows[0][1],
            &format!("{single_day} 10:00 AM"),
            "{date_format:?}"
        );
        assert_eq!(
            &csv_rows[0][2],
            &format!("{single_day} 11:00 AM"),
            "{date_format:?}"
        );
    }
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
            matches!(write_csv(Broken, &[], &[], &[], week(), &options(mode)), Err(AppError::Csv(error)) if error.is_io_error())
        );
    }
}

#[test]
fn file_export_replaces_existing_file_and_reports_path_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("nested/export.csv");
    for mode in [ReportMode::Tasks, ReportMode::Full] {
        let option_set = options(mode);
        write_csv_path(&path, &[], &[], &[], week(), &option_set)?;
        let expected = std::fs::read(&path)?;
        std::fs::write(&path, b"old")?;
        write_csv_path(&path, &[], &[], &[], week(), &option_set)?;
        assert_eq!(std::fs::read(&path)?, expected);
        assert!(
            matches!(write_csv_path(directory.path(), &[], &[], &[], week(), &option_set), Err(AppError::Io { path, .. }) if path == directory.path())
        );
        assert!(matches!(
            write_csv_path(&path.join("child"), &[], &[], &[], week(), &option_set),
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
    let summary = report_display_rows(
        &entries,
        &projects,
        &activities,
        week(),
        &options(ReportMode::Tasks),
    );
    let full = report_display_rows(
        &entries,
        &projects,
        &activities,
        week(),
        &options(ReportMode::Full),
    );
    assert_eq!(summary.len(), rows(&entries, ReportMode::Tasks)?.1.len());
    assert_eq!(full.len(), rows(&entries, ReportMode::Full)?.1.len());
    assert_eq!(summary.len(), 1);
    assert_eq!(full.len(), 3);
    assert_eq!(summary[0].title, entries[0].note);
    assert!(summary[0].subtitle.contains("2026-09-21 – 2026-09-27"));
    assert!(summary[0].subtitle.contains("3h 00m"));
    assert!(full[1].subtitle.contains("2026-09-24 2:05 PM"));
    assert!(full[1].subtitle.contains("Manual"));
    Ok(())
}

#[test]
fn task_totals_round_while_intervals_stay_exact() -> Result<(), Box<dyn std::error::Error>> {
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
    for (rounding, seconds, hours_minutes) in [
        (DurationRounding::Up, "60", "00:01"),
        (DurationRounding::Nearest, "60", "00:01"),
        (DurationRounding::Down, "60", "00:01"),
    ] {
        let totals = EntryTotals {
            rounding,
            active_entry_id: None,
        };
        let (_, summary) = rows_with(std::slice::from_ref(&task), ReportMode::Tasks, &totals)?;
        assert_eq!(&summary[0][3], seconds, "{rounding:?}");
        assert_eq!(&summary[0][4], hours_minutes, "{rounding:?}");
    }
    // The task the active timer still tracks stays exact.
    let active = EntryTotals {
        rounding: DurationRounding::Up,
        active_entry_id: task.id,
    };
    let (_, summary) = rows_with(std::slice::from_ref(&task), ReportMode::Tasks, &active)?;
    assert_eq!(&summary[0][3], "1.003");
    assert_eq!(&summary[0][4], "00:00");
    let (_, full) = rows(&[task], ReportMode::Full)?;
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

#[test]
fn task_totals_follow_the_rounding_mode() {
    let mut task = entry();
    let start = at(24, 10, 0);
    task.intervals = vec![TrackedInterval {
        id: None,
        start_ms: start,
        end_ms: start + 90_000,
        source: EntrySource::Timer,
    }];
    let entries = [task];
    for (rounding, expected_ms) in [
        (DurationRounding::Up, 120_000),
        (DurationRounding::Nearest, 120_000),
        (DurationRounding::Down, 60_000),
    ] {
        let totals = EntryTotals {
            rounding,
            active_entry_id: None,
        };
        let tasks = weekly_tasks(&entries, week(), &totals);
        assert_eq!(tasks[0].duration_ms, expected_ms, "{rounding:?}");
        assert_eq!(tasks[0].intervals[0].duration_ms(), 90_000);
    }
}

#[cfg(feature = "native-ui")]
#[test]
fn system_csv_dates_follow_the_locale_in_isolated_processes() {
    let binary = std::env::current_exe()
        .unwrap_or_else(|error| panic!("could not locate test binary: {error}"));
    for locale in ["C", "en_US.UTF-8", "de_DE.UTF-8", "ja_JP.UTF-8"] {
        let output = std::process::Command::new(&binary)
            .args(["--exact", "system_csv_dates_in_child_process"])
            .env("LC_ALL", locale)
            .env("HOURA_TEST_SYSTEM_LOCALE", locale)
            .output()
            .unwrap_or_else(|error| panic!("could not test {locale} CSV dates: {error}"));
        assert!(
            output.status.success(),
            "{locale} system CSV dates failed: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[cfg(feature = "native-ui")]
#[test]
fn system_csv_dates_in_child_process() {
    use gettextrs::{LocaleCategory, setlocale};
    let Ok(locale) = std::env::var("HOURA_TEST_SYSTEM_LOCALE") else {
        return;
    };
    if setlocale(LocaleCategory::LcAll, "").is_none() {
        eprintln!("skipping unavailable locale {locale}");
        return;
    }
    assert!(houra::locale::initialize().is_ok());
    let first =
        NaiveDate::from_ymd_opt(2026, 9, 21).unwrap_or_else(|| panic!("test date should exist"));
    let last =
        NaiveDate::from_ymd_opt(2026, 9, 27).unwrap_or_else(|| panic!("test date should exist"));
    let expected_first = houra::locale::ui_date(first, "%x");
    let expected_last = houra::locale::ui_date(last, "%x");
    let tasks = ReportOptions {
        mode: ReportMode::Tasks,
        totals: EntryTotals::default(),
        date_format: DateFormat::System,
    };
    let (_, csv_rows) = rows_with_options(&[entry()], &tasks)
        .unwrap_or_else(|error| panic!("CSV should write: {error}"));
    assert_eq!(&csv_rows[0][1], &expected_first, "locale {locale}");
    assert_eq!(&csv_rows[0][2], &expected_last, "locale {locale}");
    let full = ReportOptions {
        mode: ReportMode::Full,
        totals: EntryTotals::default(),
        date_format: DateFormat::System,
    };
    let (_, csv_rows) = rows_with_options(&[entry()], &full)
        .unwrap_or_else(|error| panic!("CSV should write: {error}"));
    assert!(
        csv_rows[0][1].starts_with(&expected_first) && csv_rows[0][1].ends_with("12:00 AM"),
        "locale {locale}: {}",
        &csv_rows[0][1]
    );
}
