// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Production executor adapter kept fail-closed until a capability is approved.

use crate::application::{
    DispatchPermit, EffectState, ExecutionFailure, ExecutionResponse, ExecutorError,
    IsolatedExecutor,
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
    ) -> Result<ExecutionResponse, ExecutorError> {
        Err(ExecutorError::new(
            ExecutionFailure::ExecutorUnavailable,
            EffectState::NotStarted,
        ))
    }
}
