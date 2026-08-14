// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Provider-neutral application orchestration and consumer-owned ports.

mod cancellation;
mod deadline;
mod durability;
mod execution;
mod executor_envelope;
mod policy;
mod ports;
mod runner;
mod terminal;
mod tool_boundary;
mod tool_execution;

#[cfg(test)]
pub(crate) use cancellation::{
    AttemptCancellationService, ExecutionInterrupter, InterruptionOutcome,
    PendingApprovalCancellation, PendingApprovalCanceller,
};
pub use cancellation::{CancelAcknowledgement, CancelPermit, CancelledEffectState};
pub use deadline::ActionDeadline;
pub use durability::{AppendPolicy, BufferLimitError};
#[cfg(test)]
pub(crate) use execution::ToolExecutionAuthorityRoot;
pub use execution::*;
pub use executor_envelope::*;
pub use policy::{
    DenialCode, PolicyDecision, TOOL_APPROVAL_SCOPE, ToolConfigurationError,
    ToolConfigurationSnapshot, ToolPolicy,
};
#[cfg(test)]
pub(crate) use policy::{ToolAuthorizationService, ToolPolicyConfiguration};
pub use ports::*;
pub use runner::TurnRunner;
pub(crate) use tool_boundary::ToolExecutionRuntimeRoot;
#[cfg(test)]
pub(crate) use tool_boundary::{ToolExecutionAssembly, ToolExecutionBoundary};
#[cfg(test)]
pub(crate) use tool_execution::ToolExecutionDriver;
pub use tool_execution::{ToolCallError, ToolCallInputs};
