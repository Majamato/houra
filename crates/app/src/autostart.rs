use std::fs;
use std::path::{Path, PathBuf};

use directories::BaseDirs;

use crate::AppError;

const FILE_NAME: &str = "io.github.majamato.Houra.desktop";

/// `$XDG_CONFIG_HOME/autostart/<app id>.desktop`.
pub fn default_path() -> Result<PathBuf, AppError> {
    let base = BaseDirs::new().ok_or(AppError::DataDirectoryUnavailable)?;
    Ok(base.config_dir().join("autostart").join(FILE_NAME))
}

/// Writes or removes the per-user autostart launcher.
pub fn set_enabled(enabled: bool, executable: &Path) -> Result<(), AppError> {
    set_enabled_at(&default_path()?, enabled, executable)
}

fn set_enabled_at(path: &Path, enabled: bool, executable: &Path) -> Result<(), AppError> {
    if enabled {
        let Some(parent) = path.parent() else {
            return Err(AppError::DataDirectoryUnavailable);
        };
        fs::create_dir_all(parent).map_err(|source| AppError::io(parent, source))?;
        let escaped = executable.to_string_lossy().replace(' ', "\\ ");
        let desktop = format!(
            "[Desktop Entry]\nType=Application\nName=Houra\nExec={escaped} --background\nIcon=io.github.majamato.Houra\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n"
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
    fn launcher_creation_replacement_and_repeated_removal() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("autostart").join(FILE_NAME);
        set_enabled_at(&path, false, Path::new("unused"))?;
        for executable in ["/tmp/my app/houra", "/tmp/replaced"] {
            set_enabled_at(&path, true, Path::new(executable))?;
            let escaped = executable.replace(' ', "\\ ");
            assert_eq!(
                fs::read_to_string(&path)?,
                format!(
                    "[Desktop Entry]\nType=Application\nName=Houra\nExec={escaped} --background\nIcon=io.github.majamato.Houra\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n"
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
            matches!(set_enabled_at(&parent.join(FILE_NAME), true, Path::new("houra")), Err(AppError::Io { path, .. }) if path == parent)
        );
        let path = directory.path().join(FILE_NAME);
        fs::create_dir(&path)?;
        for enabled in [true, false] {
            assert!(
                matches!(set_enabled_at(&path, enabled, Path::new("houra")), Err(AppError::Io { path: failed, .. }) if failed == path)
            );
        }
        Ok(())
    }
}
