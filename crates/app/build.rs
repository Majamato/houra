use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../../data/io.github.majamato.Houra.gresource.xml");
    println!("cargo:rerun-if-changed=../../data/ui/window.ui");
    println!("cargo:rerun-if-changed=../../data/ui/week-day-cell.ui");
    println!("cargo:rerun-if-changed=../../data/ui/entry-row.ui");
    println!("cargo:rerun-if-changed=../../data/ui/management-row.ui");
    if env::var_os("CARGO_FEATURE_NATIVE_UI").is_none() {
        return;
    }
    let out_dir = env::var_os("OUT_DIR").map(PathBuf::from);
    let Some(out_dir) = out_dir else {
        panic!("Cargo did not provide OUT_DIR");
    };
    let status = Command::new("glib-compile-resources")
        .arg("--target")
        .arg(out_dir.join("houra.gresource"))
        .arg("--sourcedir")
        .arg("../../data")
        .arg("../../data/io.github.majamato.Houra.gresource.xml")
        .status();
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => panic!("glib-compile-resources exited with {status}"),
        Err(error) => panic!("could not start glib-compile-resources: {error}"),
    }
    let locale_dir = out_dir.join("locale");
    println!("cargo:rerun-if-changed=../../po/LINGUAS");
    let languages = std::fs::read_to_string("../../po/LINGUAS")
        .unwrap_or_else(|error| panic!("could not read po/LINGUAS: {error}"));
    for language in languages
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let source = PathBuf::from("../../po").join(format!("{language}.po"));
        println!("cargo:rerun-if-changed={}", source.display());
        let target_dir = locale_dir.join(language).join("LC_MESSAGES");
        std::fs::create_dir_all(&target_dir)
            .unwrap_or_else(|error| panic!("could not create locale output directory: {error}"));
        let status = Command::new("msgfmt")
            .arg("--check")
            .arg("--output-file")
            .arg(target_dir.join("houra.mo"))
            .arg(source)
            .status()
            .unwrap_or_else(|error| panic!("could not start msgfmt: {error}"));
        assert!(status.success(), "msgfmt failed for {language}");
    }
    println!(
        "cargo:rustc-env=HOURA_BUILD_LOCALE_DIR={}",
        locale_dir.display()
    );
}
