use houra_core::{EntryId, EntrySource, ProjectId, TimeEntry, group_entries, validate_no_overlaps};

fn entry(id: i64, start_ms: i64, end_ms: i64) -> TimeEntry {
    TimeEntry {
        id: Some(EntryId(id)),
        project_id: ProjectId(1),
        task_id: None,
        note: String::new(),
        start_ms,
        end_ms,
        source: EntrySource::Manual,
        created_at_ms: end_ms,
        updated_at_ms: end_ms,
    }
}

#[test]
fn adjacent_entries_do_not_overlap() {
    assert!(validate_no_overlaps(&[entry(1, 0, 10), entry(2, 10, 20)]).is_ok());
}

#[test]
fn overlapping_entries_report_both_ids() {
    let result = validate_no_overlaps(&[entry(1, 0, 11), entry(2, 10, 20)]);
    let message = result
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(message.contains("EntryId(1)"));
    assert!(message.contains("EntryId(2)"));
}

#[test]
fn grouping_preserves_total_duration() {
    let entries = [entry(1, 1_700_000_000_000, 1_700_100_000_000)];
    let total: i64 = group_entries(&entries)
        .iter()
        .map(|row| row.duration_ms)
        .sum();
    assert_eq!(total, entries[0].duration_ms());
}
