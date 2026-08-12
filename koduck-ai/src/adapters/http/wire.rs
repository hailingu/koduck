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
            ItemPayload::Terminal(outcome) => events.push(terminal_event(result, item.sequence, outcome)),
            ItemPayload::UserMessage { .. } | ItemPayload::Usage(_) => {}
        }
    }
    events.concat()
}

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
