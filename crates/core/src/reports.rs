use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Local, TimeZone};
use serde::{Deserialize, Serialize};

use crate::{DomainError, ProjectId, TaskId, TimeEntry};

/// Rejects entries whose half-open intervals overlap; adjacent ones are fine.
pub fn validate_no_overlaps(entries: &[TimeEntry]) -> Result<(), DomainError> {
    let mut ordered: Vec<&TimeEntry> = entries.iter().collect();
    ordered.sort_by_key(|entry| (entry.start_ms, entry.end_ms));
    let mut conflicts = Vec::new();
    for pair in ordered.windows(2) {
        if pair[1].start_ms < pair[0].end_ms {
            if let Some(id) = pair[0].id {
                conflicts.push(id);
            }
            if let Some(id) = pair[1].id {
                conflicts.push(id);
            }
        }
    }
    conflicts.sort_unstable();
    conflicts.dedup();
    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(DomainError::Overlap { conflicts })
    }
}

/// One local day, project and task. Derived `Ord` sorts by those fields in order.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ReportBucket {
    pub local_year: i32,
    pub local_ordinal: u32,
    pub project_id: ProjectId,
    pub task_id: Option<TaskId>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ReportRow {
    pub bucket: ReportBucket,
    pub duration_ms: i64,
    pub entry_count: usize,
}

/// Groups entries by local day, project, and task.
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
                task_id: entry.task_id,
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
