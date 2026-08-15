// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Append-before-publish D-3 projections of canonical C-5 state (TC-06).

use thiserror::Error;

use crate::domain::execution::{
    ApprovalDecision, ApprovalId, ApprovalStatus, AttemptId, ExecutionStatus,
};

use super::executor_envelope::{EffectState, ExecutionFailure};

/// One append-only D-3 view of canonical D-6/D-7 state.
///
/// A projection carries its canonical identity and version and is published
/// only after its durable append succeeds; it is never authority and can never
/// be read back to authorize or redispatch execution (ADR-0003 TC-06).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolProjection {
    /// D-6 approval-status view at a canonical record version.
    ApprovalStatus {
        /// Canonical D-6 identity.
        approval_id: ApprovalId,
        /// Canonical status at this version.
        status: ApprovalStatus,
        /// Canonical decision, or `None` while requested or expired.
        decision: Option<ApprovalDecision>,
        /// Canonical D-6 record version.
        version: u64,
    },
    /// D-7 dispatch-phase view.
    ToolCall {
        /// Canonical D-7 identity.
        attempt_id: AttemptId,
        /// Canonical D-7 lifecycle phase at this version.
        status: ExecutionStatus,
        /// Canonical D-7 transition version.
        version: u64,
    },
    /// D-7 terminal-result view.
    ToolResult {
        /// Canonical D-7 identity.
        attempt_id: AttemptId,
        /// Canonical terminal lifecycle status.
        status: ExecutionStatus,
        /// Stable terminal failure code, or `None` for non-failed terminals.
        code: Option<ExecutionFailure>,
        /// Executor-observed effect state evidence.
        effect_state: EffectState,
        /// Canonical D-7 transition version.
        version: u64,
    },
}

/// A D-3 projection append could not complete durably.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ToolProjectionError {
    /// The durable append did not complete within its availability contract.
    #[error("tool projection append unavailable")]
    Unavailable,
}

/// Consumer-owned append-before-publish boundary for D-3 projections.
///
/// `append` performs the durable append; `publish` makes the projection
/// externally visible and MAY be called only after `append` reported success
/// for the same value. A failed append suppresses publication but changes no
/// canonical D-6/D-7 state: the projection is a view, so authority and
/// dispatch decisions never depend on it (ADR-0003 TC-06).
pub trait ToolProjectionSink {
    /// Durably appends one projection.
    ///
    /// # Errors
    ///
    /// Returns [`ToolProjectionError`] when the durable append cannot complete.
    fn append(&mut self, projection: &ToolProjection) -> Result<(), ToolProjectionError>;

    /// Publishes one already durably appended projection.
    fn publish(&mut self, projection: &ToolProjection);
}

/// Explicit unconfigured projection boundary.
///
/// Appends succeed without durable effect and nothing is published, so C-5
/// callers that have not been wired to a D-3 history bridge still observe the
/// canonical outcomes directly; the runtime D-3 bridge replaces this sink when
/// the transport wiring lands.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoToolProjections;

impl ToolProjectionSink for NoToolProjections {
    fn append(&mut self, _projection: &ToolProjection) -> Result<(), ToolProjectionError> {
        Ok(())
    }

    fn publish(&mut self, _projection: &ToolProjection) {}
}

/// Appends one projection and publishes it only when the append succeeded.
///
/// A failed append suppresses publication and changes no canonical D-6/D-7
/// state (ADR-0003 TC-06), but the failure is never concealed: it is reported
/// as a structured diagnostic so operators and reconciliation tooling can
/// observe the missing durable view.
pub(crate) fn emit(sink: &mut dyn ToolProjectionSink, projection: ToolProjection) {
    match sink.append(&projection) {
        Ok(()) => sink.publish(&projection),
        Err(error) => {
            eprintln!(
                "event=tool_projection_append_failed error={error} projection={projection:?}"
            );
        }
    }
}

/// Canonical D-7 transition version for one lifecycle phase.
///
/// `prepared` is version 1, `running` version 2, and every terminal phase
/// version 3, so a projection sequence references strictly increasing
/// canonical versions along one attempt's transitions.
#[must_use]
pub(crate) const fn attempt_version(status: ExecutionStatus) -> u64 {
    match status {
        ExecutionStatus::Prepared => 1,
        ExecutionStatus::Running => 2,
        ExecutionStatus::Succeeded
        | ExecutionStatus::Failed
        | ExecutionStatus::TimedOut
        | ExecutionStatus::Cancelled => 3,
    }
}
