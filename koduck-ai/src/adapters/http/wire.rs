// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: koduck-ai/docs/adr/ADR-0002-typed-http-wire-serialization.md

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::application::{TurnCommand, TurnResult, TurnStreamEvent};
use crate::domain::{
    Item, ItemPayload, TerminalOutcome, ThreadId, TrustContext, TurnId, TurnStatus, Usage,
};

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
            ItemPayload::AgentMessageDelta { content } => Some(SyncAgentMessageDelta {
                item_id: item.item_id.as_uuid().to_string(),
                sequence: item.sequence,
                kind: "agent_message_delta",
                content,
            }),
            _ => None,
        })
        .collect::<Vec<_>>();
    wire_json(&SyncTurnDocument {
        thread_id: result.thread_id.as_uuid().to_string(),
        turn_id: result.turn_id.as_uuid().to_string(),
        status: status_name(result.status),
        items,
        usage: UsageDocument::from(terminal_usage(result).unwrap_or_else(Usage::zero)),
    })
}

pub(super) fn sse_body(result: &TurnResult) -> String {
    let mut events = vec![turn_started_event(result.thread_id, result.turn_id)];
    for item in &result.published {
        if let ItemPayload::Terminal(outcome) = &item.payload {
            events.push(terminal_turn_event(
                result.thread_id,
                result.turn_id,
                item.sequence,
                status_name(result.status),
                outcome,
            ));
        } else {
            events.extend(item_created_event(result.thread_id, result.turn_id, item));
        }
    }
    events.concat()
}

pub(super) fn stream_event_body(event: TurnStreamEvent) -> String {
    match event {
        TurnStreamEvent::Started { thread_id, turn_id } => turn_started_event(thread_id, turn_id),
        TurnStreamEvent::Item {
            thread_id,
            turn_id,
            item,
        } => {
            if let ItemPayload::Terminal(outcome) = &item.payload {
                terminal_turn_event(
                    thread_id,
                    turn_id,
                    item.sequence,
                    terminal_status_name(outcome),
                    outcome,
                )
            } else {
                item_created_event(thread_id, turn_id, &item).unwrap_or_default()
            }
        }
    }
}

pub(super) fn stream_error_body(problem: &str) -> String {
    sse_event("error", problem)
}

pub(super) fn interrupt_body(turn_id: TurnId) -> String {
    wire_json(&InterruptDocument {
        turn_id: turn_id.as_uuid().to_string(),
        status: "interrupt-requested",
    })
}

pub(super) fn problem_body(status: u16, code: &str) -> String {
    let title = code.split('-').collect::<Vec<_>>().join(" ");
    let mut characters = title.chars();
    let title = characters
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
        .unwrap_or_default();
    wire_json(&ProblemDocument {
        kind: "about:blank",
        title,
        status,
        code,
        correlation_id: Uuid::new_v4().to_string(),
    })
}

/// One terminal-document serializer shared by buffered and live publication.
/// The event name follows the caller's status source — the buffered path's
/// turn status or the live outcome — and `usage` serializes only for
/// completed outcomes.
fn terminal_turn_event(
    thread_id: ThreadId,
    turn_id: TurnId,
    sequence: u64,
    status: &'static str,
    outcome: &TerminalOutcome,
) -> String {
    let usage = match outcome {
        TerminalOutcome::Completed { usage } => Some(UsageDocument::from(*usage)),
        TerminalOutcome::Failed { .. }
        | TerminalOutcome::Interrupted
        | TerminalOutcome::Cancelled => None,
    };
    sse_event(
        &format!("turn.{status}"),
        &wire_json(&TerminalDocument {
            thread_id: thread_id.as_uuid().to_string(),
            turn_id: turn_id.as_uuid().to_string(),
            sequence,
            status,
            usage,
        }),
    )
}

/// The `turn.started` document shared by buffered and live publication.
fn turn_started_event(thread_id: ThreadId, turn_id: TurnId) -> String {
    sse_event(
        "turn.started",
        &wire_json(&TurnStartedDocument {
            thread_id: thread_id.as_uuid().to_string(),
            turn_id: turn_id.as_uuid().to_string(),
            sequence: 1,
            status: "started",
        }),
    )
}

/// One exhaustive item-document serializer for every published
/// `ItemPayload`; each arm is the exact wire shape of one payload, and
/// splitting it would separate a payload from its serialized contract.
/// Returns `None` for terminal, user-message, usage, and correction payloads,
/// which are not `item.created` documents; a correction stays unpublished
/// until a later record owns its delivery (ADR-0003 CR-07).
fn item_created_event(thread_id: ThreadId, turn_id: TurnId, item: &Item) -> Option<String> {
    let data = match &item.payload {
        ItemPayload::AgentMessageDelta { content } => wire_json(&AgentMessageDeltaDocument {
            thread_id: thread_id.as_uuid().to_string(),
            turn_id: turn_id.as_uuid().to_string(),
            sequence: item.sequence,
            item_id: item.item_id.as_uuid().to_string(),
            kind: "agent_message_delta",
            content,
        }),
        ItemPayload::ApprovalStatus {
            approval_id,
            attempt_id,
            status,
            decision,
            version,
        } => wire_json(&ApprovalStatusDocument {
            thread_id: thread_id.as_uuid().to_string(),
            turn_id: turn_id.as_uuid().to_string(),
            sequence: item.sequence,
            item_id: item.item_id.as_uuid().to_string(),
            kind: "approval_status",
            approval_id: approval_id.as_uuid().to_string(),
            attempt_id: attempt_id.as_uuid().to_string(),
            status: approval_status_wire_name(*status),
            decision: decision.map(approval_decision_wire_name),
            version: *version,
        }),
        ItemPayload::ToolCall {
            descriptor_id,
            descriptor_version,
            target,
            attempt_id,
            status,
            version,
        } => wire_json(&ToolCallDocument {
            thread_id: thread_id.as_uuid().to_string(),
            turn_id: turn_id.as_uuid().to_string(),
            sequence: item.sequence,
            item_id: item.item_id.as_uuid().to_string(),
            kind: "tool_call",
            descriptor_id,
            descriptor_version,
            target,
            attempt_id: attempt_id.map(|id| id.as_uuid().to_string()),
            status: status.map(tool_status_name),
            version: *version,
        }),
        ItemPayload::ToolResult {
            attempt_id,
            status,
            code,
            effect_state,
            output_bytes,
            output_digest,
            version,
        } => wire_json(&ToolResultDocument {
            thread_id: thread_id.as_uuid().to_string(),
            turn_id: turn_id.as_uuid().to_string(),
            sequence: item.sequence,
            item_id: item.item_id.as_uuid().to_string(),
            kind: "tool_result",
            attempt_id: attempt_id.map(|id| id.as_uuid().to_string()),
            status: tool_status_name(*status),
            code: code.as_deref(),
            effect_state: effect_state.map(tool_effect_state_name),
            output_bytes: *output_bytes,
            output_digest: output_digest.as_deref(),
            version: *version,
        }),
        ItemPayload::Terminal(_)
        | ItemPayload::UserMessage { .. }
        | ItemPayload::Usage(_)
        | ItemPayload::Correction(_) => {
            return None;
        }
    };
    Some(sse_event("item.created", &data))
}

/// Serializes one private wire document. Every document field is a
/// primitive, string, sequence, or option of those, so serialization cannot
/// fail; introducing a fallible field requires explicit error propagation
/// and record reclassification instead of this expect.
fn wire_json<T: Serialize>(document: &T) -> String {
    serde_json::to_string(document).expect("wire document serialization is infallible")
}

// Serializable outbound wire shapes. Field order is declaration order and is
// the exact emitted byte order; UUID fields hold their textual form because
// the `uuid` serde feature is deliberately not enabled.

/// Successful synchronous chat response body (TW-01).
#[derive(Serialize)]
struct SyncTurnDocument<'a> {
    thread_id: String,
    turn_id: String,
    status: &'static str,
    items: Vec<SyncAgentMessageDelta<'a>>,
    usage: UsageDocument,
}

/// One synchronous `items` array member.
#[derive(Serialize)]
struct SyncAgentMessageDelta<'a> {
    item_id: String,
    sequence: u64,
    #[serde(rename = "type")]
    kind: &'static str,
    content: &'a str,
}

/// Token accounting embedded in synchronous bodies and completed terminals.
/// The `_tokens` field names are the CAND-1 wire contract and cannot be
/// renamed.
#[allow(clippy::struct_field_names)]
#[derive(Serialize)]
struct UsageDocument {
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
}

impl From<Usage> for UsageDocument {
    fn from(usage: Usage) -> Self {
        Self {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
        }
    }
}

/// `turn.started` SSE data document.
#[derive(Serialize)]
struct TurnStartedDocument {
    thread_id: String,
    turn_id: String,
    sequence: u64,
    status: &'static str,
}

/// `turn.{status}` SSE data document; `usage` is omitted unless completed.
#[derive(Serialize)]
struct TerminalDocument {
    thread_id: String,
    turn_id: String,
    sequence: u64,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<UsageDocument>,
}

/// `item.created` data document for one agent message delta.
#[derive(Serialize)]
struct AgentMessageDeltaDocument<'a> {
    thread_id: String,
    turn_id: String,
    sequence: u64,
    item_id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    content: &'a str,
}

/// `item.created` data document for one approval-status projection.
#[derive(Serialize)]
struct ApprovalStatusDocument {
    thread_id: String,
    turn_id: String,
    sequence: u64,
    item_id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    approval_id: String,
    attempt_id: String,
    status: &'static str,
    decision: Option<&'static str>,
    version: u64,
}

/// `item.created` data document for one tool-call projection.
#[derive(Serialize)]
struct ToolCallDocument<'a> {
    thread_id: String,
    turn_id: String,
    sequence: u64,
    item_id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    descriptor_id: &'a str,
    descriptor_version: &'a str,
    target: &'a str,
    attempt_id: Option<String>,
    status: Option<&'static str>,
    version: Option<u64>,
}

/// `item.created` data document for one tool-result projection.
#[derive(Serialize)]
struct ToolResultDocument<'a> {
    thread_id: String,
    turn_id: String,
    sequence: u64,
    item_id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    attempt_id: Option<String>,
    status: &'static str,
    code: Option<&'a str>,
    effect_state: Option<&'static str>,
    output_bytes: u64,
    output_digest: Option<&'a str>,
    version: Option<u64>,
}

/// 202 interrupt-accepted response body.
#[derive(Serialize)]
struct InterruptDocument {
    turn_id: String,
    status: &'static str,
}

/// Problem-detail body with a fresh correlation UUID.
#[derive(Serialize)]
struct ProblemDocument<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    title: String,
    status: u16,
    code: &'a str,
    correlation_id: String,
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

fn terminal_status_name(outcome: &TerminalOutcome) -> &'static str {
    match outcome {
        TerminalOutcome::Completed { .. } => "completed",
        TerminalOutcome::Failed { .. } => "failed",
        TerminalOutcome::Interrupted => "interrupted",
        TerminalOutcome::Cancelled => "cancelled",
    }
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

fn tool_effect_state_name(state: crate::domain::ToolEffectState) -> &'static str {
    match state {
        crate::domain::ToolEffectState::NotStarted => "not_started",
        crate::domain::ToolEffectState::Started => "started",
        crate::domain::ToolEffectState::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests;
