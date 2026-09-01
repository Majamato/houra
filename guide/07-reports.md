# Chapter 7 — Reports

**Goal.** The last piece of the core: `validate_no_overlaps` (used by backup
validation later) and `group_entries` (the weekly report), with their tests.
After this chapter the core crate is complete and identical to the original.
Files: `crates/core/src/report.rs`, `crates/core/tests/report.rs`,
`lib.rs`, `Cargo.toml`.

**You will learn**

- Slices `&[T]` and vectors of references `Vec<&T>`.
- Sorting, `windows(2)`, `dedup`, and why sorted neighbours are enough.
- `BTreeMap` and its entry API; derived `Ord` as a sort key.
- Consuming iterators: `into_iter().map(...).collect()`.
- `chrono`: local dates, and splitting at local midnight (DST-safe).

**Prerequisite.** Chapter 6 checkpoint passed.

---

## 7.1 Overlap validation

```toml
# crates/core/Cargo.toml
[dependencies]
chrono.workspace = true
serde.workspace = true
thiserror.workspace = true
```

```rust
// crates/core/src/report.rs
//! Overlap validation and report aggregation.

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
```

**What.** Given any list of entries, find the ones that overlap and report
their IDs. Adjacent entries (`[0,10)` then `[10,20)`) are not overlaps.

**Why sort first.** Once sorted by start, an entry can only overlap its
immediate neighbour: if `B` starts before `A` ends, they overlap; if it does
not, nothing after `B` can either. One pass over neighbours instead of
comparing every pair.

**Rust — slices.** `&[TimeEntry]` is a *slice*: a borrowed view of a
contiguous sequence, whatever owns it — a `Vec`, an array, part of either.
Taking `&[T]` instead of `&Vec<T>` accepts all of them. Chapter 2's `&str`
is the same idea for text.

**Rust — `Vec<&TimeEntry>`.** `entries.iter()` yields references;
`.collect()` gathers them into a vector *of references*. Sorting that
vector reorders pointers, not entries — the input is untouched and nothing
is cloned. The type annotation is needed because `collect` can build many
collection types (Exercise 2 shows what happens if you ask for the wrong
one).

**Rust — `sort_by_key`.** Sorts by the value the closure returns; a tuple
`(start_ms, end_ms)` compares field by field, so ties on start are broken by
end. `windows(2)` yields overlapping pairs `[a,b], [b,c], ...`; `pair[0]` and
`pair[1]` index into that 2-element slice. `sort_unstable` is a faster sort
that does not preserve the order of equal elements — fine for IDs about to
be deduplicated. `dedup` removes *consecutive* duplicates, hence the sort
before it.

**Rust — `if let Some(id) = pair[0].id`.** `id` is `Option<EntryId>`; the
pattern binds the ID only when present. `EntryId` is `Copy`, so reading it
out of the borrowed entry copies it.

## 7.2 Buckets and rows

```rust
// crates/core/src/report.rs
// ...

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
```

**What.** A report row is "on this local day, for this project and task,
this much time across this many entries". The day is stored as year plus
*ordinal* (1–366) — no month/day arithmetic needed, and it sorts correctly.

**Rust — derived `Ord`.** `#[derive(PartialOrd, Ord)]` compares fields in
declaration order: year, then ordinal, then project, then task. Declaring
the fields in the order you want rows sorted *is* the sort specification.
`Option<TaskId>` is ordered too: `None` sorts before any `Some`. The struct
is `Copy` because all its fields are.

**Rust — `usize`.** The unsigned integer type for counts and indexes; its
size matches the machine's pointer width.

## 7.3 Grouping by local day

```rust
// crates/core/src/report.rs
// ...

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
```

**What.** Walk each entry with a cursor. At each step, find the local day
the cursor is in, add time up to the next local midnight (or the entry's
end, whichever is first) to that day's bucket, and move the cursor there. An
entry from 23:00 to 01:00 contributes one hour to each of two days
(Exercise 3).

**Why "local midnight", not "+86 400 000 ms".** On the day clocks change,
a local day is 23 or 25 hours long. Computing the next midnight through the
calendar instead of adding a constant keeps reports correct on those days.
Times are stored in UTC and only converted to local time here, at the edge.

**Rust — `BTreeMap`.** A map kept sorted by key (a balanced tree), unlike
`HashMap`, which has arbitrary order. Since `ReportBucket` is `Ord`, the
rows come out already sorted by day, project, task — the UI can render them
directly (Exercise 4).

**Rust — the entry API.** `totals.entry(bucket).or_default()` returns a
`&mut (i64, usize)`: the existing value for that key, or a freshly inserted
`(0, 0)`. One lookup for "get or insert", and the borrow lets you update in
place.

**Rust — `while` and `break` with `let ... else`.** The `else` of a
`let ... else` must diverge; inside a loop, `break` counts. A timestamp
chrono cannot place (extreme values) ends the walk for that entry rather
than panicking.

**Rust — consuming the map.** `totals.into_iter()` *moves* the map's
contents out as `(key, value)` pairs (the map is gone afterwards — it is not
needed). `.map(|(bucket, (duration_ms, entry_count))| ...)` destructures the
pair *and* the tuple inside it in the closure parameter. `.collect()` builds
the `Vec<ReportRow>` named by the return type.

**Rust — chrono in three lines.** `Local.timestamp_millis_opt(ms)` turns
epoch milliseconds into a local `DateTime`; `.single()` gives `Some` only if
the conversion is unambiguous. `date_naive()` drops the time zone;
`succ_opt()` is tomorrow's date; `and_hms_opt(0, 0, 0)` is midnight;
`from_local_datetime(...).earliest()` re-attaches the zone, picking the
earlier instant if that midnight happens twice (it never does, but the API
makes you decide). Each step is an `Option`, chained with `and_then`, ended
with `map_or` and a fallback of `i64::MAX` ("no next midnight" means the
whole remaining entry goes into this day).

**Idiom.** Store UTC, compute in UTC, convert to local only for display and
calendar boundaries — and do the conversion in one small function you can
test.

## 7.4 Export and test

```rust
// crates/core/src/lib.rs
// ...
mod clock;
mod engine;
mod error;
mod model;
mod report;

pub use clock::{Clock, ManualClock, SystemClock};
pub use engine::TrackerEngine;
pub use error::DomainError;
pub use model::*;
pub use report::{ReportBucket, ReportRow, group_entries, validate_no_overlaps};
```

```rust
// crates/core/tests/report.rs
use work_time_core::{
    EntryId, EntrySource, ProjectId, TimeEntry, group_entries, validate_no_overlaps,
};

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
```

**What.** A fixture function builds entries with only the fields that vary.
Three facts: adjacent is fine; overlaps name both IDs; splitting an entry
over days never changes the total.

**Rust — arrays and slices.** `&[entry(1, 0, 10), entry(2, 10, 20)]` is a
borrowed 2-element array, accepted by the `&[TimeEntry]` parameter.
`entries[0]` indexes an array. `let total: i64 = ....sum();` — the annotation
on the binding tells `sum` its type (an alternative to the turbofish).

## 7.5 Checkpoint — end of Part 1

```sh
cargo test -p work-time-core
```

Expected: `properties.rs` 2 passed, `report.rs` 3 passed, `transitions.rs`
5 passed (plus any keepers). Then the Part 1 boundary checks:

```sh
cargo clippy -p work-time-core --all-targets -- -D warnings
cargo fmt --all -- --check
for f in src/lib.rs src/model.rs src/error.rs src/clock.rs src/engine.rs src/report.rs \
         tests/transitions.rs tests/properties.rs tests/report.rs; do
  echo "== $f"
  diff <(grep -v '^\s*//' ../work_time_tracker/crates/core/$f) \
       <(grep -v '^\s*//' crates/core/$f)
done
```

Acceptable differences: the position of `use crate::...` lines in `model.rs`
and `error.rs`, blank lines, doc-comment wording, and any exercise keepers.
Everything else must be identical — you have rebuilt the whole domain crate.

```sh
git add -A && git commit -m "Chapter 7: reports (core complete)"
```

## 7.6 Exercises

1. **Half-open matters.** Change `<` to `<=` in `if pair[1].start_ms <
   pair[0].end_ms` and run `cargo test -p work-time-core --test report`.

   <details><summary>Answer</summary>

   ```
   test adjacent_entries_do_not_overlap ... FAILED
   assertion failed: validate_no_overlaps(&[entry(1, 0, 10), entry(2, 10, 20)]).is_ok()
   ```

   With `<=`, an entry ending at 10 and one starting at 10 would be
   reported as overlapping, and back-to-back tracking would be impossible.
   The SQL triggers in Chapter 9 encode the same `<` rule.
   </details>

2. **`collect` needs the right target.** Change `Vec<&TimeEntry>` to
   `Vec<TimeEntry>` in `validate_no_overlaps` and build.

   <details><summary>Answer</summary>

   ```
   error[E0277]: a value of type `Vec<model::TimeEntry>` cannot be built from an iterator over elements of type `&model::TimeEntry`
   help: the trait `FromIterator<&model::TimeEntry>` is not implemented for `Vec<model::TimeEntry>`
         but trait `FromIterator<model::TimeEntry>` is implemented for it
   ```

   `iter()` yields `&T`; collecting owned `T`s would require cloning each
   one (`.cloned().collect()`). The code deliberately collects references —
   sorting pointers is cheaper and the caller keeps its entries.
   </details>

3. **Midnight split (optional keeper).** Append to `tests/report.rs`:

   ```rust
   #[test]
   fn entries_crossing_midnight_are_split_per_local_day() {
       use chrono::{Local, TimeZone};

       let start = Local
           .with_ymd_and_hms(2026, 3, 1, 23, 0, 0)
           .single()
           .map_or(0, |value| value.timestamp_millis());
       let end = Local
           .with_ymd_and_hms(2026, 3, 2, 1, 0, 0)
           .single()
           .map_or(0, |value| value.timestamp_millis());
       let rows = group_entries(&[entry(1, start, end)]);
       assert_eq!(rows.len(), 2);
       assert_eq!(rows[0].duration_ms, 3_600_000);
       assert_eq!(rows[1].duration_ms, 3_600_000);
       assert_eq!(rows[0].bucket.local_ordinal + 1, rows[1].bucket.local_ordinal);
       assert_eq!(rows[0].entry_count, 1);
   }
   ```

   <details><summary>Answer</summary>

   Passes. `chrono` is a regular dependency of the crate, so tests can use
   it directly; a `use` inside a function body is legal and keeps the import
   local. One entry became two rows on consecutive ordinals, each counting
   the entry once.
   </details>

4. **Rows come out sorted.** Append:

   ```rust
   #[test]
   fn rows_come_out_sorted_by_bucket() {
       let mut second = entry(2, 1_700_000_000_000, 1_700_000_600_000);
       second.project_id = ProjectId(2);
       let first = entry(1, 1_700_000_600_000, 1_700_001_200_000);
       let rows = group_entries(&[first, second]);
       for row in &rows {
           println!("{:?} -> {} ms", row.bucket, row.duration_ms);
       }
       panic!("show");
   }
   ```

   Run `cargo test -p work-time-core --test report sorted`.

   <details><summary>Answer</summary>

   ```
   ReportBucket { local_year: 2023, local_ordinal: 318, project_id: ProjectId(1), task_id: None } -> 600000 ms
   ReportBucket { local_year: 2023, local_ordinal: 318, project_id: ProjectId(2), task_id: None } -> 600000 ms
   ```

   The input had project 2 first; the output is ordered by the derived
   `Ord` on `ReportBucket` (same day, then project 1 before 2) because
   `BTreeMap` iterates in key order. Revert this one; it only prints.
   </details>

## Recap

- `&[T]` accepts anything contiguous; `Vec<&T>` sorts without cloning.
- Sorted neighbours are enough to find every overlap; half-open intervals
  make adjacency legal.
- `BTreeMap<ReportBucket, _>` with a derived `Ord` gives sorted report rows
  for free; the entry API accumulates in place.
- Local midnight is computed through the calendar, so DST days are right.
- The core crate is complete: pure, deterministic, tested in milliseconds.

Next: **Chapter 8 — App crate**, where errors get an application-level type
and `main` learns where the database lives.
