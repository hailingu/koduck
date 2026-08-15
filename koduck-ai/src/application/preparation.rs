// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Lease-validating D-7 preparation over the shared process authority root.

use std::sync::Arc;

use crate::domain::execution::{
    ExactActionBinding, ExecutionAttempt, ExecutionError, TurnAuthorityCatalog,
    TurnExecutionAuthority,
};

use super::execution::{ExecutionPreparationError, LeaseCheck, LeaseValidator};

/// Runtime-assembly-owned root for every live Turn execution authority.
///
/// Exactly one root is injected into runtime handles. T-3 replaces its
/// process-local catalog with canonical persistence.
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
}

/// Lease-validating preparation handle scoped to one runtime-owned Turn authority.
///
/// The first successful binding fixes this handle's Turn and profile identity.
/// Every process handle shares that authority. The process root strongly retains
/// it until T-3 can bind reclamation to canonical persistence without resurrection.
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
