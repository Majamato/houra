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
    let path = default_path()?;
    if enabled {
        let Some(parent) = path.parent() else {
            return Err(AppError::DataDirectoryUnavailable);
        };
        fs::create_dir_all(parent).map_err(|source| AppError::io(parent, source))?;
        let escaped = executable.to_string_lossy().replace(' ', "\\ ");
        let desktop = format!(
            "[Desktop Entry]\nType=Application\nName=Houra\nExec={escaped} --background\nIcon=io.github.majamato.Houra\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n"
        );
        fs::write(&path, desktop).map_err(|source| AppError::io(&path, source))
    } else if path.exists() {
        fs::remove_file(&path).map_err(|source| AppError::io(&path, source))
    } else {
        Ok(())
    }
}
