// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Consumer-owned commands, results, and external I/O ports.

use thiserror::Error;

use crate::domain::{
    Item, ItemPayload, LeaseGeneration, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId,
    TurnStatus, TurnTransitionError, Usage,
};

/// A validated request to execute one foreground, tool-free turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TurnCommand {
    /// Immutable validated caller identity.
    pub trust: TrustContext,
    /// Existing thread to resume, or `None` to allocate a new thread.
    pub thread_id: Option<ThreadId>,
    /// Non-empty plain-text input.
    pub input: String,
}

impl TurnCommand {
    /// Creates a command after enforcing the application input invariant.
    ///
    /// # Errors
    ///
    /// Returns [`TurnCommandError`] when input is empty or exceeds 65,536 bytes.
    pub fn new(
        trust: TrustContext,
        thread_id: Option<ThreadId>,
        input: impl Into<String>,
    ) -> Result<Self, TurnCommandError> {
        let input = input.into();
        if input.is_empty() {
            return Err(TurnCommandError::EmptyInput);
        }
        if input.len() > 65_536 {
            return Err(TurnCommandError::InputTooLarge);
        }
        Ok(Self {
            trust,
            thread_id,
            input,
        })
    }
}

/// A rejected turn command.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum TurnCommandError {
    /// Input contained no bytes.
    #[error("turn input must not be empty")]
    EmptyInput,
    /// Input exceeded the owned v1 byte limit.
    #[error("turn input exceeds 65536 bytes")]
    InputTooLarge,
}

/// Durable identity allocated by the initial history transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptedTurn {
    /// Tenant that owns the complete durable Turn key.
    pub tenant_id: TenantId,
    /// AI-owned thread identity.
    pub thread_id: ThreadId,
    /// New immutable turn identity.
    pub turn_id: TurnId,
    /// Initial foreground lease generation.
    pub generation: LeaseGeneration,
    /// Durable input item committed with the turn and lease.
    pub input: Item,
}

impl AcceptedTurn {
    /// Creates the result of an atomic initial history acceptance.
    #[must_use]
    pub const fn new(
        tenant_id: TenantId,
        thread_id: ThreadId,
        turn_id: TurnId,
        generation: LeaseGeneration,
        input: Item,
    ) -> Self {
        Self {
            tenant_id,
            thread_id,
            turn_id,
            generation,
            input,
        }
    }
}

/// Provider-neutral input including durable prior context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelInput {
    /// Tenant that owns the request and history.
    pub tenant_id: TenantId,
    /// Thread receiving the new immutable turn.
    pub thread_id: ThreadId,
    /// Current turn identity.
    pub turn_id: TurnId,
    /// Current plain-text user input.
    pub input: String,
    /// Durable prior Thread history supplied exactly once for resume.
    pub history: Vec<Item>,
}

/// One owned event produced by a model provider adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderEvent {
    /// One non-empty provider output delta.
    Delta(String),
    /// Final provider usage counters.
    Usage(Usage),
    /// Successful provider completion.
    Completed,
    /// Terminal provider failure with a stable owned code.
    Error { code: String },
    /// No provider frame is ready yet; orchestration may poll control state.
    Pending,
}

/// A lazy owned provider stream that can be dropped to stop consumption.
pub type ProviderStream<'a> = Box<dyn Iterator<Item = ProviderEvent> + 'a>;

/// A provider setup or protocol failure before an owned terminal event exists.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("provider unavailable: {code}")]
pub struct ProviderError {
    /// Stable provider-neutral failure code.
    pub code: String,
}

/// Consumer-owned boundary for model execution.
pub trait ModelProvider {
    /// Starts a lazy stream of owned provider events.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError`] when transport or protocol setup fails.
    fn stream(&mut self, input: ModelInput) -> Result<ProviderStream<'_>, ProviderError>;
}

/// An item request whose sequence and identity must be allocated durably.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NewItem {
    /// One provider-neutral model delta.
    AgentMessageDelta { content: String },
    /// Provider usage observed before terminal completion.
    Usage(Usage),
    /// Exactly one terminal outcome.
    Terminal(TerminalOutcome),
}

impl NewItem {
    /// Converts the application append request into owned domain content.
    #[must_use]
    pub fn into_payload(self) -> ItemPayload {
        match self {
            Self::AgentMessageDelta { content } => ItemPayload::AgentMessageDelta { content },
            Self::Usage(usage) => ItemPayload::Usage(usage),
            Self::Terminal(outcome) => ItemPayload::Terminal(outcome),
        }
    }
}

/// A typed canonical-history failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum HistoryError {
    /// The durable store did not complete the operation within its availability contract.
    #[error("durability unavailable")]
    Unavailable,
    /// The caller no longer owns the expected lease generation.
    #[error("turn owner fenced")]
    Fenced,
    /// A terminal outcome already exists for the turn.
    #[error("turn already terminal")]
    AlreadyTerminal,
    /// The tenant-scoped thread or turn does not exist.
    #[error("turn not found")]
    NotFound,
}

/// Result of transferring active-turn liveness ownership into recovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryHandoff {
    /// Liveness released its resources; the history port must schedule recovery.
    Released,
    /// Liveness transferred its reservation and completed owned recovery work.
    Recovered,
}

/// An active-turn resource whose drop stops its liveness maintenance.
pub trait TurnLiveness: Send {
    /// Stops liveness and transfers or releases adapter-owned recovery capacity.
    ///
    /// The default implementation consumes and drops the resource, leaving the
    /// history port to schedule recovery. Adapters whose worker owns admission
    /// may override this operation and move the reservation into recovery.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when an owned reservation cannot be transferred.
    fn handoff_to_recovery(self: Box<Self>) -> Result<RecoveryHandoff, HistoryError> {
        Ok(RecoveryHandoff::Released)
    }
}

struct NoopTurnLiveness;

impl TurnLiveness for NoopTurnLiveness {}

/// Consumer-owned canonical Thread/Turn/Item history boundary.
pub trait TurnHistory {
    /// Starts any adapter-owned liveness maintenance required after acceptance.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when required liveness maintenance cannot start.
    fn start_turn_liveness(
        &self,
        _turn: &AcceptedTurn,
    ) -> Result<Box<dyn TurnLiveness>, HistoryError> {
        Ok(Box::new(NoopTurnLiveness))
    }

    /// Records an authenticated interrupt request for an active tenant-owned turn.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError::NotFound`] for unknown or non-owned turns and
    /// [`HistoryError::AlreadyTerminal`] for a terminal turn.
    fn request_interrupt(
        &mut self,
        trust: &TrustContext,
        turn_id: TurnId,
    ) -> Result<(), HistoryError>;

    /// Reports whether the accepted turn has a durable interrupt request.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when history is unavailable or ownership is invalid.
    fn interruption_requested(&self, turn: &AcceptedTurn) -> Result<bool, HistoryError>;

    /// Atomically chooses `interrupted` over any provider terminal when requested.
    ///
    /// Deterministic adapters may implement this as a flag read followed by an
    /// append. Concurrent durable adapters must arbitrate under the same lock or
    /// transaction that commits the terminal Item.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when history is unavailable or ownership is invalid.
    fn append_provider_terminal(
        &mut self,
        turn: &AcceptedTurn,
        outcome: TerminalOutcome,
    ) -> Result<Item, HistoryError> {
        let outcome = if self.interruption_requested(turn)? {
            TerminalOutcome::Interrupted
        } else {
            outcome
        };
        self.append(turn, NewItem::Terminal(outcome))
    }

    /// Appends provider completion through the shared terminal arbitration operation.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when history is unavailable or ownership is invalid.
    fn append_completion(
        &mut self,
        turn: &AcceptedTurn,
        usage: Usage,
    ) -> Result<Item, HistoryError> {
        self.append_provider_terminal(turn, TerminalOutcome::Completed { usage })
    }

    /// Reads prior durable items for a subject-owned Thread in canonical order.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when history is unavailable or ownership is invalid.
    fn prior_thread_items(
        &self,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> Result<Vec<Item>, HistoryError>;

    /// Starts conditional failed-terminal recovery after an accepted append outage.
    ///
    /// The production adapter retains ownership asynchronously until it either
    /// appends `failed` or the lease generation is fenced. Deterministic adapters
    /// may close the turn synchronously.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when recovery ownership cannot be established.
    fn schedule_failed_recovery(&mut self, turn: &AcceptedTurn) -> Result<(), HistoryError> {
        self.append(
            turn,
            NewItem::Terminal(TerminalOutcome::Failed {
                code: "DURABILITY_UNAVAILABLE".to_owned(),
            }),
        )?;
        Ok(())
    }

    /// Atomically persists initial Thread, Turn, input Item, and lease state.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when initial durable acceptance fails.
    fn accept_initial(&mut self, command: &TurnCommand) -> Result<AcceptedTurn, HistoryError>;

    /// Appends exactly one item under the accepted lease generation.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when durability is unavailable or ownership is invalid.
    fn append(&mut self, turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError>;

    /// Reads the tenant-scoped durable items in increasing sequence order.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when the turn is missing or history is unavailable.
    fn replay(&self, tenant_id: &TenantId, turn_id: TurnId) -> Result<Vec<Item>, HistoryError>;
}

/// One durable-before-visible event emitted while a turn executes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TurnStreamEvent {
    /// Initial input and ownership were durably accepted.
    Started {
        /// Durable Thread identity allocated or resumed at acceptance.
        thread_id: ThreadId,
        /// Durable Turn identity allocated at acceptance.
        turn_id: TurnId,
    },
    /// One provider-visible item was durably appended.
    Item {
        /// Durable Thread identity for presentation routing.
        thread_id: ThreadId,
        /// Durable Turn identity for presentation routing.
        turn_id: TurnId,
        /// The durably appended item.
        item: Item,
    },
}

/// The observable result of one application turn execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TurnResult {
    /// Durable thread identity.
    pub thread_id: ThreadId,
    /// New immutable turn identity.
    pub turn_id: TurnId,
    /// Final owned lifecycle status.
    pub status: TurnStatus,
    /// Items published only after their successful durable append.
    pub published: Vec<Item>,
    /// Canonical ordered durable replay captured after terminal append.
    pub replay: Vec<Item>,
}

/// An orchestration failure before a normal owned terminal result can be returned.
#[derive(Debug, Error)]
pub enum TurnRunError {
    /// Initial provider setup failed.
    #[error(transparent)]
    Provider(#[from] ProviderError),
    /// Canonical durability failed, with only the committed visible prefix retained.
    #[error(transparent)]
    Durability(DurabilityFailure),
    /// Canonical history rejected an operation.
    #[error(transparent)]
    History(#[from] HistoryError),
    /// Internal lifecycle code attempted an invalid state transition.
    #[error(transparent)]
    Transition(#[from] TurnTransitionError),
}

/// Context retained when canonical history becomes unavailable.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("durability unavailable")]
pub struct DurabilityFailure {
    /// Whether the initial Turn/input/lease transaction had already committed.
    pub accepted: bool,
    /// Items published only after successful append before the outage.
    pub published: Vec<Item>,
    /// Typed underlying history result.
    pub source: HistoryError,
}
