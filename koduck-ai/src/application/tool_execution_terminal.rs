// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! D-3 projections of the C-5 driver's canonical D-6 requests and committed
//! D-7 terminals (ADR-0003 TC-06).

use crate::domain::execution::{ApprovalRequest, AttemptId, ExecutionAttempt, ExecutionStatus};

use super::execution::ToolExecutionOutcome;
use super::executor_envelope::ExecutionFailure;
use super::tool_projection::{
    ToolProjection, ToolProjectionSink, attempt_version, emit, output_digest,
};

/// Appends the D-3 terminal-result projection of one committed D-7 outcome.
pub(super) fn emit_tool_result(
    attempt: &ExecutionAttempt,
    outcome: &ToolExecutionOutcome,
    projections: &mut dyn ToolProjectionSink,
) {
    emit(
        projections,
        tool_result_projection(attempt.binding().attempt_id(), outcome),
    );
}

/// Builds the canonical D-7 terminal projection for a committed outcome.
pub(super) fn tool_result_projection(
    attempt_id: AttemptId,
    outcome: &ToolExecutionOutcome,
) -> ToolProjection {
    ToolProjection::ToolResult {
        attempt_id,
        status: outcome_status(outcome),
        code: outcome_failure_code(outcome),
        effect_state: outcome.effect_state(),
        output_bytes: outcome_output_bytes(outcome),
        output_digest: outcome_output_digest(outcome),
        version: attempt_version(outcome_status(outcome)),
    }
}

/// Appends and publishes the requested canonical D-6 projection.
pub(super) fn emit_requested_approval(
    request: &ApprovalRequest,
    projections: &mut dyn ToolProjectionSink,
) {
    emit(
        projections,
        ToolProjection::ApprovalStatus {
            approval_id: request.approval_id(),
            attempt_id: request.binding().attempt_id(),
            status: request.status(),
            decision: request.decision(),
            version: request.version(),
        },
    );
}

/// Maps one committed outcome onto its canonical D-7 terminal status.
fn outcome_status(outcome: &ToolExecutionOutcome) -> ExecutionStatus {
    match outcome {
        ToolExecutionOutcome::Succeeded { .. } => ExecutionStatus::Succeeded,
        ToolExecutionOutcome::Failed { .. } => ExecutionStatus::Failed,
        ToolExecutionOutcome::TimedOut { .. } => ExecutionStatus::TimedOut,
        ToolExecutionOutcome::Cancelled { .. } => ExecutionStatus::Cancelled,
    }
}

/// Returns the serialized size of a committed successful outcome's output.
fn outcome_output_bytes(outcome: &ToolExecutionOutcome) -> u64 {
    match outcome {
        ToolExecutionOutcome::Succeeded { output, .. } => output.len() as u64,
        ToolExecutionOutcome::Failed { .. }
        | ToolExecutionOutcome::TimedOut { .. }
        | ToolExecutionOutcome::Cancelled { .. } => 0,
    }
}

/// Returns the durable continuation-binding digest for a successful output.
fn outcome_output_digest(outcome: &ToolExecutionOutcome) -> Option<String> {
    match outcome {
        ToolExecutionOutcome::Succeeded { output, .. } => Some(output_digest(output)),
        ToolExecutionOutcome::Failed { .. }
        | ToolExecutionOutcome::TimedOut { .. }
        | ToolExecutionOutcome::Cancelled { .. } => None,
    }
}

/// Returns the stable failure code of a committed failed outcome.
fn outcome_failure_code(outcome: &ToolExecutionOutcome) -> Option<ExecutionFailure> {
    match outcome {
        ToolExecutionOutcome::Failed { code, .. } => Some(*code),
        ToolExecutionOutcome::Succeeded { .. }
        | ToolExecutionOutcome::TimedOut { .. }
        | ToolExecutionOutcome::Cancelled { .. } => None,
    }
}
