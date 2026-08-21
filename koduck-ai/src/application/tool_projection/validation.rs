// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Canonical D-3 tuple validation and exact lifecycle-reservation measurement.

use crate::application::{AppendPolicy, NewItem};
use crate::domain::ToolEffectState;
use crate::domain::execution::{
    ApprovalDecision, ApprovalId, ApprovalStatus, AttemptId, ExecutionStatus,
};
use crate::domain::tool::{
    MAX_ACTION_TARGET_BYTES, MAX_DESCRIPTOR_ID_BYTES, MAX_DESCRIPTOR_VERSION_BYTES,
    validate_action_target, validate_descriptor_id, validate_descriptor_version,
};

use super::super::executor_envelope::{ExecutionFailure, MAX_EXECUTOR_OUTPUT_BYTES};
use super::{ToolProjection, ToolProjectionError, approval_version, attempt_version};

/// Validates one projection's canonical tuple before durable D-3 persistence.
pub(super) fn validate_canonical_tuple(
    projection: &ToolProjection,
) -> Result<(), ToolProjectionError> {
    let valid = match projection {
        ToolProjection::ApprovalStatus {
            status,
            decision,
            version,
            ..
        } => {
            *version == approval_version(*status)
                && match status {
                    ApprovalStatus::Requested | ApprovalStatus::Expired => decision.is_none(),
                    ApprovalStatus::Accepted => *decision == Some(ApprovalDecision::Accepted),
                    ApprovalStatus::Declined => *decision == Some(ApprovalDecision::Declined),
                    // An authenticated interruption owns this cancellation
                    // without becoming a C-7 approval decision. Ordinary
                    // C-7 cancellation decisions retain their explicit
                    // `cancelled` value (ADR-0003 TC-06).
                    ApprovalStatus::Cancelled => {
                        decision.is_none() || *decision == Some(ApprovalDecision::Cancelled)
                    }
                }
        }
        ToolProjection::ToolCall {
            descriptor_id,
            descriptor_version,
            target,
            status,
            version,
            ..
        } => {
            validate_descriptor_id(descriptor_id).is_ok()
                && validate_descriptor_version(descriptor_version).is_ok()
                && validate_action_target(target).is_ok()
                && matches!(status, ExecutionStatus::Running)
                && *version == attempt_version(*status)
        }
        ToolProjection::ToolResult {
            status,
            code,
            output_bytes,
            output_digest,
            version,
            ..
        } => {
            !matches!(status, ExecutionStatus::Prepared | ExecutionStatus::Running)
                && (*status == ExecutionStatus::Failed) == code.is_some()
                && (*status == ExecutionStatus::Succeeded || *output_bytes == 0)
                && (*status == ExecutionStatus::Succeeded) == output_digest.is_some()
                && output_digest.as_deref().is_none_or(is_sha256_hex)
                && *output_bytes <= MAX_EXECUTOR_OUTPUT_BYTES as u64
                && *version == attempt_version(*status)
        }
        ToolProjection::Denied {
            descriptor_id,
            descriptor_version,
            target,
            code,
        } => {
            !code.is_empty()
                && (descriptor_id.is_empty() || validate_descriptor_id(descriptor_id).is_ok())
                && (descriptor_version.is_empty()
                    || validate_descriptor_version(descriptor_version).is_ok())
                && (target.is_empty() || validate_action_target(target).is_ok())
        }
    };
    valid.then_some(()).ok_or(ToolProjectionError::Unavailable)
}

/// Computes exact serialized bounds for each canonical D-3 view shape.
pub(super) fn worst_case_approval_status_bytes() -> usize {
    measured_bytes(&NewItem::ApprovalStatus {
        approval_id: ApprovalId::new(),
        attempt_id: AttemptId::new(),
        status: ApprovalStatus::Requested,
        decision: Some(ApprovalDecision::Cancelled),
        version: u64::MAX,
    })
}

/// Computes the maximum size of a canonical D-7 dispatch view.
pub(super) fn worst_case_tool_call_bytes() -> usize {
    measured_bytes(&NewItem::ToolCall {
        descriptor_id: "\"".repeat(MAX_DESCRIPTOR_ID_BYTES),
        descriptor_version: "\"".repeat(MAX_DESCRIPTOR_VERSION_BYTES),
        target: "\"".repeat(MAX_ACTION_TARGET_BYTES),
        attempt_id: Some(AttemptId::new()),
        status: Some(ExecutionStatus::Prepared),
        version: Some(u64::MAX),
    })
}

/// Computes the maximum size among valid successful and failed D-7 terminals.
pub(super) fn worst_case_tool_result_bytes() -> usize {
    let succeeded = measured_bytes(&NewItem::ToolResult {
        attempt_id: Some(AttemptId::new()),
        status: ExecutionStatus::Succeeded,
        code: None,
        effect_state: Some(ToolEffectState::NotStarted),
        output_bytes: MAX_EXECUTOR_OUTPUT_BYTES as u64,
        output_digest: Some("f".repeat(64)),
        version: Some(u64::MAX),
    });
    let failed = measured_bytes(&NewItem::ToolResult {
        attempt_id: Some(AttemptId::new()),
        status: ExecutionStatus::Failed,
        code: Some(
            ExecutionFailure::OwnerFencedBeforeDispatch
                .stable_code()
                .to_owned(),
        ),
        effect_state: Some(ToolEffectState::NotStarted),
        output_bytes: 0,
        output_digest: None,
        version: Some(u64::MAX),
    });
    succeeded.max(failed)
}

/// Recognizes the fixed-width lower-case SHA-256 encoding retained in D-3.
fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Measures one item through the canonical unpublished-buffer accounting.
fn measured_bytes(item: &NewItem) -> usize {
    AppendPolicy::cand_1()
        .accumulate_payload_bytes(0, item)
        .expect("one worst-case item fits the buffer")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interruption_owned_approval_cancellation_is_a_canonical_projection() {
        let projection = ToolProjection::ApprovalStatus {
            approval_id: ApprovalId::new(),
            attempt_id: AttemptId::new(),
            status: ApprovalStatus::Cancelled,
            decision: None,
            version: approval_version(ApprovalStatus::Cancelled),
        };

        assert_eq!(validate_canonical_tuple(&projection), Ok(()));
    }
}
