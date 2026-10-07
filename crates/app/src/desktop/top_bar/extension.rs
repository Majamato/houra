//! Keeps Houra's GNOME Shell extension enabled so the top bar shows Houra.

use std::path::PathBuf;

use gio::prelude::*;
use tracing::{info, warn};

use crate::identity::EXTENSION_UUID;

/// Adds the extension to GNOME Shell's enabled list when it is installed.
/// A running Shell that already loaded the extension enables it at once;
/// a newly installed extension appears after the next login.
pub(in crate::desktop) fn enable_shell_extension() {
    let mut data_dirs = vec![glib::user_data_dir()];
    data_dirs.extend(glib::system_data_dirs());
    if !extension_installed(&data_dirs) {
        info!(
            uuid = EXTENSION_UUID,
            "the top-bar extension is not installed"
        );
        return;
    }
    let Some(schema) = gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup("org.gnome.shell", true))
    else {
        return; // Not a GNOME Shell session.
    };
    let settings = gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None);
    let enabled = strings(&settings.strv("enabled-extensions"));
    let disabled = strings(&settings.strv("disabled-extensions"));
    if let Some(lists) = enable_extension(&enabled, &disabled, EXTENSION_UUID) {
        info!(
            uuid = EXTENSION_UUID,
            "enabling Houra's GNOME Shell extension"
        );
        for (key, value) in [
            ("enabled-extensions", &lists.enabled),
            ("disabled-extensions", &lists.disabled),
        ] {
            if let Err(error) = settings.set_strv(key, value.as_slice()) {
                warn!(%error, key, "could not enable the top-bar extension");
            }
        }
    }
    if settings.boolean("disable-user-extensions") {
        warn!("GNOME Shell extensions are turned off, so the top bar cannot show Houra");
    }
}

fn strings(values: &glib::StrV) -> Vec<String> {
    values.iter().map(ToString::to_string).collect()
}

fn extension_installed(data_dirs: &[PathBuf]) -> bool {
    data_dirs.iter().any(|directory| {
        directory
            .join("gnome-shell/extensions")
            .join(EXTENSION_UUID)
            .join("metadata.json")
            .is_file()
    })
}

/// GNOME Shell's extension lists after enabling one extension.
#[derive(Debug, PartialEq, Eq)]
struct ExtensionLists {
    enabled: Vec<String>,
    disabled: Vec<String>,
}

/// The updated lists, or `None` when the extension is already enabled.
/// GNOME Shell ignores an enabled extension that is also listed as disabled.
fn enable_extension(enabled: &[String], disabled: &[String], uuid: &str) -> Option<ExtensionLists> {
    let listed = enabled.iter().any(|item| item == uuid);
    let blocked = disabled.iter().any(|item| item == uuid);
    if listed && !blocked {
        return None;
    }
    let mut enabled = enabled.to_vec();
    if !listed {
        enabled.push(uuid.to_owned());
    }
    let disabled = disabled
        .iter()
        .filter(|item| *item != uuid)
        .cloned()
        .collect();
    Some(ExtensionLists { enabled, disabled })
}

#[cfg(test)]
mod tests {
    use super::*;

    const METADATA: &str = include_str!("../../../../../shell-extension/metadata.json");

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn enabled_extension_needs_no_change() {
        let enabled = list(&["other@example.com", EXTENSION_UUID]);
        assert_eq!(
            enable_extension(&enabled, &list(&["x@example.com"]), EXTENSION_UUID),
            None
        );
    }

    #[test]
    fn missing_extension_is_appended_after_the_others() {
        assert_eq!(
            enable_extension(
                &list(&["a@example.com", "b@example.com"]),
                &[],
                EXTENSION_UUID
            ),
            Some(ExtensionLists {
                enabled: list(&["a@example.com", "b@example.com", EXTENSION_UUID]),
                disabled: Vec::new(),
            })
        );
    }

    #[test]
    fn disabled_listing_is_removed() {
        let disabled = list(&["a@example.com", EXTENSION_UUID]);
        assert_eq!(
            enable_extension(&list(&[EXTENSION_UUID]), &disabled, EXTENSION_UUID),
            Some(ExtensionLists {
                enabled: list(&[EXTENSION_UUID]),
                disabled: list(&["a@example.com"]),
            })
        );
        assert_eq!(
            enable_extension(&[], &disabled, EXTENSION_UUID),
            Some(ExtensionLists {
                enabled: list(&[EXTENSION_UUID]),
                disabled: list(&["a@example.com"]),
            })
        );
    }

    #[test]
    fn installation_is_found_in_any_data_directory() -> Result<(), Box<dyn std::error::Error>> {
        let empty = tempfile::tempdir()?;
        let installed = tempfile::tempdir()?;
        let directory = installed
            .path()
            .join("gnome-shell/extensions")
            .join(EXTENSION_UUID);
        std::fs::create_dir_all(&directory)?;
        assert!(!extension_installed(&[empty.path().to_path_buf()]));
        std::fs::write(directory.join("metadata.json"), "{}")?;
        assert!(extension_installed(&[
            empty.path().to_path_buf(),
            installed.path().to_path_buf(),
        ]));
        Ok(())
    }

    fn manifest_uuid(variant: &str) -> String {
        crate::identity::test_manifest_value(variant, "extension_uuid")
            .as_str()
            .unwrap_or_else(|| panic!("the manifest should define {variant}.extension_uuid"))
            .to_owned()
    }

    #[test]
    fn uuid_matches_the_selected_variant() {
        assert_eq!(EXTENSION_UUID, manifest_uuid(crate::identity::APP_VARIANT));
    }

    #[test]
    fn uuid_matches_the_extension_metadata() -> Result<(), Box<dyn std::error::Error>> {
        let metadata: serde_json::Value = serde_json::from_str(METADATA)?;
        assert_eq!(metadata["uuid"], manifest_uuid("stable"));
        Ok(())
    }

    #[test]
    fn enabling_dev_preserves_production_listings() {
        let stable = manifest_uuid("stable");
        let dev = manifest_uuid("devel");
        assert_ne!(stable, dev);
        // Production stays enabled when development is appended.
        assert_eq!(
            enable_extension(&list(&[&stable]), &[], &dev),
            Some(ExtensionLists {
                enabled: list(&[&stable, &dev]),
                disabled: Vec::new(),
            })
        );
        // Production stays disabled-listed when development is enabled.
        assert_eq!(
            enable_extension(&[], &list(&[&stable, &dev]), &dev),
            Some(ExtensionLists {
                enabled: list(&[&dev]),
                disabled: list(&[&stable]),
            })
        );
    }
}
