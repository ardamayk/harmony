//! Library Search normalization (server/internal/searchdata), and the
//! deterministic SQLite functions the search triggers call when a name is
//! written: `library_search_text(text, kind)` and `library_search_version()`.
//! Every connection registers them before use, migrations included, because
//! a trigger without them fails the write rather than leaving stale data.
//!
//! The output must match the Go server's byte for byte, since both servers
//! write the same `library_search_texts` table during the move.

use libsqlite3_sys as ffi;
use regex::Regex;
use serde::Serialize;
use std::ffi::{CStr, c_char, c_int};
use std::sync::LazyLock;
use unicode_general_category::{GeneralCategory, get_general_category};
use unicode_normalization::UnicodeNormalization;

/// Increment whenever normalization changes; triggers read it through
/// `library_search_version()`, and startup rebuilds outdated data.
pub const RULES_VERSION: i64 = 1;

const VERSION_WORDS: &[&str] = &[
    "live",
    "remix",
    "acoustic",
    "instrumental",
    "demo",
    "remaster",
    "radio edit",
];

/// Spelling evidence and two punctuation alternatives. `compact` joins
/// punctuation within a word, never whitespace-separated words.
#[derive(Clone, Debug, Default, PartialEq, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Text {
    pub original: String,
    pub faithful: String,
    pub folded: String,
    pub compact: String,
    pub versions: Vec<String>,
}

/// Prepares one incoming query; its version words may occur anywhere.
pub fn prepare_query(value: &str) -> Text {
    let mut text = prepare_text(value);
    text.versions = find_versions(&text.folded);
    text
}

/// Prepares a stored name. Track and Album titles also record the version
/// qualifiers of their parenthesized or ` - ` suffix sections.
pub fn prepare_stored(value: &str, kind: &str) -> Text {
    let mut text = prepare_text(value);
    if kind == "track" || kind == "album" {
        text.versions = title_versions(value);
    }
    text
}

fn prepare_text(value: &str) -> Text {
    let folded_case: String = caseless::default_case_fold_str(value).nfc().collect();
    let faithful = fields(&folded_case);
    Text {
        original: value.to_owned(),
        folded: fold_words(&faithful, false),
        compact: fold_words(&faithful, true),
        faithful,
        versions: Vec::new(),
    }
}

/// Go's `strings.Join(strings.Fields(s), " ")`.
fn fields(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn fold_words(value: &str, compact: bool) -> String {
    let mut result = String::with_capacity(value.len());
    for character in value.nfd() {
        let category = get_general_category(character);
        if category == GeneralCategory::NonspacingMark {
            continue;
        }
        if character == 'ı' {
            result.push('i');
        } else if is_letter(category) || category == GeneralCategory::DecimalNumber {
            result.push(character);
        } else if character.is_whitespace() {
            result.push(' ');
        } else if character == '\'' || character == '’' {
            continue;
        } else if !compact {
            result.push(' ');
        }
    }
    fields(&result)
}

/// Go's `unicode.IsLetter(c) || unicode.IsDigit(c)`.
pub fn is_letter_or_digit(character: char) -> bool {
    let category = get_general_category(character);
    is_letter(category) || category == GeneralCategory::DecimalNumber
}

/// Go's `unicode.IsLetter`: the L categories only, unlike Rust's
/// `char::is_alphabetic`, which also covers letter numbers and some marks.
fn is_letter(category: GeneralCategory) -> bool {
    matches!(
        category,
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
    )
}

fn find_versions(value: &str) -> Vec<String> {
    let words = format!(" {value} ");
    VERSION_WORDS
        .iter()
        .filter(|&&version| {
            words.contains(&format!(" {version} "))
                || (version == "remaster" && words.contains(" remastered "))
        })
        .map(|&version| version.to_owned())
        .collect()
}

static VERSION_SECTIONS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\(([^()]*)\)").expect("version section pattern"));

fn title_versions(value: &str) -> Vec<String> {
    let mut sections: Vec<&str> = VERSION_SECTIONS
        .captures_iter(value)
        .filter_map(|captures| captures.get(1).map(|section| section.as_str()))
        .collect();
    if let Some((_, suffix)) = value.split_once(" - ") {
        sections.push(suffix);
    }
    let mut versions = Vec::new();
    for section in sections {
        for version in find_versions(&prepare_text(section).folded) {
            if !versions.contains(&version) {
                versions.push(version);
            }
        }
    }
    versions
}

/// Rebuilds missing or outdated Library Search texts at startup, before
/// handlers get the database (searchdata.EnsureCurrent). With `force`, it
/// regenerates all of them (Rebuild). Either way it is one transaction, so
/// readers keep the previous snapshot until it commits.
///
/// These statements call the registered functions, which the schema
/// database sqlx checks against lacks, so they are not macro-checked.
pub async fn rebuild(pool: &sqlx::SqlitePool, force: bool) -> Result<(), String> {
    let mut transaction = crate::db::begin_write(pool)
        .await
        .map_err(|error| format!("begin Library Search rebuild: {error}"))?;
    if !force {
        let is_current: bool = sqlx::query_scalar(
            "SELECT NOT EXISTS (
                SELECT 1 FROM library_search_sources source
                LEFT JOIN library_search_texts prepared ON prepared.kind = source.kind AND prepared.id = source.id
                WHERE prepared.id IS NULL OR prepared.rules_version != ?
            )",
        )
        .bind(RULES_VERSION)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| format!("check Library Search rules version: {error}"))?;
        if is_current {
            return Ok(());
        }
    }
    sqlx::query("DELETE FROM library_search_texts")
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("rebuild Library Search data: {error}"))?;
    sqlx::query(
        "INSERT INTO library_search_texts
         SELECT kind, id, library_search_text(name, kind), library_search_version() FROM library_search_sources",
    )
    .execute(&mut *transaction)
    .await
    .map_err(|error| format!("rebuild Library Search data: {error}"))?;
    transaction
        .commit()
        .await
        .map_err(|error| format!("commit Library Search rebuild: {error}"))
}

/// JSON exactly as Go's `encoding/json` writes it: struct field order, and
/// `<`, `>`, `&`, U+2028 and U+2029 escaped (serde_json leaves them as is).
pub fn encode(text: &Text) -> String {
    let json = serde_json::to_string(text).expect("Library Search text encodes");
    let mut escaped = String::with_capacity(json.len());
    for character in json.chars() {
        match character {
            '<' => escaped.push_str("\\u003c"),
            '>' => escaped.push_str("\\u003e"),
            '&' => escaped.push_str("\\u0026"),
            '\u{2028}' => escaped.push_str("\\u2028"),
            '\u{2029}' => escaped.push_str("\\u2029"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Registers both functions on a connection.
pub async fn register_on(connection: &mut sqlx::SqliteConnection) -> Result<(), sqlx::Error> {
    let mut handle = connection.lock_handle().await?;
    // SAFETY: the locked handle is a valid open connection for this call.
    unsafe { register(handle.as_raw_handle().as_ptr()) }
        .map_err(|message| sqlx::Error::Configuration(message.into()))
}

/// Registers both functions on a raw connection handle.
///
/// # Safety
/// `db` must be a valid, open SQLite connection that outlives the call.
pub unsafe fn register(db: *mut ffi::sqlite3) -> Result<(), String> {
    let flags = ffi::SQLITE_UTF8 | ffi::SQLITE_DETERMINISTIC;
    for (name, arguments, function) in [
        (
            c"library_search_text",
            2,
            library_search_text as ScalarFunction,
        ),
        (
            c"library_search_version",
            0,
            library_search_version as ScalarFunction,
        ),
    ] {
        // SAFETY: `db` is valid per this function's contract; the name is a
        // static C string and the callbacks match SQLite's signature.
        let code = unsafe {
            ffi::sqlite3_create_function_v2(
                db,
                name.as_ptr(),
                arguments,
                flags,
                std::ptr::null_mut(),
                Some(function),
                None,
                None,
                None,
            )
        };
        if code != ffi::SQLITE_OK {
            return Err(format!(
                "register Library Search normalization: {} failed with SQLite code {code}",
                name.to_string_lossy()
            ));
        }
    }
    Ok(())
}

type ScalarFunction =
    unsafe extern "C" fn(*mut ffi::sqlite3_context, c_int, *mut *mut ffi::sqlite3_value);

unsafe extern "C" fn library_search_text(
    context: *mut ffi::sqlite3_context,
    count: c_int,
    arguments: *mut *mut ffi::sqlite3_value,
) {
    // SAFETY: SQLite passes `count` valid argument pointers for this call.
    let arguments = unsafe { std::slice::from_raw_parts(arguments, count as usize) };
    // SAFETY: each argument is a live sqlite3_value for this call.
    let (value, kind) = unsafe { (text_argument(arguments[0]), text_argument(arguments[1])) };
    let (Some(value), Some(kind)) = (value, kind) else {
        // SAFETY: `context` is the live function context for this call.
        unsafe { result_error(context, c"library search requires text and entity kind") };
        return;
    };
    match std::panic::catch_unwind(|| encode(&prepare_stored(&value, &kind))) {
        // SAFETY: `context` is live; SQLITE_TRANSIENT makes SQLite copy the text.
        Ok(encoded) => unsafe {
            ffi::sqlite3_result_text(
                context,
                encoded.as_ptr().cast::<c_char>(),
                encoded.len() as c_int,
                ffi::SQLITE_TRANSIENT(),
            )
        },
        // SAFETY: `context` is the live function context for this call.
        Err(_) => unsafe { result_error(context, c"library search normalization failed") },
    }
}

unsafe extern "C" fn library_search_version(
    context: *mut ffi::sqlite3_context,
    _count: c_int,
    _arguments: *mut *mut ffi::sqlite3_value,
) {
    // SAFETY: `context` is the live function context for this call.
    unsafe { ffi::sqlite3_result_int64(context, RULES_VERSION) };
}

/// A TEXT argument as a String; any other type (including NULL) is `None`,
/// like the Go function's type check.
unsafe fn text_argument(value: *mut ffi::sqlite3_value) -> Option<String> {
    // SAFETY: the caller passes a live sqlite3_value.
    unsafe {
        if ffi::sqlite3_value_type(value) != ffi::SQLITE_TEXT {
            return None;
        }
        let pointer = ffi::sqlite3_value_text(value);
        let length = ffi::sqlite3_value_bytes(value);
        if pointer.is_null() {
            return Some(String::new());
        }
        let bytes = std::slice::from_raw_parts(pointer, length as usize);
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

unsafe fn result_error(context: *mut ffi::sqlite3_context, message: &CStr) {
    // SAFETY: the caller passes the live function context; -1 reads to NUL.
    unsafe { ffi::sqlite3_result_error(context, message.as_ptr(), -1) };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// server/internal/testutil/search_cases.go:
    /// input, faithful, folded, compact.
    const NORMALIZATION_CASES: &[(&str, &str, &str, &str)] = &[
        ("  Beyoncé  ", "beyoncé", "beyonce", "beyonce"),
        ("beyonce", "beyonce", "beyonce", "beyonce"),
        ("Şebnem", "şebnem", "sebnem", "sebnem"),
        ("sebnem", "sebnem", "sebnem", "sebnem"),
        ("I İ ı i", "i i̇ ı i", "i i i i", "i i i i"),
        (
            "don't don’t dont",
            "don't don’t dont",
            "dont dont dont",
            "dont dont dont",
        ),
        ("AC/DC", "ac/dc", "ac dc", "acdc"),
        ("ac dc", "ac dc", "ac dc", "ac dc"),
        ("acdc", "acdc", "acdc", "acdc"),
        ("a b", "a b", "a b", "a b"),
        ("ab", "ab", "ab", "ab"),
        ("東京 １２3", "東京 １２3", "東京 １２3", "東京 １２3"),
        ("... / !", "... / !", "", ""),
    ];

    #[test]
    fn queries_preserve_spelling_and_word_boundaries() {
        for &(input, faithful, folded, compact) in NORMALIZATION_CASES {
            let text = prepare_query(input);
            assert_eq!(
                (
                    text.faithful.as_str(),
                    text.folded.as_str(),
                    text.compact.as_str()
                ),
                (faithful, folded, compact),
                "{input:?}"
            );
        }
        assert_eq!(
            prepare_query("live remix radio edit").versions,
            ["live", "remix", "radio edit"]
        );
    }

    #[test]
    fn stored_titles_take_versions_only_from_qualifier_sections() {
        assert_eq!(
            prepare_stored("Shake It Off (Live) - 2019 Remastered", "track").versions,
            ["live", "remaster"]
        );
        assert!(prepare_stored("Live Wire", "track").versions.is_empty());
        assert!(prepare_stored("Alive (Live)", "artist").versions.is_empty());
    }

    #[test]
    fn encodes_like_go_encoding_json() {
        let encoded = encode(&prepare_stored("Tom & Jerry <3", "artist"));
        assert_eq!(
            encoded,
            r#"{"original":"Tom \u0026 Jerry \u003c3","faithful":"tom \u0026 jerry \u003c3","folded":"tom jerry 3","compact":"tom jerry 3","versions":[]}"#
        );
    }
}
