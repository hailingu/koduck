// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Exact CAND-1 append deadline and unpublished-buffer limits.

use std::time::Duration;

use thiserror::Error;

use super::NewItem;
use crate::domain::TerminalOutcome;

/// Exact append and unpublished-buffer limits selected by CAND-1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppendPolicy {
    deadline: Duration,
    max_items: usize,
    max_payload_bytes: usize,
}

impl AppendPolicy {
    /// Returns the approved 2-second, 64-item, 1-MiB CAND-1 policy.
    #[must_use]
    pub const fn cand_1() -> Self {
        Self {
            deadline: Duration::from_secs(2),
            max_items: 64,
            max_payload_bytes: 1_048_576,
        }
    }

    /// Returns the maximum time allowed for one production history append.
    #[must_use]
    pub const fn deadline(self) -> Duration {
        self.deadline
    }

    /// Validates the next provider item against the approved per-turn count cap.
    ///
    /// # Errors
    ///
    /// Returns [`BufferLimitError::ItemCount`] when `item_count` exceeds 64.
    pub fn check_item_count(self, item_count: usize) -> Result<(), BufferLimitError> {
        if item_count > self.max_items {
            Err(BufferLimitError::ItemCount)
        } else {
            Ok(())
        }
    }

    /// Validates one unpublished item against the approved payload cap.
    ///
    /// # Errors
    ///
    /// Returns [`BufferLimitError::PayloadBytes`] when the item exceeds 1 MiB.
    pub fn check_item(self, item: &NewItem) -> Result<(), BufferLimitError> {
        self.accumulate_payload_bytes(0, item).map(|_| ())
    }

    /// Returns the next per-turn payload total when adding one provider item.
    ///
    /// # Errors
    ///
    /// Returns [`BufferLimitError::PayloadBytes`] when the cumulative serialized
    /// payload would exceed 1 MiB.
    pub fn accumulate_payload_bytes(
        self,
        current_payload_bytes: usize,
        item: &NewItem,
    ) -> Result<usize, BufferLimitError> {
        let next_payload_bytes = current_payload_bytes.saturating_add(payload_bytes(item));
        if next_payload_bytes > self.max_payload_bytes {
            Err(BufferLimitError::PayloadBytes)
        } else {
            Ok(next_payload_bytes)
        }
    }

    /// Validates one projected cumulative payload total against the approved
    /// cap, including capacity reserved for a guaranteed remainder.
    ///
    /// # Errors
    ///
    /// Returns [`BufferLimitError::PayloadBytes`] when the total exceeds 1 MiB.
    pub(crate) fn check_payload_bytes(self, payload_bytes: usize) -> Result<(), BufferLimitError> {
        if payload_bytes > self.max_payload_bytes {
            Err(BufferLimitError::PayloadBytes)
        } else {
            Ok(())
        }
    }

    /// Checks whether one append completed inside the exact deadline.
    ///
    /// # Errors
    ///
    /// Returns [`BufferLimitError::AppendDeadline`] after 2 seconds.
    pub fn check_deadline(self, elapsed: Duration) -> Result<(), BufferLimitError> {
        if elapsed > self.deadline {
            Err(BufferLimitError::AppendDeadline)
        } else {
            Ok(())
        }
    }
}

/// A fail-closed reason for stopping provider consumption before publication.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum BufferLimitError {
    /// One append exceeded 2 seconds.
    #[error("append deadline exceeded")]
    AppendDeadline,
    /// More than 64 unpublished items would be retained.
    #[error("unpublished item count exceeded")]
    ItemCount,
    /// More than 1 MiB of unpublished serialized payload would be retained.
    #[error("unpublished payload bytes exceeded")]
    PayloadBytes,
}

impl BufferLimitError {
    /// Returns the stable presentation problem code for every fail-closed limit.
    #[must_use]
    pub const fn problem_code(self) -> &'static str {
        "durability-unavailable"
    }
}

fn payload_bytes(item: &NewItem) -> usize {
    match item {
        NewItem::AgentMessageDelta { content } => {
            "{\"content\":".len() + json_string_bytes(content) + "}".len()
        }
        NewItem::Usage(usage) | NewItem::Terminal(TerminalOutcome::Completed { usage }) => {
            usage_payload_bytes(*usage)
        }
        NewItem::Terminal(TerminalOutcome::Failed { code }) => {
            "{\"code\":".len() + json_string_bytes(code) + "}".len()
        }
        NewItem::Terminal(TerminalOutcome::Interrupted | TerminalOutcome::Cancelled) => "{}".len(),
        NewItem::ApprovalStatus {
            approval_id,
            attempt_id,
            status,
            decision,
            version,
        } => {
            "{\"approval_id\":".len()
                + approval_id.as_uuid().to_string().len()
                + 2
                + ",\"attempt_id\":".len()
                + attempt_id.as_uuid().to_string().len()
                + 2
                + ",\"status\":".len()
                + json_string_bytes(status.as_str())
                + ",\"decision\":".len()
                + decision.map_or(4, |decision| json_string_bytes(decision.as_str()))
                + ",\"version\":".len()
                + decimal_bytes(*version)
                + "}".len()
        }
        NewItem::ToolCall {
            descriptor_id,
            descriptor_version,
            target,
            attempt_id,
            status,
            version,
        } => {
            "{\"descriptor_id\":".len()
                + json_string_bytes(descriptor_id)
                + ",\"descriptor_version\":".len()
                + json_string_bytes(descriptor_version)
                + ",\"target\":".len()
                + json_string_bytes(target)
                + ",\"attempt_id\":".len()
                + attempt_id.map_or(4, |id| id.as_uuid().to_string().len() + 2)
                + ",\"status\":".len()
                + status.map_or(4, |status| status.as_str().len() + 2)
                + ",\"version\":".len()
                + version.map_or(4, decimal_bytes)
                + "}".len()
        }
        NewItem::ToolResult {
            attempt_id,
            status,
            code,
            output_bytes,
            output_digest,
            effect_state,
            version,
        } => {
            "{\"attempt_id\":".len()
                + attempt_id.map_or(4, |id| id.as_uuid().to_string().len() + 2)
                + ",\"status\":".len()
                + json_string_bytes(status.as_str())
                + ",\"code\":".len()
                + code.as_deref().map_or(4, json_string_bytes)
                + ",\"output_bytes\":".len()
                + decimal_bytes(*output_bytes)
                + ",\"output_digest\":".len()
                + output_digest.as_deref().map_or(4, json_string_bytes)
                + ",\"effect_state\":".len()
                + effect_state.map_or(4, decimal_effect_state_bytes)
                + ",\"version\":".len()
                + version.map_or(4, decimal_bytes)
                + "}".len()
        }
    }
}

/// Returns the exact serialized size of one string as a JSON value, including
/// its surrounding quotes and every escape sequence the canonical encoder
/// emits. Shared by the unpublished-buffer preflight and the model-bound
/// Tool-result boundary check so both account against the encoded size.
pub(crate) fn json_string_bytes(value: &str) -> usize {
    value.chars().fold(2_usize, |size, character| {
        size.saturating_add(match character {
            '"' | '\\' | '\u{0008}' | '\t' | '\n' | '\u{000c}' | '\r' => 2,
            '\u{0000}'..='\u{001f}' => 6,
            other => other.len_utf8(),
        })
    })
}

fn usage_payload_bytes(usage: crate::domain::Usage) -> usize {
    "{\"input_tokens\":".len()
        + decimal_bytes(usage.input_tokens)
        + ",\"output_tokens\":".len()
        + decimal_bytes(usage.output_tokens)
        + ",\"total_tokens\":".len()
        + decimal_bytes(usage.total_tokens)
        + "}".len()
}

fn decimal_bytes(value: u64) -> usize {
    value.to_string().len()
}

fn decimal_effect_state_bytes(state: crate::domain::ToolEffectState) -> usize {
    match state {
        crate::domain::ToolEffectState::NotStarted => "not_started".len() + 2,
        crate::domain::ToolEffectState::Started => "started".len() + 2,
        crate::domain::ToolEffectState::Unknown => "unknown".len() + 2,
    }
}
