use std::path::PathBuf;

use earthly_audio_core::observability;

/// The Electron shell starts the core with `--config-dir <path>`; standalone
/// runs fall back to the directory the Desktop Client has always used.
fn main() {
    // Only with SENTRY_DSN in the environment (ADR 0033); a bad setting
    // leaves Sentry off rather than playback.
    let sentry = observability::init_from_env().unwrap_or_else(|error| {
        eprintln!("Harmony core: Sentry stays off: {error}");
        None
    });
    let config_directory = match config_directory() {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("Harmony core: {error}");
            drop(sentry);
            std::process::exit(2);
        }
    };
    if let Err(error) = earthly_audio_core::run(&config_directory) {
        eprintln!("Harmony core failed: {error}");
        observability::core_failed(&error, &config_directory);
        // Sends what is queued before exiting.
        drop(sentry);
        std::process::exit(1);
    }
}

fn config_directory() -> Result<PathBuf, String> {
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {
        Some("--config-dir") => {
            return arguments
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| "--config-dir needs a path".to_owned());
        }
        Some(other) => return Err(format!("unknown argument '{other}'")),
        None => {}
    }
    Ok(default_config_base()?.join("audio.earthly.desktop"))
}

#[cfg(windows)]
fn default_config_base() -> Result<PathBuf, String> {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "APPDATA is not set".to_owned())
}

#[cfg(not(windows))]
fn default_config_base() -> Result<PathBuf, String> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| "neither XDG_CONFIG_HOME nor HOME is set".to_owned())
}
