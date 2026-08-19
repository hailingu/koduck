// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Provider-neutral application orchestration and consumer-owned ports.

mod approval_route;
mod approval_store;
pub(crate) mod attempt_store;
mod audit;
mod cancellation;
mod canonical_dispatch;
mod deadline;
mod durability;
mod execution;
mod executor_envelope;
mod policy;
mod ports;
mod preparation;
mod runner;
mod runner_terminals;
mod terminal;
pub(crate) mod tool_boundary;
mod tool_execution;
mod tool_execution_terminal;
mod tool_interruption;
pub(crate) mod tool_projection;

pub use approval_route::{ApprovalDecisionOutcome, ApprovalDecisionRoute};
pub use approval_store::{
    ApprovalDecisionResolution, ApprovalInsertResolution, ApprovalRecordStore, ApprovalStoreError,
};
pub use attempt_store::{
    AttemptInsertResolution, AttemptStoreError, AttemptTerminalResolution, CanonicalTurnTerminal,
    DispatchClaimResolution, DurableAttemptTerminal, DurableAttemptTransitions,
    ExecutionAttemptInterruptionGuard, ExecutionAttemptLiveness, ExecutionAttemptStore,
    NoCanonicalTurnTerminal, PreparedCloseResolution,
};
pub(crate) use audit::record_audit;
pub use audit::{
    MAX_AUDIT_RECORD_BYTES, NoToolAudits, PolicyDenialContext, ToolAuditEmitError, ToolAuditError,
    ToolAuditRecord, ToolAuditRecordTooLarge, ToolAuditSink, ToolAuditTrail,
};
pub(crate) use cancellation::InterruptionOutcome;
#[cfg(test)]
pub(crate) use cancellation::{AttemptCancellationService, ExecutionInterrupter};
pub use cancellation::{CancelAcknowledgement, CancelPermit, CancelledEffectState};
pub(crate) use cancellation::{PendingApprovalCancellation, PendingApprovalCanceller};
pub use deadline::ActionDeadline;
pub use durability::{AppendPolicy, BufferLimitError};
pub use execution::*;
pub use executor_envelope::*;
pub use policy::{
    DenialCode, PolicyDecision, TOOL_APPROVAL_SCOPE, ToolConfigurationError,
    ToolConfigurationSnapshot, ToolPolicy,
};
#[cfg(test)]
pub(crate) use policy::{ToolAuthorizationService, ToolPolicyConfiguration};
pub use ports::*;
#[cfg(test)]
pub(crate) use preparation::ToolExecutionAuthorityRoot;
pub use preparation::{ExecutionPreparer, ToolExecutionRuntime};
pub use runner::TurnRunner;
pub(crate) use tool_boundary::ToolExecutionRuntimeRoot;
#[cfg(test)]
pub(crate) use tool_boundary::{ToolExecutionAssembly, ToolExecutionBoundary};
#[cfg(test)]
pub(crate) use tool_execution::ToolExecutionDriver;
pub use tool_execution::{ToolCallError, ToolCallInputs};
#[cfg(test)]
pub(crate) use tool_interruption::{
    ToolInterruptionOutcome, ToolInterruptionRoute, TurnInterruptionOwnership,
    TurnOwnershipValidator,
};
pub use tool_projection::{
    NoToolProjections, ToolProjection, ToolProjectionError, ToolProjectionSink, output_digest,
};
