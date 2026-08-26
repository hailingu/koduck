// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md
// ADR: koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md

//! Strict canonical D-3 projection payload decoding.

use serde_json::Value;

use crate::application::HistoryError;
use crate::domain::execution::{
    ApprovalDecision, ApprovalId, ApprovalStatus, AttemptId, ExecutionStatus,
};
use crate::domain::{ItemPayload, ToolEffectState};

use super::{
    field, parse_approval_decision, parse_approval_status, parse_effect_state, parse_status,
    required_optional_enum, required_optional_text, required_optional_u64, required_optional_uuid,
};

/// Decodes a D-3 projection payload, or returns `None` for another item kind.
pub(super) fn decode(
    item_type: &str,
    payload: &Value,
) -> Result<Option<ItemPayload>, HistoryError> {
    match item_type {
        "approval_status" => decode_approval_status(payload).map(Some),
        "tool_call" => decode_tool_call(payload).map(Some),
        "tool_result" => decode_tool_result(payload).map(Some),
        _ => Ok(None),
    }
}

/// Decodes one D-6 view and validates its exact canonical state tuple.
fn decode_approval_status(payload: &Value) -> Result<ItemPayload, HistoryError> {
    let status = parse_approval_status(required_text(payload, "status")?)?;
    let decision = match payload.get("decision") {
        None | Some(Value::Null) => None,
        Some(Value::String(decision)) => Some(parse_approval_decision(decision)?),
        Some(_) => return Err(HistoryError::Unavailable),
    };
    let version = payload
        .get("version")
        .and_then(Value::as_u64)
        .ok_or(HistoryError::Unavailable)?;
    let canonical = version == crate::application::tool_projection::approval_version(status)
        && match status {
            ApprovalStatus::Requested | ApprovalStatus::Expired => decision.is_none(),
            ApprovalStatus::Accepted => decision == Some(ApprovalDecision::Accepted),
            ApprovalStatus::Declined => decision == Some(ApprovalDecision::Declined),
            // An authenticated interruption owns this cancellation without
            // creating a C-7 decision. Existing ordinary C-7 cancellations
            // keep their explicit `cancelled` decision.
            ApprovalStatus::Cancelled => {
                decision.is_none() || decision == Some(ApprovalDecision::Cancelled)
            }
        };
    if !canonical {
        return Err(HistoryError::Unavailable);
    }
    Ok(ItemPayload::ApprovalStatus {
        approval_id: ApprovalId::from_uuid(
            uuid::Uuid::parse_str(required_text(payload, "approval_id")?)
                .map_err(|_| HistoryError::Unavailable)?,
        ),
        attempt_id: required_optional_uuid(payload, "attempt_id")?
            .ok_or(HistoryError::Unavailable)?,
        status,
        decision,
        version,
    })
}

/// Decodes one D-7 dispatch view or a pre-D-7 denial placeholder.
fn decode_tool_call(payload: &Value) -> Result<ItemPayload, HistoryError> {
    let attempt_id = required_optional_uuid(payload, "attempt_id")?;
    let status = required_optional_enum(payload, "status", parse_status)?;
    let version = required_optional_u64(payload, "version")?;
    let canonical = attempt_id.is_some();
    let terminal = matches!(
        status,
        Some(
            ExecutionStatus::Succeeded
                | ExecutionStatus::Failed
                | ExecutionStatus::TimedOut
                | ExecutionStatus::Cancelled
        )
    );
    if canonical != status.is_some()
        || canonical != version.is_some()
        || canonical && version != status.map(crate::application::tool_projection::attempt_version)
        || terminal
    {
        return Err(HistoryError::Unavailable);
    }
    let descriptor_id = field(payload, "descriptor_id")?;
    let descriptor_version = field(payload, "descriptor_version")?;
    let target = field(payload, "target")?;
    let valid = if canonical {
        crate::domain::tool::validate_descriptor_id(&descriptor_id).is_ok()
            && crate::domain::tool::validate_descriptor_version(&descriptor_version).is_ok()
            && crate::domain::tool::validate_action_target(&target).is_ok()
    } else {
        (descriptor_id.is_empty()
            || crate::domain::tool::validate_descriptor_id(&descriptor_id).is_ok())
            && (descriptor_version.is_empty()
                || crate::domain::tool::validate_descriptor_version(&descriptor_version).is_ok())
            && (target.is_empty() || crate::domain::tool::validate_action_target(&target).is_ok())
    };
    valid
        .then_some(ItemPayload::ToolCall {
            descriptor_id,
            descriptor_version,
            target,
            attempt_id,
            status,
            version,
        })
        .ok_or(HistoryError::Unavailable)
}

/// Decodes one D-7 terminal view or the typed pre-D-7 denial result.
fn decode_tool_result(payload: &Value) -> Result<ItemPayload, HistoryError> {
    let attempt_id = required_optional_uuid(payload, "attempt_id")?;
    let status = parse_status(required_text(payload, "status")?)?;
    let code = required_optional_text(payload, "code")?;
    let effect_state = required_optional_enum(payload, "effect_state", parse_effect_state)?;
    let version = required_optional_u64(payload, "version")?;
    let output_bytes = payload
        .get("output_bytes")
        .and_then(Value::as_u64)
        .ok_or(HistoryError::Unavailable)?;
    let output_digest = required_optional_text(payload, "output_digest")?;
    let canonical = attempt_id.is_some();
    let terminal = !matches!(status, ExecutionStatus::Prepared | ExecutionStatus::Running);
    let invalid_digest = output_digest.as_deref().is_some_and(|digest| {
        digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if !terminal
        || effect_state.is_some() != canonical
        || version.is_some() != canonical
        || canonical
            && version != Some(crate::application::tool_projection::attempt_version(status))
        || (status == ExecutionStatus::Failed) != code.is_some()
        || status == ExecutionStatus::Cancelled && effect_state == Some(ToolEffectState::Unknown)
        || output_bytes > crate::application::MAX_EXECUTOR_OUTPUT_BYTES as u64
        || (status != ExecutionStatus::Succeeded && output_bytes != 0)
        || (status == ExecutionStatus::Succeeded) != output_digest.is_some()
        || invalid_digest
        || !canonical && (status != ExecutionStatus::Failed || output_bytes != 0)
    {
        return Err(HistoryError::Unavailable);
    }
    Ok(ItemPayload::ToolResult {
        attempt_id,
        status,
        code,
        effect_state,
        output_bytes,
        output_digest,
        version,
    })
}

/// Reads a mandatory text member without coercing malformed JSON.
fn required_text<'a>(payload: &'a Value, name: &str) -> Result<&'a str, HistoryError> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .ok_or(HistoryError::Unavailable)
}

/// Encodes the canonical D-6 view payload JSON for one approval status.
pub(super) fn approval_status_json(
    approval_id: ApprovalId,
    attempt_id: AttemptId,
    status: ApprovalStatus,
    decision: Option<ApprovalDecision>,
    version: u64,
) -> Value {
    serde_json::json!({
        "approval_id": approval_id.as_uuid().to_string(),
        "attempt_id": attempt_id.as_uuid().to_string(),
        "status": status.as_str(),
        "decision": decision.map(|decision| decision.as_str().to_owned()),
        "version": version,
    })
}

/// Encodes the canonical dispatch-view payload JSON for one Tool call.
pub(super) fn tool_call_json(
    descriptor_id: &str,
    descriptor_version: &str,
    target: &str,
    attempt_id: Option<AttemptId>,
    status: Option<ExecutionStatus>,
    version: Option<u64>,
) -> Value {
    serde_json::json!({
        "descriptor_id": descriptor_id,
        "descriptor_version": descriptor_version,
        "target": target,
        "attempt_id": attempt_id.map(|id| id.as_uuid().to_string()),
        "status": status.map(ExecutionStatus::as_str),
        "version": version,
    })
}

/// Encodes the canonical terminal-view payload JSON for one Tool result.
pub(super) fn tool_result_json(
    attempt_id: Option<AttemptId>,
    status: ExecutionStatus,
    code: Option<&str>,
    effect_state: Option<ToolEffectState>,
    output_bytes: u64,
    output_digest: Option<&str>,
    version: Option<u64>,
) -> Value {
    serde_json::json!({
        "attempt_id": attempt_id.map(|id| id.as_uuid().to_string()),
        "status": status.as_str(),
        "code": code,
        "effect_state": effect_state.map(effect_state_name),
        "output_bytes": output_bytes,
        "output_digest": output_digest,
        "version": version,
    })
}

/// Returns the durable name of one effect-state view.
fn effect_state_name(state: ToolEffectState) -> &'static str {
    match state {
        ToolEffectState::NotStarted => "not_started",
        ToolEffectState::Started => "started",
        ToolEffectState::Unknown => "unknown",
    }
}
