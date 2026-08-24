// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Lease-validating D-7 preparation over the shared process authority root.

use std::sync::Arc;

use crate::domain::execution::{
    AuthorityReclamation, ExactActionBinding, ExecutionAttempt, ExecutionError,
    TurnAuthorityCatalog, TurnExecutionAuthority,
};

use super::attempt_store::CanonicalTurnTerminal;
use super::execution::{ExecutionPreparationError, LeaseCheck, LeaseValidator};

/// Runtime-assembly-owned root for every live Turn execution authority.
///
/// Exactly one root is injected into runtime handles. Its process-local
/// catalog arbitrates allocation while every durable D-7 transition passes
/// through the injected canonical store, and reclamation drops a Turn's
/// authority only after its proven canonical terminal.
#[derive(Debug, Default)]
pub(crate) struct ToolExecutionAuthorityRoot {
    catalog: Arc<TurnAuthorityCatalog>,
}

impl ToolExecutionAuthorityRoot {
    /// Creates the authority root owned by runtime assembly.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

/// Runtime handle that shares the process-owned authority root across preparers.
#[derive(Clone, Debug)]
pub struct ToolExecutionRuntime {
    /// Shared authority catalog, also read by the interruption boundary.
    pub(super) catalog: Arc<TurnAuthorityCatalog>,
}

impl ToolExecutionRuntime {
    /// Creates a handle borrowing authority from runtime assembly's sole root.
    #[must_use]
    pub(crate) fn new(root: &ToolExecutionAuthorityRoot) -> Self {
        Self {
            catalog: Arc::clone(&root.catalog),
        }
    }

    /// Creates a lease-validating preparer backed by this runtime's shared catalog.
    #[must_use]
    pub fn preparer<L>(&self, lease: L) -> ExecutionPreparer<L>
    where
        L: LeaseValidator,
    {
        ExecutionPreparer {
            lease,
            catalog: Arc::clone(&self.catalog),
            authority: None,
        }
    }

    /// Reclaims one Turn's process-local authority after its canonical terminal.
    ///
    /// The durable probe must prove the canonical Turn terminal first: every
    /// durable D-7 preparation and dispatch claim requires the Turn's
    /// `started` status, so only a terminally-proven Turn can never
    /// resurrect its durable attempt budget after its process-local
    /// authority drops. An unproven or unavailable Turn retains its authority
    /// unchanged; a proven terminal retires stale local live and reserved
    /// mirrors before releasing the authority.
    pub fn reclaim_terminated(
        &self,
        canonical: &mut dyn CanonicalTurnTerminal,
        tenant_id: &crate::domain::TenantId,
        thread_id: crate::domain::ThreadId,
        turn_id: crate::domain::TurnId,
    ) -> AuthorityReclamation {
        match canonical.turn_is_terminal(tenant_id, thread_id, turn_id) {
            Ok(true) => self.catalog.reclaim(tenant_id, thread_id, turn_id),
            Ok(false) | Err(_) => AuthorityReclamation::Retained,
        }
    }
}

/// Lease-validating preparation handle scoped to one runtime-owned Turn authority.
///
/// The first successful binding fixes this handle's Turn and profile identity.
/// Every process handle shares that authority. The process root retains it
/// until [`ToolExecutionRuntime::reclaim_terminated`] proves the canonical
/// Turn terminal, so the durable attempt budget cannot be resurrected on an
/// unproven state.
pub struct ExecutionPreparer<L> {
    lease: L,
    catalog: Arc<TurnAuthorityCatalog>,
    authority: Option<TurnExecutionAuthority>,
}

impl<L> ExecutionPreparer<L>
where
    L: LeaseValidator,
{
    /// Validates the current generation before allocating one D-7 slot.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionPreparationError::OwnerFenced`] without allocation when
    /// the binding is stale, [`ExecutionPreparationError::LeaseUnavailable`]
    /// without allocation when ownership could not be validated, or the
    /// underlying authority allocation failure.
    pub fn prepare(
        &mut self,
        binding: ExactActionBinding,
    ) -> Result<(TurnExecutionAuthority, ExecutionAttempt), ExecutionPreparationError> {
        if binding.approval_requirement().is_none() {
            return Err(ExecutionPreparationError::Rejected(
                ExecutionError::PolicyAuthorizationRequired,
            ));
        }
        match self.lease.check_current(&binding) {
            LeaseCheck::Current => {}
            LeaseCheck::Fenced => return Err(ExecutionPreparationError::OwnerFenced),
            LeaseCheck::Unavailable => return Err(ExecutionPreparationError::LeaseUnavailable),
        }
        let authority = self
            .authority
            .get_or_insert_with(|| self.catalog.authority_for(&binding));
        let mut handle = authority.new_handle();
        let attempt = handle
            .allocate_attempt(binding)
            .map_err(ExecutionPreparationError::Rejected)?;
        Ok((handle, attempt))
    }
}
