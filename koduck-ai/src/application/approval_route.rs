// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Authenticated approval-decision route service over the canonical D-6 store.

use crate::domain::TrustContext;
use crate::domain::execution::{ApprovalDecision, ApprovalId, ApprovalStatus, ApproverId};

use super::approval_store::{ApprovalDecisionResolution, ApprovalRecordStore, ApprovalStoreError};

/// The route-level outcome of one authenticated approval decision.
///
/// Authorization failures, unknown approvals, and cross-tenant lookups are
/// one indistinguishable [`ApprovalDecisionOutcome::NotFound`]: the route
/// mutates no state and exposes no approval existence (TC-05).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalDecisionOutcome {
    /// The decision is canonical, whether it just won or replayed identically.
    Resolved {
        /// Canonical terminal status.
        status: ApprovalStatus,
        /// The canonical decision.
        decision: ApprovalDecision,
        /// Canonical record version.
        version: u64,
    },
    /// A different terminal (or expiry) already won canonically.
    Conflict {
        /// Canonical terminal status.
        status: ApprovalStatus,
        /// The winning decision, or `None` when the record expired undecided.
        decision: Option<ApprovalDecision>,
        /// Canonical record version.
        version: u64,
    },
    /// Unknown approval, unscoped principal, or tenant mismatch.
    NotFound,
    /// The canonical store is unavailable.
    Unavailable,
}

/// Authenticated decision route over one canonical D-6 store.
///
/// The route derives the sealed [`ApproverId`] capability from the gateway
/// validated trust context; a principal without the `ai.tool.approve` scope
/// resolves nothing and cannot distinguish an unknown approval from a
/// forbidden one, because no store operation runs for it (TC-05).
#[derive(Clone)]
pub struct ApprovalDecisionRoute<S> {
    store: S,
}

impl<S> ApprovalDecisionRoute<S>
where
    S: ApprovalRecordStore,
{
    /// Creates the route around one canonical D-6 store.
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// Returns the owned canonical store for crate-internal inspection.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "crate-internal test inspection only")
    )]
    pub(crate) fn store(&self) -> &S {
        &self.store
    }

    /// Applies one authenticated decision and returns its route outcome.
    #[must_use]
    pub fn decide(
        &mut self,
        trust: &TrustContext,
        thread_id: crate::domain::ThreadId,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        decided_at_millis: u64,
    ) -> ApprovalDecisionOutcome {
        let Some(approver) = ApproverId::from_authenticated(trust) else {
            // An unscoped or blank principal learns nothing: no store call,
            // indistinguishable from an unknown approval (TC-05).
            return ApprovalDecisionOutcome::NotFound;
        };
        // Tenant and requester ownership are enforced by the store lookup
        // itself: a record in another tenant, or one created by another
        // subject, is simply absent for this principal's key.
        match self.store.resolve_decision(
            approval_id,
            &trust.tenant_id.clone(),
            thread_id,
            trust.subject_id.as_str(),
            decision,
            &approver,
            decided_at_millis,
        ) {
            Ok(ApprovalDecisionResolution::Won { decision, version }) => {
                ApprovalDecisionOutcome::Resolved {
                    status: decision_status(decision),
                    decision,
                    version,
                }
            }
            Ok(ApprovalDecisionResolution::ExistingTerminal {
                decision: Some(existing),
                status,
                version,
            }) if existing == decision => ApprovalDecisionOutcome::Resolved {
                status,
                decision: existing,
                version,
            },
            Ok(ApprovalDecisionResolution::ExistingTerminal {
                decision,
                status,
                version,
            }) => ApprovalDecisionOutcome::Conflict {
                status,
                decision,
                version,
            },
            Ok(ApprovalDecisionResolution::NotFound) => ApprovalDecisionOutcome::NotFound,
            // The owning Turn is terminal or interrupted: the decision
            // changes nothing and reads as a conflict so no caller can
            // mistake it for a canonical resolution.
            Ok(ApprovalDecisionResolution::TurnGuardRejected) => {
                ApprovalDecisionOutcome::Conflict {
                    status: ApprovalStatus::Requested,
                    decision: None,
                    version: 1,
                }
            }
            Err(ApprovalStoreError::Unavailable | ApprovalStoreError::IdentityConflict) => {
                ApprovalDecisionOutcome::Unavailable
            }
        }
    }
}

fn decision_status(decision: ApprovalDecision) -> ApprovalStatus {
    match decision {
        ApprovalDecision::Accepted => ApprovalStatus::Accepted,
        ApprovalDecision::Declined => ApprovalStatus::Declined,
        ApprovalDecision::Cancelled => ApprovalStatus::Cancelled,
    }
}
