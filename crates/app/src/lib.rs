#![warn(missing_docs)]
//! Application services for Houra: storage worker, tracker service, export, backup.
//! Builds without GTK; `native-ui` adds the GNOME presentation and platform integrations.

pub mod autostart;
pub mod backup;
pub mod date_format;
#[cfg(any(feature = "native-ui", test))]
mod date_navigation;
pub mod error;
pub mod export;
pub mod identity;
pub mod settings;
pub mod storage;
pub mod tracker_service;

#[cfg(feature = "native-ui")]
pub mod desktop;
#[cfg(feature = "native-ui")]
pub mod locale;

pub use autostart::{default_path, set_enabled};
pub use backup::BackupDocument;
pub use error::AppError;
pub use export::{write_csv, write_csv_path};
pub use identity::{APP_ID, APP_NAME};
pub use settings::{DateFormat, Preferences};
pub use storage::Store;
pub use tracker_service::{TrackerHandle, TrackerService};
