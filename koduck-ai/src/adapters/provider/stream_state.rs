// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md
// ADR: docs/adr/ADR-0004-provider-stream-completion-normalization.md
// ADR: koduck-ai/docs/adr/ADR-0001-strict-json-duplicate-member-validation.md

//! Stateful parsing and bounded assembly for one provider stream.

use std::collections::VecDeque;

use serde_json::Value;

use crate::adapters::strict_json;
use crate::application::{ProviderError, ProviderEvent};
use crate::domain::Usage;

use super::{OpenAiFrame, OpenAiTransportError};

/// Canonical per-action serialized input bound (ADR-0003): one assembled
/// Tool call's cumulative streamed arguments never grow beyond it.
const MAX_TOOL_CALL_ARGUMENTS_BYTES: usize = 65_536;

/// Every serviced call records at least a `ToolCall` and a `ToolResult` D-3
/// item, so the 64-item per-Turn provider buffer (ADR-0001) can never record a
/// 33rd call; the assembly fails closed instead of allocating past that bound.
const MAX_ASSEMBLED_TOOL_CALLS: usize = 32;

/// Per-stream assembly state for provider Tool-call fragments (ADR-0003).
#[derive(Default)]
pub(super) struct StreamState {
    terminated: bool,
    usage_seen: bool,
    assembled: Vec<AssembledToolCall>,
    ready: VecDeque<ProviderEvent>,
    /// The stream finished a Tool-call round, so its end is not a Turn
    /// completion: the runner continues the model with the committed results.
    served_tool_round: bool,
    /// The one finish reason validated so far, when any. `stop` and
    /// `tool_calls` carry terminal semantics; every other value fails closed
    /// at explicit clean end (ADR-0004 PSC-3/PSC-4/PSC-5).
    finish: Option<String>,
}

/// One Tool call whose streamed fragments are still being assembled.
#[derive(Default)]
struct AssembledToolCall {
    index: Option<u64>,
    name: Option<String>,
    arguments: String,
}

impl StreamState {
    pub(super) fn next_event<F>(&mut self, frames: &mut F) -> Option<ProviderEvent>
    where
        F: Iterator<Item = Result<OpenAiFrame, OpenAiTransportError>>,
    {
        loop {
            if let Some(event) = self.ready.pop_front() {
                return Some(event);
            }
            if self.terminated {
                return None;
            }
            let frame = frames.next()?;
            match frame {
                Ok(OpenAiFrame::Pending) => return Some(ProviderEvent::Pending),
                Ok(OpenAiFrame::CleanEnd) => match self.clean_end() {
                    Ok(Some(event)) => {
                        self.ready.push_back(event);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        self.terminated = true;
                        self.ready.clear();
                        return Some(ProviderEvent::Error { code: error.code });
                    }
                },
                Ok(OpenAiFrame::Data(frame)) => match self.parse_frame(&frame) {
                    Ok(Some(event)) => {
                        self.ready.push_back(event);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        self.terminated = true;
                        self.ready.clear();
                        return Some(ProviderEvent::Error { code: error.code });
                    }
                },
                Err(error) => {
                    self.terminated = true;
                    self.ready.clear();
                    return Some(ProviderEvent::Error { code: error.code });
                }
            }
        }
    }

    fn parse_frame(&mut self, frame: &str) -> Result<Option<ProviderEvent>, ProviderError> {
        let data = frame
            .strip_prefix("data: ")
            .ok_or_else(|| protocol_error("INVALID_FRAME"))?;
        if data == "[DONE]" {
            return self.done_sentinel();
        }
        // `serde_json::Value` keeps only the last duplicate member, so the
        // frame is first rejected when any object member is duplicated — two
        // `finish_reason` values must never collapse into one validated
        // finish (ADR-0004 PSC-3/PSC-5).
        strict_json::ensure_unique_members(data).map_err(|_| protocol_error("INVALID_FRAME"))?;
        let document: Value =
            serde_json::from_str(data).map_err(|_| protocol_error("INVALID_FRAME"))?;
        if self.usage_seen {
            let code = if document.get("usage").is_some_and(|usage| !usage.is_null()) {
                "DUPLICATE_USAGE_FRAME"
            } else if self.finish.is_some() {
                // Non-usage output after a finish frame is late output even
                // when a valid usage frame already intervened;
                // `INVALID_USAGE_FRAME` stays reserved for invalid
                // post-finish usage (ADR-0004 PSC-5).
                "INVALID_FINISH_FRAME"
            } else {
                "INVALID_USAGE_FRAME"
            };
            return Err(protocol_error(code));
        }
        if let Some(error) = document.get("error").filter(|error| !error.is_null()) {
            if self.finish.is_some() {
                // Error output after a finish frame is late output
                // (ADR-0004 PSC-5).
                return Err(protocol_error("INVALID_FINISH_FRAME"));
            }
            let code = error
                .get("code")
                .and_then(Value::as_str)
                .ok_or_else(|| protocol_error("INVALID_ERROR_FRAME"))?;
            // The provider error frame is terminal evidence: terminating here
            // keeps the transport's trailing clean end from synthesizing a
            // second failure and a late stop finish from completing the
            // already-failed stream (ADR-0004 PSC-5).
            self.terminated = true;
            return Ok(Some(ProviderEvent::Error {
                code: code.to_owned(),
            }));
        }
        if let Some(usage_value) = document.get("usage").filter(|usage| !usage.is_null()) {
            let usage = Self::parse_usage_frame(&document, usage_value)?;
            self.usage_seen = true;
            return Ok(Some(ProviderEvent::Usage(usage)));
        }
        let choices = document
            .get("choices")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol_error("INVALID_FRAME"))?;
        if choices.len() != 1 {
            // One request selects exactly one choice; additional choices carry
            // unvalidated, potentially conflicting terminal evidence that
            // must fail closed instead of completing (ADR-0004 PSC-3/PSC-5).
            return Err(protocol_error("INVALID_FRAME"));
        }
        let choice = &choices[0];
        let finish_before_frame = self.finish.is_some();
        match choice.get("finish_reason") {
            None | Some(Value::Null) => {}
            Some(Value::String(reason)) => {
                if finish_before_frame {
                    // A repeated or conflicting finish reason is late output
                    // (ADR-0004 PSC-5).
                    return Err(protocol_error("INVALID_FINISH_FRAME"));
                }
                self.finish = Some(reason.clone());
            }
            Some(_) => return Err(protocol_error("INVALID_FRAME")),
        }
        if finish_before_frame {
            // Only an optional valid usage frame may follow a finish frame
            // before `[DONE]` or explicit clean end; later content, Tool
            // fragments, or empty deltas are late output (ADR-0004 PSC-5).
            return Err(protocol_error("INVALID_FINISH_FRAME"));
        }
        let finishes_tool_calls = self.finish.as_deref() == Some("tool_calls");
        let delta = choice.get("delta").unwrap_or(&Value::Null);
        if !matches!(delta, Value::Null | Value::Object(_)) {
            // A malformed delta envelope must fail closed before its finish
            // is trusted: clean-end completion is authorized only by a
            // validated stop frame (ADR-0004 PSC-3).
            return Err(protocol_error("INVALID_DELTA_FRAME"));
        }
        let content = match delta.get("content") {
            Some(Value::String(content)) if !content.is_empty() => Some(content.clone()),
            None | Some(Value::Null | Value::String(_)) => None,
            Some(_) => return Err(protocol_error("INVALID_DELTA_FRAME")),
        };
        match delta.get("tool_calls") {
            // An absent member falls through to the content path; a present
            // member must be the tool-call fragment array, because silently
            // ignoring wrong-typed provider output would drop malformed
            // content behind a later `[DONE]` (ADR-0003 TC-02).
            None => {}
            Some(Value::Array(fragments)) => {
                self.accumulate_tool_call_fragments(fragments)?;
                if let Some(content) = content {
                    self.ready.push_back(ProviderEvent::Delta(content));
                }
                if finishes_tool_calls {
                    self.flush_tool_calls()?;
                }
                return Ok(None);
            }
            Some(_) => return Err(protocol_error("INVALID_TOOL_CALL_FRAME")),
        }
        if finishes_tool_calls {
            if let Some(content) = content {
                self.ready.push_back(ProviderEvent::Delta(content));
            }
            self.flush_tool_calls()?;
            return Ok(None);
        }
        Ok(content.map(ProviderEvent::Delta))
    }

    /// Applies the `[DONE]` sentinel: it is terminal evidence by
    /// itself and does not require an earlier finish reason (ADR-0004 PSC-2).
    /// Terminating on the sentinel also keeps the transport's trailing
    /// clean-end frame from producing a second completion.
    fn done_sentinel(&mut self) -> Result<Option<ProviderEvent>, ProviderError> {
        if !self.assembled.is_empty() {
            // Tool-call fragments were accumulated but the provider ended
            // the stream without `finish_reason: "tool_calls"`; accepting
            // completion would silently drop the requested action, so the
            // malformed sequence fails closed instead.
            return Err(protocol_error("INVALID_TOOL_CALL_FRAME"));
        }
        if self.served_tool_round {
            // A Tool-call round's stream end carries no Turn completion:
            // the runner starts the continuation request carrying the
            // committed results and accepts completion only from that
            // continuation (ADR-0003 TC-11).
            self.terminated = true;
            return Ok(None);
        }
        self.terminated = true;
        Ok(Some(ProviderEvent::Completed))
    }

    /// Validates one final usage frame (ADR-0001): it carries no choices and
    /// its counters form exactly one owned `Usage`.
    fn parse_usage_frame(document: &Value, usage_value: &Value) -> Result<Usage, ProviderError> {
        let Some(choices) = document.get("choices").and_then(Value::as_array) else {
            return Err(protocol_error("INVALID_USAGE_FRAME"));
        };
        if !choices.is_empty() {
            return Err(protocol_error("INVALID_USAGE_FRAME"));
        }
        let input_tokens = usage_value
            .get("prompt_tokens")
            .and_then(Value::as_u64)
            .ok_or_else(|| protocol_error("INVALID_USAGE_FRAME"))?;
        let output_tokens = usage_value
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .ok_or_else(|| protocol_error("INVALID_USAGE_FRAME"))?;
        let total_tokens = usage_value
            .get("total_tokens")
            .and_then(Value::as_u64)
            .ok_or_else(|| protocol_error("INVALID_USAGE_FRAME"))?;
        let usage = Usage::new(input_tokens, output_tokens)
            .map_err(|_| protocol_error("INVALID_USAGE_FRAME"))?;
        if usage.total_tokens != total_tokens {
            return Err(protocol_error("INVALID_USAGE_FRAME"));
        }
        Ok(usage)
    }

    /// Applies the explicit transport clean end (ADR-0004 PSC-3/PSC-4/PSC-5):
    /// only a previously validated `stop` finish may complete the Turn, a
    /// served Tool-call round ends without Turn completion so the runner
    /// continues under ADR-0003, and every other terminal state fails closed
    /// with a typed provider error.
    fn clean_end(&mut self) -> Result<Option<ProviderEvent>, ProviderError> {
        if !self.assembled.is_empty() {
            // Fragments were accumulated without `finish_reason:
            // "tool_calls"`; accepting completion would silently drop the
            // requested action, so the same fail-closed rule as `[DONE]`
            // applies (ADR-0004 PSC-5).
            return Err(protocol_error("INVALID_TOOL_CALL_FRAME"));
        }
        if self.served_tool_round {
            self.terminated = true;
            return Ok(None);
        }
        if self.finish.as_deref() == Some("stop") {
            self.terminated = true;
            return Ok(Some(ProviderEvent::Completed));
        }
        Err(protocol_error("OPENAI_UNEXPECTED_EOF"))
    }

    /// Merges one frame of streamed Tool-call fragments into the assembly.
    ///
    /// Cumulative bounds are enforced before any allocation grows: one call's
    /// assembled arguments never exceed the canonical 65,536-byte serialized
    /// action input (ADR-0003), and the assembled call count never exceeds 32
    /// — each serviced call appends at least a `ToolCall` and a `ToolResult`
    /// D-3 item, so the 64-item per-Turn provider buffer (ADR-0001) could
    /// never record a 33rd call. A provider that crosses either bound fails
    /// closed.
    fn accumulate_tool_call_fragments(&mut self, fragments: &[Value]) -> Result<(), ProviderError> {
        for fragment in fragments {
            let index = fragment
                .get("index")
                .and_then(Value::as_u64)
                .ok_or_else(|| protocol_error("INVALID_TOOL_CALL_FRAME"))?;
            let function = fragment
                .get("function")
                .and_then(Value::as_object)
                .ok_or_else(|| protocol_error("INVALID_TOOL_CALL_FRAME"))?;
            let position = if let Some(position) = self
                .assembled
                .iter()
                .position(|entry| entry.index == Some(index))
            {
                position
            } else {
                if self.assembled.len() >= MAX_ASSEMBLED_TOOL_CALLS {
                    return Err(protocol_error("TOO_MANY_TOOL_CALLS"));
                }
                self.assembled.push(AssembledToolCall::default());
                self.assembled.len() - 1
            };
            let entry = &mut self.assembled[position];
            entry.index = Some(index);
            match function.get("name") {
                None => {}
                Some(Value::String(name)) if entry.name.replace(name.to_owned()).is_none() => {}
                Some(_) => return Err(protocol_error("INVALID_TOOL_CALL_FRAME")),
            }
            if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                if entry.arguments.len() + arguments.len() > MAX_TOOL_CALL_ARGUMENTS_BYTES {
                    return Err(protocol_error("TOOL_CALL_ARGUMENTS_TOO_LARGE"));
                }
                entry.arguments.push_str(arguments);
            } else if function.get("arguments").is_some() {
                return Err(protocol_error("INVALID_TOOL_CALL_FRAME"));
            }
        }
        Ok(())
    }

    /// Emits every assembled Tool call in index order after the provider
    /// finished its Tool-call round.
    fn flush_tool_calls(&mut self) -> Result<Option<ProviderEvent>, ProviderError> {
        if self.assembled.is_empty() {
            return Err(protocol_error("INVALID_TOOL_CALL_FRAME"));
        }
        self.served_tool_round = true;
        let mut assembled = std::mem::take(&mut self.assembled);
        assembled.sort_by_key(|entry| entry.index);
        for entry in assembled {
            let name = entry
                .name
                .filter(|name| !name.is_empty())
                .ok_or_else(|| protocol_error("INVALID_TOOL_CALL_FRAME"))?;
            self.ready.push_back(ProviderEvent::ToolCall {
                name,
                arguments: entry.arguments,
            });
        }
        Ok(None)
    }
}

fn protocol_error(code: &str) -> ProviderError {
    ProviderError {
        code: code.to_owned(),
    }
}
