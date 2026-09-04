use std::path::PathBuf;

use directories::BaseDirs;
use houra::{APP_NAME, AppError};
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();
    if let Err(error) = run() {
        tracing::error!(error = %error, "application terminated");
        eprintln!("{APP_NAME}: {error}");
        std::process::exit(1);
    }
}

/// `$XDG_DATA_HOME/houra/tracker.sqlite3`, usually
/// `~/.local/share/houra/tracker.sqlite3`.
fn data_path() -> Result<PathBuf, AppError> {
    let base = BaseDirs::new().ok_or(AppError::DataDirectoryUnavailable)?;
    Ok(base.data_local_dir().join("houra").join("tracker.sqlite3"))
}

#[cfg(feature = "native-ui")]
fn run() -> Result<(), AppError> {
    houra::native::run(data_path()?)
}

#[cfg(not(feature = "native-ui"))]
fn run() -> Result<(), AppError> {
    let _path = data_path()?;
    eprintln!(
        "This build contains the tested storage engine but not the GNOME UI. \
         Rebuild with `--features native-ui` (Meson does this automatically)."
    );
    Ok(())
}
