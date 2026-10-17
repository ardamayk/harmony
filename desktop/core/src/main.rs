use std::path::PathBuf;

/// The Electron shell starts the core with `--config-dir <path>`; standalone
/// runs fall back to the directory the Desktop Client has always used.
fn main() {
    let config_directory = match config_directory() {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("Earthly Audio core: {error}");
            std::process::exit(2);
        }
    };
    if let Err(error) = earthly_audio_core::run(&config_directory) {
        eprintln!("Earthly Audio core failed: {error}");
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
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| "neither XDG_CONFIG_HOME nor HOME is set".to_owned())?;
    Ok(base.join("audio.earthly.desktop"))
}
