// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Domain-owned lifecycle rules for a foreground model turn.

use thiserror::Error;

/// The authoritative lifecycle state of a turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TurnStatus {
    /// Provider consumption may produce durable items.
    Started,
    /// Durability is unavailable and the owner must stop provider consumption.
    RecoveryPending,
    /// Provider consumption finished successfully.
    Completed,
    /// Provider execution or durability recovery ended unsuccessfully.
    Failed,
    /// The authenticated owner requested an interruption.
    Interrupted,
    /// The platform, dependency, or fenced owner ended the turn.
    Cancelled,
}

impl TurnStatus {
    /// Reports whether no later lifecycle transition is permitted.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Interrupted | Self::Cancelled
        )
    }
}

/// A domain error raised when a lifecycle transition violates the state model.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("turn cannot transition from {from:?} to {to:?}")]
pub struct TurnTransitionError {
    from: TurnStatus,
    to: TurnStatus,
}

/// A turn whose status can change only through the owned lifecycle rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Turn {
    status: TurnStatus,
}

impl Turn {
    /// Creates a newly accepted turn in the `Started` state.
    #[must_use]
    pub const fn start() -> Self {
        Self {
            status: TurnStatus::Started,
        }
    }

    /// Returns the current authoritative status.
    #[must_use]
    pub const fn status(&self) -> TurnStatus {
        self.status
    }

    /// Completes a live turn after the provider finishes successfully.
    pub const fn complete(self) -> Result<Self, TurnTransitionError> {
        self.transition(TurnStatus::Completed)
    }

    /// Interrupts a live turn at the authenticated owner's request.
    pub const fn interrupt(self) -> Result<Self, TurnTransitionError> {
        self.transition(TurnStatus::Interrupted)
    }

    const fn transition(self, to: TurnStatus) -> Result<Self, TurnTransitionError> {
        if self.status == TurnStatus::Started {
            Ok(Self { status: to })
        } else {
            Err(TurnTransitionError {
                from: self.status,
                to,
            })
        }
    }
}
