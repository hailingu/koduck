// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::application::{TurnCommand, TurnResult, TurnStreamEvent};
use crate::domain::{ItemPayload, TerminalOutcome, TrustContext, TurnId, TurnStatus, Usage};

pub(super) fn parse_turn_request(body: &str, trust: TrustContext) -> Result<TurnCommand, ()> {
    let fields = parse_string_object(body)?;
    if fields
        .keys()
        .any(|field| field != "input" && field != "thread_id")
    {
        return Err(());
    }
    let input = fields.get("input").ok_or(())?.clone();
    let thread_id = fields
        .get("thread_id")
        .map(|value| Uuid::parse_str(value).map(crate::domain::ThreadId::from_uuid))
        .transpose()
        .map_err(|_| ())?;
    TurnCommand::new(trust, thread_id, input).map_err(|_| ())
}

pub(super) fn sync_body(result: &TurnResult) -> String {
    let items = result
        .replay
        .iter()
        .filter_map(|item| match &item.payload {
            ItemPayload::AgentMessageDelta { content } => Some(format!(
                "{{\"item_id\":\"{}\",\"sequence\":{},\"type\":\"agent_message_delta\",\"content\":\"{}\"}}",
                item.item_id.as_uuid(),
                item.sequence,
                escape_json(content)
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
                    "{{\"thread_id\":\"{}\",\"turn_id\":\"{}\",\"sequence\":{},\"item_id\":\"{}\",\"type\":\"agent_message_delta\",\"content\":\"{}\"}}",
                    result.thread_id.as_uuid(),
                    result.turn_id.as_uuid(),
                    item.sequence,
                    item.item_id.as_uuid(),
                    escape_json(content)
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
                    "{{\"thread_id\":\"{}\",\"turn_id\":\"{}\",\"sequence\":{},\"item_id\":\"{}\",\"type\":\"agent_message_delta\",\"content\":\"{}\"}}",
                    thread_id.as_uuid(),
                    turn_id.as_uuid(),
                    item.sequence,
                    item.item_id.as_uuid(),
                    escape_json(&content)
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

fn escape_json(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| match character {
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\\' => "\\\\".chars().collect(),
            '\n' => "\\n".chars().collect(),
            '\r' => "\\r".chars().collect(),
            '\t' => "\\t".chars().collect(),
            other => vec![other],
        })
        .collect()
}

fn parse_string_object(document: &str) -> Result<BTreeMap<String, String>, ()> {
    let bytes = document.as_bytes();
    let mut index = 0;
    skip_whitespace(bytes, &mut index);
    expect_byte(bytes, &mut index, b'{')?;
    let mut fields = BTreeMap::new();
    loop {
        skip_whitespace(bytes, &mut index);
        if take_byte(bytes, &mut index, b'}') {
            break;
        }
        let key = parse_json_string(bytes, &mut index)?;
        skip_whitespace(bytes, &mut index);
        expect_byte(bytes, &mut index, b':')?;
        skip_whitespace(bytes, &mut index);
        let value = parse_json_string(bytes, &mut index)?;
        if fields.insert(key, value).is_some() {
            return Err(());
        }
        skip_whitespace(bytes, &mut index);
        if take_byte(bytes, &mut index, b'}') {
            break;
        }
        expect_byte(bytes, &mut index, b',')?;
    }
    skip_whitespace(bytes, &mut index);
    (index == bytes.len()).then_some(fields).ok_or(())
}

fn parse_json_string(bytes: &[u8], index: &mut usize) -> Result<String, ()> {
    expect_byte(bytes, index, b'"')?;
    let mut value = String::new();
    while let Some(&byte) = bytes.get(*index) {
        *index += 1;
        match byte {
            b'"' => return Ok(value),
            b'\\' => {
                let escaped = *bytes.get(*index).ok_or(())?;
                *index += 1;
                value.push(match escaped {
                    b'"' => '"',
                    b'\\' => '\\',
                    b'n' => '\n',
                    b'r' => '\r',
                    b't' => '\t',
                    _ => return Err(()),
                });
            }
            0..=31 => return Err(()),
            _ => {
                let suffix = std::str::from_utf8(&bytes[*index - 1..]).map_err(|_| ())?;
                let character = suffix.chars().next().ok_or(())?;
                value.push(character);
                *index += character.len_utf8() - 1;
            }
        }
    }
    Err(())
}

fn skip_whitespace(bytes: &[u8], index: &mut usize) {
    while bytes.get(*index).is_some_and(u8::is_ascii_whitespace) {
        *index += 1;
    }
}

fn expect_byte(bytes: &[u8], index: &mut usize, expected: u8) -> Result<(), ()> {
    take_byte(bytes, index, expected).then_some(()).ok_or(())
}

fn take_byte(bytes: &[u8], index: &mut usize, expected: u8) -> bool {
    if bytes.get(*index) == Some(&expected) {
        *index += 1;
        true
    } else {
        false
    }
}
