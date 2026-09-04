//! Domain rules for Houra.
//!
//! This crate knows nothing about GTK, SQLite, D-Bus, or the filesystem.
mod clock;
mod engine;
mod error;
mod id;
mod idle;
mod notification;
mod reports;
mod time;
mod tracker;
mod validation;
mod work;

pub use clock::{Clock, ManualClock, SystemClock};
pub use engine::TrackerEngine;
pub use error::DomainError;
pub use id::{EntryId, ProjectId, TaskId};
pub use idle::IdleDecision;
pub use notification::Notification;
pub use reports::{group_entries, validate_no_overlaps, ReportBucket, ReportRow};
pub use time::{ActiveTimer, EntrySource, PendingIdle, PendingRecovery, TimeEntry};
pub use tracker::{TrackerCommand, TrackerSnapshot, TrackerState, Transition};
pub use validation::{validate_color, validate_name};
pub use work::{Project, Task};
