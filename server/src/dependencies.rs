//! Server Dependencies: external programs the Music Server calls at runtime,
//! probed once at startup (server/internal/dependencies).

use regex::Regex;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

pub const FFMPEG: &str = "ffmpeg";
pub const FFPROBE: &str = "ffprobe";
pub const YT_DLP: &str = "yt-dlp";
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

/// Probes the operator's yt-dlp, the optional dependency of the External
/// Source: `binary` is a name on PATH or a path to the program.
pub async fn probe_yt_dlp(binary: &str) -> Dependency {
    let mut dependency = Dependency {
        name: YT_DLP.to_owned(),
        required: false,
        available: false,
        version: String::new(),
    };
    let path = if binary.contains('/') {
        Some(PathBuf::from(binary)).filter(|path| is_executable(path))
    } else {
        look_path(binary)
    };
    if let Some(path) = path {
        dependency.available = true;
        if let Some(output) = run_version(&path, &["--version"]).await {
            dependency.version = parse_yt_dlp_version(&output);
        }
    }
    dependency
}

static YT_DLP_VERSION_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{4}\.\d{2}\.\d{2}(\.\d+)?$").expect("yt-dlp version pattern"));

/// yt-dlp prints only its version, such as `2026.10.26` (`.N` for nightlies).
pub fn parse_yt_dlp_version(output: &str) -> String {
    let first_line = output.trim().lines().next().unwrap_or_default().trim();
    if YT_DLP_VERSION_PATTERN.is_match(first_line) {
        first_line.to_owned()
    } else {
        String::new()
    }
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
    fn parses_bare_yt_dlp_versions() {
        assert_eq!(parse_yt_dlp_version("2026.10.26\n"), "2026.10.26");
        assert_eq!(
            parse_yt_dlp_version("2026.10.26.232912\n"),
            "2026.10.26.232912"
        );
        assert_eq!(parse_yt_dlp_version("yt-dlp 2026.10.26"), "");
        assert_eq!(parse_yt_dlp_version(""), "");
    }

    #[tokio::test]
    async fn probes_yt_dlp_as_an_optional_dependency() {
        let missing = probe_yt_dlp("/nonexistent/yt-dlp").await;
        assert_eq!(
            missing,
            Dependency {
                name: YT_DLP.to_owned(),
                required: false,
                available: false,
                version: String::new(),
            }
        );
        let directory = tempfile::tempdir().expect("tempdir");
        let binary = directory.path().join("yt-dlp");
        std::fs::write(&binary, "#!/bin/sh\necho 2026.10.26\n").expect("write");
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
                .expect("chmod");
        }
        let found = probe_yt_dlp(binary.to_str().expect("path")).await;
        assert!(found.available && !found.required);
        assert_eq!(found.version, "2026.10.26");
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
