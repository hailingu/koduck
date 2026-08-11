// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Domain-owned lifecycle rules for a foreground model turn.

use thiserror::Error;
use uuid::Uuid;

/// A validation error for an owned domain value.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DomainValueError {
    /// A required identifier component was empty.
    #[error("{field} must not be empty")]
    Empty { field: &'static str },
    /// Token counters could not be added without overflow.
    #[error("usage token total overflowed")]
    UsageOverflow,
}

/// A tenant identifier established by the validated trust boundary.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TenantId(String);

impl TenantId {
    /// Creates a non-empty tenant identifier.
    ///
    /// # Errors
    ///
    /// Returns [`DomainValueError::Empty`] when the value is blank.
    pub fn new(value: impl Into<String>) -> Result<Self, DomainValueError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(DomainValueError::Empty { field: "tenant_id" })
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the validated tenant identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Immutable identity information supplied by the configured trust boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustContext {
    /// Tenant that owns all history touched by this request.
    pub tenant_id: TenantId,
    /// Authenticated subject within the tenant.
    pub subject_id: String,
}

impl TrustContext {
    /// Creates a trust context from already validated identity components.
    ///
    /// # Errors
    ///
    /// Returns [`DomainValueError::Empty`] when the subject identifier is blank.
    pub fn new(
        tenant_id: TenantId,
        subject_id: impl Into<String>,
    ) -> Result<Self, DomainValueError> {
        let subject_id = subject_id.into();
        if subject_id.trim().is_empty() {
            Err(DomainValueError::Empty {
                field: "subject_id",
            })
        } else {
            Ok(Self {
                tenant_id,
                subject_id,
            })
        }
    }
}

macro_rules! uuid_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub struct $name(Uuid);

        impl $name {
            /// Allocates a random version-4 identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Wraps an existing UUID received from a validated adapter.
            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            /// Returns the underlying UUID for adapter serialization.
            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

uuid_id!(ThreadId, "An AI-owned conversation thread identifier.");
uuid_id!(TurnId, "An immutable execution attempt within a thread.");
uuid_id!(ItemId, "An append-only history item identifier.");

/// The monotonically increasing ownership generation for a foreground turn.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LeaseGeneration(u64);

impl LeaseGeneration {
    /// Returns the first generation allocated during initial acceptance.
    #[must_use]
    pub const fn initial() -> Self {
        Self(1)
    }

    /// Returns the persisted numeric generation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Provider token accounting attached to a completed turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Usage {
    /// Tokens consumed by model input.
    pub input_tokens: u64,
    /// Tokens produced by the model.
    pub output_tokens: u64,
    /// Sum of input and output tokens.
    pub total_tokens: u64,
}

impl Usage {
    /// Creates usage counters with a checked total.
    ///
    /// # Errors
    ///
    /// Returns [`DomainValueError::UsageOverflow`] when the counters cannot be added.
    pub fn new(input_tokens: u64, output_tokens: u64) -> Result<Self, DomainValueError> {
        let total_tokens = input_tokens
            .checked_add(output_tokens)
            .ok_or(DomainValueError::UsageOverflow)?;
        Ok(Self {
            input_tokens,
            output_tokens,
            total_tokens,
        })
    }

    /// Returns zero usage for a provider that omits counters.
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
        }
    }
}

/// The durable reason a turn stopped producing output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalOutcome {
    /// Provider completion with final token accounting.
    Completed { usage: Usage },
    /// Provider or durability recovery failure with a stable code.
    Failed { code: String },
    /// Authenticated client interruption.
    Interrupted,
    /// Platform, dependency, or fenced-owner cancellation.
    Cancelled,
}

/// The owned content of an append-only history item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ItemPayload {
    /// The authenticated user's plain-text input.
    UserMessage { content: String },
    /// One non-empty provider-neutral model delta.
    AgentMessageDelta { content: String },
    /// Provider token accounting observed before completion.
    Usage(Usage),
    /// Exactly one terminal outcome for the turn.
    Terminal(TerminalOutcome),
}

/// A durably sequenced Thread/Turn history item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    /// Stable identity of this append.
    pub item_id: ItemId,
    /// Positive, strictly increasing sequence within one turn.
    pub sequence: u64,
    /// Domain-owned item content.
    pub payload: ItemPayload,
}

impl Item {
    /// Creates an item after a history implementation allocates its sequence.
    #[must_use]
    pub fn new(sequence: u64, payload: ItemPayload) -> Self {
        debug_assert!(sequence > 0, "history sequences must be positive");
        Self {
            item_id: ItemId::new(),
            sequence,
            payload,
        }
    }
}

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
    ///
    /// # Errors
    ///
    /// Returns [`TurnTransitionError`] unless the turn is currently started.
    pub fn complete(self) -> Result<Self, TurnTransitionError> {
        self.transition(TurnStatus::Completed)
    }

    /// Interrupts a live turn at the authenticated owner's request.
    ///
    /// # Errors
    ///
    /// Returns [`TurnTransitionError`] unless the turn is currently started.
    pub fn interrupt(self) -> Result<Self, TurnTransitionError> {
        self.transition(TurnStatus::Interrupted)
    }

    /// Fails a live turn after a provider error.
    ///
    /// # Errors
    ///
    /// Returns [`TurnTransitionError`] unless the turn is started or recovery-pending.
    pub fn fail(self) -> Result<Self, TurnTransitionError> {
        self.transition(TurnStatus::Failed)
    }

    /// Cancels a live turn after a platform or dependency stop.
    ///
    /// # Errors
    ///
    /// Returns [`TurnTransitionError`] unless the turn is started or recovery-pending.
    pub fn cancel(self) -> Result<Self, TurnTransitionError> {
        self.transition(TurnStatus::Cancelled)
    }

    /// Suspends a live turn after durability becomes unavailable.
    ///
    /// # Errors
    ///
    /// Returns [`TurnTransitionError`] unless the turn is currently started.
    pub fn recovery_pending(self) -> Result<Self, TurnTransitionError> {
        self.transition(TurnStatus::RecoveryPending)
    }

    fn transition(self, to: TurnStatus) -> Result<Self, TurnTransitionError> {
        let permitted = matches!(
            (self.status, to),
            (
                TurnStatus::Started,
                TurnStatus::Completed
                    | TurnStatus::Failed
                    | TurnStatus::Interrupted
                    | TurnStatus::Cancelled
                    | TurnStatus::RecoveryPending
            ) | (
                TurnStatus::RecoveryPending,
                TurnStatus::Failed | TurnStatus::Cancelled
            )
        );
        if permitted {
            Ok(Self { status: to })
        } else {
            Err(TurnTransitionError {
                from: self.status,
                to,
            })
        }
    }
}
