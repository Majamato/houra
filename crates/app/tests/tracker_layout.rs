//! The tracker page must stay anchored to the top of its scrolled window.
//! It used to vertically center, making the page jump while navigating the week.

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

#[test]
fn running_panel_pairs_pause_with_finish() {
    for id in ["pause_button", "stop_button", "tracking_eyebrow"] {
        assert!(
            WINDOW_UI.contains(&format!("id=\"{id}\"")),
            "window.ui must declare #{id}"
        );
    }
    assert!(
        WINDOW_UI.contains(
            "<object class=\"HouraTimerActionButton\" id=\"pause_button\">\
             <property name=\"label\" translatable=\"yes\">Pause</property>"
        ),
        "pause button must default to the Pause label"
    );
    assert!(
        WINDOW_UI.contains(
            "<object class=\"HouraTimerActionButton\" id=\"stop_button\">\
             <property name=\"label\" translatable=\"yes\">Finish</property>"
        ),
        "stop button must carry the Finish label"
    );
    let pause = WINDOW_UI
        .find("id=\"pause_button\"")
        .unwrap_or_else(|| panic!("pause button missing from window.ui"));
    let stop = WINDOW_UI
        .find("id=\"stop_button\"")
        .unwrap_or_else(|| panic!("stop button missing from window.ui"));
    assert!(
        pause < stop,
        "pause must precede finish in the running panel actions"
    );
}

/// One `<object>` element with its place in the nesting tree.
struct UiObject {
    id: Option<String>,
    parent: Option<usize>,
}

/// Parses `<object>` nesting from window.ui. Only object tags affect the
/// stack; properties, children, styles, and text never do.
fn parse_objects(ui: &str) -> Vec<UiObject> {
    let mut objects = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut rest = ui;
    while let Some(open) = rest.find('<') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('>') else {
            break;
        };
        let tag = &rest[..close];
        rest = &rest[close + 1..];
        if tag.starts_with(['?', '!']) {
            continue;
        }
        if let Some(closing) = tag.strip_prefix('/') {
            if is_object_tag(closing.trim_start()) {
                stack.pop();
            }
            continue;
        }
        if !is_object_tag(tag) {
            continue;
        }
        let parent = stack.last().copied();
        objects.push(UiObject {
            id: tag_id(tag).map(str::to_owned),
            parent,
        });
        if !tag.trim_end().ends_with('/') {
            stack.push(objects.len() - 1);
        }
    }
    objects
}

/// Matches `object` exactly, not a longer tag name that shares the prefix.
fn is_object_tag(tag: &str) -> bool {
    tag.strip_prefix("object")
        .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with([' ', '\t', '\n', '\r', '/']))
}

fn tag_id(tag: &str) -> Option<&str> {
    let key = "id=\"";
    let start = tag.find(key)? + key.len();
    let end = start + tag[start..].find('"')?;
    Some(&tag[start..end])
}

fn objects_with_id(objects: &[UiObject], wanted: &str) -> Vec<usize> {
    objects
        .iter()
        .enumerate()
        .filter(|(_, object)| object.id.as_deref() == Some(wanted))
        .map(|(index, _)| index)
        .collect()
}

#[test]
fn tracking_status_sits_beneath_the_counter_outside_the_details_button() {
    let objects = parse_objects(WINDOW_UI);
    assert!(
        objects_with_id(&objects, "active_entry_total_label").is_empty(),
        "active_entry_total_label must be removed from window.ui"
    );
    let eyebrows = objects_with_id(&objects, "tracking_eyebrow");
    assert_eq!(
        eyebrows.len(),
        1,
        "window.ui must declare exactly one tracking_eyebrow"
    );
    let timers = objects_with_id(&objects, "timer_label");
    assert_eq!(
        timers.len(),
        1,
        "window.ui must declare exactly one timer_label"
    );
    let Some(eyebrow) = eyebrows.first().copied() else {
        panic!("tracking_eyebrow missing from window.ui");
    };
    let Some(timer) = timers.first().copied() else {
        panic!("timer_label missing from window.ui");
    };
    assert_eq!(
        objects[eyebrow].parent, objects[timer].parent,
        "timer_label and tracking_eyebrow must share a parent"
    );
    assert!(
        timer < eyebrow,
        "tracking_eyebrow must follow timer_label beneath the counter"
    );
    let mut ancestor = objects[eyebrow].parent;
    while let Some(index) = ancestor {
        assert_ne!(
            objects[index].id.as_deref(),
            Some("active_details_button"),
            "tracking_eyebrow must sit outside active_details_button"
        );
        ancestor = objects[index].parent;
    }
}
