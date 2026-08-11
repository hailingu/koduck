// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! OpenAI-compatible protocol translation into provider-neutral application events.

use std::thread;
use std::time::Duration;

use serde_json::Value;
use thiserror::Error;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;

use crate::application::{ModelInput, ModelProvider, ProviderError, ProviderEvent, ProviderStream};
use crate::domain::{ItemPayload, Usage};

const MAX_OPENAI_FRAME_BYTES: usize = 1_048_576;

/// A transport-level failure before OpenAI-compatible frames are available.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("OpenAI-compatible transport failed: {code}")]
pub struct OpenAiTransportError {
    /// Stable adapter-owned transport code.
    pub code: String,
}

/// Boundary implemented by the configured HTTP transport or deterministic protocol server.
pub trait OpenAiProtocolTransport {
    /// Returns the ordered `data:` frames for one chat-completions request.
    ///
    /// # Errors
    ///
    /// Returns [`OpenAiTransportError`] when the configured transport cannot
    /// open or consume the provider stream.
    fn chat_completion_frames(
        &mut self,
        input: &ModelInput,
    ) -> Result<OpenAiFrameStream, OpenAiTransportError>;
}

/// One data frame or bounded idle poll from an OpenAI-compatible stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenAiFrame {
    /// One complete `data:` line.
    Data(String),
    /// No frame arrived during the bounded control-poll interval.
    Pending,
}

/// A lazy sequence of OpenAI-compatible SSE frames and idle polls.
pub type OpenAiFrameStream =
    Box<dyn Iterator<Item = Result<OpenAiFrame, OpenAiTransportError>> + Send>;

/// Reqwest transport for one explicitly configured OpenAI-compatible endpoint.
#[derive(Clone)]
pub struct ReqwestOpenAiTransport {
    client: reqwest::Client,
    runtime: Handle,
    endpoint: String,
    model: String,
    api_key: String,
}

impl ReqwestOpenAiTransport {
    /// Creates the production provider transport without issuing a request.
    #[must_use]
    pub fn new(
        client: reqwest::Client,
        runtime: Handle,
        base_url: &str,
        model: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            client,
            runtime,
            endpoint: format!("{}/chat/completions", base_url.trim_end_matches('/')),
            model: model.into(),
            api_key: api_key.into(),
        }
    }
}

impl OpenAiProtocolTransport for ReqwestOpenAiTransport {
    fn chat_completion_frames(
        &mut self,
        input: &ModelInput,
    ) -> Result<OpenAiFrameStream, OpenAiTransportError> {
        let messages = provider_messages(input);
        let request = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({
                "model": self.model,
                "messages": messages,
                "stream": true,
                "stream_options": { "include_usage": true },
            }));
        let (sender, mut receiver) = mpsc::channel(64);
        self.runtime.spawn(pump_request(request, sender));
        Ok(Box::new(std::iter::from_fn(move || {
            match receiver.try_recv() {
                Ok(frame) => Some(frame),
                Err(TryRecvError::Empty) => {
                    thread::sleep(Duration::from_millis(50));
                    Some(Ok(OpenAiFrame::Pending))
                }
                Err(TryRecvError::Disconnected) => None,
            }
        })))
    }
}

async fn pump_request(
    request: reqwest::RequestBuilder,
    sender: mpsc::Sender<Result<OpenAiFrame, OpenAiTransportError>>,
) {
    let response = tokio::select! {
        () = sender.closed() => return,
        response = request.send() => response,
    };
    let Ok(response) = response else {
        let _ = sender
            .send(Err(transport_error("OPENAI_REQUEST_FAILED")))
            .await;
        return;
    };
    let status = response.status();
    if !status.is_success() {
        let _ = sender
            .send(Err(transport_error(&format!(
                "OPENAI_HTTP_{}",
                status.as_u16()
            ))))
            .await;
        return;
    }
    pump_response(response, sender).await;
}

async fn pump_response(
    mut response: reqwest::Response,
    sender: mpsc::Sender<Result<OpenAiFrame, OpenAiTransportError>>,
) {
    let mut pending = Vec::new();
    let mut saw_frame = false;
    loop {
        let chunk = tokio::select! {
            () = sender.closed() => return,
            chunk = response.chunk() => chunk,
        };
        match chunk {
            Ok(Some(chunk)) => {
                if let Err(error) =
                    consume_chunk(&sender, &mut pending, &chunk, &mut saw_frame).await
                {
                    let _ = sender.send(Err(error)).await;
                    return;
                }
            }
            Ok(None) => break,
            Err(_) => {
                let _ = sender
                    .send(Err(transport_error("OPENAI_BODY_FAILED")))
                    .await;
                return;
            }
        }
    }
    if !pending.is_empty() && send_frame(&sender, pending, &mut saw_frame).await.is_err() {
        let _ = sender
            .send(Err(transport_error("OPENAI_BODY_FAILED")))
            .await;
        return;
    }
    if !saw_frame {
        let _ = sender
            .send(Err(transport_error("OPENAI_EMPTY_STREAM")))
            .await;
    }
}

async fn consume_chunk(
    sender: &mpsc::Sender<Result<OpenAiFrame, OpenAiTransportError>>,
    pending: &mut Vec<u8>,
    chunk: &[u8],
    saw_frame: &mut bool,
) -> Result<(), OpenAiTransportError> {
    let mut remaining = chunk;
    while let Some(newline) = remaining.iter().position(|byte| *byte == b'\n') {
        append_frame_bytes(pending, &remaining[..=newline])?;
        let mut line = std::mem::take(pending);
        while line
            .last()
            .is_some_and(|byte| matches!(*byte, b'\n' | b'\r'))
        {
            line.pop();
        }
        send_frame(sender, line, saw_frame)
            .await
            .map_err(|()| transport_error("OPENAI_BODY_FAILED"))?;
        remaining = &remaining[newline + 1..];
    }
    append_frame_bytes(pending, remaining)
}

fn append_frame_bytes(pending: &mut Vec<u8>, bytes: &[u8]) -> Result<(), OpenAiTransportError> {
    if pending
        .len()
        .checked_add(bytes.len())
        .is_none_or(|length| length > MAX_OPENAI_FRAME_BYTES)
    {
        return Err(transport_error("OPENAI_FRAME_TOO_LARGE"));
    }
    pending.extend_from_slice(bytes);
    Ok(())
}

async fn send_frame(
    sender: &mpsc::Sender<Result<OpenAiFrame, OpenAiTransportError>>,
    line: Vec<u8>,
    saw_frame: &mut bool,
) -> Result<(), ()> {
    let line = String::from_utf8(line).map_err(|_| ())?;
    if !line.starts_with("data: ") {
        return Ok(());
    }
    *saw_frame = true;
    sender
        .send(Ok(OpenAiFrame::Data(line)))
        .await
        .map_err(|_| ())
}

fn provider_messages(input: &ModelInput) -> Vec<serde_json::Value> {
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
            ItemPayload::Usage(_) => {}
        }
    }
    flush_assistant(&mut messages, &mut assistant);
    messages.push(serde_json::json!({
        "role": "user",
        "content": input.input,
    }));
    messages
}

fn flush_assistant(messages: &mut Vec<serde_json::Value>, assistant: &mut String) {
    if !assistant.is_empty() {
        messages.push(serde_json::json!({
            "role": "assistant",
            "content": std::mem::take(assistant),
        }));
    }
}

fn transport_error(code: &str) -> OpenAiTransportError {
    OpenAiTransportError {
        code: code.to_owned(),
    }
}

/// Translates OpenAI-compatible streaming frames into owned provider events.
#[derive(Clone)]
pub struct OpenAiCompatibleProvider<T> {
    transport: T,
}

impl<T> OpenAiCompatibleProvider<T> {
    /// Creates the adapter around one explicitly configured transport.
    #[must_use]
    pub const fn new(transport: T) -> Self {
        Self { transport }
    }
}

impl<T> ModelProvider for OpenAiCompatibleProvider<T>
where
    T: OpenAiProtocolTransport,
{
    fn stream(&mut self, input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        let mut frames = self
            .transport
            .chat_completion_frames(&input)
            .map_err(|error| ProviderError { code: error.code })?;
        let mut terminated = false;
        let events = std::iter::from_fn(move || {
            if terminated {
                return None;
            }
            let frame = frames.next()?;
            match frame {
                Ok(OpenAiFrame::Pending) => Some(ProviderEvent::Pending),
                Ok(OpenAiFrame::Data(frame)) => Some(parse_owned_frame(&frame, &mut terminated)),
                Err(error) => {
                    terminated = true;
                    Some(ProviderEvent::Error { code: error.code })
                }
            }
        });
        Ok(Box::new(events))
    }
}

fn parse_owned_frame(frame: &str, terminated: &mut bool) -> ProviderEvent {
    match parse_frame(frame) {
        Ok(Some(event)) => event,
        Ok(None) => ProviderEvent::Pending,
        Err(error) => {
            *terminated = true;
            ProviderEvent::Error { code: error.code }
        }
    }
}

fn parse_frame(frame: &str) -> Result<Option<ProviderEvent>, ProviderError> {
    let data = frame
        .strip_prefix("data: ")
        .ok_or_else(|| protocol_error("INVALID_FRAME"))?;
    if data == "[DONE]" {
        return Ok(Some(ProviderEvent::Completed));
    }
    let document: Value =
        serde_json::from_str(data).map_err(|_| protocol_error("INVALID_FRAME"))?;
    if let Some(error) = document.get("error").filter(|error| !error.is_null()) {
        let code = error
            .get("code")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol_error("INVALID_ERROR_FRAME"))?;
        return Ok(Some(ProviderEvent::Error {
            code: code.to_owned(),
        }));
    }
    if let Some(usage_value) = document.get("usage").filter(|usage| !usage.is_null()) {
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
        return Ok(Some(ProviderEvent::Usage(usage)));
    }
    match document.pointer("/choices/0/delta/content") {
        Some(Value::String(content)) if !content.is_empty() => {
            Ok(Some(ProviderEvent::Delta(content.clone())))
        }
        None | Some(Value::Null | Value::String(_)) => Ok(None),
        Some(_) => Err(protocol_error("INVALID_DELTA_FRAME")),
    }
}

fn protocol_error(code: &str) -> ProviderError {
    ProviderError {
        code: code.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use crate::application::ModelInput;
    use crate::domain::{Item, ItemPayload, TenantId, ThreadId, TurnId};

    use super::provider_messages;

    #[test]
    fn provider_history_coalesces_deltas_within_each_prior_turn() {
        let input = ModelInput {
            tenant_id: TenantId::new("tenant-a").expect("valid tenant"),
            thread_id: ThreadId::new(),
            turn_id: TurnId::new(),
            input: "second".to_owned(),
            history: vec![
                Item::new(
                    1,
                    ItemPayload::UserMessage {
                        content: "first".to_owned(),
                    },
                ),
                Item::new(
                    2,
                    ItemPayload::AgentMessageDelta {
                        content: "A".to_owned(),
                    },
                ),
                Item::new(
                    3,
                    ItemPayload::AgentMessageDelta {
                        content: "B".to_owned(),
                    },
                ),
            ],
        };

        assert_eq!(
            provider_messages(&input),
            vec![
                serde_json::json!({ "role": "user", "content": "first" }),
                serde_json::json!({ "role": "assistant", "content": "AB" }),
                serde_json::json!({ "role": "user", "content": "second" }),
            ]
        );
    }
}
