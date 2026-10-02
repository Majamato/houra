//! Dialogs opened by the main window and application actions.

mod backup_restore;
mod entry_editor;
mod entry_report;
mod idle;
mod name_prompt;
mod preferences;
mod quit;
mod recovery;

use std::path::PathBuf;

use gtk::prelude::*;

use crate::AppError;

/// The path the user chose, or `None` when they closed the chooser.
pub(in crate::desktop) fn chosen_path(
    result: Result<gio::File, glib::Error>,
    not_local: impl FnOnce() -> AppError,
) -> Result<Option<PathBuf>, AppError> {
    match result {
        Ok(file) => file.path().map(Some).ok_or_else(not_local),
        Err(error)
            if matches!(
                error.kind::<gtk::DialogError>(),
                Some(gtk::DialogError::Dismissed | gtk::DialogError::Cancelled)
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(AppError::FileChooser(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn not_local() -> AppError {
        AppError::InvalidBackup("requires a local file".into())
    }

    #[test]
    fn closing_the_chooser_is_not_an_error() {
        for kind in [gtk::DialogError::Dismissed, gtk::DialogError::Cancelled] {
            let result = chosen_path(Err(glib::Error::new(kind, "closed")), not_local);
            assert!(matches!(result, Ok(None)));
        }
    }

    #[test]
    fn a_failed_chooser_is_a_file_chooser_error() {
        let result = chosen_path(
            Err(glib::Error::new(
                gtk::DialogError::Failed,
                "portal unavailable",
            )),
            not_local,
        );
        assert!(
            matches!(result, Err(AppError::FileChooser(message)) if message == "portal unavailable")
        );
    }

    #[test]
    fn a_local_file_returns_its_path() {
        let path = PathBuf::from("/tmp/houra-backup.json");
        let result = chosen_path(Ok(gio::File::for_path(&path)), not_local);
        assert!(matches!(result, Ok(Some(chosen)) if chosen == path));
    }

    #[test]
    fn a_file_without_a_local_path_uses_the_callers_error() {
        let file = gio::File::for_uri("https://example.org/houra-backup.json");
        let result = chosen_path(Ok(file), not_local);
        assert!(matches!(result, Err(AppError::InvalidBackup(_))));
    }
}
