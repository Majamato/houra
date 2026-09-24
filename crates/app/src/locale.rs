//! Runtime gettext configuration.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use chrono::{Datelike, NaiveDate};
use gettextrs::{LocaleCategory, bind_textdomain_codeset, bindtextdomain, setlocale, textdomain};

use crate::AppError;

/// Translate a UI literal once for this process. The system language is fixed at startup.
pub fn tr(message: &'static str) -> &'static str {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, &'static str>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache
        .entry(message)
        .or_insert_with(|| Box::leak(gettextrs::gettext(message).into_boxed_str()))
}

/// Fill named placeholders in a translated UI message without changing user text.
pub fn trf(message: &'static str, values: &[(&str, &str)]) -> String {
    let translated = tr(message);
    let mut result = String::with_capacity(translated.len());
    let mut remainder = translated;
    while let Some(open) = remainder.find('{') {
        result.push_str(&remainder[..open]);
        let after_open = &remainder[open + 1..];
        if let Some(close) = after_open.find('}') {
            let key = &after_open[..close];
            if let Some((_, value)) = values.iter().find(|(name, _)| *name == key) {
                result.push_str(value);
            } else {
                result.push_str(&remainder[open..open + close + 2]);
            }
            remainder = &after_open[close + 1..];
        } else {
            result.push_str(remainder);
            return result;
        }
    }
    result.push_str(remainder);
    result
}

pub fn trn(singular: &'static str, plural: &'static str, count: u32) -> String {
    gettextrs::ngettext(singular, plural, count)
}

pub fn ui_date(date: NaiveDate, format: &str) -> String {
    glib::DateTime::new(
        &glib::TimeZone::local(),
        date.year(),
        date.month() as i32,
        date.day() as i32,
        12,
        0,
        0.0,
    )
    .ok()
    .and_then(|date| date.format(format).ok())
    .map_or_else(|| date.to_string(), |value| value.to_string())
}

pub fn ui_datetime(ms: i64, format: &str) -> String {
    glib::DateTime::from_unix_local(ms.div_euclid(1_000))
        .ok()
        .and_then(|date| date.format(format).ok())
        .map_or_else(|| ms.to_string(), |value| value.to_string())
}

fn catalog_directory() -> PathBuf {
    let installed = std::env::current_exe().ok().and_then(|binary| {
        binary
            .parent()?
            .parent()
            .map(|prefix| prefix.join("share/locale"))
    });
    installed
        .filter(|path| path.join("es/LC_MESSAGES/houra.mo").exists())
        .unwrap_or_else(|| PathBuf::from(env!("HOURA_BUILD_LOCALE_DIR")))
}

pub fn initialize() -> Result<(), AppError> {
    let _locale = setlocale(LocaleCategory::LcAll, "");
    bindtextdomain("houra", catalog_directory())
        .map_err(|error| AppError::Localization(error.to_string()))?;
    bind_textdomain_codeset("houra", "UTF-8")
        .map_err(|error| AppError::Localization(error.to_string()))?;
    textdomain("houra").map_err(|error| AppError::Localization(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::trf;

    #[test]
    fn formatting_does_not_reinterpret_user_text() {
        assert_eq!(
            trf("Project: {project}", &[("project", "{other} & work")]),
            "Project: {other} & work"
        );
    }
}
