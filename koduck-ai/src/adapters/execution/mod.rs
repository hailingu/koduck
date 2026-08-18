// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Production executor adapter kept fail-closed until a capability is approved.

pub mod attempts;
pub mod lease;
pub mod postgres;

pub use attempts::SqlxExecutionAttemptStore;
pub use lease::SqlxTurnLeaseValidator;
pub use postgres::SqlxApprovalRecordStore;

use crate::application::{
    ActionDeadline, CancelAcknowledgement, CancelPermit, DispatchPermit, EffectState,
    ExecutionFailure, ExecutionResponse, ExecutorError, IsolatedExecutor,
};
use crate::domain::execution::ExactActionBinding;

/// Empty-production-inventory executor that exposes no effect path or fallback.
#[derive(Clone, Copy, Debug, Default)]
pub struct DisabledExecutor;

impl IsolatedExecutor for DisabledExecutor {
    fn execute(
        &mut self,
        _permit: &DispatchPermit,
        _binding: &ExactActionBinding,
        _deadline: ActionDeadline,
    ) -> Result<ExecutionResponse, ExecutorError> {
        Err(ExecutorError::new(
            ExecutionFailure::ExecutorUnavailable,
            EffectState::NotStarted,
        ))
    }

    fn cancel(
        &mut self,
        _permit: &CancelPermit,
        _binding: &ExactActionBinding,
        _deadline: ActionDeadline,
    ) -> CancelAcknowledgement {
        // Nothing is ever dispatched, and this adapter cannot wait for a
        // cancellation deadline that belongs to an external executor.
        CancelAcknowledgement::Unavailable
    }
}
