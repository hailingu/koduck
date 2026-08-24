// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Canonical executor deadline values for one D-7 attempt.

/// Maximum duration of one dispatched Tool action before its D-7 times out.
pub const MAX_ACTION_DURATION_MILLIS: u64 = 30_000;

/// Remaining C-5 action budget the isolated executor must enforce for one D-7.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionDeadline {
    remaining_millis: u64,
}

impl ActionDeadline {
    /// Derives the remaining action budget from two readings in the same C-5 clock domain.
    #[must_use]
    pub(crate) const fn from_started_at(started_at_millis: u64, observed_at_millis: u64) -> Self {
        Self {
            remaining_millis: MAX_ACTION_DURATION_MILLIS
                .saturating_sub(observed_at_millis.saturating_sub(started_at_millis)),
        }
    }

    /// Returns the relative timeout supplied to the executor boundary.
    #[must_use]
    pub const fn remaining_millis(self) -> u64 {
        self.remaining_millis
    }
}
