use houra::storage::Store;
use houra_core::ProjectId;
use tempfile::TempDir;

fn temporary_store() -> (TempDir, Store) {
    let directory = TempDir::new().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let store = Store::open(&directory.path().join("tracker.sqlite3"))
        .unwrap_or_else(|error| panic!("store failed: {error}"));
    (directory, store)
}

#[test]
fn migration_creates_non_archivable_general_project() {
    let (_directory, store) = temporary_store();
    let projects = store
        .list_projects(false)
        .unwrap_or_else(|error| panic!("list failed: {error}"));
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "General");
    assert!(store.set_project_archived(ProjectId(1), true, 1).is_err());
}

#[test]
fn corrupt_database_returns_a_contextual_error() {
    let directory = TempDir::new().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let path = directory.path().join("corrupt.sqlite3");
    std::fs::write(&path, b"this is not sqlite")
        .unwrap_or_else(|error| panic!("fixture write failed: {error}"));
    assert!(Store::open(&path).is_err());
}
