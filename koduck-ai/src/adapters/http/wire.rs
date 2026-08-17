// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use serde::Deserialize;
use uuid::Uuid;

use crate::application::{TurnCommand, TurnResult, TurnStreamEvent};
use crate::domain::{ItemPayload, TerminalOutcome, TrustContext, TurnId, TurnStatus, Usage};

pub(super) fn parse_turn_request(body: &str, trust: TrustContext) -> Result<TurnCommand, ()> {
    let document: TurnRequestDocument = serde_json::from_str(body).map_err(|_| ())?;
    let thread_id = document
        .thread_id
        .map(|value| Uuid::parse_str(&value).map(crate::domain::ThreadId::from_uuid))
        .transpose()
        .map_err(|_| ())?;
    TurnCommand::new(trust, thread_id, document.input).map_err(|_| ())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TurnRequestDocument {
    input: String,
    thread_id: Option<String>,
}

pub(super) fn sync_body(result: &TurnResult) -> String {
    let items = result
        .replay
        .iter()
        .filter_map(|item| match &item.payload {
            ItemPayload::AgentMessageDelta { content } => Some(format!(
                "{{\"item_id\":\"{}\",\"sequence\":{},\"type\":\"agent_message_delta\",\"content\":{}}}",
                item.item_id.as_uuid(),
                item.sequence,
                json_string(content)
            )),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(",");
    let usage = terminal_usage(result).unwrap_or_else(Usage::zero);
    format!(
        "{{\"thread_id\":\"{}\",\"turn_id\":\"{}\",\"status\":\"{}\",\"items\":[{}],\"usage\":{}}}",
        result.thread_id.as_uuid(),
        result.turn_id.as_uuid(),
        status_name(result.status),
        items,
        usage_json(usage)
    )
}

pub(super) fn sse_body(result: &TurnResult) -> String {
    let mut events = vec![sse_event(
        "turn.started",
        &format!(
            "{{\"thread_id\":\"{}\",\"turn_id\":\"{}\",\"sequence\":1,\"status\":\"started\"}}",
            result.thread_id.as_uuid(),
            result.turn_id.as_uuid()
        ),
    )];
    for item in &result.published {
        match &item.payload {
            ItemPayload::AgentMessageDelta { content } => events.push(sse_event(
                "item.created",
                &format!(
                    "{{\"thread_id\":\"{}\",\"turn_id\":\"{}\",\"sequence\":{},\"item_id\":\"{}\",\"type\":\"agent_message_delta\",\"content\":{}}}",
                    result.thread_id.as_uuid(),
                    result.turn_id.as_uuid(),
                    item.sequence,
                    item.item_id.as_uuid(),
                    json_string(content)
                ),
            )),
            ItemPayload::ApprovalStatus {
                approval_id,
                attempt_id,
                status,
                decision,
                version,
            } => events.push(sse_event(
                "item.created",
                &format!(
                    "{{\"thread_id\":\"{}\",\"turn_id\":\"{}\",\"sequence\":{},\"item_id\":\"{}\",\"type\":\"approval_status\",\"approval_id\":\"{}\",\"attempt_id\":\"{}\",\"status\":\"{}\",\"decision\":{},\"version\":{}}}",
                    result.thread_id.as_uuid(),
                    result.turn_id.as_uuid(),
                    item.sequence,
                    item.item_id.as_uuid(),
                    approval_id.as_uuid(),
                    attempt_id.as_uuid(),
                    approval_status_wire_name(*status),
                    approval_decision_wire(*decision),
                    version,
                ),
            )),
            ItemPayload::ToolCall {
                descriptor_id,
                descriptor_version,
                target,
                attempt_id,
                status,
                version,
            } => events.push(sse_event(
                "item.created",
                &format!(
                    "{{\"thread_id\":\"{}\",\"turn_id\":\"{}\",\"sequence\":{},\"item_id\":\"{}\",\"type\":\"tool_call\",\"descriptor_id\":{},\"descriptor_version\":{},\"target\":{},\"attempt_id\":{},\"status\":{},\"version\":{}}}",
                    result.thread_id.as_uuid(),
                    result.turn_id.as_uuid(),
                    item.sequence,
                    item.item_id.as_uuid(),
                    json_string(descriptor_id),
                    json_string(descriptor_version),
                    json_string(target),
                    optional_uuid(attempt_id.as_ref()),
                    status.map_or_else(|| "null".to_owned(), |status| format!("\"{}\"", tool_status_name(status))),
                    optional_version(*version),
                ),
            )),
            ItemPayload::ToolResult {
                attempt_id,
                status,
                code,
                effect_state,
                output_bytes,
                output_digest,
                version,
            } => events.push(sse_event(
                "item.created",
                &format!(
                    "{{\"thread_id\":\"{}\",\"turn_id\":\"{}\",\"sequence\":{},\"item_id\":\"{}\",\"type\":\"tool_result\",\"attempt_id\":{},\"status\":\"{}\",\"code\":{},\"effect_state\":{},\"output_bytes\":{},\"output_digest\":{},\"version\":{}}}",
                    result.thread_id.as_uuid(),
                    result.turn_id.as_uuid(),
                    item.sequence,
                    item.item_id.as_uuid(),
                    optional_uuid(attempt_id.as_ref()),
                    tool_status_name(*status),
                    code.as_deref().map_or("null".to_owned(), json_string),
                    effect_state.map_or_else(|| "null".to_owned(), |state| format!("\"{}\"", tool_effect_state_name(state))),
                    output_bytes,
                    output_digest.as_deref().map_or("null".to_owned(), json_string),
                    optional_version(*version),
                ),
            )),
            ItemPayload::Terminal(outcome) => events.push(terminal_event(result, item.sequence, outcome)),
            ItemPayload::UserMessage { .. } | ItemPayload::Usage(_) => {}
        }
    }
    events.concat()
}

// One exhaustive wire serializer for every published item payload; each arm
// is the exact wire shape of one payload and splitting it would separate a
// payload from its serialized contract.
#[allow(clippy::too_many_lines)]
pub(super) fn stream_event_body(event: TurnStreamEvent) -> String {
    match event {
        TurnStreamEvent::Started { thread_id, turn_id } => sse_event(
            "turn.started",
            &format!(
                "{{\"thread_id\":\"{}\",\"turn_id\":\"{}\",\"sequence\":1,\"status\":\"started\"}}",
                thread_id.as_uuid(),
                turn_id.as_uuid()
            ),
        ),
        TurnStreamEvent::Item {
            thread_id,
            turn_id,
            item,
        } => match item.payload {
            ItemPayload::AgentMessageDelta { content } => sse_event(
                "item.created",
                &format!(
                    "{{\"thread_id\":\"{}\",\"turn_id\":\"{}\",\"sequence\":{},\"item_id\":\"{}\",\"type\":\"agent_message_delta\",\"content\":{}}}",
                    thread_id.as_uuid(),
                    turn_id.as_uuid(),
                    item.sequence,
                    item.item_id.as_uuid(),
                    json_string(&content)
                ),
            ),
            ItemPayload::ApprovalStatus {
                approval_id,
                attempt_id,
                status,
                decision,
                version,
            } => sse_event(
                "item.created",
                &format!(
                    "{{\"thread_id\":\"{}\",\"turn_id\":\"{}\",\"sequence\":{},\"item_id\":\"{}\",\"type\":\"approval_status\",\"approval_id\":\"{}\",\"attempt_id\":\"{}\",\"status\":\"{}\",\"decision\":{},\"version\":{}}}",
                    thread_id.as_uuid(),
                    turn_id.as_uuid(),
                    item.sequence,
                    item.item_id.as_uuid(),
                    approval_id.as_uuid(),
                    attempt_id.as_uuid(),
                    approval_status_wire_name(status),
                    approval_decision_wire(decision),
                    version,
                ),
            ),
            ItemPayload::ToolCall {
                descriptor_id,
                descriptor_version,
                target,
                attempt_id,
                status,
                version,
            } => sse_event(
                "item.created",
                &format!(
                    "{{\"thread_id\":\"{}\",\"turn_id\":\"{}\",\"sequence\":{},\"item_id\":\"{}\",\"type\":\"tool_call\",\"descriptor_id\":{},\"descriptor_version\":{},\"target\":{},\"attempt_id\":{},\"status\":{},\"version\":{}}}",
                    thread_id.as_uuid(),
                    turn_id.as_uuid(),
                    item.sequence,
                    item.item_id.as_uuid(),
                    json_string(&descriptor_id),
                    json_string(&descriptor_version),
                    json_string(&target),
                    optional_uuid(attempt_id.as_ref()),
                    status.map_or_else(
                        || "null".to_owned(),
                        |status| format!("\"{}\"", tool_status_name(status))
                    ),
                    optional_version(version),
                ),
            ),
            ItemPayload::ToolResult {
                attempt_id,
                status,
                code,
                effect_state,
                output_bytes,
                output_digest,
                version,
            } => sse_event(
                "item.created",
                &format!(
                    "{{\"thread_id\":\"{}\",\"turn_id\":\"{}\",\"sequence\":{},\"item_id\":\"{}\",\"type\":\"tool_result\",\"attempt_id\":{},\"status\":\"{}\",\"code\":{},\"effect_state\":{},\"output_bytes\":{},\"output_digest\":{},\"version\":{}}}",
                    thread_id.as_uuid(),
                    turn_id.as_uuid(),
                    item.sequence,
                    item.item_id.as_uuid(),
                    optional_uuid(attempt_id.as_ref()),
                    tool_status_name(status),
                    code.as_deref().map_or("null".to_owned(), json_string),
                    effect_state.map_or_else(
                        || "null".to_owned(),
                        |state| format!("\"{}\"", tool_effect_state_name(state))
                    ),
                    output_bytes,
                    output_digest
                        .as_deref()
                        .map_or("null".to_owned(), json_string),
                    optional_version(version),
                ),
            ),
            ItemPayload::Terminal(outcome) => {
                stream_terminal_event(thread_id, turn_id, item.sequence, &outcome)
            }
            ItemPayload::UserMessage { .. } | ItemPayload::Usage(_) => String::new(),
        },
    }
}

pub(super) fn stream_error_body(problem: &str) -> String {
    sse_event("error", problem)
}

pub(super) fn interrupt_body(turn_id: TurnId) -> String {
    format!(
        "{{\"turn_id\":\"{}\",\"status\":\"interrupt-requested\"}}",
        turn_id.as_uuid()
    )
}

pub(super) fn problem_body(status: u16, code: &str) -> String {
    let title = code.split('-').collect::<Vec<_>>().join(" ");
    let mut characters = title.chars();
    let title = characters
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
        .unwrap_or_default();
    format!(
        "{{\"type\":\"about:blank\",\"title\":\"{}\",\"status\":{},\"code\":\"{}\",\"correlation_id\":\"{}\"}}",
        title,
        status,
        code,
        Uuid::new_v4()
    )
}

fn terminal_event(result: &TurnResult, sequence: u64, outcome: &TerminalOutcome) -> String {
    let status = status_name(result.status);
    let usage = match outcome {
        TerminalOutcome::Completed { usage } => format!(",\"usage\":{}", usage_json(*usage)),
        TerminalOutcome::Failed { .. }
        | TerminalOutcome::Interrupted
        | TerminalOutcome::Cancelled => String::new(),
    };
    sse_event(
        &format!("turn.{status}"),
        &format!(
            "{{\"thread_id\":\"{}\",\"turn_id\":\"{}\",\"sequence\":{},\"status\":\"{}\"{}}}",
            result.thread_id.as_uuid(),
            result.turn_id.as_uuid(),
            sequence,
            status,
            usage
        ),
    )
}

fn stream_terminal_event(
    thread_id: crate::domain::ThreadId,
    turn_id: TurnId,
    sequence: u64,
    outcome: &TerminalOutcome,
) -> String {
    let (status, usage) = match outcome {
        TerminalOutcome::Completed { usage } => {
            ("completed", format!(",\"usage\":{}", usage_json(*usage)))
        }
        TerminalOutcome::Failed { .. } => ("failed", String::new()),
        TerminalOutcome::Interrupted => ("interrupted", String::new()),
        TerminalOutcome::Cancelled => ("cancelled", String::new()),
    };
    sse_event(
        &format!("turn.{status}"),
        &format!(
            "{{\"thread_id\":\"{}\",\"turn_id\":\"{}\",\"sequence\":{},\"status\":\"{}\"{}}}",
            thread_id.as_uuid(),
            turn_id.as_uuid(),
            sequence,
            status,
            usage
        ),
    )
}

fn sse_event(name: &str, data: &str) -> String {
    format!("event: {name}\ndata: {data}\n\n")
}

fn terminal_usage(result: &TurnResult) -> Option<Usage> {
    result.replay.iter().find_map(|item| match item.payload {
        ItemPayload::Terminal(TerminalOutcome::Completed { usage }) => Some(usage),
        _ => None,
    })
}

fn usage_json(usage: Usage) -> String {
    format!(
        "{{\"input_tokens\":{},\"output_tokens\":{},\"total_tokens\":{}}}",
        usage.input_tokens, usage.output_tokens, usage.total_tokens
    )
}

fn status_name(status: TurnStatus) -> &'static str {
    match status {
        TurnStatus::Started => "started",
        TurnStatus::RecoveryPending => "recovery-pending",
        TurnStatus::Completed => "completed",
        TurnStatus::Failed => "failed",
        TurnStatus::Interrupted => "interrupted",
        TurnStatus::Cancelled => "cancelled",
    }
}

fn json_string(value: &str) -> String {
    serde_json::Value::String(value.to_owned()).to_string()
}

fn tool_status_name(status: crate::domain::execution::ExecutionStatus) -> &'static str {
    use crate::domain::execution::ExecutionStatus;
    match status {
        ExecutionStatus::Prepared => "prepared",
        ExecutionStatus::Running => "running",
        ExecutionStatus::Succeeded => "succeeded",
        ExecutionStatus::Failed => "failed",
        ExecutionStatus::TimedOut => "timed_out",
        ExecutionStatus::Cancelled => "cancelled",
    }
}

fn approval_status_wire_name(status: crate::domain::execution::ApprovalStatus) -> &'static str {
    use crate::domain::execution::ApprovalStatus;
    match status {
        ApprovalStatus::Requested => "requested",
        ApprovalStatus::Accepted => "accepted",
        ApprovalStatus::Declined => "declined",
        ApprovalStatus::Cancelled => "cancelled",
        ApprovalStatus::Expired => "expired",
    }
}

fn approval_decision_wire_name(
    decision: crate::domain::execution::ApprovalDecision,
) -> &'static str {
    use crate::domain::execution::ApprovalDecision;
    match decision {
        ApprovalDecision::Accepted => "accepted",
        ApprovalDecision::Declined => "declined",
        ApprovalDecision::Cancelled => "cancelled",
    }
}

/// Serializes the canonical D-6 decision as its exact wire value or `null`.
///
/// Both the buffered and the streamed approval-projection serializers use
/// this one mapping, so every client observes the same payload shape
/// (ADR-0003 TC-06).
fn approval_decision_wire(decision: Option<crate::domain::execution::ApprovalDecision>) -> String {
    decision.map_or_else(
        || "null".to_owned(),
        |decision| format!("\"{}\"", approval_decision_wire_name(decision)),
    )
}

fn optional_uuid(id: Option<&crate::domain::execution::AttemptId>) -> String {
    id.map_or_else(|| "null".to_owned(), |id| format!("\"{}\"", id.as_uuid()))
}

fn optional_version(version: Option<u64>) -> String {
    version.map_or_else(|| "null".to_owned(), |version| version.to_string())
}

fn tool_effect_state_name(state: crate::domain::ToolEffectState) -> &'static str {
    match state {
        crate::domain::ToolEffectState::NotStarted => "not_started",
        crate::domain::ToolEffectState::Started => "started",
        crate::domain::ToolEffectState::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use crate::application::{TurnResult, TurnStreamEvent};
    use crate::domain::execution::{ApprovalDecision, ApprovalId, ApprovalStatus, AttemptId};
    use crate::domain::{Item, ItemPayload, ThreadId, TurnId, TurnStatus};

    use super::{sse_body, stream_event_body};

    fn approval_item(status: ApprovalStatus, decision: Option<ApprovalDecision>) -> Item {
        Item::new(
            2,
            ItemPayload::ApprovalStatus {
                approval_id: ApprovalId::new(),
                attempt_id: AttemptId::new(),
                status,
                decision,
                version: 2,
            },
        )
    }

    #[test]
    fn streamed_and_buffered_approval_projections_carry_the_same_decision_field() {
        for (status, decision, expected) in [
            (
                ApprovalStatus::Accepted,
                Some(ApprovalDecision::Accepted),
                "\"decision\":\"accepted\"",
            ),
            (
                ApprovalStatus::Declined,
                Some(ApprovalDecision::Declined),
                "\"decision\":\"declined\"",
            ),
            (
                ApprovalStatus::Cancelled,
                Some(ApprovalDecision::Cancelled),
                "\"decision\":\"cancelled\"",
            ),
            (ApprovalStatus::Requested, None, "\"decision\":null"),
            (ApprovalStatus::Expired, None, "\"decision\":null"),
        ] {
            let item = approval_item(status, decision);
            let streamed = stream_event_body(TurnStreamEvent::Item {
                thread_id: ThreadId::new(),
                turn_id: TurnId::new(),
                item: item.clone(),
            });
            assert!(
                streamed.contains(expected),
                "the streamed projection carries {expected}: {streamed}"
            );
            let buffered = sse_body(&TurnResult {
                thread_id: ThreadId::new(),
                turn_id: TurnId::new(),
                status: TurnStatus::Completed,
                published: vec![item],
                replay: Vec::new(),
            });
            assert!(
                buffered.contains(expected),
                "the buffered projection carries {expected}: {buffered}"
            );
        }
    }
}
