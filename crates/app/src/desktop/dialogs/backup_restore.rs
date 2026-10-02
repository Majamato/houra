use crate::TrackerHandle;
use crate::locale::tr;
use chrono::Local;
use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use super::chosen_path;
use crate::desktop::window::MainWindow;

impl MainWindow {
    pub fn backup_data(&self) {
        let Some(handle) = self.handle() else { return };
        let chooser = gtk::FileDialog::builder()
            .title(tr("Back Up Houra"))
            .initial_name(format!(
                "houra-backup-{}.json",
                Local::now().format("%Y-%m-%d")
            ))
            .build();
        let weak = self.downgrade();
        chooser.save(Some(self), None::<&gio::Cancellable>, move |result| {
            let Some(window) = weak.upgrade() else { return };
            let Some(path) = chosen_path(result, || {
                crate::AppError::InvalidBackup(tr("backup requires a local file").into())
            })
            .transpose() else {
                return;
            };
            let result = path.and_then(|path| {
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
            self.show_database_error(tr("Stop the timer before restoring a backup."));
            return;
        }
        let chooser = gtk::FileDialog::builder()
            .title(tr("Choose a Houra Backup"))
            .build();
        let weak = self.downgrade();
        chooser.open(Some(self), None::<&gio::Cancellable>, move |result| {
            let Some(window) = weak.upgrade() else { return };
            let Some(path) = chosen_path(result, || {
                crate::AppError::InvalidBackup(tr("restore requires a local file").into())
            })
            .transpose() else {
                return;
            };
            let document =
                path.and_then(|path| crate::backup::BackupDocument::read_from_path(&path));
            match document {
                Ok(document) => window.confirm_restore(handle.clone(), document),
                Err(error) => window.show_database_error(&error.to_string()),
            }
        });
    }

    fn confirm_restore(&self, handle: TrackerHandle, document: crate::backup::BackupDocument) {
        let dialog = adw::AlertDialog::builder()
            .heading(tr("Replace all local data?"))
            .body(tr("The validated backup will replace projects, activities, and entries. This cannot be undone."))
            .build();
        dialog.add_responses(&[("cancel", tr("Cancel")), ("restore", tr("Replace Data"))]);
        dialog.set_response_appearance("restore", adw::ResponseAppearance::Destructive);
        let weak = self.downgrade();
        dialog.connect_response(Some("restore"), move |_, _| {
            let result = handle.restore(document.clone());
            if let Some(window) = weak.upgrade() {
                if let Err(error) = result {
                    window.show_database_error(&error.to_string());
                }
                window.reload_projects();
                window.reload_activities();
                window.refresh();
                window.refresh_projects_page();
                window.refresh_report();
            }
        });
        dialog.present(Some(self));
    }
}
