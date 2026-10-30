//! Matching, typo fallback and ranking (librarysearch/rank.go). The rules
//! are the Go server's, step for step; see its README for the reasoning.

use super::index::{Entity, Index, PreparedText, fields, split_faithful};
use crate::searchdata::Text;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

/// Relevance classes. No boost crosses a class boundary.
const CLASS_FULL_NAME: u8 = 1;
const CLASS_PRIMARY_ONLY: u8 = 2;
const CLASS_PRIMARY_RELATED: u8 = 3;

const MAX_TOTAL_EDITS: usize = 2;
pub const GROUP_LIMIT: usize = 5;

/// Credit fields that break otherwise equal name matches.
const RELATED_KINDS: &[&str] = &["album", "track_artist", "album_artist"];

#[derive(Clone, Debug)]
pub struct Query {
    /// Folded words, plus the punctuation-joined alternative when it differs.
    variants: Vec<Vec<String>>,
    faithful: Vec<String>,
    versions: Vec<String>,
    /// A one-character query returns only exact primary-name matches.
    single: bool,
}

impl Query {
    pub fn new(text: &Text) -> Self {
        let mut variants = vec![fields(&text.folded)];
        if text.compact != text.folded {
            variants.push(fields(&text.compact));
        }
        Self {
            variants,
            faithful: split_faithful(&text.faithful),
            versions: text.versions.clone(),
            single: text.folded.chars().count() == 1,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.variants[0].is_empty()
    }
}

/// Typo budget per normalized query word.
fn max_edits(word: &str) -> usize {
    match word.chars().count() {
        0..=3 => 0,
        4..=7 => 1,
        _ => 2,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum MatchKind {
    None,
    Full,
    Prefix,
    Fuzzy,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct WordMatch {
    kind: MatchKind,
    edits: usize,
}

const NO_MATCH: WordMatch = WordMatch {
    kind: MatchKind::None,
    edits: 0,
};

impl WordMatch {
    fn better(self, other: WordMatch) -> bool {
        if self.kind == MatchKind::None || other.kind == MatchKind::None {
            return self.kind != MatchKind::None;
        }
        if self.edits != other.edits {
            return self.edits < other.edits;
        }
        self.kind < other.kind
    }
}

/// How one query word matches one prepared text. Prefix completion and
/// typo correction are never combined within a word.
fn match_word(word: &str, text: &PreparedText, allow_prefix: bool, limit: usize) -> WordMatch {
    let mut best = NO_MATCH;
    for (variant, words) in [&text.folded_words, &text.compact_words]
        .into_iter()
        .enumerate()
    {
        if variant == 1 && text.text.compact == text.text.folded {
            break;
        }
        for candidate in words {
            if candidate == word {
                return WordMatch {
                    kind: MatchKind::Full,
                    edits: 0,
                };
            }
            if allow_prefix && candidate.starts_with(word) {
                best = WordMatch {
                    kind: MatchKind::Prefix,
                    edits: 0,
                };
            }
            if limit > 0 && best.kind != MatchKind::Prefix {
                let edits = edit_distance(word, candidate, limit);
                if edits <= limit && (best.kind == MatchKind::None || edits < best.edits) {
                    best = WordMatch {
                        kind: MatchKind::Fuzzy,
                        edits,
                    };
                }
            }
        }
    }
    best
}

/// Optimal string alignment distance (insertion, deletion, substitution,
/// adjacent transposition) over characters; `limit + 1` once exceeded.
pub fn edit_distance(a: &str, b: &str, limit: usize) -> usize {
    if a == b {
        return 0;
    }
    if limit == 0 {
        return 1;
    }
    let left: Vec<char> = a.chars().collect();
    let right: Vec<char> = b.chars().collect();
    if left.len().abs_diff(right.len()) > limit {
        return limit + 1;
    }
    let width = right.len() + 1;
    let mut older = vec![0; width];
    let mut previous: Vec<usize> = (0..width).collect();
    let mut current = vec![0; width];
    for i in 1..=left.len() {
        current[0] = i;
        for j in 1..=right.len() {
            let cost = usize::from(left[i - 1] != right[j - 1]);
            let mut value = (previous[j] + 1)
                .min(current[j - 1] + 1)
                .min(previous[j - 1] + cost);
            if i > 1 && j > 1 && left[i - 1] == right[j - 2] && left[i - 2] == right[j - 1] {
                value = value.min(older[j - 2] + 1);
            }
            current[j] = value;
        }
        std::mem::swap(&mut older, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

#[derive(Clone, Debug)]
struct Candidate {
    entity: Arc<Entity>,
    class: u8,
    edits: usize,
    prefix_dependent: bool,
    faithful: usize,
    version_rank: u8,
    name: String,
    secondary: String,
}

impl Candidate {
    fn new(entity: &Arc<Entity>) -> Self {
        Self {
            entity: entity.clone(),
            class: 0,
            edits: 0,
            prefix_dependent: false,
            faithful: 0,
            version_rank: 0,
            name: entity.primary.text.folded.clone(),
            secondary: String::new(),
        }
    }

    /// The complete within-class ordering; class stays the outer key.
    fn less(&self, other: &Candidate) -> bool {
        if self.class != other.class {
            return self.class < other.class;
        }
        if self.edits != other.edits {
            return self.edits < other.edits;
        }
        if self.prefix_dependent != other.prefix_dependent {
            return !self.prefix_dependent;
        }
        if self.faithful != other.faithful {
            return self.faithful > other.faithful;
        }
        if self.version_rank != other.version_rank {
            return self.version_rank < other.version_rank;
        }
        if self.name != other.name {
            return self.name < other.name;
        }
        if self.secondary != other.secondary {
            return self.secondary < other.secondary;
        }
        if self.entity.id != other.entity.id {
            return self.entity.id < other.entity.id;
        }
        self.entity.kind < other.entity.kind
    }
}

fn evaluate(item: &Arc<Entity>, query: &Query, fuzzy: bool) -> Option<Candidate> {
    let mut best: Option<Candidate> = None;
    for words in &query.variants {
        if let Some(result) = evaluate_variant(item, words, query, fuzzy)
            && best.as_ref().is_none_or(|best| result.less(best))
        {
            best = Some(result);
        }
    }
    best.map(|mut best| {
        best.secondary = secondary_text(item);
        best
    })
}

fn evaluate_variant(
    item: &Arc<Entity>,
    words: &[String],
    query: &Query,
    fuzzy: bool,
) -> Option<Candidate> {
    let limits: Vec<usize> = if fuzzy {
        words.iter().map(|word| max_edits(word)).collect()
    } else {
        vec![0; words.len()]
    };
    let mut result = Candidate::new(item);
    if query.single {
        if words != item.primary.folded_words.as_slice()
            && words != item.primary.compact_words.as_slice()
        {
            return None;
        }
        result.class = CLASS_FULL_NAME;
        return Some(finish(result, query, item, false));
    }
    if let Some(edits) = aligned_edits(words, &item.primary, &limits) {
        result.class = CLASS_FULL_NAME;
        result.edits = edits;
        return Some(finish(result, query, item, false));
    }
    let last = words.len() - 1;
    let mut primary = Vec::with_capacity(words.len());
    let mut related = Vec::with_capacity(words.len());
    for (i, word) in words.iter().enumerate() {
        let primary_match = match_word(word, &item.primary, i == last, limits[i]);
        let mut related_match = NO_MATCH;
        for field in &item.related {
            let candidate = match_word(word, &field.text, i == last, limits[i]);
            if candidate.better(related_match) {
                related_match = candidate;
            }
        }
        if primary_match.kind == MatchKind::None && related_match.kind == MatchKind::None {
            return None;
        }
        primary.push(primary_match);
        related.push(related_match);
    }
    let (mut chosen, mut in_primary, mut total) = choose(&primary, &related, false);
    if total > MAX_TOTAL_EDITS {
        (chosen, in_primary, total) = choose(&primary, &related, true);
        if total > MAX_TOTAL_EDITS {
            return None;
        }
    }
    result.class = if in_primary == words.len() {
        CLASS_PRIMARY_ONLY
    } else if in_primary > 0 {
        CLASS_PRIMARY_RELATED
    } else {
        return None;
    };
    result.edits = total;
    result.prefix_dependent = chosen.iter().any(|choice| choice.kind == MatchKind::Prefix);
    Some(finish(result, query, item, true))
}

/// Assigns each word to the primary field when it can match there, which
/// raises the class; past the query edit budget it minimizes edits.
fn choose(
    primary: &[WordMatch],
    related: &[WordMatch],
    minimize_edits: bool,
) -> (Vec<WordMatch>, usize, usize) {
    let mut chosen = Vec::with_capacity(primary.len());
    let mut in_primary = 0;
    let mut total = 0;
    for i in 0..primary.len() {
        let mut use_primary = primary[i].kind != MatchKind::None;
        if minimize_edits
            && use_primary
            && related[i].kind != MatchKind::None
            && related[i].edits < primary[i].edits
        {
            use_primary = false;
        }
        if use_primary {
            chosen.push(primary[i]);
            in_primary += 1;
        } else {
            chosen.push(related[i]);
        }
        total += chosen[i].edits;
    }
    if minimize_edits && in_primary == 0 {
        // Keep the cheapest title/name contribution; related-only is ineligible.
        let mut best: Option<usize> = None;
        for (i, choice) in primary.iter().enumerate() {
            let cost = |index: usize| primary[index].edits as isize - chosen[index].edits as isize;
            if choice.kind != MatchKind::None && best.is_none_or(|best| cost(i) < cost(best)) {
                best = Some(i);
            }
        }
        if let Some(best) = best {
            total = (total as isize + primary[best].edits as isize - chosen[best].edits as isize)
                as usize;
            chosen[best] = primary[best];
            in_primary = 1;
        }
    }
    (chosen, in_primary, total)
}

/// Whether the query words are the whole primary name, word for word,
/// within the per-word and total edit limits.
fn aligned_edits(words: &[String], text: &PreparedText, limits: &[usize]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (variant, primary) in [&text.folded_words, &text.compact_words]
        .into_iter()
        .enumerate()
    {
        if variant == 1 && text.text.compact == text.text.folded {
            break;
        }
        if primary.len() != words.len() {
            continue;
        }
        let mut total = 0;
        let mut aligned = true;
        for (i, word) in words.iter().enumerate() {
            if *word == primary[i] {
                continue;
            }
            let edits = edit_distance(word, &primary[i], limits[i]);
            if edits > limits[i] {
                aligned = false;
                break;
            }
            total += edits;
        }
        if aligned && total <= MAX_TOTAL_EDITS && best.is_none_or(|best| total < best) {
            best = Some(total);
        }
    }
    best
}

fn finish(mut result: Candidate, query: &Query, item: &Entity, allow_prefix: bool) -> Candidate {
    result.faithful = faithful_words(&query.faithful, item, allow_prefix);
    result.version_rank = version_rank(&query.versions, &item.primary.text.versions);
    result
}

/// Query words whose written form (case-folded, accents and punctuation
/// kept) appears in the record's own fields.
fn faithful_words(query_words: &[String], item: &Entity, allow_prefix: bool) -> usize {
    let last = query_words.len().wrapping_sub(1);
    query_words
        .iter()
        .enumerate()
        .filter(|(i, word)| {
            let prefix = allow_prefix && *i == last;
            has_faithful(&item.primary, word, prefix)
                || item
                    .related
                    .iter()
                    .any(|field| has_faithful(&field.text, word, prefix))
        })
        .count()
}

fn has_faithful(text: &PreparedText, word: &str, allow_prefix: bool) -> bool {
    text.faithful_words
        .iter()
        .any(|candidate| candidate == word || (allow_prefix && candidate.starts_with(word)))
}

/// Prefers the requested allowlisted version, or the plain title when none
/// is requested.
fn version_rank(requested: &[String], stored: &[String]) -> u8 {
    if requested.is_empty() {
        return u8::from(!stored.is_empty());
    }
    u8::from(requested.iter().any(|version| !stored.contains(version)))
}

fn secondary_text(item: &Entity) -> String {
    item.related
        .iter()
        .filter(|field| RELATED_KINDS.contains(&field.name.as_str()))
        .map(|field| field.text.text.folded.as_str())
        .collect::<Vec<_>>()
        .join(" / ")
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Match {
    Direct,
    Corrected,
}

impl Match {
    pub fn as_str(self) -> &'static str {
        match self {
            Match::Direct => "direct",
            Match::Corrected => "corrected",
        }
    }
}

pub struct Ranked {
    pub best_match: Option<Arc<Entity>>,
    pub groups: HashMap<String, Vec<Arc<Entity>>>,
    pub matched: Match,
    pub total: usize,
}

/// Strong direct matches first; typo correction only when no type has one.
/// A `hidden_kind` is left out before ranking, so it never counts anywhere.
pub fn rank(index: &Index, query: &Query, all: bool, hidden_kind: Option<&str>) -> Ranked {
    let strong = evaluate_all(index, query, false, hidden_kind);
    if !strong.is_empty() {
        let best = strong[0].entity.clone();
        let mut ranked = assemble(strong, Match::Direct, all);
        ranked.best_match = Some(best);
        return ranked;
    }
    assemble(
        evaluate_all(index, query, true, hidden_kind),
        Match::Corrected,
        all,
    )
}

fn evaluate_all(
    index: &Index,
    query: &Query,
    fuzzy: bool,
    hidden_kind: Option<&str>,
) -> Vec<Candidate> {
    let mut candidates: Vec<Candidate> = index
        .entities
        .iter()
        .filter(|item| hidden_kind != Some(item.kind.as_str()))
        .filter_map(|item| evaluate(item, query, fuzzy))
        .collect();
    // Stable, like sort.SliceStable.
    candidates.sort_by(|a, b| {
        if a.less(b) {
            Ordering::Less
        } else if b.less(a) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    });
    candidates
}

fn assemble(candidates: Vec<Candidate>, matched: Match, all: bool) -> Ranked {
    let total = candidates.len();
    let mut groups: HashMap<String, Vec<Arc<Entity>>> = HashMap::new();
    for candidate in candidates {
        let group = groups.entry(candidate.entity.kind.clone()).or_default();
        if all || group.len() < GROUP_LIMIT {
            group.push(candidate.entity);
        }
    }
    Ranked {
        best_match: None,
        groups,
        matched,
        total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_counts_adjacent_transpositions_once() {
        assert_eq!(edit_distance("beyonce", "beyocne", 2), 1);
        assert_eq!(edit_distance("kitten", "sitting", 2), 3);
        assert_eq!(edit_distance("abc", "abcdef", 2), 3);
        assert_eq!(edit_distance("same", "same", 0), 0);
        assert_eq!(edit_distance("a", "b", 0), 1);
        assert_eq!(edit_distance("şebnem", "sebnem", 1), 1);
    }

    #[test]
    fn typo_budget_grows_with_word_length() {
        assert_eq!(max_edits("abc"), 0);
        assert_eq!(max_edits("abcd"), 1);
        assert_eq!(max_edits("abcdefgh"), 2);
        assert_eq!(max_edits("ışık"), 1);
    }

    #[test]
    fn version_rank_prefers_the_requested_or_the_plain_title() {
        let strings = |values: &[&str]| -> Vec<String> {
            values.iter().map(|value| (*value).to_owned()).collect()
        };
        assert_eq!(version_rank(&[], &[]), 0);
        assert_eq!(version_rank(&[], &strings(&["live"])), 1);
        assert_eq!(
            version_rank(&strings(&["live"]), &strings(&["live", "remix"])),
            0
        );
        assert_eq!(version_rank(&strings(&["live"]), &[]), 1);
    }
}
