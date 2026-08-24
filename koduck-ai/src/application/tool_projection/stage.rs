// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Canonical D-3 lifecycle sequencing and reservation transitions.

use crate::domain::execution::{
    ApprovalDecision, ApprovalId, ApprovalStatus, AttemptId, ExecutionStatus,
};

use super::super::executor_envelope::EffectState;
use super::ToolProjection;
use super::validation::{
    worst_case_approval_status_bytes, worst_case_tool_call_bytes, worst_case_tool_result_bytes,
};

/// Canonical lifecycle stage of the serviced call's projection sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProjectionStage {
    /// No lifecycle is open.
    Open,
    /// The named requested D-6 awaits its resolution view.
    ApprovalRequested {
        /// Canonical D-6 identity awaiting its resolution.
        approval_id: ApprovalId,
        /// Exact D-7 identity the open D-6 is bound to.
        attempt_id: AttemptId,
        /// Whether the following D-7 already consumes the one retry.
        retried: bool,
    },
    /// The named accepted D-6 awaits its dispatch view.
    Approved {
        /// Canonical accepted D-6 identity awaiting dispatch.
        approval_id: ApprovalId,
        /// Exact D-7 identity the accepted D-6 authorizes.
        attempt_id: AttemptId,
        /// Whether the following D-7 already consumes the one retry.
        retried: bool,
    },
    /// A closed D-6 awaits the cancelled terminal view of its D-7.
    ApprovalClosed {
        /// Exact prepared D-7 identity the closed D-6 must cancel.
        attempt_id: AttemptId,
    },
    /// The named dispatched D-7 awaits its terminal view.
    Dispatched {
        /// Canonical D-7 identity awaiting its terminal view.
        attempt_id: AttemptId,
        /// Whether this lifecycle already consumed the one retry allowance.
        retried: bool,
    },
    /// A pre-effect failure completed and may be retried exactly once.
    RetryAvailable,
    /// A complete execution or approval lifecycle was recorded.
    Complete,
    /// A complete denial was recorded; the call's sequence is over.
    Denied,
}

/// The lifecycle transition one validated projection performs.
pub(super) struct StagePlan {
    /// Reserved slots released because their projection is landing now.
    pub(super) release_items: usize,
    /// Worst-case bytes released with those slots.
    pub(super) release_bytes: usize,
    /// Newly reserved slots for the remainder of the opened lifecycle.
    pub(super) reserve_items: usize,
    /// Worst-case bytes reserved with those slots.
    pub(super) reserve_bytes: usize,
    /// The lifecycle stage after the projection lands.
    pub(super) stage: ProjectionStage,
}

impl StagePlan {
    /// Builds one transition from its reservation delta and resulting stage.
    const fn new(
        release_items: usize,
        release_bytes: usize,
        reserve_items: usize,
        reserve_bytes: usize,
        stage: ProjectionStage,
    ) -> Self {
        Self {
            release_items,
            release_bytes,
            reserve_items,
            reserve_bytes,
            stage,
        }
    }

    /// Builds one transition that only reserves the opened remainder.
    const fn reserve(reserve_items: usize, reserve_bytes: usize, stage: ProjectionStage) -> Self {
        Self::new(0, 0, reserve_items, reserve_bytes, stage)
    }

    /// Builds one transition that only releases the landing remainder.
    const fn release(release_items: usize, release_bytes: usize, stage: ProjectionStage) -> Self {
        Self::new(release_items, release_bytes, 0, 0, stage)
    }
}

impl ProjectionStage {
    /// Plans the lifecycle transition for one tuple-validated projection.
    pub(super) fn plan(self, projection: &ToolProjection) -> Option<StagePlan> {
        match self {
            Self::Open => plan_open(projection),
            Self::ApprovalRequested {
                approval_id,
                attempt_id,
                retried,
            } => plan_requested(projection, approval_id, attempt_id, retried),
            Self::Approved {
                attempt_id,
                retried,
                ..
            } => plan_approved(projection, attempt_id, retried),
            Self::ApprovalClosed { attempt_id } => plan_closed(projection, attempt_id),
            Self::Dispatched {
                attempt_id,
                retried,
            } => plan_dispatched(projection, attempt_id, retried),
            Self::RetryAvailable => plan_retry(projection),
            Self::Complete | Self::Denied => None,
        }
    }
}

/// Plans the first projection of a canonical lifecycle.
fn plan_open(projection: &ToolProjection) -> Option<StagePlan> {
    let approval_bytes = worst_case_approval_status_bytes();
    let call_bytes = worst_case_tool_call_bytes();
    let result_bytes = worst_case_tool_result_bytes();
    match projection {
        ToolProjection::ApprovalStatus {
            approval_id,
            attempt_id,
            status: ApprovalStatus::Requested,
            ..
        } => Some(StagePlan::reserve(
            3,
            approval_bytes + call_bytes + result_bytes,
            ProjectionStage::ApprovalRequested {
                approval_id: *approval_id,
                attempt_id: *attempt_id,
                retried: false,
            },
        )),
        ToolProjection::ToolCall { attempt_id, .. } => Some(StagePlan::reserve(
            1,
            result_bytes,
            ProjectionStage::Dispatched {
                attempt_id: *attempt_id,
                retried: false,
            },
        )),
        ToolProjection::Denied { .. } => Some(StagePlan::reserve(0, 0, ProjectionStage::Denied)),
        ToolProjection::ToolResult {
            status: ExecutionStatus::Cancelled,
            ..
        } => Some(StagePlan::reserve(0, 0, ProjectionStage::Complete)),
        _ => None,
    }
}

/// Plans the resolution of one requested approval record.
fn plan_requested(
    projection: &ToolProjection,
    open_id: ApprovalId,
    open_attempt_id: AttemptId,
    retried: bool,
) -> Option<StagePlan> {
    let ToolProjection::ApprovalStatus {
        approval_id,
        attempt_id,
        status,
        decision,
        ..
    } = projection
    else {
        return None;
    };
    if matches!(status, ApprovalStatus::Requested)
        || *approval_id != open_id
        || *attempt_id != open_attempt_id
    {
        return None;
    }
    let approval_bytes = worst_case_approval_status_bytes();
    let call_bytes = worst_case_tool_call_bytes();
    let approved = *decision == Some(ApprovalDecision::Accepted);
    Some(StagePlan::release(
        if approved { 1 } else { 2 },
        if approved {
            approval_bytes
        } else {
            approval_bytes + call_bytes
        },
        if approved {
            ProjectionStage::Approved {
                approval_id: open_id,
                attempt_id: open_attempt_id,
                retried,
            }
        } else {
            ProjectionStage::ApprovalClosed {
                attempt_id: open_attempt_id,
            }
        },
    ))
}

/// Plans the approved dispatch, closed cancellation, or dispatched terminal.
fn plan_approved(
    projection: &ToolProjection,
    open_id: AttemptId,
    retried: bool,
) -> Option<StagePlan> {
    let ToolProjection::ToolCall { attempt_id, .. } = projection else {
        return None;
    };
    (*attempt_id == open_id).then(|| {
        StagePlan::release(
            1,
            worst_case_tool_call_bytes(),
            ProjectionStage::Dispatched {
                attempt_id: *attempt_id,
                retried,
            },
        )
    })
}

/// Plans the cancellation required after a closed approval record.
fn plan_closed(projection: &ToolProjection, open_id: AttemptId) -> Option<StagePlan> {
    let ToolProjection::ToolResult {
        attempt_id,
        status: ExecutionStatus::Cancelled,
        ..
    } = projection
    else {
        return None;
    };
    (*attempt_id == open_id)
        .then(|| StagePlan::release(1, worst_case_tool_result_bytes(), ProjectionStage::Complete))
}

/// Plans the terminal projection for a dispatched attempt.
fn plan_dispatched(
    projection: &ToolProjection,
    open_id: AttemptId,
    retried: bool,
) -> Option<StagePlan> {
    let ToolProjection::ToolResult {
        attempt_id,
        status,
        effect_state,
        ..
    } = projection
    else {
        return None;
    };
    if *attempt_id != open_id {
        return None;
    }
    let retry =
        *status == ExecutionStatus::Failed && *effect_state == EffectState::NotStarted && !retried;
    Some(StagePlan::release(
        1,
        worst_case_tool_result_bytes(),
        if retry {
            ProjectionStage::RetryAvailable
        } else {
            ProjectionStage::Complete
        },
    ))
}

/// Plans the one allowed pre-effect retry.
fn plan_retry(projection: &ToolProjection) -> Option<StagePlan> {
    let approval_bytes = worst_case_approval_status_bytes();
    let call_bytes = worst_case_tool_call_bytes();
    let result_bytes = worst_case_tool_result_bytes();
    match projection {
        ToolProjection::Denied { .. } => Some(StagePlan::reserve(0, 0, ProjectionStage::Denied)),
        ToolProjection::ToolCall { attempt_id, .. } => Some(StagePlan::reserve(
            1,
            result_bytes,
            ProjectionStage::Dispatched {
                attempt_id: *attempt_id,
                retried: true,
            },
        )),
        ToolProjection::ApprovalStatus {
            approval_id,
            attempt_id,
            status: ApprovalStatus::Requested,
            ..
        } => Some(StagePlan::reserve(
            3,
            approval_bytes + call_bytes + result_bytes,
            ProjectionStage::ApprovalRequested {
                approval_id: *approval_id,
                attempt_id: *attempt_id,
                retried: true,
            },
        )),
        _ => None,
    }
}
