// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md
// ADR: koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md

//! OpenAI-compatible message serialization for history and Tool continuations.

use crate::application::ModelInput;
use crate::domain::ItemPayload;

/// Serializes one provider input without reordering causal Tool rounds.
pub(super) fn provider_messages(input: &ModelInput) -> Vec<serde_json::Value> {
    let mut messages = Vec::new();
    let mut assistant = String::new();
    for item in &input.history {
        match &item.payload {
            ItemPayload::UserMessage { content } => {
                flush_assistant(&mut messages, &mut assistant);
                messages.push(serde_json::json!({ "role": "user", "content": content }));
            }
            ItemPayload::AgentMessageDelta { content } => assistant.push_str(content),
            ItemPayload::Terminal(_) => flush_assistant(&mut messages, &mut assistant),
            ItemPayload::Usage(_)
            | ItemPayload::ApprovalStatus { .. }
            | ItemPayload::ToolCall { .. }
            | ItemPayload::ToolResult { .. }
            // A correction is not provider input in this slice; effective
            // corrected meaning is owned by CAND-12/CAND-13 (ADR-0003 CR-07).
            | ItemPayload::Correction(_) => {}
        }
    }
    flush_assistant(&mut messages, &mut assistant);
    messages.push(serde_json::json!({
        "role": "user",
        "content": input.input,
    }));
    let mut position = 0;
    for round in &input.tool_rounds {
        let ids = round
            .calls
            .iter()
            .map(|_| {
                let id = format!("call_{position}");
                position += 1;
                id
            })
            .collect::<Vec<_>>();
        let calls = round
            .calls
            .iter()
            .zip(&ids)
            .map(|(committed, id)| {
                serde_json::json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": committed.call.name,
                        "arguments": committed.call.arguments,
                    },
                })
            })
            .collect::<Vec<_>>();
        messages.push(serde_json::json!({
            "role": "assistant",
            "content": round.assistant_content,
            "tool_calls": calls,
        }));
        for (committed, id) in round.calls.iter().zip(&ids) {
            messages.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": id,
                "content": committed.result.content,
            }));
        }
    }
    messages
}

/// Flushes accumulated assistant deltas before a new role boundary.
fn flush_assistant(messages: &mut Vec<serde_json::Value>, assistant: &mut String) {
    if !assistant.is_empty() {
        messages.push(serde_json::json!({
            "role": "assistant",
            "content": std::mem::take(assistant),
        }));
    }
}
