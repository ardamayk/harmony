//! Server Dependencies: external programs the Music Server calls at runtime,
//! probed once at startup (server/internal/dependencies).

use regex::Regex;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

pub const FFMPEG: &str = "ffmpeg";
pub const FFPROBE: &str = "ffprobe";
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);

/// One probed program. `version` is empty when the program is missing or
/// its version output could not be parsed.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Dependency {
    pub name: String,
    pub required: bool,
    pub available: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub version: String,
}

/// Every Server Dependency in a stable order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report(pub Vec<Dependency>);

impl Report {
    pub fn has(&self, name: &str) -> bool {
        self.0
            .iter()
            .any(|dependency| dependency.name == name && dependency.available)
    }
}

const PROGRAMS: &[(&str, bool, &[&str])] = &[
    (FFMPEG, true, &["-version"]),
    (FFPROBE, true, &["-version"]),
];

/// Probes every program once. It never fails: a missing program is reported
/// as unavailable and a failing version command as available without a
/// version.
pub async fn probe_system() -> Report {
    let mut report = Vec::with_capacity(PROGRAMS.len());
    for (name, required, args) in PROGRAMS {
        let mut dependency = Dependency {
            name: (*name).to_owned(),
            required: *required,
            available: false,
            version: String::new(),
        };
        if let Some(path) = look_path(name) {
            dependency.available = true;
            if let Some(output) = run_version(&path, args).await {
                dependency.version = parse_version(name, &output);
            }
        }
        report.push(dependency);
    }
    Report(report)
}

fn look_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

async fn run_version(path: &Path, args: &[&str]) -> Option<String> {
    let output = tokio::time::timeout(
        VERSION_TIMEOUT,
        tokio::process::Command::new(path)
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

static VERSION_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\S+) version (\S+)").expect("version pattern"));

/// Reads "<name> version <version> ..." from the first line, the format
/// shared by ffmpeg and ffprobe.
pub fn parse_version(name: &str, output: &str) -> String {
    let first_line = output.trim().lines().next().unwrap_or_default();
    VERSION_PATTERN
        .captures(first_line)
        .filter(|captures| &captures[1] == name)
        .map(|captures| captures[2].to_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_version_from_the_first_line_only_for_the_named_program() {
        assert_eq!(
            parse_version(
                "ffmpeg",
                "ffmpeg version n9.0.2 Copyright (c) 2000-2026\nbuilt with gcc"
            ),
            "n9.0.2"
        );
        assert_eq!(parse_version("ffprobe", "ffmpeg version n9.0.2"), "");
        assert_eq!(parse_version("ffmpeg", "garbage"), "");
    }

    #[test]
    fn report_has_only_available_programs() {
        let report = Report(vec![
            Dependency {
                name: FFMPEG.to_owned(),
                required: true,
                available: true,
                version: String::new(),
            },
            Dependency {
                name: FFPROBE.to_owned(),
                required: true,
                available: false,
                version: String::new(),
            },
        ]);
        assert!(report.has(FFMPEG));
        assert!(!report.has(FFPROBE));
        assert!(!report.has("sox"));
    }
}
