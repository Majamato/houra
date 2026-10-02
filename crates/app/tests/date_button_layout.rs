//! The date display below "Your day" must read as a clickable date picker.

const WINDOW_UI: &str = include_str!("../../../data/ui/window.ui");
const STYLE_CSS: &str = include_str!("../../../data/style.css");

fn day_button_line() -> &'static str {
    WINDOW_UI
        .lines()
        .find(|line| line.contains(r#"id="day_button""#))
        .unwrap_or_else(|| panic!("day_button missing from window.ui"))
}

#[test]
fn date_button_carries_a_calendar_icon_and_a_label() {
    let line = day_button_line();
    assert!(
        line.contains("x-office-calendar-symbolic"),
        "date button must carry a calendar icon"
    );
    assert!(
        line.contains(r#"id="day_label""#),
        "date button must keep a label child for the formatted date"
    );
}

#[test]
fn date_button_hints_it_opens_a_picker() {
    let line = day_button_line();
    assert!(
        line.contains("pan-down-symbolic"),
        "date button must hint that it drops down"
    );
    assert!(
        line.contains("Choose date"),
        "date button must keep its tooltip"
    );
}

#[test]
fn date_button_highlights_on_hover() {
    assert!(
        STYLE_CSS.contains(".day-date:hover"),
        "date button must highlight on hover so it reads as clickable"
    );
}
