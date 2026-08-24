// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Canonical D-6 approval lifecycle and its conditional terminal transition.

use thiserror::Error;

use super::{
    APPROVAL_MAX_AGE_MILLIS, ApprovalId, ApprovalRequirement, ExactActionBinding, TenantId,
    ThreadId,
};

/// A client decision for one requested D-6.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalDecision {
    /// Authorizes the exact bound D-7.
    Accepted,
    /// Declines the proposed action.
    Declined,
    /// Cancels the pending request without execution.
    Cancelled,
}

impl ApprovalDecision {
    /// Returns the canonical wire name carried by D-3/D-6 payloads.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Declined => "declined",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Canonical D-6 lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalStatus {
    /// No terminal decision has won.
    Requested,
    /// The exact attempt is authorized.
    Accepted,
    /// The approver declined the action.
    Declined,
    /// The caller or platform cancelled the request.
    Cancelled,
    /// The earlier of the Turn or five-minute deadline elapsed.
    Expired,
}

impl ApprovalStatus {
    /// Returns the terminal status a committed decision produces.
    #[must_use]
    pub const fn from_decision(decision: ApprovalDecision) -> Self {
        match decision {
            ApprovalDecision::Accepted => Self::Accepted,
            ApprovalDecision::Declined => Self::Declined,
            ApprovalDecision::Cancelled => Self::Cancelled,
        }
    }

    /// Returns the canonical wire name carried by D-3/D-6 payloads.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Accepted => "accepted",
            Self::Declined => "declined",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }
}

/// A rejected D-6 transition or authorization check.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ApprovalError {
    /// Another terminal decision already won.
    #[error("approval already resolved")]
    AlreadyResolved,
    /// The approval expired before this transition.
    #[error("approval expired")]
    Expired,
    /// The exact action does not match this approval.
    #[error("approval does not match execution attempt")]
    BindingMismatch,
    /// The request has not been accepted.
    #[error("approval is not accepted")]
    NotAccepted,
    /// The approver identity is empty.
    #[error("approver identity is invalid")]
    InvalidApprover,
    /// C-7 did not validate the principal for this approval identity and scope.
    #[error("approver is not authorized")]
    NotAuthorized,
    /// C-5 did not authorize this exact action for approval creation.
    #[error("policy did not authorize an approval request")]
    PolicyAuthorizationRequired,
    /// The policy-authorized action is not permitted to create a D-6.
    #[error("approval is not required for this action")]
    ApprovalNotRequired,
}

/// Canonical D-6 state with one conditional terminal transition.
#[derive(Debug)]
#[allow(dead_code)] // T-2 approval transport wiring is incomplete.
pub struct ApprovalRequest {
    approval_id: ApprovalId,
    binding: ExactActionBinding,
    requested_at_millis: u64,
    expires_at_millis: u64,
    status: ApprovalStatus,
    decision: Option<ApprovalDecision>,
    approver: Option<String>,
    version: u64,
}

impl ApprovalRequest {
    /// Creates a requested approval with the earlier applicable deadline.
    ///
    /// # Errors
    ///
    /// Returns [`ApprovalError::Expired`] if no positive approval window exists.
    pub fn new(
        binding: ExactActionBinding,
        requested_at_millis: u64,
        turn_deadline_millis: u64,
    ) -> Result<Self, ApprovalError> {
        match binding.approval_requirement() {
            Some(ApprovalRequirement::Required) => {}
            Some(ApprovalRequirement::NotRequired) => {
                return Err(ApprovalError::ApprovalNotRequired);
            }
            None => return Err(ApprovalError::PolicyAuthorizationRequired),
        }
        let five_minute_deadline = requested_at_millis.saturating_add(APPROVAL_MAX_AGE_MILLIS);
        let expires_at_millis = turn_deadline_millis.min(five_minute_deadline);
        if expires_at_millis <= requested_at_millis {
            return Err(ApprovalError::Expired);
        }
        Ok(Self {
            approval_id: ApprovalId::new(),
            binding,
            requested_at_millis,
            expires_at_millis,
            status: ApprovalStatus::Requested,
            decision: None,
            approver: None,
            version: 1,
        })
    }

    /// Returns the stable D-6 identity.
    #[must_use]
    pub const fn approval_id(&self) -> ApprovalId {
        self.approval_id
    }

    /// Returns the current canonical status.
    #[must_use]
    pub const fn status(&self) -> ApprovalStatus {
        self.status
    }

    /// Returns the current canonical record version.
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// Returns the canonical decision, or `None` while requested or expired.
    #[must_use]
    pub const fn decision(&self) -> Option<ApprovalDecision> {
        self.decision
    }

    /// Returns the earlier of the Turn deadline and five-minute age limit.
    #[must_use]
    pub const fn expires_at_millis(&self) -> u64 {
        self.expires_at_millis
    }

    /// Returns the durable D-6 creation time the expiry window was computed from.
    #[must_use]
    pub const fn requested_at_millis(&self) -> u64 {
        self.requested_at_millis
    }

    /// Applies one authenticated decision with idempotent identical replay.
    ///
    /// # Errors
    ///
    /// Returns a typed error for expiry, invalid identity, or conflicting decision.
    #[allow(dead_code)] // T-2 approval transport wiring is incomplete.
    pub(crate) fn apply_validated_decision(
        &mut self,
        decision: ApprovalDecision,
        approver: impl Into<String>,
        decided_at_millis: u64,
    ) -> Result<u64, ApprovalError> {
        let approver = approver.into();
        if approver.trim().is_empty() {
            return Err(ApprovalError::InvalidApprover);
        }
        if self.status != ApprovalStatus::Requested {
            return if self.decision == Some(decision) && self.approver.as_deref() == Some(&approver)
            {
                Ok(self.version)
            } else {
                Err(ApprovalError::AlreadyResolved)
            };
        }
        if decided_at_millis >= self.expires_at_millis {
            self.status = ApprovalStatus::Expired;
            self.version += 1;
            return Err(ApprovalError::Expired);
        }
        self.status = match decision {
            ApprovalDecision::Accepted => ApprovalStatus::Accepted,
            ApprovalDecision::Declined => ApprovalStatus::Declined,
            ApprovalDecision::Cancelled => ApprovalStatus::Cancelled,
        };
        self.decision = Some(decision);
        self.approver = Some(approver);
        self.version += 1;
        Ok(self.version)
    }

    /// Verifies that this accepted D-6 matches every exact binding field.
    ///
    /// # Errors
    ///
    /// Returns [`ApprovalError`] when the request is not accepted or differs.
    pub fn authorize(&self, binding: &ExactActionBinding) -> Result<(), ApprovalError> {
        if self.status != ApprovalStatus::Accepted {
            return Err(ApprovalError::NotAccepted);
        }
        if &self.binding != binding {
            return Err(ApprovalError::BindingMismatch);
        }
        Ok(())
    }

    /// Returns the tenant bound to this canonical approval.
    #[must_use]
    pub fn tenant_id(&self) -> &TenantId {
        self.binding.tenant_id()
    }

    /// Returns the exact D-7 binding this approval authorizes.
    #[must_use]
    pub const fn binding(&self) -> &ExactActionBinding {
        &self.binding
    }

    /// Returns the Thread bound to this canonical approval.
    #[must_use]
    pub const fn thread_id(&self) -> ThreadId {
        self.binding.thread_id()
    }
}
