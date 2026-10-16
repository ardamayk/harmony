//! A visibility-filtered snapshot of every searchable entity
//! (librarysearch/index.go). Candidate collection covers the whole eligible
//! library, so ranking and group limits apply globally.

use crate::searchdata::{Text, is_letter_or_digit};
use std::sync::Arc;

/// One stored representation with its word lists computed once per load;
/// queries never normalize library names again.
#[derive(Debug)]
pub struct PreparedText {
    pub text: Text,
    pub faithful_words: Vec<String>,
    pub folded_words: Vec<String>,
    pub compact_words: Vec<String>,
}

impl PreparedText {
    pub(super) fn new(text: Text) -> Self {
        Self {
            faithful_words: split_faithful(&text.faithful),
            folded_words: fields(&text.folded),
            compact_words: fields(&text.compact),
            text,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FieldRef {
    pub name: String,
    pub related_id: String,
    pub text: Arc<PreparedText>,
}

#[derive(Debug)]
pub struct Entity {
    pub kind: String,
    pub id: String,
    pub primary: Arc<PreparedText>,
    pub related: Vec<FieldRef>,
}

impl Entity {
    pub fn fields<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a FieldRef> + 'a {
        self.related.iter().filter(move |field| field.name == name)
    }
}

#[derive(Debug, Default)]
pub struct Index {
    pub entities: Vec<Arc<Entity>>,
}

pub fn fields(value: &str) -> Vec<String> {
    value.split_whitespace().map(str::to_owned).collect()
}

/// Keeps accents and inner punctuation (don't, ac/dc) as spelling evidence
/// while ignoring enclosing punctuation such as a version section's
/// parentheses.
pub fn split_faithful(faithful: &str) -> Vec<String> {
    faithful
        .split_whitespace()
        .map(|word| word.trim_matches(|character: char| !is_letter_or_digit(character)))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faithful_words_drop_enclosing_punctuation_only() {
        assert_eq!(
            split_faithful("don't (live) ac/dc ... «x»"),
            ["don't", "live", "ac/dc", "x"]
        );
    }
}
