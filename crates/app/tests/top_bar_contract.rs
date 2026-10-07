//! Houra and its GNOME Shell extension share a D-Bus contract and install layout.

const INTERFACE_XML: &str =
    include_str!("../../../data/dbus/io.github.majamato.Houra.ActiveTimer.xml");
const ACTIVE_TIMER_JS: &str = include_str!("../../../shell-extension/activeTimer.js");
const IDENTITY_JS: &str = include_str!("../../../shell-extension/identity.js");
const METADATA: &str = include_str!("../../../shell-extension/metadata.json");
const MANIFEST: &str = include_str!("../../../data/app-variants.json");
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
/// path the extension derives from this ID, for stable and generated dev
/// modules. `shell-extension/tests/identity.test.js` verifies the generated
/// dev identity separately.
#[test]
fn extension_identity_matches_the_stable_manifest() -> Result<(), Box<dyn std::error::Error>> {
    assert!(
        ACTIVE_TIMER_JS.contains("import {APP_ID} from './identity.js';"),
        "activeTimer.js must import APP_ID from identity.js"
    );
    assert!(
        ACTIVE_TIMER_JS.contains("export {APP_ID};"),
        "activeTimer.js must re-export APP_ID"
    );
    assert!(
        ACTIVE_TIMER_JS.contains("export const BUS_NAME = APP_ID;"),
        "activeTimer.js must derive BUS_NAME from APP_ID"
    );
    assert!(
        ACTIVE_TIMER_JS.contains("export const OBJECT_PATH = `/${APP_ID.replaceAll('.', '/')}`;"),
        "activeTimer.js must derive OBJECT_PATH from APP_ID"
    );
    let manifest: serde_json::Value = serde_json::from_str(MANIFEST)?;
    let stable = manifest
        .get("stable")
        .ok_or("the manifest must define stable")?;
    for (key, name) in [
        ("app_id", "APP_ID"),
        ("app_name", "APP_NAME"),
        ("extension_gtype_name", "INDICATOR_GTYPE_NAME"),
        ("extension_style_prefix", "STYLE_PREFIX"),
    ] {
        let value = stable
            .get(key)
            .and_then(serde_json::Value::as_str)
            .ok_or(format!("the manifest must define stable.{key}"))?;
        let declaration = format!("export const {name} = '{value}';");
        assert!(
            IDENTITY_JS.contains(&declaration),
            "shell-extension/identity.js is missing {declaration}"
        );
    }
    assert!(
        IDENTITY_JS.contains("export function styleClass(suffix)"),
        "identity.js must export styleClass"
    );
    Ok(())
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
