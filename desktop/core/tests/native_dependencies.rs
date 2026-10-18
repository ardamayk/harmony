//! Native dependency boundary for the Desktop Client core.
//!
//! Only the Music Server inspects audio metadata; the core uploads bytes as
//! they are. The renderer trust boundary (CSP, the command allowlist) lives
//! with the Electron shell in `desktop/src/main/security.test.ts`.

const CARGO_MANIFEST: &str = include_str!("../Cargo.toml");

/// Crates that would let native code parse authoritative metadata; ADR-level
/// intent is that only the Music Server inspects audio.
const METADATA_PARSER_CRATES: &[&str] = &[
    "lofty",
    "symphonia",
    "id3",
    "metaflac",
    "mp4ameta",
    "mp3-metadata",
    "audiotags",
    "claxon",
    "hound",
    "opus",
    "ogg",
];

#[test]
fn native_code_has_no_audio_metadata_parser_dependency() {
    let manifest: toml::Value = toml::from_str(CARGO_MANIFEST).expect("Cargo manifest");
    let mut dependencies = Vec::new();
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = manifest.get(section).and_then(toml::Value::as_table) {
            dependencies.extend(table.keys().cloned());
        }
    }
    assert!(!dependencies.is_empty());
    for dependency in &dependencies {
        assert!(
            !METADATA_PARSER_CRATES.contains(&dependency.as_str()),
            "{dependency} would let native code parse authoritative metadata"
        );
    }
}
