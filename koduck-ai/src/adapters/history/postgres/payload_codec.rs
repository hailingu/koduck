// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! `PostgreSQL` payload encoding and durable-row decoding for canonical Items.

use serde_json::{Value, json};
use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

use crate::application::HistoryError;
use crate::domain::{Item, ItemPayload, TerminalOutcome, Usage};

/// Encodes one owned payload into its `PostgreSQL` discriminator and JSON text.
pub(super) fn encode_payload(
    payload: &ItemPayload,
) -> (&'static str, String, bool, Option<&'static str>) {
    let (item_type, payload, is_terminal, terminal_status) = match payload {
        ItemPayload::UserMessage { content } => {
            ("user_message", json!({ "content": content }), false, None)
        }
        ItemPayload::AgentMessageDelta { content } => (
            "agent_message_delta",
            json!({ "content": content }),
            false,
            None,
        ),
        ItemPayload::Usage(usage) => ("usage", usage_json(*usage), false, None),
        ItemPayload::Terminal(TerminalOutcome::Completed { usage }) => {
            ("completed", usage_json(*usage), true, Some("completed"))
        }
        ItemPayload::Terminal(TerminalOutcome::Failed { code }) => {
            ("failed", json!({ "code": code }), true, Some("failed"))
        }
        ItemPayload::Terminal(TerminalOutcome::Interrupted) => {
            ("interrupted", json!({}), true, Some("interrupted"))
        }
        ItemPayload::Terminal(TerminalOutcome::Cancelled) => {
            ("cancelled", json!({}), true, Some("cancelled"))
        }
    };
    (item_type, payload.to_string(), is_terminal, terminal_status)
}

/// Decodes one `PostgreSQL` row into the owned canonical Item representation.
pub(super) fn row_to_item(row: &PgRow) -> Result<Item, HistoryError> {
    let item_id: Uuid = row.try_get("item_id").map_err(unavailable)?;
    let sequence: i64 = row.try_get("sequence").map_err(unavailable)?;
    let item_type: String = row.try_get("item_type").map_err(unavailable)?;
    let payload: String = row.try_get("payload").map_err(unavailable)?;
    let payload: Value = serde_json::from_str(&payload).map_err(|_| HistoryError::Unavailable)?;
    let payload = decode_payload(&item_type, &payload)?;
    Ok(Item {
        item_id: crate::domain::ItemId::from_uuid(item_id),
        sequence: u64::try_from(sequence).map_err(|_| HistoryError::Unavailable)?,
        payload,
    })
}

fn usage_json(usage: Usage) -> Value {
    json!({
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "total_tokens": usage.total_tokens,
    })
}

fn decode_payload(item_type: &str, payload: &Value) -> Result<ItemPayload, HistoryError> {
    let text = || {
        payload
            .get("content")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(HistoryError::Unavailable)
    };
    match item_type {
        "user_message" => Ok(ItemPayload::UserMessage { content: text()? }),
        "agent_message_delta" => Ok(ItemPayload::AgentMessageDelta { content: text()? }),
        "usage" => Ok(ItemPayload::Usage(decode_usage(payload)?)),
        "completed" => Ok(ItemPayload::Terminal(TerminalOutcome::Completed {
            usage: decode_usage(payload)?,
        })),
        "failed" => Ok(ItemPayload::Terminal(TerminalOutcome::Failed {
            code: payload
                .get("code")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or(HistoryError::Unavailable)?,
        })),
        "interrupted" => Ok(ItemPayload::Terminal(TerminalOutcome::Interrupted)),
        "cancelled" => Ok(ItemPayload::Terminal(TerminalOutcome::Cancelled)),
        _ => Err(HistoryError::Unavailable),
    }
}

fn decode_usage(payload: &Value) -> Result<Usage, HistoryError> {
    let input = payload
        .get("input_tokens")
        .and_then(Value::as_u64)
        .ok_or(HistoryError::Unavailable)?;
    let output = payload
        .get("output_tokens")
        .and_then(Value::as_u64)
        .ok_or(HistoryError::Unavailable)?;
    let usage = Usage::new(input, output).map_err(|_| HistoryError::Unavailable)?;
    (payload.get("total_tokens").and_then(Value::as_u64) == Some(usage.total_tokens))
        .then_some(usage)
        .ok_or(HistoryError::Unavailable)
}

fn unavailable(_error: sqlx::Error) -> HistoryError {
    HistoryError::Unavailable
}
