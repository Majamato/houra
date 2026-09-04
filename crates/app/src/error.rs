use std::path::PathBuf;

use houra_core::DomainError;
use thiserror::Error;

/// Everything that can go wrong at the application boundary: rejected domain
/// operations plus database, serialization, filesystem, and worker failures.
#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error("database operation failed: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("JSON operation failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error("CSV export failed: {0}")]
    Csv(#[from] csv::Error),

    #[error("I/O operation failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("application data directory is unavailable")]
    DataDirectoryUnavailable,

    #[error("the storage worker stopped unexpectedly")]
    WorkerStopped,

    #[error("backup version {found} is unsupported; expected {expected}")]
    UnsupportedBackupVersion { found: u32, expected: u32 },

    #[error("restore requires a stopped timer")]
    RestoreWhileActive,

    #[error("backup validation failed: {0}")]
    InvalidBackup(String),

    #[error("project {0:?} does not exist or is archived")]
    InvalidProject(houra_core::ProjectId),

    #[error("task {0:?} does not exist, is archived, or belongs to another project")]
    InvalidTask(houra_core::TaskId),

    #[error("project/task cannot be permanently deleted because history references it")]
    ReferencedItem,
}

impl AppError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
