//! The tracker page must stay anchored to the top of its scrolled window.
//!
//! The tracker content used to be vertically centered, so days with few time
//! entries rendered lower on screen than days with many entries and the whole
//! page jumped while navigating the week.

const WINDOW_UI: &str = include_str!("../../../data/ui/window.ui");

/// Properties declared on an `AdwClamp` before its first child element.
fn clamp_header(ui: &str, maximum_size: u32) -> &str {
    let marker = format!(
        "<object class=\"AdwClamp\"><property name=\"maximum-size\">{maximum_size}</property>"
    );
    let start = ui.find(marker.as_str()).unwrap_or_else(|| {
        panic!("AdwClamp with maximum-size {maximum_size} missing from window.ui")
    });
    let rest = &ui[start..];
    let end = rest
        .find("<property name=\"child\">")
        .unwrap_or_else(|| panic!("AdwClamp {maximum_size} must declare a child"));
    &rest[..end]
}

#[test]
fn tracker_content_is_top_aligned() {
    let header = clamp_header(WINDOW_UI, 710);
    assert!(
        header.contains("<property name=\"valign\">start</property>"),
        "tracker clamp must pin content to the top, got: {header}"
    );
}

#[test]
fn no_page_clamp_is_vertically_centered() {
    for maximum_size in [710, 620] {
        let header = clamp_header(WINDOW_UI, maximum_size);
        assert!(
            !header.contains("<property name=\"valign\">center</property>"),
            "clamp {maximum_size} must not center content vertically"
        );
    }
}
