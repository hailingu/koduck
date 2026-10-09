// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! Validated client submission identity for durable chat acceptance.

use thiserror::Error;
use uuid::Uuid;

/// A non-nil client submission identity supplied on one chat request (SI-01a).
///
/// The wire adapter parses the strict 36-ASCII-character hyphenated form with
/// case-insensitive hexadecimal digits; this type keeps only the parsed value,
/// so a nil UUID can never enter the acceptance boundary and UUID text case is
/// not semantic (SI-02c).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SubmissionId(Uuid);

impl SubmissionId {
    /// Validates a parsed UUID as a non-nil submission identity.
    ///
    /// # Errors
    ///
    /// Returns [`SubmissionIdError`] for the nil UUID, which SI-01a rejects
    /// before any history operation.
    pub fn from_uuid(value: Uuid) -> Result<Self, SubmissionIdError> {
        if value.is_nil() {
            Err(SubmissionIdError)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the parsed UUID value.
    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

/// A rejected submission identity.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("submission id must not be nil")]
pub struct SubmissionIdError;
