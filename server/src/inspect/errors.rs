//! Inspection failures (library/media_inspector.go InspectionError,
//! inspection-errors.go, optional-artwork.go).
//!
//! Go joins independent failures with `errors.Join`; here a [`Failure`] is
//! the flattened list of those leaves in check order.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Code {
    #[serde(rename = "file_read_failed")]
    FileRead,
    #[serde(rename = "unsupported_format")]
    UnsupportedFormat,
    #[serde(rename = "invalid_metadata")]
    InvalidMetadata,
    #[serde(rename = "missing_artwork")]
    MissingArtwork,
    #[serde(rename = "invalid_artwork")]
    InvalidArtwork,
    #[serde(rename = "audio_decode_failed")]
    AudioDecode,
    #[serde(rename = "validation_cancelled")]
    ValidationCancelled,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::FileRead => "file_read_failed",
            Code::UnsupportedFormat => "unsupported_format",
            Code::InvalidMetadata => "invalid_metadata",
            Code::MissingArtwork => "missing_artwork",
            Code::InvalidArtwork => "invalid_artwork",
            Code::AudioDecode => "audio_decode_failed",
            Code::ValidationCancelled => "validation_cancelled",
        }
    }

    /// The reason shown to people; internal details stay in the message.
    fn public_reason(self, message: &str) -> String {
        match self {
            Code::InvalidMetadata => message
                .split_once(": ")
                .map_or(message, |(reason, _)| reason)
                .to_owned(),
            Code::MissingArtwork => message.to_owned(),
            Code::FileRead => "file could not be read".to_owned(),
            Code::UnsupportedFormat => "file container or audio codec is not supported".to_owned(),
            Code::InvalidArtwork => "embedded artwork is invalid".to_owned(),
            Code::AudioDecode => "audio stream failed full decode".to_owned(),
            Code::ValidationCancelled => "validation failed".to_owned(),
        }
    }
}

/// One actionable failure without internal details, as stored in the
/// `validation_issues` columns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub code: Code,
    pub field: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InspectionError {
    pub code: Code,
    pub field: String,
    pub reason: String,
    pub message: String,
}

impl fmt::Display for InspectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.field.is_empty() {
            write!(f, "inspect: {}: {}", self.code.as_str(), self.message)
        } else {
            write!(
                f,
                "inspect {}: {}: {}",
                self.field,
                self.code.as_str(),
                self.message
            )
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Leaf {
    Inspection(InspectionError),
    /// A failure outside the Strict Import Profile, such as a progress
    /// reporter that could not save.
    Other(String),
}

impl fmt::Display for Leaf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Leaf::Inspection(error) => error.fmt(f),
            Leaf::Other(message) => f.write_str(message),
        }
    }
}

/// Independent failures in check order; never empty when returned as an
/// error.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Failure(pub Vec<Leaf>);

pub type Result<T> = std::result::Result<T, Failure>;

/// `inspectionError(code, field, err)`.
pub fn error(code: Code, field: &str, message: impl fmt::Display) -> Failure {
    let message = message.to_string();
    Failure(vec![Leaf::Inspection(InspectionError {
        code,
        field: field.to_owned(),
        reason: code.public_reason(&message),
        message,
    })])
}

pub fn other(message: impl fmt::Display) -> Failure {
    Failure(vec![Leaf::Other(message.to_string())])
}

pub fn cancelled() -> Failure {
    error(Code::ValidationCancelled, "validation", "context canceled")
}

impl Failure {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `errors.Join`: the failures of every part, in order.
    pub fn join<I>(parts: I) -> Failure
    where
        I: IntoIterator<Item = Option<Failure>>,
    {
        Failure(
            parts
                .into_iter()
                .flatten()
                .flat_map(|part| part.0)
                .collect(),
        )
    }

    pub fn into_result(self) -> Result<()> {
        if self.0.is_empty() { Ok(()) } else { Err(self) }
    }

    /// `InspectionIssues`.
    pub fn issues(&self) -> Vec<Issue> {
        self.inspection_errors()
            .map(|error| Issue {
                code: error.code,
                field: error.field.clone(),
                reason: error.reason.clone(),
            })
            .collect()
    }

    /// The first inspection error, which `errors.As` finds.
    pub fn first(&self) -> Option<&InspectionError> {
        self.inspection_errors().next()
    }

    fn inspection_errors(&self) -> impl Iterator<Item = &InspectionError> {
        self.0.iter().filter_map(|leaf| match leaf {
            Leaf::Inspection(error) => Some(error),
            Leaf::Other(_) => None,
        })
    }

    /// `excludeInspectionFields`: drops failures of fields whose source could
    /// not be parsed.
    pub fn exclude_fields(self, fields: &[String]) -> Failure {
        Failure(
            self.0
                .into_iter()
                .filter(|leaf| match leaf {
                    Leaf::Inspection(error) => !fields.contains(&error.field),
                    Leaf::Other(_) => true,
                })
                .collect(),
        )
    }

    pub fn fields(&self) -> Vec<String> {
        self.inspection_errors()
            .map(|error| error.field.clone())
            .collect()
    }

    pub fn has_code(&self, code: Code) -> bool {
        self.inspection_errors().any(|error| error.code == code)
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, leaf) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str("\n")?;
            }
            leaf.fmt(f)?;
        }
        Ok(())
    }
}

impl std::error::Error for Failure {}

pub fn err_of<T>(result: &Result<T>) -> Option<Failure> {
    result.as_ref().err().cloned()
}

pub const INVALID_ARTWORK_WARNING: &str =
    "Embedded artwork is invalid; choose another cover or continue without artwork";

/// `optionalArtwork`: missing artwork is allowed and malformed artwork
/// becomes a warning.
pub fn optional_artwork(result: Result<super::AlbumArtwork>) -> Result<super::AlbumArtwork> {
    let failure = match result {
        Ok(artwork) => return Ok(artwork),
        Err(failure) => failure,
    };
    match failure.first().map(|error| error.code) {
        Some(Code::MissingArtwork) => Ok(super::AlbumArtwork::default()),
        Some(Code::InvalidArtwork) => Ok(super::AlbumArtwork {
            warning: INVALID_ARTWORK_WARNING.to_owned(),
            ..super::AlbumArtwork::default()
        }),
        _ => Err(failure),
    }
}

/// Go's wording for I/O errors that cross into messages.
pub fn io_message(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::UnexpectedEof => "unexpected EOF".to_owned(),
        _ => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_follow_the_code() {
        let failure = error(
            Code::InvalidMetadata,
            "TRACKNUMBER",
            "position must be between 1 and 9999 with an optional total: bad",
        );
        assert_eq!(
            failure.issues(),
            [Issue {
                code: Code::InvalidMetadata,
                field: "TRACKNUMBER".into(),
                reason: "position must be between 1 and 9999 with an optional total".into(),
            }]
        );
        assert_eq!(
            error(Code::AudioDecode, "audio", "x").issues()[0].reason,
            "audio stream failed full decode"
        );
        assert_eq!(
            failure.to_string(),
            "inspect TRACKNUMBER: invalid_metadata: position must be between 1 and 9999 with an optional total: bad"
        );
    }

    #[test]
    fn issues_serialize_like_go() {
        let issues = error(
            Code::MissingArtwork,
            "artwork",
            "embedded front cover is required",
        )
        .issues();
        assert_eq!(
            serde_json::to_string(&issues).unwrap(),
            r#"[{"code":"missing_artwork","field":"artwork","reason":"embedded front cover is required"}]"#
        );
    }

    #[test]
    fn optional_artwork_keeps_other_failures() {
        let missing = optional_artwork(Err(error(Code::MissingArtwork, "artwork", "x"))).unwrap();
        assert!(missing.warning.is_empty());
        let invalid = optional_artwork(Err(error(Code::InvalidArtwork, "artwork", "x"))).unwrap();
        assert_eq!(invalid.warning, INVALID_ARTWORK_WARNING);
        assert!(optional_artwork(Err(error(Code::FileRead, "file", "x"))).is_err());
    }
}
