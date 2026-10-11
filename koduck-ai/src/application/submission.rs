// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! Submission identity policy: scoped lock-key derivation, direct-port
//! guards, and the owned outcome types carried through the runner and the
//! presentation boundary (SI-01d, SI-02a, SI-03b, SI-04, SI-05).

use sha2::{Digest, Sha256};

use crate::application::{TurnCommand, TurnCommandError, TurnResult};
use crate::domain::{SubmissionId, ThreadId, TrustContext, TurnId};

pub mod acceptance;
#[cfg(test)]
mod tests;

pub use acceptance::{
    ACCEPTANCE_DEADLINE, AcceptanceBudget, AcceptanceClock, AcceptanceInstant, LOOKUP_BUDGET,
    RECONCILIATION_BUDGET, SystemAcceptanceClock, WRITE_BUDGET, WRITE_GATE_MINIMUM, system_clock,
};

/// The fixed ASCII namespace tag hashed into every submission lock key
/// (SI-03b).
const LOCK_KEY_TAG: &[u8] = b"koduck.chat.submission.v1";

/// The public accepted identity of one exact identified submission (SI-04).
///
/// The receipt carries no generation, approval, credentials, input, or
/// creator-operation identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmissionObservation {
    /// The client's submission identity.
    pub submission_id: SubmissionId,
    /// Accepted Thread identity.
    pub thread_id: ThreadId,
    /// Accepted Turn identity.
    pub turn_id: TurnId,
}

/// The owned outcome of one identified chat execution (SI-04).
///
/// Only the invocation whose private creator-operation identity committed the
/// binding receives [`TurnOutcome::Owned`]; every other invocation observes
/// [`TurnOutcome::Observed`] without execution authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TurnOutcome {
    /// This invocation created the binding and owns the accepted Turn.
    Owned(TurnResult),
    /// The key already existed with the exact original input; observation
    /// only, never execution.
    Observed(SubmissionObservation),
}

/// The acceptance-port result that separates the created owner from an
/// observed existing key (SI-04).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdentifiedAcceptance {
    /// This invocation's binding committed; the carried [`AcceptedTurn`] is
    /// the original lease-bearing owner result.
    Created(crate::application::AcceptedTurn),
    /// Another invocation's exact binding already committed; the observer
    /// receives only the public identity receipt and no authority.
    Existing(SubmissionObservation),
}

/// Derives the two signed 32-bit advisory-lock keys for one identified
/// submission (SI-03b).
///
/// The SHA-256 digest covers the fixed ASCII tag, the tenant and subject
/// UTF-8 values each prefixed by their unsigned 64-bit big-endian byte
/// length, and the 16 parsed submission UUID bytes. The keys reuse the first
/// eight digest bytes as two big-endian signed 32-bit values. Hash collisions
/// may delay another submission but cannot change equality or authorization.
#[must_use]
pub fn submission_lock_keys(trust: &TrustContext, submission_id: SubmissionId) -> (i32, i32) {
    let tenant = trust.tenant_id.as_str();
    let subject = trust.subject_id.as_str();
    let mut digest = Sha256::new();
    digest.update(LOCK_KEY_TAG);
    digest.update((tenant.len() as u64).to_be_bytes());
    digest.update(tenant.as_bytes());
    digest.update((subject.len() as u64).to_be_bytes());
    digest.update(subject.as_bytes());
    digest.update(*submission_id.as_uuid().as_bytes());
    let hashed = digest.finalize();
    // The two u32 words are reinterpreted, never converted: every 32-bit
    // pattern is a valid signed key and the mapping stays stable.
    let leading = i32::from_be_bytes([hashed[0], hashed[1], hashed[2], hashed[3]]);
    let trailing = i32::from_be_bytes([hashed[4], hashed[5], hashed[6], hashed[7]]);
    (leading, trailing)
}

/// Independently validates one identified command at the direct acceptance
/// port before any database I/O (SI-01d).
///
/// The input byte bound and the non-nil submission identity are rechecked here
/// so an invalid command cannot bypass the guard by constructing public fields
/// directly. An adapter or default port implementation must not silently
/// ignore a supplied key.
///
/// # Errors
///
/// Returns [`TurnCommandError`] for an empty or oversized input, an absent
/// identity on the identified port, or a nil submission identity.
pub fn validate_identified_command(command: &TurnCommand) -> Result<(), TurnCommandError> {
    if command.input.is_empty() {
        return Err(TurnCommandError::EmptyInput);
    }
    if command.input.len() > 65_536 {
        return Err(TurnCommandError::InputTooLarge);
    }
    match command.submission_id {
        Some(submission_id) if submission_id.as_uuid().is_nil() => {
            Err(TurnCommandError::NilSubmission)
        }
        Some(_) => Ok(()),
        None => Err(TurnCommandError::MissingSubmission),
    }
}
