use houra::{APP_NAME, AppError};
use tracing_subscriber::EnvFilter;

fn main() {
    // Answered before any display or D-Bus access, so installers can run it
    // to check that the binary's libraries resolve on this system.
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("houra {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();
    if let Err(error) = run() {
        tracing::error!(error = %error, "application terminated");
        eprintln!("{APP_NAME}: {error}");
        std::process::exit(1);
    }
}

#[cfg(feature = "native-ui")]
fn run() -> Result<(), AppError> {
    houra::desktop::run(houra::identity::default_database_path()?)
}

#[cfg(not(feature = "native-ui"))]
fn run() -> Result<(), AppError> {
    let _path = houra::identity::default_database_path()?;
    eprintln!(
        "This build contains the tested storage engine but not the GNOME UI. \
         Rebuild with `--features native-ui` (Meson does this automatically)."
    );
    Ok(())
}
