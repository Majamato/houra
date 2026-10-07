//! Per-user autostart launcher management.

use std::fs;
use std::path::{Path, PathBuf};

use directories::BaseDirs;

use crate::{APP_ID, APP_NAME, AppError};

/// Returns this variant's autostart launcher filename: `<app id>.desktop`.
fn file_name() -> String {
    format!("{APP_ID}.desktop")
}

/// Returns the autostart launcher path under a supplied config directory.
/// `$XDG_CONFIG_HOME/autostart/<app id>.desktop`; pure for unit testing.
fn path_in(config_dir: &Path) -> PathBuf {
    config_dir.join("autostart").join(file_name())
}

/// Returns the per-user autostart launcher path.
/// `$XDG_CONFIG_HOME/autostart/<app id>.desktop`.
pub fn default_path() -> Result<PathBuf, AppError> {
    let base = BaseDirs::new().ok_or(AppError::DataDirectoryUnavailable)?;
    Ok(path_in(base.config_dir()))
}

/// Writes or removes the per-user autostart launcher.
pub fn set_enabled(enabled: bool, executable: &Path) -> Result<(), AppError> {
    set_enabled_at(&default_path()?, enabled, executable)
}

pub(crate) fn set_enabled_at(
    path: &Path,
    enabled: bool,
    executable: &Path,
) -> Result<(), AppError> {
    if enabled {
        let Some(parent) = path.parent() else {
            return Err(AppError::DataDirectoryUnavailable);
        };
        fs::create_dir_all(parent).map_err(|source| AppError::io(parent, source))?;
        let escaped = executable.to_string_lossy().replace(' ', "\\ ");
        let desktop = format!(
            "[Desktop Entry]\nType=Application\nName={APP_NAME}\nExec={escaped}\nIcon={APP_ID}\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n"
        );
        fs::write(path, desktop).map_err(|source| AppError::io(path, source))
    } else if path.exists() {
        fs::remove_file(path).map_err(|source| AppError::io(path, source))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launcher_path_uses_the_variant_app_id() {
        for base in [Path::new("/config"), Path::new("/tmp/dir with spaces")] {
            assert_eq!(
                path_in(base),
                base.join("autostart").join(format!("{APP_ID}.desktop"))
            );
        }
        #[cfg(not(feature = "dev-app"))]
        assert_eq!(
            path_in(Path::new("/config")),
            Path::new("/config/autostart/io.github.majamato.Houra.desktop")
        );
        #[cfg(feature = "dev-app")]
        assert_eq!(
            path_in(Path::new("/config")),
            Path::new("/config/autostart/io.github.majamato.Houra.Devel.desktop")
        );
    }

    #[test]
    fn launcher_creation_replacement_and_repeated_removal() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        let path = path_in(directory.path());
        set_enabled_at(&path, false, Path::new("unused"))?;
        for executable in ["/tmp/my app/houra", "/tmp/replaced"] {
            set_enabled_at(&path, true, Path::new(executable))?;
            let escaped = executable.replace(' ', "\\ ");
            assert_eq!(
                fs::read_to_string(&path)?,
                format!(
                    "[Desktop Entry]\nType=Application\nName={APP_NAME}\nExec={escaped}\nIcon={APP_ID}\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n"
                )
            );
        }
        set_enabled_at(&path, false, Path::new("unused"))?;
        assert!(!path.exists());
        set_enabled_at(&path, false, Path::new("unused"))?;
        Ok(())
    }
    #[test]
    fn filesystem_failures_include_the_failed_path() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let parent = directory.path().join("file");
        fs::write(&parent, b"block")?;
        assert!(
            matches!(set_enabled_at(&parent.join(file_name()), true, Path::new("houra")), Err(AppError::Io { path, .. }) if path == parent)
        );
        let path = directory.path().join(file_name());
        fs::create_dir(&path)?;
        for enabled in [true, false] {
            assert!(
                matches!(set_enabled_at(&path, enabled, Path::new("houra")), Err(AppError::Io { path: failed, .. }) if failed == path)
            );
        }
        Ok(())
    }
}
