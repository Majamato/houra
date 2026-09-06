//! Application services for Houra.
//!
//! The storage actor is available without GTK, which lets database and backup
//! behavior run in CI or on a server. `native-ui` adds the GNOME presentation
//! and platform integrations.

pub mod actor;
pub mod autostart;
pub mod backup;
pub mod error;
pub mod export;
pub mod settings;
pub mod storage;

pub use actor::{TrackerHandle, TrackerService};
pub use autostart::{default_path, set_enabled};
pub use backup::BackupDocument;
pub use error::AppError;
pub use export::{write_csv, write_csv_path};
pub use settings::Preferences;
pub use storage::Store;

/// Reverse-DNS application ID shared by GApplication, the desktop file,
/// icons, GSettings, resources, notifications, and package metadata.
pub const APP_ID: &str = "io.github.majamato.Houra";
pub const APP_NAME: &str = "Houra";
