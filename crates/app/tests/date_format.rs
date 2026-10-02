//! Locale- and time-zone-isolated date-format tests. Every case runs in a
//! child process; the parent never mutates locale or environment.

use chrono::{Local, NaiveDate, TimeZone};
use houra::DateFormat;
#[cfg(feature = "native-ui")]
use houra::date_format::format_ui_datetime;
use houra::date_format::{LocalTimestampFormat, format_csv_datetime, format_date};

fn test_binary() -> std::path::PathBuf {
    std::env::current_exe().unwrap_or_else(|error| panic!("could not locate test binary: {error}"))
}

fn run_child(test: &str, locale: &str, tz: &str) -> std::process::Output {
    std::process::Command::new(test_binary())
        .args(["--exact", test])
        .env("LC_ALL", locale)
        .env("TZ", tz)
        .env("HOURA_TEST_DATE_LOCALE", locale)
        .env("HOURA_TEST_TZ", tz)
        .output()
        .unwrap_or_else(|error| panic!("could not test {test} ({locale}, {tz}): {error}"))
}

/// A local midnight, skipping the rare day a zone transition removes it.
fn local_midnight() -> (NaiveDate, i64) {
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

#[cfg(feature = "native-ui")]
const LOCALES: [&str; 12] = [
    "C",
    "en_US.UTF-8",
    "es_ES.UTF-8",
    "es_CO.UTF-8",
    "pt_BR.UTF-8",
    "fr_FR.UTF-8",
    "de_DE.UTF-8",
    "it_IT.UTF-8",
    "ru_RU.UTF-8",
    "ja_JP.UTF-8",
    "ko_KR.UTF-8",
    "zh_CN.UTF-8",
];

#[cfg(feature = "native-ui")]
#[test]
fn system_dates_follow_each_locale_in_isolated_processes() {
    let mut unavailable = Vec::new();
    for locale in LOCALES {
        let output = run_child("locale_dates_in_child_process", locale, "UTC");
        assert!(
            output.status.success(),
            "{locale} locale dates failed: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        if String::from_utf8_lossy(&output.stderr).contains("HOURA-UNAVAILABLE-LOCALE") {
            unavailable.push(locale);
        }
    }
    if unavailable.is_empty() {
        eprintln!("locale coverage: all {} locales available", LOCALES.len());
    } else {
        eprintln!(
            "locale coverage: unavailable locales: {}",
            unavailable.join(", ")
        );
    }
}

/// Maximal ASCII digit runs classified as year, month, or day tokens.
#[cfg(feature = "native-ui")]
fn run_classes(text: &str, years: &[&str], months: &[&str], days: &[&str]) -> Option<Vec<char>> {
    let mut classes = Vec::new();
    let mut run = String::new();
    for ch in text.chars().chain([' ']) {
        if ch.is_ascii_digit() {
            run.push(ch);
            continue;
        }
        if run.is_empty() {
            continue;
        }
        let year = years.contains(&run.as_str());
        let month = months.contains(&run.as_str());
        let day = days.contains(&run.as_str());
        if u8::from(year) + u8::from(month) + u8::from(day) != 1 {
            return None;
        }
        classes.push(if year {
            'Y'
        } else if month {
            'M'
        } else {
            'D'
        });
        run.clear();
    }
    Some(classes)
}

/// Maximal non-digit spans between the digit runs.
#[cfg(feature = "native-ui")]
fn separators(text: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    for (index, ch) in text.char_indices().chain([(text.len(), ' ')]) {
        if ch.is_ascii_digit() {
            if let Some(begin) = start.take() {
                spans.push(&text[begin..index]);
            }
        } else if start.is_none() {
            start = Some(index);
        }
    }
    spans
}

#[cfg(feature = "native-ui")]
#[test]
fn locale_dates_in_child_process() {
    use gettextrs::{LocaleCategory, setlocale};
    let Ok(locale) = std::env::var("HOURA_TEST_DATE_LOCALE") else {
        return;
    };
    if setlocale(LocaleCategory::LcAll, "").is_none() {
        eprintln!("HOURA-UNAVAILABLE-LOCALE {locale}");
        return;
    }
    assert!(houra::locale::initialize().is_ok());
    let day =
        NaiveDate::from_ymd_opt(2026, 9, 17).unwrap_or_else(|| panic!("test date should exist"));
    // System read-only dates equal GLib %x.
    assert_eq!(
        format_date(day, DateFormat::System),
        houra::locale::ui_date(day, "%x"),
        "locale {locale}"
    );
    // Explicit numeric choices remain independent of locale.
    assert_eq!(format_date(day, DateFormat::DayMonthYear), "17/09/2026");
    assert_eq!(format_date(day, DateFormat::MonthDayYear), "09/17/2026");
    assert_eq!(format_date(day, DateFormat::YearMonthDay), "2026-09-17");
    // System inputs preserve locale order and separators with a four-digit year.
    let input = LocalTimestampFormat::new(DateFormat::System);
    let example = input.example();
    let Some(date_part) = example.strip_suffix(" 14:05:06") else {
        panic!("locale {locale}: unexpected example {example:?}");
    };
    assert!(date_part.contains("2026"), "locale {locale}: {example:?}");
    let probe = houra::locale::ui_date(
        NaiveDate::from_ymd_opt(2006, 11, 22).unwrap_or_else(|| panic!("probe should exist")),
        "%x",
    );
    match (
        run_classes(&probe, &["2006", "06"], &["11"], &["22"]),
        run_classes(date_part, &["2026"], &["09", "9"], &["17"]),
    ) {
        (Some(expected), Some(actual)) => {
            assert_eq!(actual, expected, "locale {locale}: {example:?}");
            assert_eq!(
                separators(date_part),
                separators(&probe),
                "locale {locale}: {example:?}"
            );
        }
        _ => eprintln!("locale {locale}: System input uses the ISO fallback for {probe:?}"),
    }
    // A timestamp round-trips through the System input.
    let ms = Local
        .with_ymd_and_hms(2026, 9, 17, 14, 5, 6)
        .single()
        .unwrap_or_else(|| panic!("test local time should exist"))
        .timestamp_millis();
    assert_eq!(input.parse(&input.format(ms)), Some(ms), "locale {locale}");
    assert_eq!(input.parse(&example), Some(ms), "locale {locale}");
    // UI date-time output retains the existing locale clock text.
    assert_eq!(
        format_ui_datetime(ms, DateFormat::System),
        houra::locale::ui_datetime(ms, "%x %X"),
        "locale {locale}"
    );
}

#[test]
fn timestamp_validation_follows_the_time_zone_in_isolated_processes() {
    for tz in ["UTC", "America/New_York"] {
        let output = run_child("timestamp_validation_in_child_process", "C", tz);
        assert!(
            output.status.success(),
            "{tz} timestamp validation failed: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

fn new_york_zone_available() -> bool {
    Local
        .with_ymd_and_hms(2026, 1, 15, 12, 0, 0)
        .single()
        .is_some_and(|time| time.offset().local_minus_utc() == -5 * 3600)
}

/// System input texts for the daylight-saving probes. Children run in the C
/// locale, where the System pattern is month-first; headless builds use ISO.
#[cfg(feature = "native-ui")]
const SYSTEM_NONEXISTENT: &str = "03/08/2026 02:30:00";
#[cfg(not(feature = "native-ui"))]
const SYSTEM_NONEXISTENT: &str = "2026-03-08 02:30:00";
#[cfg(feature = "native-ui")]
const SYSTEM_AMBIGUOUS: &str = "11/01/2026 01:30:00";
#[cfg(not(feature = "native-ui"))]
const SYSTEM_AMBIGUOUS: &str = "2026-11-01 01:30:00";
#[cfg(feature = "native-ui")]
const SYSTEM_EXAMPLE: &str = "09/17/2026 14:05:06";
#[cfg(not(feature = "native-ui"))]
const SYSTEM_EXAMPLE: &str = "2026-09-17 14:05:06";

#[test]
fn timestamp_validation_in_child_process() {
    let Ok(tz) = std::env::var("HOURA_TEST_TZ") else {
        return;
    };
    if tz != "UTC" && tz != "America/New_York" {
        return;
    }
    if tz == "America/New_York" && !new_york_zone_available() {
        eprintln!("skipping timestamp validation: America/New_York zone data unavailable");
        return;
    }
    // The System probe texts below assume the C-locale derivation.
    assert_eq!(
        LocalTimestampFormat::new(DateFormat::System).example(),
        SYSTEM_EXAMPLE,
        "time zone {tz}"
    );
    let (midnight_date, midnight_ms) = local_midnight();
    let ordinary_ms = Local
        .with_ymd_and_hms(2026, 9, 17, 14, 5, 6)
        .single()
        .unwrap_or_else(|| panic!("test local time should exist"))
        .timestamp_millis();
    for format in DateFormat::ALL {
        // Local dates convert at midnight.
        assert!(
            format_csv_datetime(midnight_ms, format)
                .starts_with(&format_date(midnight_date, format)),
            "time zone {tz}: {format:?}"
        );
        // Valid ordinary local timestamps round-trip.
        let input = LocalTimestampFormat::new(format);
        assert_eq!(
            input.parse(&input.format(ordinary_ms)),
            Some(ordinary_ms),
            "time zone {tz}: {format:?}"
        );
    }
    let probes = [
        (
            DateFormat::DayMonthYear,
            "08/03/2026 02:30:00",
            "01/11/2026 01:30:00",
        ),
        (
            DateFormat::MonthDayYear,
            "03/08/2026 02:30:00",
            "11/01/2026 01:30:00",
        ),
        (
            DateFormat::YearMonthDay,
            "2026-03-08 02:30:00",
            "2026-11-01 01:30:00",
        ),
        (DateFormat::System, SYSTEM_NONEXISTENT, SYSTEM_AMBIGUOUS),
    ];
    if tz == "America/New_York" {
        for (format, nonexistent, ambiguous) in probes {
            let input = LocalTimestampFormat::new(format);
            assert_eq!(input.parse(nonexistent), None, "{format:?}");
            assert_eq!(input.parse(ambiguous), None, "{format:?}");
        }
    } else {
        for (format, nonexistent, ambiguous) in probes {
            let input = LocalTimestampFormat::new(format);
            let spring = Local
                .with_ymd_and_hms(2026, 3, 8, 2, 30, 0)
                .single()
                .unwrap_or_else(|| panic!("UTC spring time should exist"))
                .timestamp_millis();
            let fall = Local
                .with_ymd_and_hms(2026, 11, 1, 1, 30, 0)
                .single()
                .unwrap_or_else(|| panic!("UTC fall time should exist"))
                .timestamp_millis();
            assert_eq!(input.parse(nonexistent), Some(spring), "{format:?}");
            assert_eq!(input.parse(ambiguous), Some(fall), "{format:?}");
        }
    }
}
