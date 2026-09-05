// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md
// ADR: docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md
// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md

//! Provider-neutral application orchestration and consumer-owned ports.

mod approval_route;
mod approval_store;
pub(crate) mod attempt_store;
mod audit;
mod cancellation;
mod canonical_dispatch;
mod correction_store;
mod deadline;
mod delta_coalescer;
mod durability;
mod execution;
mod execution_dispatch;
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
    InterruptionBarrierResolution, NoCanonicalTurnTerminal, PreparedCloseResolution,
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
pub use correction_store::{
    CorrectionCommand, CorrectionError, CorrectionStore, MAX_CORRECTION_CONTENT_BYTES,
};
pub use deadline::ActionDeadline;
pub(crate) use deadline::MAX_ACTION_DURATION_MILLIS;
pub use delta_coalescer::{DELTA_FLUSH_LATENCY, DeltaCoalescer, MAX_BUFFERED_DELTA_BYTES};
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
pub(crate) use tool_projection::validate_interruption_terminals;
pub use tool_projection::{
    NoToolProjections, ToolProjection, ToolProjectionError, ToolProjectionSink, output_digest,
};
