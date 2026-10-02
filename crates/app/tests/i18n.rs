#![cfg(feature = "native-ui")]

use std::process::Command;

use gettextrs::{LocaleCategory, setlocale};

const CASES: [(&str, &str, &str, &str, &str); 9] = [
    ("es", "es_ES.UTF-8", "Salir", "Seguimiento", "2 entradas"),
    (
        "pt_BR",
        "pt_BR.UTF-8",
        "Sair",
        "Registro de tempo",
        "2 registros",
    ),
    (
        "fr",
        "fr_FR.UTF-8",
        "Quitter",
        "Suivi du temps",
        "2 entrées",
    ),
    ("zh_CN", "zh_CN.UTF-8", "退出", "计时", "2 条记录"),
    ("ja", "ja_JP.UTF-8", "終了", "時間記録", "2 件の記録"),
    (
        "de",
        "de_DE.UTF-8",
        "Beenden",
        "Zeiterfassung",
        "2 Einträge",
    ),
    ("ko", "ko_KR.UTF-8", "종료", "시간 기록", "2 개 기록"),
    (
        "it",
        "it_IT.UTF-8",
        "Esci",
        "Monitoraggio del tempo",
        "2 voci",
    ),
    ("ru", "ru_RU.UTF-8", "Выйти", "Учёт времени", "2 записи"),
];

#[test]
fn all_locale_catalogs_load_in_isolated_processes() {
    let binary = std::env::current_exe()
        .unwrap_or_else(|error| panic!("could not locate i18n test binary: {error}"));
    for (language, locale, quit, tracker, plural) in CASES {
        let output = Command::new(&binary)
            .args(["--exact", "catalog_lookup_in_child_process"])
            .env("LC_ALL", locale)
            .env("LANGUAGE", language)
            .env("HOURA_TEST_QUIT", quit)
            .env("HOURA_TEST_TRACKER", tracker)
            .env("HOURA_TEST_PLURAL", plural)
            .output()
            .unwrap_or_else(|error| panic!("could not test {language} catalog: {error}"));
        assert!(
            output.status.success(),
            "{language} catalog lookup failed: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[test]
fn catalog_lookup_in_child_process() {
    let Ok(expected_quit) = std::env::var("HOURA_TEST_QUIT") else {
        return;
    };
    let expected_tracker = std::env::var("HOURA_TEST_TRACKER")
        .unwrap_or_else(|error| panic!("missing test expectation: {error}"));
    let expected_plural = std::env::var("HOURA_TEST_PLURAL")
        .unwrap_or_else(|error| panic!("missing plural expectation: {error}"));
    if setlocale(LocaleCategory::LcAll, "").is_none() {
        // Minimal CI images may omit the requested system locale.
        return;
    }
    assert!(houra::locale::initialize().is_ok());
    assert_eq!(houra::locale::tr("Quit"), expected_quit);
    assert_eq!(houra::locale::tr("Tracker"), expected_tracker);
    let entries = houra::locale::trn("{count} entry", "{count} entries", 2);
    assert_eq!(entries.replace("{count}", "2"), expected_plural);
    if std::env::var("LANGUAGE").is_ok_and(|language| language == "ru") {
        let entries = houra::locale::trn("{count} entry", "{count} entries", 5);
        assert_eq!(entries.replace("{count}", "5"), "5 записей");
    }
    if gtk::init().is_ok() {
        let builder = gtk::Builder::from_string(
            r#"<interface domain="houra"><object class="GtkLabel" id="translated"><property name="label" translatable="yes">Quit</property></object></interface>"#,
        );
        let label = builder.object::<gtk::Label>("translated");
        assert!(label.is_some_and(|label| label.label() == expected_quit));
    }
}
