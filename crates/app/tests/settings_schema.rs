#![cfg(feature = "native-ui")]

//! Verifies the stable and development settings schemas against each other.
//! Uses memory backends so tests never alter real desktop preferences.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use gio::prelude::*;
use houra::{DateFormat, Preferences};

fn manifest() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/app-variants.json");
    let manifest = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("the variant manifest should read: {error}"));
    serde_json::from_str(&manifest)
        .unwrap_or_else(|error| panic!("the variant manifest should parse: {error}"))
}

fn manifest_str(variant: &str, key: &str) -> String {
    manifest()
        .get(variant)
        .unwrap_or_else(|| panic!("the manifest should define {variant}"))
        .get(key)
        .unwrap_or_else(|| panic!("the manifest should define {variant}.{key}"))
        .as_str()
        .unwrap_or_else(|| panic!("{variant}.{key} should be a string"))
        .to_owned()
}

/// Compiles the production schema alone in a temporary directory.
fn compile_stable_schema() -> tempfile::TempDir {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("test schema dir should build: {error}"));
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/io.github.majamato.Houra.gschema.xml");
    std::fs::copy(
        &source,
        directory
            .path()
            .join("io.github.majamato.Houra.gschema.xml"),
    )
    .unwrap_or_else(|error| panic!("the stable schema should copy: {error}"));
    compile(directory.path());
    directory
}

/// Generates the development schema in a temporary directory.
fn prepare_dev_schema() -> tempfile::TempDir {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("test schema dir should build: {error}"));
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/prepare-dev.py");
    let status = std::process::Command::new("python3")
        .arg("-B")
        .arg(&script)
        .arg("--output-dir")
        .arg(directory.path())
        .arg("--schemas-only")
        .status()
        .unwrap_or_else(|error| panic!("dev schemas should prepare: {error}"));
    assert!(status.success(), "dev schemas should prepare");
    assert!(
        directory.path().join("gschemas.compiled").is_file(),
        "dev schemas should compile"
    );
    directory
}

fn compile(directory: &Path) {
    let status = std::process::Command::new("glib-compile-schemas")
        .arg("--strict")
        .arg(format!("--targetdir={}", directory.display()))
        .arg(directory)
        .status()
        .unwrap_or_else(|error| panic!("schema should compile: {error}"));
    assert!(status.success(), "schema should compile");
}

fn source_for(directory: &Path) -> gio::SettingsSchemaSource {
    gio::SettingsSchemaSource::from_directory(directory, None, false)
        .unwrap_or_else(|error| panic!("compiled schema should load: {error}"))
}

fn schema_for(source: &gio::SettingsSchemaSource, schema_id: &str) -> gio::SettingsSchema {
    source
        .lookup(schema_id, false)
        .unwrap_or_else(|| panic!("{schema_id} should exist"))
}

fn settings_for(schema: &gio::SettingsSchema, backend: &gio::SettingsBackend) -> gio::Settings {
    gio::Settings::new_full(schema, Some(backend), None)
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
fn schema_ids_and_paths_differ() {
    let stable_directory = compile_stable_schema();
    let dev_directory = prepare_dev_schema();
    let stable_id = manifest_str("stable", "app_id");
    let dev_id = manifest_str("devel", "app_id");
    assert_ne!(stable_id, dev_id);
    let stable = schema_for(&source_for(stable_directory.path()), &stable_id);
    let dev = schema_for(&source_for(dev_directory.path()), &dev_id);
    assert_eq!(stable.id().as_str(), stable_id);
    assert_eq!(dev.id().as_str(), dev_id);
    assert_eq!(
        stable.path().as_deref(),
        Some(manifest_str("stable", "settings_path").as_str())
    );
    assert_eq!(
        dev.path().as_deref(),
        Some(manifest_str("devel", "settings_path").as_str())
    );
    assert_ne!(stable.path(), dev.path());
}

#[test]
fn key_definitions_agree_except_launch_at_login_default() {
    let stable_directory = compile_stable_schema();
    let dev_directory = prepare_dev_schema();
    let stable = schema_for(
        &source_for(stable_directory.path()),
        &manifest_str("stable", "app_id"),
    );
    let dev = schema_for(
        &source_for(dev_directory.path()),
        &manifest_str("devel", "app_id"),
    );
    assert_eq!(stable.list_keys(), dev.list_keys());
    for key in stable.list_keys() {
        let stable_key = stable.key(&key);
        let dev_key = dev.key(&key);
        assert_eq!(
            stable_key.value_type().as_str(),
            dev_key.value_type().as_str(),
            "{key} type"
        );
        assert_eq!(stable_key.range(), dev_key.range(), "{key} range");
        if key == "launch-at-login" {
            assert_eq!(stable_key.default_value().print(true).as_str(), "true");
            assert_eq!(dev_key.default_value().print(true).as_str(), "false");
        } else {
            assert_eq!(
                stable_key.default_value(),
                dev_key.default_value(),
                "{key} default"
            );
        }
    }
}

#[test]
fn writes_stay_within_their_variant() {
    let stable_directory = compile_stable_schema();
    let dev_directory = prepare_dev_schema();
    let backend = gio::memory_settings_backend_new();
    let stable = settings_for(
        &schema_for(
            &source_for(stable_directory.path()),
            &manifest_str("stable", "app_id"),
        ),
        &backend,
    );
    let dev = settings_for(
        &schema_for(
            &source_for(dev_directory.path()),
            &manifest_str("devel", "app_id"),
        ),
        &backend,
    );
    dev.set_uint("idle-threshold-minutes", 42)
        .unwrap_or_else(|error| panic!("dev values should persist: {error}"));
    assert_eq!(stable.uint("idle-threshold-minutes"), 5);
    stable
        .set_uint("idle-threshold-minutes", 7)
        .unwrap_or_else(|error| panic!("stable values should persist: {error}"));
    assert_eq!(dev.uint("idle-threshold-minutes"), 42);
    dev.set_string("date-format", DateFormat::DayMonthYear.key())
        .unwrap_or_else(|error| panic!("dev values should persist: {error}"));
    assert_eq!(
        stable.string("date-format").as_str(),
        DateFormat::System.key()
    );
    stable.reset("idle-threshold-minutes");
    assert_eq!(dev.uint("idle-threshold-minutes"), 42);
}

#[test]
fn dev_lookup_without_dev_schema_does_not_fall_back() {
    let stable_directory = compile_stable_schema();
    let source = source_for(stable_directory.path());
    assert!(
        source
            .lookup(&manifest_str("devel", "app_id"), false)
            .is_none(),
        "a source with only the stable schema must not resolve the dev schema"
    );
}

fn check_date_format(directory: &Path, schema_id: &str) {
    let backend = gio::memory_settings_backend_new();
    let settings = settings_for(&schema_for(&source_for(directory), schema_id), &backend);
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
    let schema = schema_for(&source_for(directory), schema_id);
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

#[test]
fn date_format_schema_matches_the_preference() {
    let stable_directory = compile_stable_schema();
    check_date_format(stable_directory.path(), &manifest_str("stable", "app_id"));
    let dev_directory = prepare_dev_schema();
    check_date_format(dev_directory.path(), &manifest_str("devel", "app_id"));
}
