use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Local, TimeZone};
use serde::{Deserialize, Serialize};

use crate::{ActivityId, DomainError, ProjectId, TimeEntry};

/// Rejects entries whose half-open intervals overlap; adjacent ones are fine.
pub fn validate_no_overlaps(entries: &[TimeEntry]) -> Result<(), DomainError> {
    let mut ordered: Vec<&TimeEntry> = entries.iter().collect();
    ordered.sort_by_key(|entry| (entry.start_ms, entry.end_ms));
    let mut conflicts = Vec::new();
    let mut found_overlap = false;
    // An earlier long interval can overlap entries beyond its immediate neighbor.
    let mut furthest_end = i64::MIN;
    for (index, entry) in ordered.iter().enumerate() {
        let overlaps_previous = entry.start_ms < furthest_end;
        let overlaps_next = ordered
            .get(index + 1)
            .is_some_and(|next| next.start_ms < entry.end_ms);
        if overlaps_previous || overlaps_next {
            found_overlap = true;
            if let Some(id) = entry.id {
                conflicts.push(id);
            }
        }
        furthest_end = furthest_end.max(entry.end_ms);
    }
    conflicts.sort_unstable();
    conflicts.dedup();
    if found_overlap {
        Err(DomainError::Overlap { conflicts })
    } else {
        Ok(())
    }
}

/// One local day, project and activity. Derived `Ord` sorts by those fields in order.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ReportBucket {
    pub local_year: i32,
    pub local_ordinal: u32,
    pub project_id: ProjectId,
    pub activity_id: Option<ActivityId>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ReportRow {
    pub bucket: ReportBucket,
    pub duration_ms: i64,
    pub entry_count: usize,
}

/// Groups entries by local day, project, and activity.
///
/// Entries crossing midnight are split at the local boundary, including DST
/// days whose actual length is not 24 hours.
pub fn group_entries(entries: &[TimeEntry]) -> Vec<ReportRow> {
    let mut totals: BTreeMap<ReportBucket, (i64, usize)> = BTreeMap::new();
    for entry in entries {
        let mut cursor = entry.start_ms;
        while cursor < entry.end_ms {
            let Some(local) = Local.timestamp_millis_opt(cursor).single() else {
                break;
            };
            let next_midnight = next_local_midnight(local).min(entry.end_ms);
            let bucket = ReportBucket {
                local_year: local.year(),
                local_ordinal: local.ordinal(),
                project_id: entry.project_id,
                activity_id: entry.activity_id,
            };
            let total = totals.entry(bucket).or_default();
            total.0 = total.0.saturating_add(next_midnight.saturating_sub(cursor));
            total.1 = total.1.saturating_add(1);
            cursor = next_midnight;
        }
    }
    totals
        .into_iter()
        .map(|(bucket, (duration_ms, entry_count))| ReportRow {
            bucket,
            duration_ms,
            entry_count,
        })
        .collect()
}

fn next_local_midnight(local: DateTime<Local>) -> i64 {
    let next_date = local.date_naive().succ_opt();
    next_date
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .and_then(|naive| Local.from_local_datetime(&naive).earliest())
        .map_or(i64::MAX, |date| date.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntryId, EntrySource};

    fn entry(id: Option<i64>, start_ms: i64, end_ms: i64) -> TimeEntry {
        TimeEntry {
            id: id.map(EntryId),
            project_id: ProjectId(1),
            activity_id: None,
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
        assert!(validate_no_overlaps(&[entry(Some(1), 0, 10), entry(Some(2), 10, 20)]).is_ok());
    }

    #[test]
    fn overlapping_entries_report_both_ids() {
        let result = validate_no_overlaps(&[entry(Some(1), 0, 11), entry(Some(2), 10, 20)]);
        assert_eq!(
            result,
            Err(DomainError::Overlap {
                conflicts: vec![EntryId(1), EntryId(2)]
            })
        );
    }

    #[test]
    fn grouping_preserves_total_duration() {
        let entries = [entry(Some(1), 1_700_000_000_000, 1_700_100_000_000)];
        let total: i64 = group_entries(&entries)
            .iter()
            .map(|row| row.duration_ms)
            .sum();
        assert_eq!(total, entries[0].duration_ms());
    }
    #[test]
    fn anonymous_overlaps_are_rejected() {
        assert_eq!(
            validate_no_overlaps(&[entry(None, 0, 10), entry(None, 5, 15)]),
            Err(DomainError::Overlap { conflicts: vec![] })
        );
    }
    #[test]
    fn nested_unordered_and_duplicate_ids_report_every_known_conflict() {
        assert_eq!(
            validate_no_overlaps(&[
                entry(Some(3), 8, 9),
                entry(Some(2), 2, 3),
                entry(Some(1), 0, 10),
                entry(Some(2), 2, 3)
            ]),
            Err(DomainError::Overlap {
                conflicts: vec![EntryId(1), EntryId(2), EntryId(3)]
            })
        );
    }
    #[test]
    fn local_days_and_dst_in_isolated_processes() {
        if let Ok(zone) = std::env::var("HOURA_REPORT_TEST_ZONE") {
            for (month, day, ordinal, dst_hours) in [(3, 10, 70, 23), (11, 3, 308, 25)] {
                let start = Local
                    .with_ymd_and_hms(2024, month, day, 0, 0, 0)
                    .single()
                    .unwrap_or_else(|| panic!("invalid fixture"));
                let end = Local
                    .with_ymd_and_hms(2024, month, day + 1, 0, 0, 0)
                    .single()
                    .unwrap_or_else(|| panic!("invalid fixture"));
                let entry = TimeEntry {
                    id: None,
                    project_id: ProjectId(1),
                    activity_id: None,
                    note: String::new(),
                    start_ms: start.timestamp_millis(),
                    end_ms: end.timestamp_millis() + 1000,
                    source: EntrySource::Manual,
                    created_at_ms: 0,
                    updated_at_ms: 0,
                };
                let mut other = entry.clone();
                other.end_ms = other.start_ms + 1000;
                other.activity_id = Some(ActivityId(2));
                let mut project = other.clone();
                project.project_id = ProjectId(2);
                let rows = group_entries(&[entry.clone(), entry, other, project]);
                assert_eq!(
                    rows,
                    vec![
                        ReportRow {
                            bucket: ReportBucket {
                                local_year: 2024,
                                local_ordinal: ordinal,
                                project_id: ProjectId(1),
                                activity_id: None
                            },
                            duration_ms: if zone == "UTC" {
                                172_800_000
                            } else {
                                dst_hours * 7_200_000
                            },
                            entry_count: 2
                        },
                        ReportRow {
                            bucket: ReportBucket {
                                local_year: 2024,
                                local_ordinal: ordinal,
                                project_id: ProjectId(1),
                                activity_id: Some(ActivityId(2))
                            },
                            duration_ms: 1000,
                            entry_count: 1
                        },
                        ReportRow {
                            bucket: ReportBucket {
                                local_year: 2024,
                                local_ordinal: ordinal,
                                project_id: ProjectId(2),
                                activity_id: Some(ActivityId(2))
                            },
                            duration_ms: 1000,
                            entry_count: 1
                        },
                        ReportRow {
                            bucket: ReportBucket {
                                local_year: 2024,
                                local_ordinal: ordinal + 1,
                                project_id: ProjectId(1),
                                activity_id: None
                            },
                            duration_ms: 2000,
                            entry_count: 2
                        },
                    ]
                );
            }
            return;
        }
        for zone in ["UTC", "America/New_York"] {
            let status = std::process::Command::new(
                std::env::current_exe().unwrap_or_else(|e| panic!("{e}")),
            )
            .args([
                "--exact",
                "reports::tests::local_days_and_dst_in_isolated_processes",
            ])
            .env("TZ", zone)
            .env("HOURA_REPORT_TEST_ZONE", zone)
            .status()
            .unwrap_or_else(|e| panic!("{e}"));
            assert!(status.success(), "report child failed for {zone}");
        }
    }
}
