//! Everything that can fail at the application boundary, in one error type.

use std::path::PathBuf;

use houra_core::DomainError;
use thiserror::Error;

/// Everything that can go wrong at the application boundary: rejected domain
/// operations plus database, serialization, filesystem, and worker failures.
#[derive(Debug, Error)]
pub enum AppError {
    /// A domain rule rejected the operation.
    #[error(transparent)]
    Domain(#[from] DomainError),

    /// SQLite refused an operation.
    #[error("database operation failed: {0}")]
    Database(#[from] rusqlite::Error),

    /// Backup JSON could not be read or written.
    #[error("JSON operation failed: {0}")]
    Json(#[from] serde_json::Error),

    /// CSV export failed.
    #[error("CSV export failed: {0}")]
    Csv(#[from] csv::Error),

    /// A filesystem operation failed.
    #[error("I/O operation failed for {path}: {source}")]
    Io {
        /// File or directory the failed operation targeted.
        path: PathBuf,
        #[source]
        /// Underlying OS error.
        source: std::io::Error,
    },

    /// The platform data directory is missing or unusable.
    #[error("application data directory is unavailable")]
    DataDirectoryUnavailable,

    /// The storage worker died; the handle can no longer serve requests.
    #[error("the storage worker stopped unexpectedly")]
    WorkerStopped,

    /// Translations could not be loaded at startup.
    #[error("localization setup failed: {0}")]
    Localization(String),

    /// The app could not register with the desktop session.
    #[cfg(feature = "native-ui")]
    #[error("could not register desktop application: {0}")]
    DesktopRegistration(#[source] glib::Error),

    /// The development settings schema could not be loaded. Houra Dev refuses
    /// to start rather than read or write production preferences.
    #[cfg(all(feature = "native-ui", feature = "dev-app"))]
    #[error(
        "could not load the development settings schema from the executable or build directory"
    )]
    DevSettingsUnavailable,

    /// The active timer could not be published for the top-bar extension.
    #[cfg(feature = "native-ui")]
    #[error("could not publish the active timer for the top bar: {0}")]
    TopBar(String),

    /// The file chooser failed for a reason other than the user closing it.
    #[cfg(feature = "native-ui")]
    #[error("could not open the file chooser: {0}")]
    FileChooser(String),

    /// GNOME's idle monitor could not be reached, so idle time is not detected.
    #[cfg(feature = "native-ui")]
    #[error("idle detection is unavailable: {0}")]
    IdleDetection(String),

    /// The weekly report could not be exported as CSV.
    #[cfg(feature = "native-ui")]
    #[error("CSV export failed: {0}")]
    CsvExport(String),

    /// The bundled UI resources could not be loaded.
    #[cfg(feature = "native-ui")]
    #[error("could not load application resources: {0}")]
    Resources(String),

    /// The backup comes from an unknown Houra version.
    #[error("backup version {found} is unsupported; expected {expected}")]
    UnsupportedBackupVersion {
        /// Version found in the backup.
        found: u32,
        /// Version this Houra understands.
        expected: u32,
    },

    /// The database schema is newer or older than expected.
    #[error(
        "database schema {found} is unsupported; Houra requires schema {expected}. Remove the old database file before starting Houra"
    )]
    UnsupportedDatabaseSchema {
        /// Schema version found in the database.
        found: i64,
        /// Schema version this Houra requires.
        expected: i64,
    },

    /// Restore needs a stopped timer; stop it first.
    #[error("restore requires a stopped timer")]
    RestoreWhileActive,

    /// The backup failed validation and was not applied.
    #[error("backup validation failed: {0}")]
    InvalidBackup(String),

    /// The referenced project does not exist.
    #[error("project {0:?} does not exist")]
    InvalidProject(houra_core::ProjectId),

    /// The referenced activity is missing or archived.
    #[error("activity {0:?} does not exist or is archived")]
    InvalidActivity(houra_core::ActivityId),

    /// The item cannot be deleted because history references it.
    #[error("project or activity cannot be permanently deleted because history references it")]
    ReferencedItem,

    /// The time entry cannot be deleted while the active timer tracks it.
    #[error("this time entry is being tracked; stop the timer before deleting it")]
    ActiveTimeEntry,
}

impl AppError {
    /// Builds a filesystem error for the given path.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
