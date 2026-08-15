// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Consumer-owned port for canonical D-6 approval-record persistence.

use thiserror::Error;

use crate::domain::TenantId;
use crate::domain::execution::{
    ApprovalDecision, ApprovalId, ApprovalRequest, ApprovalStatus, ApproverId,
};

/// A canonical D-6 store operation could not complete or was rejected.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ApprovalStoreError {
    /// The durable store did not complete within its availability contract.
    #[error("approval store unavailable")]
    Unavailable,
    /// The canonical identity exists with different immutable fields.
    ///
    /// A replay whose binding no longer matches the committed record can
    /// never be reconciled as the same approval and must not overwrite it.
    #[error("approval identity conflicts with the canonical record")]
    IdentityConflict,
}

/// The outcome of one durable idempotent D-6 insert.
///
/// A replay of the same immutable record — including after a lost
/// acknowledgement whose write actually committed — reports
/// [`ApprovalInsertResolution::Existing`] after verifying every immutable
/// field and returning the record's current canonical projection, so the
/// caller can decide unambiguously whether to publish requested version 1,
/// suppress publication, or publish the already-terminal state
/// (ADR-0003 TC-12 and the versioned projection contract).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalInsertResolution {
    /// This call committed the requested record at version 1.
    Inserted,
    /// The identical canonical record already exists; no state changed, and
    /// the row's current canonical projection is returned.
    Existing {
        /// The canonical status at replay time.
        status: ApprovalStatus,
        /// The canonical decision, or `None` while requested or expired.
        decision: Option<ApprovalDecision>,
        /// The canonical record version at replay time.
        version: u64,
    },
}

/// The outcome of one durable conditional D-6 decision transition.
///
/// Exactly one contender observes [`ApprovalDecisionResolution::Won`]; every
/// other contender observes the already-committed canonical terminal through
/// [`ApprovalDecisionResolution::ExistingTerminal`] (TC-12).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalDecisionResolution {
    /// This caller's conditional transition won and is now canonical.
    Won {
        /// The decision that won.
        decision: ApprovalDecision,
        /// The canonical record version after the transition.
        version: u64,
    },
    /// A terminal already won canonically; this caller changed no state.
    ExistingTerminal {
        /// The winning decision, or `None` when the record expired undecided.
        decision: Option<ApprovalDecision>,
        /// The canonical terminal status.
        status: ApprovalStatus,
        /// The canonical record version of the existing terminal.
        version: u64,
    },
    /// No canonical D-6 exists for this identity in this tenant.
    NotFound,
}

/// Consumer-owned boundary for durable canonical D-6 records.
///
/// The store is the only authority for cross-instance D-6 state: inserts and
/// decision transitions are conditional durable writes, so competing
/// approvers, dispatchers, and reconcilers converge on one canonical outcome
/// (ADR-0003 TC-12). A decision whose `decided_at_millis` is at or after the
/// record's expiry commits no decision; the still-requested record transitions
/// to `expired` and is reported as an existing terminal.
pub trait ApprovalRecordStore {
    /// Durably inserts one newly created requested D-6, idempotently.
    ///
    /// A replay of the same immutable record — including after a lost
    /// acknowledgement whose write actually committed — reports
    /// [`ApprovalInsertResolution::Existing`] after verifying every immutable
    /// field, so the canonical row can be reconciled without ambiguity. The
    /// same identity with different immutable fields is a typed
    /// [`ApprovalStoreError::IdentityConflict`] that changes no state.
    ///
    /// # Errors
    ///
    /// Returns [`ApprovalStoreError::Unavailable`] when the durable write
    /// cannot complete within its availability contract, or
    /// [`ApprovalStoreError::IdentityConflict`] when the identity exists with
    /// different immutable fields.
    fn insert_requested(
        &mut self,
        request: &ApprovalRequest,
        requester_subject: &str,
    ) -> Result<ApprovalInsertResolution, ApprovalStoreError>;

    /// Applies one authenticated decision through a conditional transition.
    ///
    /// `approver` is the C-7-validated subject identity; it is a validated
    /// [`ApproverId`] so a durable terminal can never commit with a blank or
    /// unvalidated approver. `thread_id` is the canonical Thread ownership dimension and `requester_subject` is the canonical ownership
    /// dimension: the conditional lookup includes it, so a same-tenant
    /// principal that does not own the approval observes an indistinguishable
    /// `NotFound` with zero mutation.
    ///
    /// # Errors
    ///
    /// Returns [`ApprovalStoreError::Unavailable`] when the durable
    /// transition cannot complete within its availability contract.
    #[allow(
        clippy::too_many_arguments,
        reason = "ownership dimensions are individually conditional lookup keys"
    )]
    fn resolve_decision(
        &mut self,
        approval_id: ApprovalId,
        tenant_id: &TenantId,
        thread_id: crate::domain::ThreadId,
        requester_subject: &str,
        decision: ApprovalDecision,
        approver: &ApproverId,
        decided_at_millis: u64,
    ) -> Result<ApprovalDecisionResolution, ApprovalStoreError>;
}
