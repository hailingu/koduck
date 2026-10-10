// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md
// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md

//! Consumer-owned commands, results, and external I/O ports.

use thiserror::Error;

mod tool_calls;

pub use tool_calls::{
    CommittedToolCall, ModelToolCall, ModelToolResult, NoToolExecution, ToolCallExecutor,
    ToolCallTurnContext, ToolRound,
};

use crate::domain::{
    Item, ItemPayload, LeaseGeneration, SubmissionId, TenantId, TerminalOutcome, ThreadId,
    TrustContext, TurnId, TurnStatus, TurnTransitionError, Usage,
};

use super::provider_context::{PriorTurnHistory, ProviderContextError, ProviderHistoryItem};

/// A validated request to execute one foreground, tool-free turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TurnCommand {
    /// Immutable validated caller identity.
    pub trust: TrustContext,
    /// Existing thread to resume, or `None` to allocate a new thread.
    pub thread_id: Option<ThreadId>,
    /// Non-empty plain-text input.
    pub input: String,
    /// Optional durable client submission identity (ADR-0018 SI-01a).
    ///
    /// A present value opts this request into subject-scoped deduplication:
    /// the exact repeated submission returns its acceptance receipt instead
    /// of creating another Turn. Omission retains the existing fresh-submission
    /// behavior and writes no binding.
    pub submission_id: Option<SubmissionId>,
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
            submission_id: None,
        })
    }

    /// Attaches an already validated non-nil submission identity.
    ///
    /// # Errors
    ///
    /// Returns [`TurnCommandError`] when the supplied value is nil, so a nil
    /// identity cannot ride into the acceptance boundary (SI-01a).
    pub fn with_submission_id(
        mut self,
        submission_id: SubmissionId,
    ) -> Result<Self, TurnCommandError> {
        if submission_id.as_uuid().is_nil() {
            return Err(TurnCommandError::NilSubmission);
        }
        self.submission_id = Some(submission_id);
        Ok(self)
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
    /// A nil UUID was supplied as the submission identity (SI-01a).
    #[error("submission id must not be nil")]
    NilSubmission,
    /// The identified acceptance port received no submission identity.
    #[error("identified acceptance requires a submission id")]
    MissingSubmission,
    /// An identified command reached a legacy execution entry point that
    /// cannot carry submission semantics (ADR-0018 SI-01d).
    #[error("submission id requires the submission execution entry")]
    SubmissionOnLegacyEntry,
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
    /// Owned effective prior-history view prepared once before acceptance
    /// from the scoped canonical read and reused unchanged by every
    /// continuation of this Turn (ADR-0006 PC-02/PC-04).
    pub history: Vec<ProviderHistoryItem>,
    /// Serviced Tool rounds whose committed results a continuation request
    /// carries; empty for the initial request of a Turn.
    ///
    /// Each element is one provider stream's Tool-call batch. The runner
    /// starts a continuation request only after the C-5 boundary durably
    /// committed each carried result in the current lease generation
    /// (ADR-0003 TC-11), and the provider adapter serializes the rounds in
    /// order as alternating assistant-call/result groups after the user
    /// message, preserving the causal order of the committed interaction.
    pub tool_rounds: Vec<ToolRound>,
}

/// One owned event produced by a model provider adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderEvent {
    /// One non-empty provider output delta.
    Delta(String),
    /// One fully assembled model-originated Tool call.
    ///
    /// The provider adapter assembles streamed fragments into complete calls
    /// before emitting this event; `name` and `arguments` are untrusted
    /// provider content and never authority (ADR-0003 TC-02/TC-11).
    ToolCall {
        /// Declared tool name exactly as the provider delivered it.
        name: String,
        /// Serialized arguments exactly as the provider delivered them.
        arguments: String,
    },
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
    /// Append-only D-3 view of one canonical D-6 approval status.
    ApprovalStatus {
        approval_id: crate::domain::execution::ApprovalId,
        attempt_id: crate::domain::execution::AttemptId,
        status: crate::domain::execution::ApprovalStatus,
        decision: Option<crate::domain::execution::ApprovalDecision>,
        version: u64,
    },
    /// Append-only D-3 view of one model-originated Tool call.
    ToolCall {
        descriptor_id: String,
        descriptor_version: String,
        target: String,
        attempt_id: Option<crate::domain::execution::AttemptId>,
        status: Option<crate::domain::execution::ExecutionStatus>,
        version: Option<u64>,
    },
    /// Append-only D-3 view of one tool-execution terminal.
    ToolResult {
        attempt_id: Option<crate::domain::execution::AttemptId>,
        status: crate::domain::execution::ExecutionStatus,
        code: Option<String>,
        effect_state: Option<crate::domain::ToolEffectState>,
        output_bytes: u64,
        output_digest: Option<String>,
        version: Option<u64>,
    },
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
            Self::ApprovalStatus {
                approval_id,
                attempt_id,
                status,
                decision,
                version,
            } => ItemPayload::ApprovalStatus {
                approval_id,
                attempt_id,
                status,
                decision,
                version,
            },
            Self::ToolCall {
                descriptor_id,
                descriptor_version,
                target,
                attempt_id,
                status,
                version,
            } => ItemPayload::ToolCall {
                descriptor_id,
                descriptor_version,
                target,
                attempt_id,
                status,
                version,
            },
            Self::ToolResult {
                attempt_id,
                status,
                code,
                effect_state,
                output_bytes,
                output_digest,
                version,
            } => ItemPayload::ToolResult {
                attempt_id,
                status,
                code,
                effect_state,
                output_bytes,
                output_digest,
                version,
            },
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
    /// Prior durable history exceeds the owned provider-context budget.
    #[error("thread history exceeds provider context budget")]
    ContextLimit,
    /// An owned submission key arrived with changed semantic input or a
    /// changed requested selector (ADR-0018 SI-02d).
    #[error("submission identity conflict")]
    SubmissionConflict,
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

    /// Records D-7 interruption terminals followed by the authenticated Turn
    /// interruption terminal as one ordered operation.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError::NotFound`] for unknown or non-owned turns and
    /// [`HistoryError::AlreadyTerminal`] for a terminal turn. Implementations
    /// must append none of the supplied D-7 items when the Turn terminal loses.
    fn request_interrupt(
        &mut self,
        trust: &TrustContext,
        turn_id: TurnId,
        tool_terminals: Vec<NewItem>,
    ) -> Result<(), HistoryError>;

    /// Resolves the authenticated Turn's Thread for a paired C-5 interruption.
    ///
    /// History adapters that do not host a C-5 execution boundary return
    /// `None`, preserving their canonical history-only interruption behavior.
    /// Production adapters return the tenant- and subject-owned Thread so the
    /// runner can cancel live D-7 work before recording the Turn terminal.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when the authenticated ownership lookup cannot
    /// complete.
    fn interruption_thread(
        &self,
        _trust: &TrustContext,
        _turn_id: TurnId,
    ) -> Result<Option<ThreadId>, HistoryError> {
        Ok(None)
    }

    /// Reports whether the accepted turn has a durable interrupt request.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when history is unavailable, ownership is invalid,
    /// or the canonical provider context exceeds its aggregate budget.
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

    /// Reads the complete prior source-Turn groups of a subject-owned Thread
    /// in canonical order, retaining each row's Item and reported provenance
    /// (ADR-0006 PC-01).
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when history is unavailable, ownership is
    /// invalid, or the raw history exceeds its aggregate budget.
    fn prior_thread_turns(
        &self,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> Result<Vec<PriorTurnHistory>, HistoryError>;

    /// Reads the bounded prior history within one caller-owned deadline that
    /// the identified acceptance flow clamps to its remaining acceptance
    /// time (ADR-0018 SI-07c).
    ///
    /// The default fails closed: an adapter that has not implemented a
    /// deadline-aware read must never let the identified flow block past its
    /// acceptance budget through an unbounded delegation.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError::Unavailable`] from the default and
    /// [`HistoryError`] from implementing adapters when the bounded read
    /// fails.
    fn prior_thread_turns_bounded(
        &self,
        _trust: &TrustContext,
        _thread_id: ThreadId,
        _deadline: std::time::Duration,
    ) -> Result<Vec<PriorTurnHistory>, HistoryError> {
        Err(HistoryError::Unavailable)
    }

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

    /// Reads one owned submission binding without taking any lock, observing
    /// only committed state (ADR-0018 SI-07a).
    ///
    /// The identified command carries the owned key and the candidate input:
    /// the lookup validates the joined Thread owner, Turn, and original
    /// sequence-1 user input against them. A genuinely absent key returns
    /// `Ok(None)`; a present binding whose joined canonical data is missing or
    /// inconsistent fails unavailable instead of establishing absence
    /// (SI-08c), and a present binding with changed original input or
    /// selector is the typed [`HistoryError::SubmissionConflict`] (SI-02b,
    /// SI-02d).
    ///
    /// `deadline` bounds the whole read including pool and lock waits, already
    /// clamped by the caller to its remaining acceptance time (SI-07c).
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] for unavailability, corrupt structure, or the
    /// typed drift conflict.
    fn submission_observation(
        &self,
        command: &TurnCommand,
        _deadline: std::time::Duration,
    ) -> Result<Option<super::submission::SubmissionObservation>, HistoryError> {
        // SI-01d: every direct observation entry independently rejects an
        // invalid identified command before reporting absence; a history
        // adapter without submission bindings then reports genuine absence,
        // and the identified acceptance below still fails closed.
        super::submission::validate_identified_command(command)
            .map_err(|_| HistoryError::Unavailable)?;
        Ok(None)
    }

    /// Atomically rechecks and accepts one identified submission: the binding
    /// key, original selector, accepted Thread/Turn, and a server-private
    /// creator-operation identity commit in the same transaction as the
    /// initial Turn, sequence-1 input, and generation-1 lease (ADR-0018
    /// SI-03a).
    ///
    /// The full owned key is rechecked under the submission lock before any
    /// canonical state is allocated: an exact existing binding returns
    /// [`super::submission::IdentifiedAcceptance::Existing`] with no canonical
    /// mutation, a drifted key returns [`HistoryError::SubmissionConflict`],
    /// and only the winning invocation receives `Created`.
    ///
    /// `deadline` bounds this single write attempt at its full reserved
    /// budget; the implementation owes the one permitted read-only
    /// reconciliation its own identical full budget (SI-06b, SI-07c).
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when the attempt cannot commit or the key
    /// drifted.
    fn accept_initial_with_submission(
        &mut self,
        _command: &TurnCommand,
        _deadline: std::time::Duration,
    ) -> Result<super::submission::IdentifiedAcceptance, HistoryError> {
        // Fail closed: an adapter without an identified-acceptance
        // implementation returns unavailability instead of silently ignoring
        // the supplied key (SI-01d).
        Err(HistoryError::Unavailable)
    }

    /// Appends exactly one item under the accepted lease generation.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when durability is unavailable or ownership is invalid.
    fn append(&mut self, turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError>;

    /// Atomically appends every D-3 item emitted for one Tool projection.
    ///
    /// Implementations MUST either append the complete sequence in order or
    /// append none of it. The default denies multi-item projections so an
    /// adapter cannot silently downgrade this contract to per-item appends.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when the complete sequence cannot be made
    /// durable under the accepted lease generation.
    fn append_tool_projection(
        &mut self,
        turn: &AcceptedTurn,
        items: Vec<NewItem>,
    ) -> Result<Vec<Item>, HistoryError> {
        if items.len() != 1 {
            return Err(HistoryError::Unavailable);
        }
        let item = items
            .into_iter()
            .next()
            .expect("one checked projection item exists");
        self.append(turn, item).map(|durable| vec![durable])
    }

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
    /// The Turn exceeded its exact 512-Item or 1-MiB durable output budget
    /// and durably closed as `failed` with `RESOURCE_LIMIT_EXCEEDED`
    /// (ADR-0005 PLB-5/PLB-7).
    #[error(transparent)]
    ResourceLimit(#[from] ResourceLimitFailure),
    /// Canonical history rejected an operation.
    #[error(transparent)]
    History(#[from] HistoryError),
    /// The prepared provider context was rejected before acceptance; the
    /// safe typed cause carries no source payload or identity values
    /// (ADR-0006 PC-07).
    #[error(transparent)]
    Context(#[from] ProviderContextError),
    /// Live C-5 work could not be terminalized for an authenticated interrupt.
    #[error(transparent)]
    Tool(#[from] super::ToolCallError),
    /// Internal lifecycle code attempted an invalid state transition.
    #[error(transparent)]
    Transition(#[from] TurnTransitionError),
    /// An identified creator's cancellation was observed before its
    /// acceptance write began (ADR-0018 SI-07g); no binding or Turn exists.
    #[error("turn cancelled before acceptance")]
    Cancelled,
    /// A directly constructed identified command violated the owned input or
    /// identity guard before any history operation (ADR-0018 SI-01d).
    #[error(transparent)]
    InvalidCommand(#[from] TurnCommandError),
}

/// Context retained when the Turn's exact durable output budget was exceeded.
///
/// The durable terminal has already committed when this failure surfaces;
/// synchronous delivery maps it to `422 resource-limit-exceeded` while a
/// started SSE stream has already emitted the exact durable `turn.failed`
/// terminal (ADR-0005 PLB-7).
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("turn resource limit exceeded")]
pub struct ResourceLimitFailure {
    /// Items published only after their successful durable append, ending
    /// with the durable `RESOURCE_LIMIT_EXCEEDED` terminal.
    pub published: Vec<Item>,
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
