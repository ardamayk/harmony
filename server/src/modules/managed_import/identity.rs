//! Identity keys that match Tracks, Albums, Artists and Genres across imports
//! (repository_commit.go normalizeIdentity, albumIdentityKey).

use crate::inspect::NormalizedMetadata;
use unicode_normalization::UnicodeNormalization;

/// NFC, whitespace collapsed, then Unicode case folding.
pub fn normalize_identity(value: &str) -> String {
    let nfc: String = value.nfc().collect();
    let collapsed = nfc.split_whitespace().collect::<Vec<_>>().join(" ");
    caseless::default_case_fold_str(&collapsed)
}

pub fn normalize_credits(credits: &[String]) -> Vec<String> {
    credits
        .iter()
        .map(|credit| normalize_identity(credit))
        .collect()
}

pub fn album_identity_key(metadata: &NormalizedMetadata) -> String {
    if !metadata.album_identity_key.is_empty() {
        return metadata.album_identity_key.clone();
    }
    album_key_of(&metadata.album_artists, &metadata.album)
}

pub fn album_key_of(album_artists: &[String], album: &str) -> String {
    format!(
        "{}\u{1f}{}",
        normalize_credits(album_artists).join("\u{1e}"),
        normalize_identity(album)
    )
}

pub fn track_identity_key(metadata: &NormalizedMetadata) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}",
        metadata.disc_position.number,
        metadata.track_position.number,
        normalize_identity(&metadata.title)
    )
}

pub fn sha256_hex(value: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(value.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_case_and_whitespace() {
        assert_eq!(normalize_identity("  Straße   Bänd "), "strasse bänd");
        assert_eq!(normalize_identity("ΣΊΣΥΦΟΣ"), "σίσυφοσ");
        assert_eq!(
            album_key_of(&["A".into(), "B b".into()], "X"),
            "a\u{1e}b b\u{1f}x"
        );
    }
}
