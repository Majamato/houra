use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../../data/io.github.majamato.Houra.gresource.xml");
    println!("cargo:rerun-if-changed=../../data/ui/window.ui");
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
}
