#![cfg(feature = "native-ui")]

//! Verifies the compiled settings schema against the date-format preference.
//! Uses a memory backend so tests never alter real desktop preferences.

use std::cell::Cell;
use std::rc::Rc;

use gio::prelude::*;
use houra::{DateFormat, Preferences};

fn test_settings() -> (tempfile::TempDir, gio::Settings) {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("test schema dir should build: {error}"));
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let status = std::process::Command::new("glib-compile-schemas")
        .arg("--strict")
        .arg(format!("--targetdir={}", directory.path().display()))
        .arg(&data)
        .status()
        .unwrap_or_else(|error| panic!("schema should compile: {error}"));
    assert!(status.success(), "schema should compile");
    let source = gio::SettingsSchemaSource::from_directory(directory.path(), None, false)
        .unwrap_or_else(|error| panic!("compiled schema should load: {error}"));
    let schema = source
        .lookup(houra::APP_ID, false)
        .unwrap_or_else(|| panic!("Houra schema should exist"));
    let backend = gio::memory_settings_backend_new();
    let settings = gio::Settings::new_full(&schema, Some(&backend), None);
    (directory, settings)
}

fn allowed_values(key: &gio::SettingsSchemaKey) -> Vec<String> {
    let range = key.range();
    assert_eq!(
        range.child_value(0).get::<String>().as_deref(),
        Some("enum"),
        "date-format should declare choices: {range:?}"
    );
    let detail = range.child_value(1);
    let choices = detail
        .get::<glib::Variant>()
        .unwrap_or_else(|| panic!("date-format range should box its choices: {range:?}"));
    choices
        .get::<Vec<String>>()
        .unwrap_or_else(|| panic!("date-format range should list choices: {range:?}"))
}

#[test]
fn date_format_schema_matches_the_preference() {
    let (directory, settings) = test_settings();
    // The schema default agrees with Preferences.
    assert_eq!(
        settings.string("date-format").as_str(),
        Preferences::default().date_format.key()
    );
    // Every supported key can be written and read, notifying once each time.
    let notifications = Rc::new(Cell::new(0));
    let count = notifications.clone();
    settings.connect_changed(Some("date-format"), move |_, _| {
        count.set(count.get() + 1);
    });
    for format in DateFormat::ALL {
        settings
            .set_string("date-format", format.key())
            .unwrap_or_else(|error| panic!("{format:?} should persist: {error}"));
        assert_eq!(settings.string("date-format").as_str(), format.key());
        assert_eq!(
            DateFormat::from_key(&settings.string("date-format")),
            Some(format)
        );
    }
    assert_eq!(notifications.get(), DateFormat::ALL.len());
    // The declared allowed values match DateFormat::ALL.
    let source = gio::SettingsSchemaSource::from_directory(directory.path(), None, false)
        .unwrap_or_else(|error| panic!("compiled schema should load: {error}"));
    let schema = source
        .lookup(houra::APP_ID, false)
        .unwrap_or_else(|| panic!("Houra schema should exist"));
    assert!(schema.list_keys().iter().any(|key| key == "date-format"));
    let key = schema.key("date-format");
    assert_eq!(
        allowed_values(&key),
        DateFormat::ALL.map(|format| format.key().to_owned())
    );
    // Resetting the key returns to System default.
    settings.reset("date-format");
    assert_eq!(
        settings.string("date-format").as_str(),
        DateFormat::System.key()
    );
    assert_eq!(
        DateFormat::from_key(&settings.string("date-format")),
        Some(DateFormat::System)
    );
}
