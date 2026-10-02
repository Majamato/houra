//! Houra and its GNOME Shell extension share a D-Bus contract and install layout.

const INTERFACE_XML: &str =
    include_str!("../../../data/dbus/io.github.majamato.Houra.ActiveTimer.xml");
const ACTIVE_TIMER_JS: &str = include_str!("../../../shell-extension/activeTimer.js");
const METADATA: &str = include_str!("../../../shell-extension/metadata.json");
const MESON: &str = include_str!("../../../meson.build");
const POTFILES: &str = include_str!("../../../po/POTFILES.in");

#[test]
fn extension_embeds_the_published_interface() {
    assert!(
        ACTIVE_TIMER_JS.contains(INTERFACE_XML.trim()),
        "shell-extension/activeTimer.js must embed data/dbus/io.github.majamato.Houra.ActiveTimer.xml verbatim"
    );
}

/// `shell-extension/tests/activeTimer.test.js` checks the bus name and object
/// path the extension derives from this ID.
#[test]
fn extension_uses_the_application_id() {
    let declaration = format!("export const APP_ID = '{}';", houra::APP_ID);
    assert!(
        ACTIVE_TIMER_JS.contains(&declaration),
        "missing {declaration}"
    );
}

#[test]
fn extension_metadata_matches_the_application() -> Result<(), serde_json::Error> {
    let metadata: serde_json::Value = serde_json::from_str(METADATA)?;
    assert_eq!(metadata["uuid"], "houra@majamato.github.io");
    assert_eq!(metadata["gettext-domain"], "houra");
    // Clutter.ClickGesture, used by indicator.js, first ships in GNOME 49.
    // README.md's compatibility table lists the same versions.
    assert_eq!(
        metadata["shell-version"],
        serde_json::json!(["49", "50", "51"])
    );
    Ok(())
}

#[test]
fn meson_installs_every_extension_file() -> std::io::Result<()> {
    let directory = concat!(env!("CARGO_MANIFEST_DIR"), "/../../shell-extension");
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            MESON.contains(&format!("'shell-extension/{name}'")),
            "meson.build does not install shell-extension/{name}"
        );
    }
    Ok(())
}

#[test]
fn translatable_top_bar_sources_are_listed() {
    for path in [
        "crates/app/src/desktop/top_bar/mod.rs",
        "shell-extension/indicator.js",
    ] {
        assert!(POTFILES.lines().any(|line| line == path), "{path}");
    }
}
