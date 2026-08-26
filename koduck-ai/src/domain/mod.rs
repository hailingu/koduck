// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md
// ADR: koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md

//! Domain-owned lifecycle rules for a foreground model turn.

pub mod execution;
pub mod item_correction;
pub mod tool;

use std::collections::BTreeSet;

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

/// C-7-validated approval scopes for one authenticated principal.
///
/// Construction is crate-internal: only the configured authenticated trust
/// adapter may seal scopes it has validated, so no external caller can mint
/// `ai.tool.approve` or any other approval scope (ADR-0003 TC-05).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ApprovalScopes {
    scopes: BTreeSet<String>,
}

impl ApprovalScopes {
    /// Wraps scopes the configured C-7 boundary has already validated.
    pub(crate) fn from_validated<I, S>(scopes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            scopes: scopes.into_iter().map(Into::into).collect(),
        }
    }

    /// Reports whether the validated identity carries one exact scope.
    #[must_use]
    pub fn contains(&self, scope: &str) -> bool {
        self.scopes.contains(scope)
    }
}

/// Immutable identity information supplied by the configured trust boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustContext {
    /// Tenant that owns all history touched by this request.
    pub tenant_id: TenantId,
    /// Authenticated subject within the tenant.
    pub subject_id: String,
    /// C-7-validated approval scopes; empty until the authenticated adapter
    /// supplies them, so an unscoped principal can never resolve an approval.
    approval_scopes: ApprovalScopes,
}

impl TrustContext {
    /// Creates a trust context from already validated identity components.
    ///
    /// The returned context carries no approval scopes; use
    /// [`TrustContext::with_approval_scopes`] only with scopes the configured
    /// C-7 boundary has already validated.
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
                approval_scopes: ApprovalScopes::default(),
            })
        }
    }

    /// Returns a copy of this context carrying already-validated scopes.
    ///
    /// Only the sealed [`ApprovalScopes`] capability can enter this method, so
    /// request, Tool, and MCP content can never attach approval scope.
    #[must_use]
    pub fn with_approval_scopes(mut self, scopes: ApprovalScopes) -> Self {
        self.approval_scopes = scopes;
        self
    }

    /// Reports whether the validated identity carries one exact approval scope.
    #[must_use]
    pub fn has_approval_scope(&self, scope: &str) -> bool {
        self.approval_scopes.contains(scope)
    }
}

macro_rules! uuid_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
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

    /// Reconstructs a non-zero generation read from canonical storage.
    #[must_use]
    pub const fn from_persisted(value: u64) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
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

    /// Returns the combined counters of two requests with checked overflow.
    ///
    /// Each continuation request of one Turn reports its own counters, so the
    /// Turn terminal must carry their sum (ADR-0003 TC-11).
    ///
    /// # Errors
    ///
    /// Returns [`DomainValueError::UsageOverflow`] when any counter sum
    /// overflows.
    pub fn checked_accumulate(&self, other: &Self) -> Result<Self, DomainValueError> {
        Self::new(
            self.input_tokens
                .checked_add(other.input_tokens)
                .ok_or(DomainValueError::UsageOverflow)?,
            self.output_tokens
                .checked_add(other.output_tokens)
                .ok_or(DomainValueError::UsageOverflow)?,
        )
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
    /// Append-only D-3 view of one canonical D-6 approval status (ADR-0003
    /// TC-06). Carries canonical identity and version; never authority.
    ApprovalStatus {
        /// Canonical D-6 identity.
        approval_id: execution::ApprovalId,
        /// Exact D-7 identity bound by the canonical D-6 record.
        attempt_id: execution::AttemptId,
        /// Canonical status at this version.
        status: execution::ApprovalStatus,
        /// Canonical decision, or `None` while requested or expired.
        decision: Option<execution::ApprovalDecision>,
        /// Canonical D-6 record version.
        version: u64,
    },
    /// Append-only D-3 view of one model-originated Tool call (ADR-0003
    /// TC-06). A projection of the requested action and its canonical D-7
    /// dispatch transition; never authority.
    ToolCall {
        /// Descriptor identity the call addressed.
        descriptor_id: String,
        /// Descriptor version the call addressed.
        descriptor_version: String,
        /// Exact target the call addressed.
        target: String,
        /// Canonical D-7 identity of the dispatch view, or `None` when
        /// policy denied before any D-7 existed.
        attempt_id: Option<execution::AttemptId>,
        /// Canonical D-7 lifecycle phase of the dispatch view, or `None`
        /// for a pre-D-7 denial record.
        status: Option<execution::ExecutionStatus>,
        /// Canonical D-7 transition version of the dispatch view, or `None`
        /// for a pre-D-7 denial record.
        version: Option<u64>,
    },
    /// Append-only D-3 view of one tool-execution terminal (ADR-0003
    /// TC-06). Carries canonical identity, transition version, and bounded
    /// metadata only.
    ToolResult {
        /// Canonical D-7 identity, or `None` when policy denied before any
        /// D-7 existed.
        attempt_id: Option<execution::AttemptId>,
        /// Canonical D-7 lifecycle status of the terminal.
        status: execution::ExecutionStatus,
        /// Stable failure or denial code, or `None` for a success, timeout,
        /// or cancellation.
        code: Option<String>,
        /// Executor-observed effect-state evidence.
        effect_state: Option<ToolEffectState>,
        /// Serialized size of the bounded executor output.
        output_bytes: u64,
        /// SHA-256 digest of a successful model-bound output, or `None` for
        /// failure, timeout, cancellation, and pre-D-7 denial records.
        output_digest: Option<String>,
        /// Canonical D-7 terminal transition version, or `None` for a
        /// pre-D-7 denial record.
        version: Option<u64>,
    },
    /// Exactly one terminal outcome for the turn.
    Terminal(TerminalOutcome),
    /// One typed correction of one earlier Item in the same Turn: replacement
    /// content plus the corrected predecessor identity (ADR-0003 CR-01).
    /// Raw replay keeps the original and this correction side by side;
    /// admission is owned by a later candidate, so no caller can append one.
    Correction(item_correction::ItemCorrection),
}

/// Executor-observed effect-state evidence mirrored into D-3 views.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolEffectState {
    /// The executor proved the effect never started.
    NotStarted,
    /// The executor confirmed the effect started.
    Started,
    /// The executor could not determine whether the effect started.
    Unknown,
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
