use std::process::Command;

#[test]
fn version_flag_prints_the_package_version() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_houra"))
        .arg("--version")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env("DBUS_SESSION_BUS_ADDRESS", "disabled:")
        .output()?;
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout)?,
        format!("houra {}\n", env!("CARGO_PKG_VERSION"))
    );
    Ok(())
}
