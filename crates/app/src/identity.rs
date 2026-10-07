//! Build-selected application identity: the stable or development variant.
//!
//! The explicit `dev-app` Cargo feature selects the development identity, which
//! keeps its database, preferences, launcher, autostart entry, and GNOME Shell
//! extension separate from the installed production app. Debug or release
//! optimization never decides which database the app uses.
//!
//! The values come from `data/app-variants.json`, read by `build.rs`, so the
//! running app never needs the manifest file. [`RESOURCE_BASE_PATH`] and
//! [`ACTIVE_TIMER_INTERFACE`] stay shared: both variants expose the same
//! resources and D-Bus contract.

use std::path::{Path, PathBuf};

use directories::BaseDirs;

use crate::AppError;

include!(concat!(env!("OUT_DIR"), "/app_identity.rs"));

/// Resource prefix both variants share. Changing the application ID otherwise
/// changes GApplication's default resource base path, so development builds
/// set this path explicitly.
pub const RESOURCE_BASE_PATH: &str = "/io/github/majamato/Houra";
/// D-Bus interface both variants expose for their top-bar extension, under
/// different bus names and object paths.
pub const ACTIVE_TIMER_INTERFACE: &str = "io.github.majamato.Houra.ActiveTimer";

/// Returns this variant's default database path in the local data directory:
/// `~/.local/share/houra/houra.sqlite3` in production,
/// `~/.local/share/houra-dev/houra.sqlite3` in development.
pub fn default_database_path() -> Result<PathBuf, AppError> {
    let base = BaseDirs::new().ok_or(AppError::DataDirectoryUnavailable)?;
    Ok(database_path_in(base.data_local_dir()))
}

fn database_path_in(data_dir: &Path) -> PathBuf {
    data_dir
        .join(DATA_SUBDIR)
        .join(crate::storage::DATABASE_FILENAME)
}

/// Suggests a backup filename for the given `YYYY-MM-DD` date:
/// `houra-backup-<date>.json` in production, `houra-dev-backup-<date>.json`
/// in development. File contents stay shared.
pub fn backup_filename_suggestion(date: &str) -> String {
    format!("{DATA_SUBDIR}-backup-{date}.json")
}

/// Suggests a CSV export filename for the given `YYYY-MM-DD` week start:
/// `houra-<date>.csv` in production, `houra-dev-<date>.csv` in development.
/// File contents stay shared.
pub fn csv_filename_suggestion(date: &str) -> String {
    format!("{DATA_SUBDIR}-{date}.csv")
}

/// Reads one value from the variant manifest for tests.
#[cfg(test)]
pub(crate) fn test_manifest_value(variant: &str, key: &str) -> serde_json::Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/app-variants.json");
    let manifest = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("the variant manifest should read: {error}"));
    let manifest: serde_json::Value = serde_json::from_str(&manifest)
        .unwrap_or_else(|error| panic!("the variant manifest should parse: {error}"));
    manifest
        .get(variant)
        .unwrap_or_else(|| panic!("the manifest should define {variant}"))
        .get(key)
        .unwrap_or_else(|| panic!("the manifest should define {variant}.{key}"))
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::test_manifest_value as manifest_value;

    #[test]
    fn manifest_defines_both_variants() {
        for variant in ["stable", "devel"] {
            for key in [
                "app_id",
                "app_name",
                "data_subdir",
                "settings_path",
                "extension_uuid",
                "launch_at_login_default",
                "extension_gettext_domain",
                "extension_style_prefix",
                "extension_gtype_name",
            ] {
                // Panics when the key is missing.
                let _defined = manifest_value(variant, key);
            }
        }
    }

    #[test]
    fn shared_contracts_stay_fixed() {
        assert_eq!(RESOURCE_BASE_PATH, "/io/github/majamato/Houra");
        assert_eq!(
            ACTIVE_TIMER_INTERFACE,
            "io.github.majamato.Houra.ActiveTimer"
        );
        assert_eq!(crate::storage::DATABASE_FILENAME, "houra.sqlite3");
    }

    #[cfg(not(feature = "dev-app"))]
    #[test]
    fn stable_identity_matches_the_released_values() {
        assert_eq!(APP_VARIANT, "stable");
        assert_eq!(APP_ID, "io.github.majamato.Houra");
        assert_eq!(APP_NAME, "Houra");
        assert_eq!(DATA_SUBDIR, "houra");
        assert_eq!(SETTINGS_PATH, "/io/github/majamato/Houra/");
        assert_eq!(EXTENSION_UUID, "houra@majamato.github.io");
        const {
            assert!(DEFAULT_LAUNCH_AT_LOGIN);
        }
        assert_eq!(
            crate::settings::Preferences::default().launch_at_login,
            DEFAULT_LAUNCH_AT_LOGIN
        );
    }

    #[cfg(feature = "dev-app")]
    #[test]
    fn dev_identity_matches_the_manifest() {
        assert_eq!(APP_VARIANT, "devel");
        for (actual, key) in [
            (APP_ID, "app_id"),
            (APP_NAME, "app_name"),
            (DATA_SUBDIR, "data_subdir"),
            (SETTINGS_PATH, "settings_path"),
            (EXTENSION_UUID, "extension_uuid"),
        ] {
            assert_eq!(manifest_value("devel", key).as_str(), Some(actual), "{key}");
        }
        assert_eq!(
            manifest_value("devel", "launch_at_login_default").as_bool(),
            Some(DEFAULT_LAUNCH_AT_LOGIN)
        );
        const {
            assert!(!DEFAULT_LAUNCH_AT_LOGIN);
        }
    }

    #[test]
    fn database_path_uses_the_variant_subdirectory() {
        for base in [Path::new("/data"), Path::new("/tmp/dir with spaces")] {
            assert_eq!(
                database_path_in(base),
                base.join(DATA_SUBDIR).join("houra.sqlite3")
            );
        }
    }

    #[test]
    fn filename_suggestions_use_the_variant_prefix() {
        assert_eq!(
            backup_filename_suggestion("2026-10-06"),
            format!("{DATA_SUBDIR}-backup-2026-10-06.json")
        );
        assert_eq!(
            csv_filename_suggestion("2026-10-06"),
            format!("{DATA_SUBDIR}-2026-10-06.csv")
        );
        #[cfg(not(feature = "dev-app"))]
        {
            assert_eq!(
                backup_filename_suggestion("2026-10-06"),
                "houra-backup-2026-10-06.json"
            );
            assert_eq!(
                csv_filename_suggestion("2026-10-06"),
                "houra-2026-10-06.csv"
            );
        }
        #[cfg(feature = "dev-app")]
        {
            assert_eq!(
                backup_filename_suggestion("2026-10-06"),
                "houra-dev-backup-2026-10-06.json"
            );
            assert_eq!(
                csv_filename_suggestion("2026-10-06"),
                "houra-dev-2026-10-06.csv"
            );
        }
    }
}
