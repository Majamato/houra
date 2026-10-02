//! The entry row must expose delete and continue actions with the expected affordances.

const ENTRY_ROW_UI: &str = include_str!("../../../data/ui/entry-row.ui");

fn position_of(marker: &str) -> usize {
    ENTRY_ROW_UI
        .find(marker)
        .unwrap_or_else(|| panic!("{marker} missing from entry-row.ui"))
}

#[test]
fn delete_button_has_trash_icon_and_tooltip() {
    position_of(r#"<object class="GtkButton" id="delete_button">"#);
    assert!(
        ENTRY_ROW_UI.contains(r#"<property name="icon-name">user-trash-symbolic</property>"#),
        "delete button must use the trash icon"
    );
    assert!(
        ENTRY_ROW_UI.contains("Delete this time entry"),
        "delete button must explain its action"
    );
}

#[test]
fn continue_button_is_icon_only() {
    let line = ENTRY_ROW_UI
        .lines()
        .find(|line| line.contains(r#"id="continue_button""#))
        .unwrap_or_else(|| panic!("continue_button missing from entry-row.ui"));
    assert!(
        line.contains("media-playback-start-symbolic"),
        "continue button must carry a start icon"
    );
    assert!(
        line.contains("Continue this work"),
        "continue button must keep its tooltip"
    );
    assert!(
        !line.contains(">Continue<"),
        "continue button must not carry a text label"
    );
}

#[test]
fn delete_action_comes_before_continue() {
    let delete = position_of(r#"id="delete_button""#);
    let cont = position_of(r#"id="continue_button""#);
    assert!(
        delete < cont,
        "delete must sit before continue so continue stays last"
    );
}

#[test]
fn status_shares_the_duration_position_before_the_actions() {
    let duration = position_of(r#"id="duration""#);
    let status = position_of(r#"id="tracking_status""#);
    let separator = position_of(r#"id="actions_separator""#);
    let actions = position_of(r#"id="actions_box""#);
    let report = position_of(r#"id="report_button""#);
    assert!(duration < status && status < separator && separator < actions && actions < report);

    let label = &ENTRY_ROW_UI[status..separator];
    assert!(label.contains(r#"<property name="halign">end</property>"#));
    assert!(label.contains(r#"<property name="xalign">1</property>"#));
    assert!(label.contains(r#"<class name="tracking-status"/>"#));
}
