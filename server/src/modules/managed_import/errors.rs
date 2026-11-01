//! Managed Import failures (managedimport/types.go sentinels and
//! ValidationError). Go joins errors and asks `errors.Is`; an [`Error`]
//! carries every sentinel of the errors joined into it.

use crate::inspect;
use serde::{Deserialize, Serialize};
use std::fmt;

/// One stored validation issue (`library.InspectionIssue`); Managed Import
/// adds its own codes, so the code is free text here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub code: String,
    pub field: String,
    pub reason: String,
}

impl From<inspect::Issue> for Issue {
    fn from(issue: inspect::Issue) -> Self {
        Self {
            code: issue.code.as_str().to_owned(),
            field: issue.field,
            reason: issue.reason,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Sentinel {
    NotFound,
    InvalidState,
    RevisionConflict,
    ExactDuplicate,
    UploadTooLarge,
    BatchTooLarge,
    UploadInterrupted,
    InvalidUpload,
    InsufficientStorage,
    UnsafeStoragePath,
    TrackNotFound,
    AlbumNotFound,
    NotManagedTrack,
    DeletionConflict,
    ReplacementConflict,
    NotReplacementJob,
    ReplacementRequired,
    CommitFailed,
    /// context.Canceled / DeadlineExceeded.
    Cancelled,
}

impl Sentinel {
    pub fn message(self) -> &'static str {
        match self {
            Sentinel::NotFound => "managed import job not found",
            Sentinel::InvalidState => "managed import job is not awaiting this operation",
            Sentinel::RevisionConflict => "managed import revision conflict",
            Sentinel::ExactDuplicate => "managed import file exactly duplicates a committed Track",
            Sentinel::UploadTooLarge => "managed import file exceeds upload limit",
            Sentinel::BatchTooLarge => "managed import batch exceeds upload limit",
            Sentinel::UploadInterrupted => "managed import upload was interrupted",
            Sentinel::InvalidUpload => "managed import upload is invalid",
            Sentinel::InsufficientStorage => "managed storage capacity is insufficient",
            Sentinel::UnsafeStoragePath => "managed storage path is unsafe",
            Sentinel::TrackNotFound => "track not found",
            Sentinel::AlbumNotFound => "album not found",
            Sentinel::NotManagedTrack => "track is not managed",
            Sentinel::DeletionConflict => "permanent track deletion preview changed",
            Sentinel::ReplacementConflict => "track replacement preview changed",
            Sentinel::NotReplacementJob => "managed import job does not replace a track",
            Sentinel::ReplacementRequired => {
                "managed import job requires track replacement confirmation"
            }
            Sentinel::CommitFailed => "managed import could not write the Track to the database",
            Sentinel::Cancelled => "context canceled",
        }
    }
}

/// A file failed the Strict Import Profile or a storage rule that people
/// can act on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidationError {
    pub issues: Vec<Issue>,
    pub code: String,
    pub field: String,
    pub reason: String,
}

impl ValidationError {
    pub fn new(code: &str, field: &str, reason: &str) -> Self {
        Self {
            issues: Vec::new(),
            code: code.to_owned(),
            field: field.to_owned(),
            reason: reason.to_owned(),
        }
    }

    /// `strictValidationReason`.
    pub fn strict_reason(&self) -> &str {
        if self.reason.is_empty() {
            "validation failed"
        } else {
            &self.reason
        }
    }

    /// `validationIssues`: the stored issues, or one built from the error.
    pub fn validation_issues(&self) -> Vec<Issue> {
        if !self.issues.is_empty() {
            return self.issues.clone();
        }
        vec![Issue {
            code: self.code.clone(),
            field: self.field.clone(),
            reason: self.strict_reason().to_owned(),
        }]
    }
}

#[derive(Clone, Debug, Default)]
pub struct Error {
    sentinels: Vec<Sentinel>,
    validation: Option<Box<ValidationError>>,
    message: String,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(message: impl fmt::Display) -> Self {
        Self {
            message: message.to_string(),
            ..Self::default()
        }
    }

    pub fn sentinel(sentinel: Sentinel) -> Self {
        Self {
            sentinels: vec![sentinel],
            message: sentinel.message().to_owned(),
            validation: None,
        }
    }

    /// `fmt.Errorf("%w: ...", sentinel)`.
    pub fn wrap(sentinel: Sentinel, detail: impl fmt::Display) -> Self {
        Self {
            sentinels: vec![sentinel],
            message: format!("{}: {detail}", sentinel.message()),
            validation: None,
        }
    }

    pub fn validation(validation: ValidationError, message: impl fmt::Display) -> Self {
        Self {
            sentinels: Vec::new(),
            message: message.to_string(),
            validation: Some(Box::new(validation)),
        }
    }

    /// `validationError(err)` for an inspection failure.
    pub fn from_inspection(failure: inspect::errors::Failure) -> Self {
        match failure.first() {
            Some(first) => Self::validation(
                ValidationError {
                    issues: failure.issues().into_iter().map(Issue::from).collect(),
                    code: first.code.as_str().to_owned(),
                    field: first.field.clone(),
                    reason: first.reason.clone(),
                },
                failure.to_string(),
            ),
            None => Self::new(failure),
        }
    }

    /// `fmt.Errorf("context: %w", err)`.
    pub fn context(mut self, context: impl fmt::Display) -> Self {
        self.message = format!("{context}: {}", self.message);
        self
    }

    /// `errors.Join`: `None` when every part is `None`.
    pub fn join<I: IntoIterator<Item = Option<Error>>>(parts: I) -> Option<Error> {
        let mut joined: Option<Error> = None;
        for part in parts.into_iter().flatten() {
            joined = Some(match joined {
                None => part,
                Some(mut error) => {
                    error.sentinels.extend(part.sentinels);
                    if error.validation.is_none() {
                        error.validation = part.validation;
                    }
                    error.message = format!("{}\n{}", error.message, part.message);
                    error
                }
            });
        }
        joined
    }

    /// `errors.Is`.
    pub fn is(&self, sentinel: Sentinel) -> bool {
        self.sentinels.contains(&sentinel)
    }

    /// `errors.As(err, &validationErr)`.
    pub fn as_validation(&self) -> Option<&ValidationError> {
        self.validation.as_deref()
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        Error::new(crate::gojson::sqlite_error(&error))
    }
}

/// Joins an optional extra failure onto a result's error.
pub fn also<T>(result: Result<T>, extra: Option<Error>) -> Result<T> {
    match (result, extra) {
        (result, None) => result,
        (Ok(_), Some(extra)) => Err(extra),
        (Err(error), Some(extra)) => Err(Error::join([Some(error), Some(extra)]).unwrap()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joined_errors_keep_every_sentinel_and_the_first_validation() {
        let joined = Error::join([
            Some(Error::wrap(Sentinel::UnsafeStoragePath, "x")),
            None,
            Some(Error::validation(ValidationError::new("a", "b", "c"), "v")),
            Some(Error::sentinel(Sentinel::InsufficientStorage)),
        ])
        .unwrap();
        assert!(joined.is(Sentinel::UnsafeStoragePath));
        assert!(joined.is(Sentinel::InsufficientStorage));
        assert!(!joined.is(Sentinel::NotFound));
        assert_eq!(joined.as_validation().unwrap().code, "a");
        assert_eq!(
            joined.to_string(),
            "managed storage path is unsafe: x\nv\nmanaged storage capacity is insufficient"
        );
        assert!(Error::join([None, None]).is_none());
    }
}
