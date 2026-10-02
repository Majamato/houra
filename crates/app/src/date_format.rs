//! Preference-aware calendar-date formatting and timestamp parsing.
//!
//! Every complete-date display and CSV date value goes through this module,
//! with the [`DateFormat`](crate::settings::DateFormat) passed explicitly.
//! This module never loads GSettings and keeps no global preference.

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike};

use crate::settings::DateFormat;

/// Formats a calendar date in the preferred representation.
///
/// With [`DateFormat::System`] under `native-ui`, this is the current system
/// locale's date (`%x`). Headless builds use ISO `%Y-%m-%d` instead because
/// they do not initialize GLib's locale facilities.
pub fn format_date(date: NaiveDate, format: DateFormat) -> String {
    match format {
        DateFormat::DayMonthYear => date.format("%d/%m/%Y").to_string(),
        DateFormat::MonthDayYear => date.format("%m/%d/%Y").to_string(),
        DateFormat::YearMonthDay => date.format("%Y-%m-%d").to_string(),
        DateFormat::System => system_date(date),
    }
}

#[cfg(feature = "native-ui")]
fn system_date(date: NaiveDate) -> String {
    crate::locale::ui_date(date, "%x")
}

#[cfg(not(feature = "native-ui"))]
fn system_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Formats one date, or the two complete dates when the range spans days.
pub fn format_date_range(first: NaiveDate, last: NaiveDate, format: DateFormat) -> String {
    if first == last {
        format_date(first, format)
    } else {
        format!(
            "{} – {}",
            format_date(first, format),
            format_date(last, format)
        )
    }
}

/// Combines the localized full weekday with the preferred complete date,
/// as in `Thursday, 17/09/2026`.
#[cfg(feature = "native-ui")]
pub fn format_date_with_weekday(date: NaiveDate, format: DateFormat) -> String {
    format!(
        "{}, {}",
        crate::locale::ui_date(date, "%A"),
        format_date(date, format)
    )
}

/// Formats wall-clock milliseconds as a preferred local date plus the
/// existing locale clock text; raw milliseconds when unrepresentable.
/// Headless builds use `HH:MM:SS` for the clock portion.
pub fn format_ui_datetime(ms: i64, format: DateFormat) -> String {
    let Some(local) = Local.timestamp_millis_opt(ms).single() else {
        return ms.to_string();
    };
    #[cfg(feature = "native-ui")]
    let clock = crate::locale::ui_datetime(ms, "%X");
    #[cfg(not(feature = "native-ui"))]
    let clock = local.format("%H:%M:%S").to_string();
    format!("{} {clock}", format_date(local.date_naive(), format))
}

/// Formats wall-clock milliseconds as a preferred local date plus the
/// existing 12-hour CSV clock text; empty when unrepresentable.
pub fn format_csv_datetime(ms: i64, format: DateFormat) -> String {
    let Some(local) = Local.timestamp_millis_opt(ms).single() else {
        return String::new();
    };
    format!(
        "{} {}",
        format_date(local.date_naive(), format),
        local.format("%-I:%M %p")
    )
}

/// Editable-timestamp format, captured when a dialog opens so a preference
/// change cannot reinterpret partially edited text.
#[derive(Clone, Debug)]
pub struct LocalTimestampFormat {
    pattern: String,
}

impl LocalTimestampFormat {
    /// Resolves the input pattern for the preference once. System inputs use
    /// the system's date order and separators with a four-digit year,
    /// followed by `HH:MM:SS`.
    pub fn new(format: DateFormat) -> Self {
        let date_pattern = match format {
            DateFormat::DayMonthYear => "%d/%m/%Y".to_owned(),
            DateFormat::MonthDayYear => "%m/%d/%Y".to_owned(),
            DateFormat::YearMonthDay => "%Y-%m-%d".to_owned(),
            DateFormat::System => system_input_pattern(),
        };
        Self {
            pattern: format!("{date_pattern} %H:%M:%S"),
        }
    }

    /// Formats wall-clock milliseconds for editing; empty when unrepresentable.
    pub fn format(&self, ms: i64) -> String {
        Local
            .timestamp_millis_opt(ms)
            .single()
            .map_or_else(String::new, |time| time.format(&self.pattern).to_string())
    }

    /// Parses edited text into milliseconds, requiring one unambiguous local
    /// instant. Rejects nonexistent and ambiguous daylight-saving times.
    pub fn parse(&self, text: &str) -> Option<i64> {
        let trimmed = text.trim();
        let value = NaiveDateTime::parse_from_str(trimmed, &self.pattern).ok()?;
        // Chrono represents a parsed leap second as nanosecond 1_000_000_000.
        if !(1..=9999).contains(&value.year())
            || value.second() > 59
            || value.nanosecond() >= 1_000_000_000
        {
            return None;
        }
        // Reject incomplete years, missing padding, and trailing text.
        if value.format(&self.pattern).to_string() != trimmed {
            return None;
        }
        Local
            .from_local_datetime(&value)
            .single()
            .map(|time| time.timestamp_millis())
    }

    /// Shows the civil example September 17, 2026 at 14:05:06 in this format.
    /// Explanatory text, so it never shifts with the time zone.
    pub fn example(&self) -> String {
        NaiveDate::from_ymd_opt(2026, 9, 17)
            .and_then(|date| date.and_hms_opt(14, 5, 6))
            .map_or_else(String::new, |civil| civil.format(&self.pattern).to_string())
    }
}

#[cfg(feature = "native-ui")]
fn system_input_pattern() -> String {
    let first = probe(2006, 11, 22);
    let second = probe(2007, 3, 4);
    input_pattern_from_probes(&first, &second).unwrap_or_else(|| "%Y-%m-%d".to_owned())
}

#[cfg(not(feature = "native-ui"))]
fn system_input_pattern() -> String {
    "%Y-%m-%d".to_owned()
}

/// Renders a probe date through the system locale, or an empty marker the
/// derivation below always rejects.
#[cfg(feature = "native-ui")]
fn probe(year: i32, month: u32, day: u32) -> String {
    NaiveDate::from_ymd_opt(year, month, day)
        .map_or_else(String::new, |date| crate::locale::ui_date(date, "%x"))
}

/// Derives a Chrono date pattern from two rendered system probes. The second
/// probe confirms the order and separators found in the first; anything
/// unidentified falls back to ISO at the call site.
#[cfg(any(feature = "native-ui", test))]
fn input_pattern_from_probes(first: &str, second: &str) -> Option<String> {
    let pattern = pattern_from_probe(first, &["2006", "06"], &["11"], &["22"])?;
    let confirm = pattern_from_probe(second, &["2007", "07"], &["03", "3"], &["04", "4"])?;
    (pattern == confirm).then_some(pattern)
}

/// Maps one rendered probe to a Chrono date pattern. The probe must hold
/// exactly three ASCII digit runs: one year, one month, and one day token.
/// Literal separators are preserved; literal `%` characters are escaped.
#[cfg(any(feature = "native-ui", test))]
fn pattern_from_probe(
    text: &str,
    years: &[&str],
    months: &[&str],
    days: &[&str],
) -> Option<String> {
    enum Part<'a> {
        Digits(&'a str),
        Literal(&'a str),
    }
    let mut parts: Vec<Part<'_>> = Vec::new();
    let mut start = 0;
    let mut digit_run: Option<bool> = None;
    for (index, ch) in text.char_indices() {
        let is_digit = ch.is_ascii_digit();
        match digit_run {
            None => {
                start = index;
                digit_run = Some(is_digit);
            }
            Some(current) if current == is_digit => {}
            Some(current) => {
                let part = &text[start..index];
                parts.push(if current {
                    Part::Digits(part)
                } else {
                    Part::Literal(part)
                });
                start = index;
                digit_run = Some(is_digit);
            }
        }
    }
    if let Some(current) = digit_run {
        let part = &text[start..];
        parts.push(if current {
            Part::Digits(part)
        } else {
            Part::Literal(part)
        });
    }
    let runs = parts
        .iter()
        .filter_map(|part| match part {
            Part::Digits(run) => Some(*run),
            Part::Literal(_) => None,
        })
        .count();
    if runs != 3 {
        return None;
    }
    let mut seen_year = false;
    let mut seen_month = false;
    let mut seen_day = false;
    let mut pattern = String::new();
    for part in &parts {
        match part {
            Part::Literal(literal) => {
                for ch in literal.chars() {
                    if ch == '%' {
                        pattern.push_str("%%");
                    } else {
                        pattern.push(ch);
                    }
                }
            }
            Part::Digits(run) => {
                let is_year = years.contains(run);
                let is_month = months.contains(run);
                let is_day = days.contains(run);
                if u8::from(is_year) + u8::from(is_month) + u8::from(is_day) != 1 {
                    return None;
                }
                if is_year {
                    if seen_year {
                        return None;
                    }
                    seen_year = true;
                    pattern.push_str("%Y");
                } else if is_month {
                    if seen_month {
                        return None;
                    }
                    seen_month = true;
                    pattern.push_str("%m");
                } else {
                    if seen_day {
                        return None;
                    }
                    seen_day = true;
                    pattern.push_str("%d");
                }
            }
        }
    }
    (seen_year && seen_month && seen_day).then_some(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day)
            .unwrap_or_else(|| panic!("test date should exist"))
    }

    fn local_ms(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .unwrap_or_else(|| panic!("test local time should exist"))
            .timestamp_millis()
    }

    /// A local midnight, skipping the rare day a zone transition removes it.
    fn local_midnight_ms() -> (NaiveDate, i64) {
        for day in 17..24 {
            if let Some(date) = NaiveDate::from_ymd_opt(2026, 9, day)
                && let Some(midnight) = date.and_hms_opt(0, 0, 0)
                && let Some(time) = Local.from_local_datetime(&midnight).single()
            {
                return (date, time.timestamp_millis());
            }
        }
        panic!("a local midnight near September 2026 should exist")
    }

    #[test]
    fn numeric_formats_render_expected_dates() {
        let day = date(2026, 9, 17);
        assert_eq!(format_date(day, DateFormat::DayMonthYear), "17/09/2026");
        assert_eq!(format_date(day, DateFormat::MonthDayYear), "09/17/2026");
        assert_eq!(format_date(day, DateFormat::YearMonthDay), "2026-09-17");
    }

    #[test]
    fn numeric_formats_pad_leap_days_and_year_boundaries() {
        let day = date(2026, 1, 5);
        assert_eq!(format_date(day, DateFormat::DayMonthYear), "05/01/2026");
        assert_eq!(format_date(day, DateFormat::MonthDayYear), "01/05/2026");
        let leap = date(2024, 2, 29);
        assert_eq!(format_date(leap, DateFormat::DayMonthYear), "29/02/2024");
        assert_eq!(
            format_date_range(
                date(2026, 12, 31),
                date(2027, 1, 1),
                DateFormat::DayMonthYear
            ),
            "31/12/2026 – 01/01/2027"
        );
        assert_eq!(
            format_date_range(
                date(2026, 12, 31),
                date(2027, 1, 1),
                DateFormat::MonthDayYear
            ),
            "12/31/2026 – 01/01/2027"
        );
    }

    #[test]
    fn equal_dates_render_once() {
        let day = date(2026, 9, 17);
        for format in DateFormat::ALL {
            assert_eq!(
                format_date_range(day, day, format),
                format_date(day, format)
            );
        }
    }

    #[test]
    fn csv_datetimes_keep_the_existing_clock_near_midnight() {
        let (midnight, ms) = local_midnight_ms();
        let before = date(
            midnight.year(),
            midnight.month(),
            midnight.day().saturating_sub(1),
        );
        assert_eq!(
            format_csv_datetime(ms - 1, DateFormat::YearMonthDay),
            format!("{} 11:59 PM", before.format("%Y-%m-%d"))
        );
        assert_eq!(
            format_csv_datetime(ms, DateFormat::YearMonthDay),
            format!("{} 12:00 AM", midnight.format("%Y-%m-%d"))
        );
        assert_eq!(
            format_csv_datetime(ms - 1, DateFormat::DayMonthYear),
            format!("{} 11:59 PM", before.format("%d/%m/%Y"))
        );
    }

    #[test]
    fn ui_datetimes_keep_the_preferred_date_near_midnight() {
        let (midnight, ms) = local_midnight_ms();
        let before = date(
            midnight.year(),
            midnight.month(),
            midnight.day().saturating_sub(1),
        );
        assert!(
            format_ui_datetime(ms - 1, DateFormat::YearMonthDay)
                .starts_with(&format!("{} ", before.format("%Y-%m-%d")))
        );
        assert!(
            format_ui_datetime(ms, DateFormat::YearMonthDay)
                .starts_with(&format!("{} ", midnight.format("%Y-%m-%d")))
        );
    }

    #[test]
    fn unrepresentable_timestamps_keep_existing_fallbacks() {
        for format in DateFormat::ALL {
            assert_eq!(format_csv_datetime(i64::MAX, format), "");
            assert_eq!(format_ui_datetime(i64::MAX, format), i64::MAX.to_string());
            assert_eq!(LocalTimestampFormat::new(format).format(i64::MAX), "");
        }
    }

    #[test]
    fn explicit_timestamp_inputs_round_trip() {
        let ms = local_ms(2026, 9, 17, 14, 5, 6);
        for (format, text) in [
            (DateFormat::DayMonthYear, "17/09/2026 14:05:06"),
            (DateFormat::MonthDayYear, "09/17/2026 14:05:06"),
            (DateFormat::YearMonthDay, "2026-09-17 14:05:06"),
        ] {
            let input = LocalTimestampFormat::new(format);
            assert_eq!(input.format(ms), text);
            assert_eq!(input.parse(text), Some(ms));
            assert_eq!(input.example(), text);
        }
    }

    #[test]
    fn timestamp_parsing_rejects_invalid_dates_and_clocks() {
        let input = LocalTimestampFormat::new(DateFormat::YearMonthDay);
        for text in [
            "2026-02-30 14:05:06",
            "2026-09-17 14:05",
            "26-09-17 14:05:06",
            "2026-09-17 14:05:06 trailing",
            "2026-09-17 25:05:06",
            "2026-09-17 14:61:06",
            "2026-09-17 14:05:60",
            "2026-9-7 14:05:06",
            "2026-09-17  14:05:06",
            "2026-09-17 4:05:06",
            "",
        ] {
            assert_eq!(input.parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn timestamp_parsing_ignores_surrounding_whitespace() {
        let input = LocalTimestampFormat::new(DateFormat::YearMonthDay);
        assert_eq!(
            input.parse("  2026-09-17 14:05:06\t"),
            Some(local_ms(2026, 9, 17, 14, 5, 6))
        );
    }

    #[test]
    fn timestamp_parsing_follows_day_month_order() {
        let day_first = LocalTimestampFormat::new(DateFormat::DayMonthYear);
        let month_first = LocalTimestampFormat::new(DateFormat::MonthDayYear);
        assert_eq!(
            day_first.parse("17/09/2026 10:00:00"),
            Some(local_ms(2026, 9, 17, 10, 0, 0))
        );
        assert_eq!(month_first.parse("17/09/2026 10:00:00"), None);
        assert_eq!(
            day_first.parse("03/04/2026 10:00:00"),
            Some(local_ms(2026, 4, 3, 10, 0, 0))
        );
        assert_eq!(
            month_first.parse("03/04/2026 10:00:00"),
            Some(local_ms(2026, 3, 4, 10, 0, 0))
        );
    }

    #[test]
    fn probe_derivation_matches_known_system_renders() {
        for (first, second, pattern) in [
            ("11/22/06", "03/04/07", "%m/%d/%Y"),
            ("22/11/06", "04/03/07", "%d/%m/%Y"),
            ("22.11.2006", "04.03.2007", "%d.%m.%Y"),
            ("2006年11月22日", "2007年3月4日", "%Y年%m月%d日"),
            ("2006년 11월 22일", "2007년 3월 4일", "%Y년 %m월 %d일"),
        ] {
            assert_eq!(
                input_pattern_from_probes(first, second).as_deref(),
                Some(pattern)
            );
        }
    }

    #[test]
    fn probe_derivation_rejects_unsafe_renders() {
        // Unidentified, duplicate, missing, and textual-month tokens.
        for (first, second) in [
            ("11/22/99", "03/04/99"),
            ("2006-11-2006", "2007-03-2007"),
            ("11/2006", "03/2007"),
            ("22 November 2006", "04 March 2007"),
            // Non-ASCII digits carry no usable token.
            ("٢٢/١١/٢٠٠٦", "٠٤/٠٣/٢٠٠٧"),
            // A confirming probe that disagrees on order or separators.
            ("11/22/06", "04.03.2007"),
            ("11/22/06", "22/11/06"),
            ("", ""),
        ] {
            assert_eq!(input_pattern_from_probes(first, second), None, "{first:?}");
        }
    }

    #[test]
    fn probe_derivation_escapes_literal_percent() {
        assert_eq!(
            input_pattern_from_probes("11%22%06", "03%04%07").as_deref(),
            Some("%m%%%d%%%Y")
        );
    }

    #[test]
    fn example_never_shifts_with_the_time_zone() {
        for format in [
            DateFormat::DayMonthYear,
            DateFormat::MonthDayYear,
            DateFormat::YearMonthDay,
        ] {
            let example = LocalTimestampFormat::new(format).example();
            assert!(example.ends_with(" 14:05:06"), "{example:?}");
            assert!(example.contains("2026"), "{example:?}");
        }
    }

    #[test]
    #[cfg(not(feature = "native-ui"))]
    fn headless_system_dates_use_iso() {
        let day = date(2026, 9, 17);
        assert_eq!(format_date(day, DateFormat::System), "2026-09-17");
        assert_eq!(
            format_date_range(day, date(2026, 9, 18), DateFormat::System),
            "2026-09-17 – 2026-09-18"
        );
        let ms = local_ms(2026, 9, 17, 14, 5, 6);
        assert_eq!(
            format_ui_datetime(ms, DateFormat::System),
            "2026-09-17 14:05:06"
        );
        assert_eq!(
            format_csv_datetime(ms, DateFormat::System),
            "2026-09-17 2:05 PM"
        );
        let input = LocalTimestampFormat::new(DateFormat::System);
        assert_eq!(input.example(), "2026-09-17 14:05:06");
        assert_eq!(input.parse("2026-09-17 14:05:06"), Some(ms));
    }

    #[test]
    #[cfg(feature = "native-ui")]
    fn weekday_dates_combine_localized_weekday_and_preferred_date() {
        let day = date(2026, 9, 17);
        assert_eq!(
            format_date_with_weekday(day, DateFormat::DayMonthYear),
            format!("{}, 17/09/2026", crate::locale::ui_date(day, "%A"))
        );
        assert_eq!(
            format_date_with_weekday(day, DateFormat::System),
            crate::locale::ui_date(day, "%A, %x")
        );
    }

    #[test]
    #[cfg(feature = "native-ui")]
    fn system_dates_follow_glib() {
        let day = date(2026, 9, 17);
        assert_eq!(
            format_date(day, DateFormat::System),
            crate::locale::ui_date(day, "%x")
        );
        let ms = local_ms(2026, 9, 17, 14, 5, 6);
        assert_eq!(
            format_ui_datetime(ms, DateFormat::System),
            crate::locale::ui_datetime(ms, "%x %X")
        );
    }
}
