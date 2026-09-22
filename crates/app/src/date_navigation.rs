use chrono::{Datelike, Duration, NaiveDate};

pub(crate) fn week_start(date: NaiveDate) -> Option<NaiveDate> {
    let days_from_start = date.weekday().num_days_from_monday();
    date.checked_sub_signed(Duration::days(i64::from(days_from_start)))
}

pub(crate) fn visible_week_start(today: NaiveDate, week_offset: i32) -> Option<NaiveDate> {
    week_start(today)?.checked_add_signed(Duration::weeks(i64::from(week_offset.min(0))))
}

pub(crate) fn week_offset_for_date(date: NaiveDate, today: NaiveDate) -> Option<i32> {
    let current_start = week_start(today)?;
    let selected_start = week_start(date)?;
    i32::try_from(
        selected_start
            .signed_duration_since(current_start)
            .num_weeks()
            .min(0),
    )
    .ok()
}

pub(crate) fn previous_week_offset(offset: i32) -> i32 {
    offset.min(0).saturating_sub(1)
}

pub(crate) fn next_week_offset(offset: i32) -> i32 {
    offset.saturating_add(1).min(0)
}

pub(crate) fn date_can_be_selected(date: NaiveDate, today: NaiveDate) -> bool {
    date <= today
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap_or(NaiveDate::MIN)
    }

    #[test]
    fn monday_weeks_cross_month_and_year_boundaries() {
        let today = date(2026, 1, 1);
        assert_eq!(week_start(today), Some(date(2025, 12, 29)));
        assert_eq!(visible_week_start(today, -1), Some(date(2025, 12, 22)));
        assert_eq!(week_offset_for_date(date(2025, 12, 28), today), Some(-1));
        assert_eq!(week_start(date(2026, 5, 1)), Some(date(2026, 4, 27)));
    }

    #[test]
    fn next_week_stops_at_the_current_week() {
        assert_eq!(previous_week_offset(0), -1);
        assert_eq!(next_week_offset(-2), -1);
        assert_eq!(next_week_offset(-1), 0);
        assert_eq!(next_week_offset(0), 0);
        assert_eq!(
            visible_week_start(date(2026, 9, 22), 1),
            Some(date(2026, 9, 21))
        );
    }

    #[test]
    fn future_dates_cannot_be_selected() {
        let today = date(2026, 9, 22);
        assert!(date_can_be_selected(today, today));
        assert!(date_can_be_selected(date(2026, 9, 21), today));
        assert!(!date_can_be_selected(date(2026, 9, 23), today));
    }
}
