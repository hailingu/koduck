// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! OpenAI-compatible protocol translation into provider-neutral application events.

use thiserror::Error;

use crate::application::{ModelInput, ModelProvider, ProviderError, ProviderEvent, ProviderStream};
use crate::domain::Usage;

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
