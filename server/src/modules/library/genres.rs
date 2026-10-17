//! Album genre lists as stored on the album row (library/genres.go).

use regex::Regex;
use std::collections::{BTreeMap, HashSet};
use std::sync::LazyLock;

static DELIMITERS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[;/|,]+").expect("genre delimiter pattern"));

/// Splits one delimited genre string, keeping first spellings and dropping
/// case-insensitive duplicates.
pub fn split_genres(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    DELIMITERS
        .split(raw)
        .map(str::trim)
        .filter(|part| !part.is_empty() && seen.insert(part.to_lowercase()))
        .map(str::to_owned)
        .collect()
}

/// Merges lists into one, deduplicated case-insensitively and sorted by
/// the lowercase name.
fn merge_genres(genres: &[String]) -> Vec<String> {
    let mut by_key: BTreeMap<String, String> = BTreeMap::new();
    for genre in genres {
        let genre = genre.trim();
        if genre.is_empty() {
            continue;
        }
        by_key
            .entry(genre.to_lowercase())
            .or_insert_with(|| genre.to_owned());
    }
    by_key.into_values().collect()
}

/// The `genres` column: a JSON array, or a delimited string from older rows.
pub fn decode_genres(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw == "[]" {
        return Vec::new();
    }
    match serde_json::from_str::<Vec<String>>(raw) {
        Ok(genres) => merge_genres(&genres),
        Err(_) => split_genres(raw),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_json_lists_and_delimited_strings() {
        assert_eq!(
            decode_genres(r#"["Rock","pop","rock"," "]"#),
            ["pop", "Rock"]
        );
        assert_eq!(
            decode_genres("Pop, Rock;pop / Jazz"),
            ["Pop", "Rock", "Jazz"]
        );
        assert!(decode_genres("").is_empty());
        assert!(decode_genres("[]").is_empty());
    }
}
