// ADR: docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md

//! Application-owned coalescing of raw provider deltas into bounded content.
//!
//! Provider chunk boundaries are transport artifacts, so raw fragments never
//! become canonical Items directly: one accumulator per Turn retains the
//! ordered bytes and flushes them at exact byte, latency, or semantic
//! boundaries while preserving byte-for-byte UTF-8 content order (ADR-0005
//! PLB-1/PLB-2/PLB-9).

use std::time::{Duration, Instant};

/// Maximum buffered content bytes before the accumulator must flush.
pub const MAX_BUFFERED_DELTA_BYTES: usize = 16_384;

/// Maximum latency from the first buffered byte to a flush under an
/// advancing runtime clock.
pub const DELTA_FLUSH_LATENCY: Duration = Duration::from_millis(500);

/// One Turn-scoped delta accumulator that turns raw provider fragments into
/// bounded coalesced content (ADR-0005 PLB-1/PLB-2).
///
/// The coalescer is a pure state machine over caller-supplied clock samples:
/// every operation takes the current [`Instant`], so deterministic tests can
/// pause and advance time while production passes a live clock.
#[derive(Debug, Default)]
pub struct DeltaCoalescer {
    buffer: String,
    first_buffered_at: Option<Instant>,
}

impl DeltaCoalescer {
    /// Creates an empty accumulator.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            buffer: String::new(),
            first_buffered_at: None,
        }
    }

    /// Buffers one non-empty provider fragment and returns the complete
    /// coalesced chunks that must be appended now, in order.
    ///
    /// The accumulator flushes immediately before adding content that would
    /// exceed [`MAX_BUFFERED_DELTA_BYTES`]. A fragment above that cap is
    /// split at UTF-8 scalar boundaries into the minimum ordered sequence of
    /// non-empty chunks no larger than the cap; every full chunk is returned
    /// and only a sub-cap remainder stays buffered, so concatenating every
    /// emitted chunk with the retained content reproduces the provider bytes
    /// exactly (ADR-0005 PLB-2).
    pub fn push(&mut self, fragment: &str, now: Instant) -> Vec<String> {
        let mut emitted = Vec::new();
        if fragment.is_empty() {
            return emitted;
        }
        if !self.buffer.is_empty() && self.buffer.len() + fragment.len() > MAX_BUFFERED_DELTA_BYTES
        {
            emitted.push(self.take_buffered());
        }
        if fragment.len() > MAX_BUFFERED_DELTA_BYTES {
            let mut remainder = fragment;
            while remainder.len() > MAX_BUFFERED_DELTA_BYTES {
                let split = scalar_boundary(remainder, MAX_BUFFERED_DELTA_BYTES);
                let (chunk, rest) = remainder.split_at(split);
                emitted.push(chunk.to_owned());
                remainder = rest;
            }
            remainder.clone_into(&mut self.buffer);
            self.first_buffered_at = Some(now);
            return emitted;
        }
        if self.buffer.is_empty() {
            self.first_buffered_at = Some(now);
        }
        self.buffer.push_str(fragment);
        emitted
    }

    /// Returns the buffered content when the latency bound elapsed, restarting
    /// the timer, or `None` while the accumulator is empty or still inside
    /// [`DELTA_FLUSH_LATENCY`] of its first buffered byte.
    pub fn take_due_flush(&mut self, now: Instant) -> Option<String> {
        let first = self.first_buffered_at?;
        if now.saturating_duration_since(first) < DELTA_FLUSH_LATENCY {
            return None;
        }
        Some(self.take_buffered())
    }

    /// Returns all buffered content for a semantic-boundary flush, or `None`
    /// when the accumulator is empty (ADR-0005 PLB-3).
    pub fn take_forced_flush(&mut self) -> Option<String> {
        if self.buffer.is_empty() {
            None
        } else {
            Some(self.take_buffered())
        }
    }

    /// Reports whether content is currently buffered.
    #[must_use]
    pub fn has_pending(&self) -> bool {
        !self.buffer.is_empty()
    }

    fn take_buffered(&mut self) -> String {
        self.first_buffered_at = None;
        std::mem::take(&mut self.buffer)
    }
}

/// Returns the largest UTF-8 scalar boundary at or below `cap` bytes.
fn scalar_boundary(fragment: &str, cap: usize) -> usize {
    let mut end = cap.min(fragment.len());
    while end > 0 && !fragment.is_char_boundary(end) {
        end -= 1;
    }
    end
}
