// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Exact CAND-1 append deadline and unpublished-buffer limits.

use std::time::Duration;

use thiserror::Error;

use super::NewItem;
use crate::domain::{Item, TerminalOutcome};

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

/// Bounded unpublished items waiting for durable append confirmation.
pub struct UnpublishedBuffer {
    policy: AppendPolicy,
    pending: Vec<NewItem>,
    pending_payload_bytes: usize,
    durable_prefix: Vec<Item>,
}

impl UnpublishedBuffer {
    /// Creates an empty unpublished buffer under an explicit policy.
    #[must_use]
    pub const fn new(policy: AppendPolicy) -> Self {
        Self {
            policy,
            pending: Vec::new(),
            pending_payload_bytes: 0,
            durable_prefix: Vec::new(),
        }
    }

    /// Adds one unpublished item without exceeding the exact count or byte cap.
    ///
    /// # Errors
    ///
    /// Returns [`BufferLimitError`] without changing the buffer when either cap
    /// would be exceeded.
    pub fn push(&mut self, item: NewItem) -> Result<(), BufferLimitError> {
        if self.pending.len() == self.policy.max_items {
            return Err(BufferLimitError::ItemCount);
        }
        let payload_bytes = payload_bytes(&item);
        if self.pending_payload_bytes.saturating_add(payload_bytes) > self.policy.max_payload_bytes
        {
            return Err(BufferLimitError::PayloadBytes);
        }
        self.pending_payload_bytes += payload_bytes;
        self.pending.push(item);
        Ok(())
    }

    /// Removes and returns only items already confirmed durable.
    #[must_use]
    pub fn take_durable_prefix(&mut self) -> Vec<Item> {
        std::mem::take(&mut self.durable_prefix)
    }
}

fn payload_bytes(item: &NewItem) -> usize {
    match item {
        NewItem::AgentMessageDelta { content } => content.len(),
        NewItem::Usage(_) => 24,
        NewItem::Terminal(TerminalOutcome::Completed { .. }) => 32,
        NewItem::Terminal(TerminalOutcome::Failed { code }) => code.len(),
        NewItem::Terminal(TerminalOutcome::Interrupted | TerminalOutcome::Cancelled) => 0,
    }
}
