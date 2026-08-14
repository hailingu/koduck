// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! C-5 coordination around the isolated one-attempt executor port.

use std::sync::Arc;

use crate::domain::execution::{
    ApprovalDecision, ApprovalError, ApprovalRequest, ExactActionBinding, ExecutionAttempt,
    ExecutionError, ExecutionStatus, TurnAuthorityCatalog, TurnExecutionAuthority,
};
use crate::domain::{ThreadId, TrustContext};

use super::cancellation::{CancelAcknowledgement, CancelPermit};
use super::deadline::{ActionDeadline, MAX_ACTION_DURATION_MILLIS};
use super::terminal::TerminalReservationFailure;

/// Maximum buffered byte size for one isolated executor response.
pub const MAX_EXECUTOR_OUTPUT_BYTES: usize = 1_048_576;

/// Executor-observed state of an external effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectState {
    /// The executor proves that no effect started.
    NotStarted,
    /// The executor observed that the effect started.
    Started,
    /// The executor cannot prove whether the effect started.
    Unknown,
}

/// A stable failure emitted by the C-5 execution boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionFailure {
    /// The configured isolated executor is unavailable.
    ExecutorUnavailable,
    /// The owner was fenced before executor dispatch and no terminal write won.
    OwnerFencedBeforeDispatch,
    /// The owner was fenced after dispatch and no result may reach the model.
    OwnerFencedAfterDispatch,
    /// The isolated result exceeded 1,048,576 serialized bytes.
    OutputLimitExceeded,
    /// D-6 does not authorize this exact binding.
    ApprovalMismatch,
    /// The canonical D-7 already claimed its only dispatch.
    ApprovalAlreadyConsumed,
    /// An authenticated interruption sealed the Turn before the dispatch claim.
    InterruptionRequested,
    /// The canonical result could not be committed durably.
    DurabilityUnavailable,
    /// A different canonical terminal won the conditional commit race.
    TerminalConflict,
    /// Another D-7 owns this Turn's single running slot.
    ConcurrentAttempt,
    /// The Turn's 16-slot D-7 attempt budget is exhausted.
    AttemptLimit,
    /// The addressed D-7 is not running, so no bounded cancellation exists.
    AttemptNotRunning,
}

/// Executor failure paired with truthful external-effect evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutorError {
    code: ExecutionFailure,
    effect_state: EffectState,
}

impl ExecutorError {
    /// Creates a failure whose effect state was observed by the executor boundary.
    #[must_use]
    pub const fn new(code: ExecutionFailure, effect_state: EffectState) -> Self {
        Self { code, effect_state }
    }
}

/// One bounded response from the isolated executor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionResponse {
    effect_state: EffectState,
    output: Vec<u8>,
}

impl ExecutionResponse {
    /// Returns executor evidence about whether the external effect started.
    #[must_use]
    pub const fn effect_state(&self) -> EffectState {
        self.effect_state
    }

    /// Returns the opaque bounded output for conditional durable commit.
    #[must_use]
    pub fn output(&self) -> &[u8] {
        &self.output
    }
}

/// Incremental constructor that enforces the executor output cap before buffering.
#[derive(Debug)]
pub struct ExecutionResponseBuilder {
    effect_state: EffectState,
    output: Vec<u8>,
    overflowed: bool,
}

impl ExecutionResponseBuilder {
    /// Starts an empty response with the executor's observed effect state.
    #[must_use]
    pub const fn new(effect_state: EffectState) -> Self {
        Self {
            effect_state,
            output: Vec::new(),
            overflowed: false,
        }
    }

    /// Appends one transport chunk without allowing the buffer to exceed 1,048,576 bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionFailure::OutputLimitExceeded`] before appending any chunk that
    /// would cross the response limit.
    pub fn push_chunk(&mut self, chunk: &[u8]) -> Result<(), ExecutorError> {
        if self.overflowed
            || chunk.len() > MAX_EXECUTOR_OUTPUT_BYTES.saturating_sub(self.output.len())
        {
            self.overflowed = true;
            return Err(ExecutorError::new(
                ExecutionFailure::OutputLimitExceeded,
                self.effect_state,
            ));
        }
        self.output.extend_from_slice(chunk);
        Ok(())
    }

    /// Finishes the already bounded response for coordinator commitment.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionFailure::OutputLimitExceeded`] when any prior chunk
    /// crossed the response limit, even if the caller ignored that append error.
    pub fn finish(self) -> Result<ExecutionResponse, ExecutorError> {
        if self.overflowed {
            return Err(ExecutorError::new(
                ExecutionFailure::OutputLimitExceeded,
                self.effect_state,
            ));
        }
        Ok(ExecutionResponse {
            effect_state: self.effect_state,
            output: self.output,
        })
    }
}

/// Final application outcome after lease, bounds, and durable-commit validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolExecutionOutcome {
    /// A current owner durably committed this still-untrusted output.
    Succeeded {
        /// Opaque executor output that remains untrusted.
        output: Vec<u8>,
        /// Executor evidence retained for the canonical terminal and audit record.
        effect_state: EffectState,
    },
    /// No result is delivered because policy or ownership cancelled the attempt.
    Cancelled { effect_state: EffectState },
    /// The 30-second action deadline elapsed before a bounded terminal commit.
    TimedOut {
        /// Best executor evidence about whether an effect started.
        effect_state: EffectState,
    },
    /// Execution failed with truthful effect-state evidence.
    Failed {
        /// Stable owned failure code.
        code: ExecutionFailure,
        /// Best executor evidence about whether an effect started.
        effect_state: EffectState,
    },
}

impl ToolExecutionOutcome {
    pub(crate) const fn effect_state(&self) -> EffectState {
        match self {
            Self::Succeeded { effect_state, .. }
            | Self::Cancelled { effect_state }
            | Self::TimedOut { effect_state }
            | Self::Failed { effect_state, .. } => *effect_state,
        }
    }

    pub(super) const fn status(&self) -> ExecutionStatus {
        match self {
            Self::Succeeded { .. } => ExecutionStatus::Succeeded,
            Self::Cancelled { .. } => ExecutionStatus::Cancelled,
            Self::TimedOut { .. } => ExecutionStatus::TimedOut,
            Self::Failed { .. } => ExecutionStatus::Failed,
        }
    }

    fn is_bounded(&self) -> bool {
        match self {
            Self::Succeeded { output, .. } => output.len() <= MAX_EXECUTOR_OUTPUT_BYTES,
            Self::Cancelled { .. } | Self::TimedOut { .. } | Self::Failed { .. } => true,
        }
    }
}

/// An execution request that cannot return a canonical final Tool outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionPending {
    /// The canonical dispatch claim was rejected before executor dispatch.
    DispatchRejected {
        /// Stable rejection code that must not be exposed as a terminal Tool result.
        code: ExecutionFailure,
    },
    /// No terminal is reported because the conditional durable write did not win.
    ReconciliationRequired {
        /// Stable reason reconciliation owns the next transition.
        code: ExecutionFailure,
        /// Best executor evidence retained for the reconciler.
        effect_state: EffectState,
    },
}

/// Consumer-owned port for a separately isolated Tool service or worker.
pub trait IsolatedExecutor {
    /// Dispatches one bounded owned D-7 envelope.
    ///
    /// # Errors
    ///
    /// Returns a stable failure without selecting a legacy or direct fallback.
    fn execute(
        &mut self,
        permit: &DispatchPermit,
        binding: &ExactActionBinding,
        deadline: ActionDeadline,
    ) -> Result<ExecutionResponse, ExecutorError>;

    /// Sends exactly one bounded cancellation for a running D-7.
    ///
    /// The implementation must stop waiting at `deadline` and return
    /// [`CancelAcknowledgement::NotAcknowledged`] when no acknowledgement has
    /// arrived by then. It returns [`CancelAcknowledgement::Unavailable`]
    /// immediately only when no cancellation boundary exists to perform that
    /// bounded wait; the coordinator then retains the D-7 for reconciliation.
    fn cancel(
        &mut self,
        permit: &CancelPermit,
        binding: &ExactActionBinding,
        deadline: ActionDeadline,
    ) -> CancelAcknowledgement;
}

/// Opaque single-call authority created only by [`ExecutionCoordinator`].
pub struct DispatchPermit {
    _private: (),
}

/// A conditional durable result-commit failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttemptCommitError {
    /// The bound foreground generation is no longer current.
    Fenced,
    /// Canonical D-7 storage is unavailable.
    Unavailable,
    /// A different canonical terminal already won this D-7 transition.
    Conflict,
}

/// A rejected terminal reconstructed from canonical persistence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CanonicalTerminalError {
    /// Canonical record versions start at one.
    InvalidVersion,
    /// Persisted output exceeds the executor/model boundary.
    OutputTooLarge,
}

/// One validated canonical D-7 terminal returned by the persistence boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalAttemptTerminal {
    binding: ExactActionBinding,
    version: u64,
    outcome: ToolExecutionOutcome,
}

impl CanonicalAttemptTerminal {
    /// Validates a terminal reconstructed by the canonical persistence adapter.
    ///
    /// # Errors
    ///
    /// Returns a typed error for a zero version or output beyond 1,048,576 bytes.
    pub fn from_persistence(
        binding: ExactActionBinding,
        version: u64,
        outcome: ToolExecutionOutcome,
    ) -> Result<Self, CanonicalTerminalError> {
        if version == 0 {
            return Err(CanonicalTerminalError::InvalidVersion);
        }
        if !outcome.is_bounded() {
            return Err(CanonicalTerminalError::OutputTooLarge);
        }
        Ok(Self {
            binding,
            version,
            outcome,
        })
    }

    /// Returns the exact D-7 binding read with this terminal.
    #[must_use]
    pub const fn binding(&self) -> &ExactActionBinding {
        &self.binding
    }

    /// Returns the monotonically increasing canonical record version.
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// Returns the validated bounded canonical outcome.
    #[must_use]
    pub const fn outcome(&self) -> &ToolExecutionOutcome {
        &self.outcome
    }
}

/// Result of a conditional canonical D-7 terminal commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttemptCommitResult {
    /// This caller won and durably wrote the supplied terminal.
    Won,
    /// An idempotent canonical terminal already existed and must be returned.
    Existing(Box<CanonicalAttemptTerminal>),
}

/// Consumer-owned port for conditional current-generation D-7 result commit.
pub trait AttemptCommitter {
    /// Durably commits one exact terminal if the bound canonical transition wins.
    ///
    /// # Errors
    ///
    /// Returns [`AttemptCommitError`] without making output visible when the
    /// generation is fenced or canonical storage is unavailable.
    fn commit_outcome(
        &mut self,
        binding: &ExactActionBinding,
        outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError>;
}

/// Consumer-owned port for C-6 foreground generation validation.
pub trait LeaseValidator {
    /// Reports whether every bound owner field still identifies the current owner.
    fn is_current(&mut self, binding: &ExactActionBinding) -> bool;
}

/// A rejected attempt preparation before any D-7 slot is consumed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionPreparationError {
    /// The exact binding no longer belongs to the current foreground generation.
    OwnerFenced,
    /// The Turn authority rejected the requested allocation.
    Rejected(ExecutionError),
}

/// Runtime-assembly-owned root for every live Turn execution authority.
///
/// Exactly one root is injected into runtime handles. T-3 replaces its
/// process-local catalog with canonical persistence.
#[derive(Debug, Default)]
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "T-2 runtime execution wiring is not complete")
)]
pub(crate) struct ToolExecutionAuthorityRoot {
    catalog: Arc<TurnAuthorityCatalog>,
}

#[cfg_attr(
    not(test),
    allow(dead_code, reason = "T-2 runtime execution wiring is not complete")
)]
impl ToolExecutionAuthorityRoot {
    /// Creates the authority root owned by runtime assembly.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

/// Runtime handle that shares the process-owned authority root across preparers.
#[derive(Clone, Debug)]
pub struct ToolExecutionRuntime {
    /// Shared authority catalog, also read by the interruption boundary.
    pub(super) catalog: Arc<TurnAuthorityCatalog>,
}

impl ToolExecutionRuntime {
    /// Creates a handle borrowing authority from runtime assembly's sole root.
    #[must_use]
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "T-2 runtime execution wiring is not complete")
    )]
    pub(crate) fn new(root: &ToolExecutionAuthorityRoot) -> Self {
        Self {
            catalog: Arc::clone(&root.catalog),
        }
    }

    /// Creates a lease-validating preparer backed by this runtime's shared catalog.
    #[must_use]
    pub fn preparer<L>(&self, lease: L) -> ExecutionPreparer<L>
    where
        L: LeaseValidator,
    {
        ExecutionPreparer {
            lease,
            catalog: Arc::clone(&self.catalog),
            authority: None,
        }
    }
}

/// Lease-validating preparation handle scoped to one runtime-owned Turn authority.
///
/// The first successful binding fixes this handle's Turn and profile identity.
/// Every process handle shares that authority. The process root strongly retains
/// it until T-3 can bind reclamation to canonical persistence without resurrection.
pub struct ExecutionPreparer<L> {
    lease: L,
    catalog: Arc<TurnAuthorityCatalog>,
    authority: Option<TurnExecutionAuthority>,
}

impl<L> ExecutionPreparer<L>
where
    L: LeaseValidator,
{
    /// Validates the current generation before allocating one D-7 slot.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionPreparationError::OwnerFenced`] without allocation when
    /// the binding is stale, or the underlying authority allocation failure.
    pub fn prepare(
        &mut self,
        binding: ExactActionBinding,
    ) -> Result<(TurnExecutionAuthority, ExecutionAttempt), ExecutionPreparationError> {
        if binding.approval_requirement().is_none() {
            return Err(ExecutionPreparationError::Rejected(
                ExecutionError::PolicyAuthorizationRequired,
            ));
        }
        if !self.lease.is_current(&binding) {
            return Err(ExecutionPreparationError::OwnerFenced);
        }
        let authority = self
            .authority
            .get_or_insert_with(|| self.catalog.authority_for(&binding));
        let mut handle = authority.new_handle();
        let attempt = handle
            .allocate_attempt(binding)
            .map_err(ExecutionPreparationError::Rejected)?;
        Ok((handle, attempt))
    }
}

/// Trusted C-7 port for the exact approval identity and `ai.tool.approve` scope.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "T-2 approval transport wiring is not complete")
)]
pub(crate) trait ApprovalAuthorizer {
    /// Reports whether this authenticated principal owns the approval context and scope.
    fn can_resolve_tool_approval(
        &mut self,
        approval: &ApprovalRequest,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> bool;
}

/// Sole application service allowed to mutate one requested D-6.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "T-2 approval transport wiring is not complete")
)]
pub(crate) struct ApprovalDecisionService<A> {
    authorizer: A,
}

#[cfg_attr(
    not(test),
    allow(dead_code, reason = "T-2 approval transport wiring is not complete")
)]
impl<A> ApprovalDecisionService<A>
where
    A: ApprovalAuthorizer,
{
    /// Creates the decision service around the configured C-7 authorization adapter.
    #[must_use]
    pub(crate) const fn new(authorizer: A) -> Self {
        Self { authorizer }
    }

    /// Applies an authenticated same-tenant, same-Thread, scoped decision.
    ///
    /// # Errors
    ///
    /// Returns [`ApprovalError::NotAuthorized`] without mutation for invalid ownership or scope,
    /// or the canonical guarded-transition error from the D-6 state machine.
    pub(crate) fn resolve(
        &mut self,
        approval: &mut ApprovalRequest,
        trust: &TrustContext,
        thread_id: ThreadId,
        decision: ApprovalDecision,
        decided_at_millis: u64,
    ) -> Result<u64, ApprovalError> {
        if approval.tenant_id() != &trust.tenant_id
            || approval.thread_id() != thread_id
            || !self
                .authorizer
                .can_resolve_tool_approval(approval, trust, thread_id)
        {
            return Err(ApprovalError::NotAuthorized);
        }
        approval.apply_validated_decision(decision, trust.subject_id.clone(), decided_at_millis)
    }
}

/// C-5 coordinator that makes isolation and lease fencing unbypassable.
pub struct ExecutionCoordinator<E, L, C> {
    /// Isolated executor port; only the cancellation boundary in this module
    /// family may present the bounded cancel permit beside the dispatch path.
    pub(super) executor: E,
    /// C-6 current-generation validation shared with the cancellation boundary.
    pub(super) lease: L,
    /// Conditional canonical terminal committer shared with cancellation.
    pub(super) committer: C,
    /// Started-at timestamp of the most recent dispatch, retained as evidence.
    #[cfg(test)]
    last_started_at_millis: u64,
}

#[derive(Clone, Copy)]
pub(super) enum DispatchPhase {
    BeforeDispatch,
    AfterDispatch,
}

impl<E, L, C> ExecutionCoordinator<E, L, C>
where
    E: IsolatedExecutor,
    L: LeaseValidator,
    C: AttemptCommitter,
{
    /// Creates a coordinator around the only executor and lease ports.
    #[must_use]
    pub const fn new(executor: E, lease: L, committer: C) -> Self {
        Self {
            executor,
            lease,
            committer,
            #[cfg(test)]
            last_started_at_millis: 0,
        }
    }

    /// Returns the executor for deterministic evidence inspection.
    #[must_use]
    pub const fn executor(&self) -> &E {
        &self.executor
    }

    /// Returns the started-at timestamp recorded by the most recent dispatch.
    #[cfg(test)]
    pub(crate) const fn last_started_at_millis(&self) -> u64 {
        self.last_started_at_millis
    }

    /// Returns the committer for deterministic unit-test evidence inspection.
    #[cfg(test)]
    pub(crate) const fn committer(&self) -> &C {
        &self.committer
    }

    /// Authorizes, dispatches, and validates one exact D-7 result.
    ///
    /// `now` is re-read after the executor returns; when the observed
    /// completion time is at or beyond `started_at_millis` plus 30 seconds,
    /// the deadline dominates and the D-7 commits `timed_out` with the
    /// executor's observed effect-state evidence instead of a succeeded or
    /// failed result.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionPending`] when the canonical dispatch claim was rejected
    /// or no canonical terminal write won. No error variant is a final Tool result.
    pub fn execute(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        approval: Option<&ApprovalRequest>,
        attempt: &mut ExecutionAttempt,
        started_at_millis: u64,
        now: &mut dyn FnMut() -> u64,
    ) -> Result<ToolExecutionOutcome, ExecutionPending> {
        if attempt.status() != ExecutionStatus::Prepared {
            return Err(rejected_start(ExecutionError::AlreadyDispatched));
        }
        let binding = attempt.binding().clone();
        if !self.lease.is_current(&binding) {
            return self.commit_terminal(
                authority,
                attempt,
                ToolExecutionOutcome::Cancelled {
                    effect_state: EffectState::NotStarted,
                },
                ExecutionStatus::Cancelled,
                DispatchPhase::BeforeDispatch,
            );
        }
        if let Err(error) = authority.claim_dispatch(attempt, approval, started_at_millis) {
            return Err(rejected_start(error));
        }
        if !self.lease.is_current(&binding) {
            return self.commit_terminal(
                authority,
                attempt,
                ToolExecutionOutcome::Cancelled {
                    effect_state: EffectState::NotStarted,
                },
                ExecutionStatus::Cancelled,
                DispatchPhase::BeforeDispatch,
            );
        }
        let permit = DispatchPermit { _private: () };
        let deadline = ActionDeadline::from_started_at(started_at_millis, now());
        if deadline.remaining_millis() == 0 {
            return self.commit_terminal(
                authority,
                attempt,
                ToolExecutionOutcome::TimedOut {
                    effect_state: EffectState::NotStarted,
                },
                ExecutionStatus::TimedOut,
                DispatchPhase::BeforeDispatch,
            );
        }
        #[cfg(test)]
        {
            self.last_started_at_millis = started_at_millis;
        }
        let response = self.executor.execute(&permit, &binding, deadline);
        let effect_state = match &response {
            Ok(response) => response.effect_state,
            Err(error) => error.effect_state,
        };
        if !self.lease.is_current(&binding) {
            return Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::OwnerFencedAfterDispatch,
                effect_state,
            });
        }
        let deadline_elapsed =
            now().saturating_sub(started_at_millis) >= MAX_ACTION_DURATION_MILLIS;
        if deadline_elapsed {
            return self.commit_terminal(
                authority,
                attempt,
                ToolExecutionOutcome::TimedOut { effect_state },
                ExecutionStatus::TimedOut,
                DispatchPhase::AfterDispatch,
            );
        }
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                return self.commit_terminal(
                    authority,
                    attempt,
                    ToolExecutionOutcome::Failed {
                        code: error.code,
                        effect_state: error.effect_state,
                    },
                    ExecutionStatus::Failed,
                    DispatchPhase::AfterDispatch,
                );
            }
        };
        let outcome = ToolExecutionOutcome::Succeeded {
            output: response.output,
            effect_state: response.effect_state,
        };
        self.commit_terminal(
            authority,
            attempt,
            outcome,
            ExecutionStatus::Succeeded,
            DispatchPhase::AfterDispatch,
        )
    }

    pub(super) fn commit_terminal(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        attempt: &mut ExecutionAttempt,
        outcome: ToolExecutionOutcome,
        status: ExecutionStatus,
        dispatch_phase: DispatchPhase,
    ) -> Result<ToolExecutionOutcome, ExecutionPending> {
        if authority.reserve_terminal(attempt).is_err() {
            return Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::TerminalConflict,
                effect_state: outcome.effect_state(),
            });
        }
        // A running D-7 has consumed its dispatch claim even when the deadline
        // or a lease fence prevents the executor call. An unresolved terminal
        // write must retain that reservation so an interrupter cannot send a
        // cancellation for an effect that was never requested. A still-prepared
        // D-7 may safely release its pre-effect reservation for another close.
        let reservation_failure = match dispatch_phase {
            DispatchPhase::BeforeDispatch if attempt.status() == ExecutionStatus::Prepared => {
                TerminalReservationFailure::ReleaseBeforeExternalEffect
            }
            DispatchPhase::BeforeDispatch | DispatchPhase::AfterDispatch => {
                TerminalReservationFailure::HoldForReconciliation
            }
        };
        self.commit_reserved_terminal(
            authority,
            attempt,
            outcome,
            status,
            dispatch_phase,
            reservation_failure,
        )
    }
}

/// Maps a rejected canonical dispatch claim without reporting a terminal result.
fn rejected_start(error: ExecutionError) -> ExecutionPending {
    ExecutionPending::DispatchRejected {
        code: match error {
            ExecutionError::AlreadyDispatched => ExecutionFailure::ApprovalAlreadyConsumed,
            ExecutionError::ApprovalMismatch
            | ExecutionError::AttemptLimit
            | ExecutionError::TurnMismatch
            | ExecutionError::InvalidTransition
            | ExecutionError::AttemptAlreadyAllocated
            | ExecutionError::PolicyAuthorizationRequired => ExecutionFailure::ApprovalMismatch,
            // A sealed Turn is an interruption, not an approval problem: report
            // its own code so callers do not take the approval-failure path.
            ExecutionError::InterruptionRequested => ExecutionFailure::InterruptionRequested,
            ExecutionError::ConcurrentAttempt => ExecutionFailure::ConcurrentAttempt,
        },
    }
}
