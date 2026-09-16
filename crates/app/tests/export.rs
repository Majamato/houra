use houra::{
    AppError,
    export::{write_csv, write_csv_path},
};
use houra_core::{Activity, ActivityId, EntrySource, Project, ProjectId, TimeEntry};

#[test]
fn csv_headers_escaping_names_sources_and_fallbacks() -> Result<(), Box<dyn std::error::Error>> {
    let project = Project {
        id: ProjectId(1),
        name: "Work, \"one\"".into(),
        color: "#123456".into(),
        archived: true,
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    let activity = Activity {
        id: ActivityId(1),
        project_id: project.id,
        name: "line\nbreak".into(),
        archived: true,
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    let mut entries = vec![];
    for source in [
        EntrySource::Timer,
        EntrySource::Manual,
        EntrySource::IdleReassignment,
        EntrySource::Recovery,
    ] {
        entries.push(TimeEntry {
            id: None,
            project_id: project.id,
            activity_id: Some(activity.id),
            note: "comma, quote\" newline\n".into(),
            start_ms: 0,
            end_ms: 1999,
            source,
            created_at_ms: 0,
            updated_at_ms: 0,
        });
    }
    let mut missing = entries[0].clone();
    missing.project_id = ProjectId(99);
    missing.activity_id = Some(ActivityId(99));
    entries.push(missing);
    let mut output = vec![];
    write_csv(
        &mut output,
        &entries,
        std::slice::from_ref(&project),
        std::slice::from_ref(&activity),
    )?;
    let mut reader = csv::Reader::from_reader(output.as_slice());
    assert_eq!(
        reader.headers()?,
        &csv::StringRecord::from(vec![
            "date",
            "start_local",
            "end_local",
            "duration_seconds",
            "project",
            "activity",
            "note",
            "source"
        ])
    );
    let rows = reader.records().collect::<Result<Vec<_>, _>>()?;
    assert_eq!(rows.len(), 5);
    for (index, source) in ["Timer", "Manual", "IdleReassignment", "Recovery"]
        .iter()
        .enumerate()
    {
        assert_eq!(&rows[index][3], "1");
        assert_eq!(&rows[index][4], project.name);
        assert_eq!(&rows[index][5], activity.name);
        assert_eq!(&rows[index][6], entries[index].note);
        assert_eq!(&rows[index][7], *source);
        assert_eq!(
            chrono::DateTime::parse_from_rfc3339(&rows[index][1])?.timestamp_millis(),
            0
        );
        // RFC3339 retains the entry's subsecond precision.
        assert_eq!(
            chrono::DateTime::parse_from_rfc3339(&rows[index][2])?.timestamp_millis(),
            1999
        );
    }
    assert_eq!(&rows[4][4], "(missing)");
    assert_eq!(&rows[4][5], "");
    Ok(())
}

#[test]
fn writer_errors_are_propagated() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("broken"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("broken"))
        }
    }
    assert!(
        matches!(write_csv(Broken, &[], &[], &[]), Err(AppError::Csv(error)) if error.is_io_error())
    );
}

#[test]
fn file_export_replaces_existing_file_and_reports_path_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("nested/export.csv");
    write_csv_path(&path, &[], &[], &[])?;
    let expected = std::fs::read(&path)?;
    assert_eq!(
        expected,
        b"date,start_local,end_local,duration_seconds,project,activity,note,source\n"
    );
    std::fs::write(&path, b"old")?;
    write_csv_path(&path, &[], &[], &[])?;
    assert_eq!(std::fs::read(&path)?, expected);
    assert!(
        matches!(write_csv_path(directory.path(), &[], &[], &[]), Err(AppError::Io { path, .. }) if path == directory.path())
    );
    assert!(matches!(
        write_csv_path(&path.join("child"), &[], &[], &[]),
        Err(AppError::Io { .. })
    ));
    Ok(())
}
