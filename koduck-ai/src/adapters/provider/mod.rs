// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! OpenAI-compatible protocol translation into provider-neutral application events.

use thiserror::Error;
use tokio::runtime::Handle;

use crate::application::{ModelInput, ModelProvider, ProviderError, ProviderEvent, ProviderStream};
use crate::domain::{ItemPayload, Usage};

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
    ) -> Result<Vec<String>, OpenAiTransportError>;
}

/// Reqwest transport for one explicitly configured OpenAI-compatible endpoint.
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
    ) -> Result<Vec<String>, OpenAiTransportError> {
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
        let response = self
            .runtime
            .block_on(request.send())
            .map_err(|_| transport_error("OPENAI_REQUEST_FAILED"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(transport_error(&format!("OPENAI_HTTP_{}", status.as_u16())));
        }
        let body = self
            .runtime
            .block_on(response.text())
            .map_err(|_| transport_error("OPENAI_BODY_FAILED"))?;
        let frames = body
            .lines()
            .filter(|line| line.starts_with("data: "))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if frames.is_empty() {
            Err(transport_error("OPENAI_EMPTY_STREAM"))
        } else {
            Ok(frames)
        }
    }
}

fn provider_messages(input: &ModelInput) -> Vec<serde_json::Value> {
    let mut messages = input
        .history
        .iter()
        .filter_map(|item| match &item.payload {
            ItemPayload::UserMessage { content } => {
                Some(serde_json::json!({ "role": "user", "content": content }))
            }
            ItemPayload::AgentMessageDelta { content } => {
                Some(serde_json::json!({ "role": "assistant", "content": content }))
            }
            ItemPayload::Usage(_) | ItemPayload::Terminal(_) => None,
        })
        .collect::<Vec<_>>();
    messages.push(serde_json::json!({
        "role": "user",
        "content": input.input,
    }));
    messages
}

fn transport_error(code: &str) -> OpenAiTransportError {
    OpenAiTransportError {
        code: code.to_owned(),
    }
}

/// Translates OpenAI-compatible streaming frames into owned provider events.
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
        let frames = self
            .transport
            .chat_completion_frames(&input)
            .map_err(|error| ProviderError { code: error.code })?;
        let events = frames
            .iter()
            .map(|frame| parse_frame(frame))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Box::new(events.into_iter().flatten()))
    }
}

fn parse_frame(frame: &str) -> Result<Option<ProviderEvent>, ProviderError> {
    let data = frame
        .strip_prefix("data: ")
        .ok_or_else(|| protocol_error("INVALID_FRAME"))?;
    if data == "[DONE]" {
        return Ok(Some(ProviderEvent::Completed));
    }
    if data.contains("\"error\"") {
        return extract_string(data, "code")
            .map(|code| Some(ProviderEvent::Error { code }))
            .ok_or_else(|| protocol_error("INVALID_ERROR_FRAME"));
    }
    if data.contains("\"usage\"") {
        let input_tokens = extract_u64(data, "prompt_tokens")
            .ok_or_else(|| protocol_error("INVALID_USAGE_FRAME"))?;
        let output_tokens = extract_u64(data, "completion_tokens")
            .ok_or_else(|| protocol_error("INVALID_USAGE_FRAME"))?;
        let total_tokens = extract_u64(data, "total_tokens")
            .ok_or_else(|| protocol_error("INVALID_USAGE_FRAME"))?;
        let usage = Usage::new(input_tokens, output_tokens)
            .map_err(|_| protocol_error("INVALID_USAGE_FRAME"))?;
        if usage.total_tokens != total_tokens {
            return Err(protocol_error("INVALID_USAGE_FRAME"));
        }
        return Ok(Some(ProviderEvent::Usage(usage)));
    }
    extract_string(data, "content")
        .filter(|content| !content.is_empty())
        .map(|content| Some(ProviderEvent::Delta(content)))
        .ok_or_else(|| protocol_error("INVALID_DELTA_FRAME"))
}

fn protocol_error(code: &str) -> ProviderError {
    ProviderError {
        code: code.to_owned(),
    }
}

fn extract_u64(document: &str, field: &str) -> Option<u64> {
    let marker = format!("\"{field}\":");
    let suffix = document.split_once(&marker)?.1;
    let digits = suffix
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn extract_string(document: &str, field: &str) -> Option<String> {
    let marker = format!("\"{field}\":\"");
    let suffix = document.split_once(&marker)?.1;
    let mut escaped = false;
    let mut value = String::new();
    for character in suffix.chars() {
        if escaped {
            value.push(match character {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '\\' => '\\',
                '"' => '"',
                _ => return None,
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            return Some(value);
        } else {
            value.push(character);
        }
    }
    None
}
