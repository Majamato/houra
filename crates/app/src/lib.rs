//! Application services for Houra.
//!
//! The storage actor is available without GTK, which lets database and backup
//! behavior run in CI or on a server. `native-ui` adds the GNOME presentation
//! and platform integrations.

pub mod error;
pub mod storage;

pub use error::AppError;
pub use storage::Store;

/// Reverse-DNS application ID shared by GApplication, the desktop file,
/// icons, GSettings, resources, notifications, and package metadata.
pub const APP_ID: &str = "io.github.majamato.Houra";
pub const APP_NAME: &str = "Houra";
