use crate::TrackerHandle;
use chrono::Local;
use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::window::MainWindow;

impl MainWindow {
    pub fn backup_data(&self) {
        let Some(handle) = self.handle() else { return };
        let chooser = gtk::FileDialog::builder()
            .title("Back Up Houra")
            .initial_name(format!(
                "houra-backup-{}.json",
                Local::now().format("%Y-%m-%d")
            ))
            .build();
        let weak = self.downgrade();
        chooser.save(Some(self), None::<&gio::Cancellable>, move |result| {
            let Some(window) = weak.upgrade() else { return };
            let result = result
                .map_err(|error| crate::AppError::InvalidBackup(error.to_string()))
                .and_then(|file| {
                    let path = file.path().ok_or_else(|| {
                        crate::AppError::InvalidBackup("backup requires a local file".into())
                    })?;
                    let document = handle.backup(chrono::Utc::now().timestamp_millis())?;
                    document.write_to_path(&path)
                });
            if let Err(error) = result {
                window.show_database_error(&error.to_string());
            }
        });
    }

    pub fn restore_data(&self) {
        let Some(handle) = self.handle() else { return };
        if handle
            .snapshot()
            .is_ok_and(|snapshot| snapshot.state.active().is_some())
        {
            self.show_database_error("Stop the timer before restoring a backup.");
            return;
        }
        let chooser = gtk::FileDialog::builder()
            .title("Choose a Houra Backup")
            .build();
        let weak = self.downgrade();
        chooser.open(Some(self), None::<&gio::Cancellable>, move |result| {
            let Some(window) = weak.upgrade() else { return };
            let document = result
                .map_err(|error| crate::AppError::InvalidBackup(error.to_string()))
                .and_then(|file| {
                    let path = file.path().ok_or_else(|| {
                        crate::AppError::InvalidBackup("restore requires a local file".into())
                    })?;
                    crate::backup::BackupDocument::read_from_path(&path)
                });
            match document {
                Ok(document) => window.confirm_restore(handle.clone(), document),
                Err(error) => window.show_database_error(&error.to_string()),
            }
        });
    }

    fn confirm_restore(&self, handle: TrackerHandle, document: crate::backup::BackupDocument) {
        let dialog = adw::AlertDialog::builder()
            .heading("Replace all local data?")
            .body("The validated backup will replace projects, tasks, and entries. This cannot be undone.")
            .build();
        dialog.add_responses(&[("cancel", "Cancel"), ("restore", "Replace Data")]);
        dialog.set_response_appearance("restore", adw::ResponseAppearance::Destructive);
        let weak = self.downgrade();
        dialog.connect_response(Some("restore"), move |_, _| {
            let result = handle.restore(document.clone());
            if let Some(window) = weak.upgrade() {
                if let Err(error) = result {
                    window.show_database_error(&error.to_string());
                }
                window.reload_projects();
                window.reload_tasks();
                window.refresh();
                window.refresh_projects_page();
                window.refresh_report();
            }
        });
        dialog.present(Some(self));
    }
}
