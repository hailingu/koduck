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

use super::tool_projection::{ToolProjection, ToolProjectionSink};

/// One model-originated Tool call exactly as the provider delivered it.
///
/// `name` and `arguments` are untrusted provider content; they never carry
/// authority and are only resolved against configured descriptors by the
/// tool-execution boundary (ADR-0003 TC-02).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelToolCall {
    /// Declared tool name as delivered.
    pub name: String,
    /// Serialized arguments as delivered.
    pub arguments: String,
}

/// The model-bound view of one committed Tool-call result.
///
/// `content` is the bounded committed executor output, a stable
/// denial/failure summary when the call did not produce output, or the stable
/// non-UTF-8 summary bound by the projection sink to an opaque committed
/// success. It is delivered to the model only inside a continuation request
/// started after the current-generation durable result commit the C-5 boundary
/// proved, and it remains untrusted content there (ADR-0003 TC-11).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelToolResult {
    /// Bounded committed result content for the continuation request.
    pub content: String,
    /// Whether the call failed, was denied, or was unavailable.
    pub is_error: bool,
}

/// One serviced Tool call paired with its committed result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommittedToolCall {
    /// The model-originated call exactly as delivered (untrusted).
    pub call: ModelToolCall,
    /// The committed result carried into the continuation request.
    pub result: ModelToolResult,
}

/// One provider Tool-call round: every call the model raised in one stream,
/// each paired with its committed result.
///
/// Continuation requests carry rounds in order and the provider adapter
/// serializes them as alternating assistant-call/result groups, so a later
/// round raised on an earlier result is never rewritten as concurrent with
/// it (ADR-0003 TC-11).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolRound {
    /// Assistant text emitted in this stream before or alongside the Tool
    /// calls. It is retained with the call batch so the continuation can
    /// reconstruct the model's causal assistant message.
    pub assistant_content: String,
    /// The round's serviced calls in the order the model raised them.
    pub calls: Vec<CommittedToolCall>,
}

/// Turn-scoped identity context for one serviced Tool call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCallTurnContext {
    /// Tenant that owns the Turn.
    pub tenant_id: TenantId,
    /// Thread that owns the Turn.
    pub thread_id: ThreadId,
    /// Turn whose D-7 budget the call consumes.
    pub turn_id: TurnId,
    /// Foreground lease generation that must remain current.
    pub lease_generation: LeaseGeneration,
}

/// Consumer-owned boundary that services one model Tool call through C-5 and
/// returns the ordered append-only D-3 items to record for it plus the bounded
/// committed result the runner's continuation request carries.
///
/// The runner owns the durable append-before-publish ordering; the port owns
/// C-5 policy, approval, execution, and the D-3 projection contents. A typed
/// denial or unavailability is returned as recorded items, never as an error.
pub trait ToolCallExecutor {
    /// Services one Tool call and returns its D-3 items and committed result.
    ///
    /// # Errors
    ///
    /// Returns [`ToolCallError`] only for turn-level failures that own the
    /// turn terminal, such as canonical reconciliation or durability.
    fn execute_tool_call(
        &mut self,
        call: ModelToolCall,
        context: &ToolCallTurnContext,
        trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, super::ToolCallError>;

    /// Cancels live C-5 work and returns its canonical D-7 terminal items.
    ///
    /// The default is deliberately a no-op because configurations without a
    /// live C-5 boundary have no process-owned execution work to cancel. The
    /// production boundary overrides it to close catalogued D-7 attempts.
    ///
    /// # Errors
    ///
    /// Returns [`ToolCallError`] when live execution work cannot reach a
    /// canonical terminal and requires reconciliation. Returned items must be
    /// persisted before the Turn interruption terminal.
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _thread_id: ThreadId,
        _turn_id: TurnId,
    ) -> Result<Vec<NewItem>, super::ToolCallError> {
        Ok(Vec::new())
    }

    /// Notifies the boundary that one Turn's durable terminal committed.
    ///
    /// The default is deliberately a no-op because configurations without a
    /// live C-5 boundary retain no process-owned authority. The production
    /// boundary overrides it to reclaim its process-local Turn authority
    /// against the proven canonical terminal; reclamation is hygiene, so an
    /// unproven probe retains the authority instead of surfacing an error.
    fn turn_terminal_committed(
        &mut self,
        _tenant_id: &TenantId,
        _thread_id: ThreadId,
        _turn_id: TurnId,
    ) {
    }
}

/// Explicit unconfigured tool-execution boundary.
///
/// Every call is recorded as a typed unavailability without any execution,
/// caching, or fallback path (ADR-0003 TC-13).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoToolExecution;

impl ToolCallExecutor for NoToolExecution {
    fn execute_tool_call(
        &mut self,
        call: ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, super::ToolCallError> {
        let descriptor_id = if crate::domain::tool::validate_descriptor_id(&call.name).is_ok() {
            call.name
        } else {
            String::new()
        };
        // The unconfigured boundary is recorded through the same durable
        // projection sink as every other outcome (ADR-0003 TC-13).
        crate::application::tool_projection::emit(
            projections,
            ToolProjection::Denied {
                descriptor_id,
                descriptor_version: String::new(),
                target: String::new(),
                code: "tool_execution_unavailable".to_owned(),
            },
        );
        Ok(ModelToolResult {
            content: "tool_execution_unavailable".to_owned(),
            is_error: true,
        })
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
    /// Canonical history rejected an operation.
    #[error(transparent)]
    History(#[from] HistoryError),
    /// Live C-5 work could not be terminalized for an authenticated interrupt.
    #[error(transparent)]
    Tool(#[from] super::ToolCallError),
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
