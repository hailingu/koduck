// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md
// ADR: koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md
// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md

//! OpenAI-compatible message serialization for history and Tool continuations.

use crate::application::{ModelInput, ProviderHistoryKind};

/// Serializes one provider input without reordering causal Tool rounds.
///
/// Prior history arrives as the prepared effective view (ADR-0006 PC-05):
/// each view's original kind selects its translation while its effective
/// content supplies the exact selected bytes, so a correction after a
/// terminal changes its earlier root before that terminal's flush and no
/// correction is ever serialized as its own message.
pub(super) fn provider_messages(input: &ModelInput) -> Vec<serde_json::Value> {
    let mut messages = Vec::new();
    let mut assistant = String::new();
    for item in &input.history {
        match item.kind {
            ProviderHistoryKind::UserMessage => {
                flush_assistant(&mut messages, &mut assistant);
                messages.push(serde_json::json!({
                    "role": "user",
                    "content": item.effective_text().unwrap_or_default(),
                }));
            }
            ProviderHistoryKind::AgentMessageDelta => {
                assistant.push_str(item.effective_text().unwrap_or_default());
            }
            ProviderHistoryKind::Terminal => flush_assistant(&mut messages, &mut assistant),
            ProviderHistoryKind::Usage
            | ProviderHistoryKind::ApprovalStatus
            | ProviderHistoryKind::ToolCall
            | ProviderHistoryKind::ToolResult
            // Preparation absorbs corrections into their roots; a view of
            // this kind never carries a separate message (ADR-0006 PC-04).
            | ProviderHistoryKind::Correction => {}
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
