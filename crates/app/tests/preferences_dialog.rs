//! The preferences dialog must not open with the idle-threshold value selected.

const PREFERENCES_RS: &str = include_str!("../src/desktop/dialogs/preferences.rs");

fn position_of(marker: &str) -> usize {
    PREFERENCES_RS
        .find(marker)
        .unwrap_or_else(|| panic!("{marker} missing from desktop/dialogs/preferences.rs"))
}

#[test]
fn dialog_clears_initial_focus_after_present() {
    let present = position_of("dialog.present(");
    let watch = position_of("connect_focus_widget_notify");
    let clear = position_of("set_focus(Option::<&gtk::Widget>::None)");
    assert!(
        present < watch && watch < clear,
        "preferences must drop the dialog's first focus when it arrives after \
         presenting it, otherwise the idle-threshold value opens selected"
    );
}
