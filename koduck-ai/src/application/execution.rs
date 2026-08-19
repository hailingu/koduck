// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! C-5 coordination around the isolated one-attempt executor port.

use crate::domain::execution::{
    ApprovalDecision, ApprovalError, ApprovalRequest, ExactActionBinding, ExecutionAttempt,
    ExecutionError, ExecutionStatus, TurnExecutionAuthority,
};
use crate::domain::{ThreadId, TrustContext};

use super::attempt_store::DurableAttemptTransitions;
use super::cancellation::{CancelAcknowledgement, CancelPermit};
use super::deadline::{ActionDeadline, MAX_ACTION_DURATION_MILLIS};
use super::executor_envelope::{
    EffectState, ExecutionFailure, ExecutionResponse, ExecutorError, MAX_EXECUTOR_OUTPUT_BYTES,
};
pub(super) use super::preparation::{
    ExecutionPreparer, ToolExecutionAuthorityRoot, ToolExecutionRuntime,
};
use super::terminal::TerminalReservationFailure;
use super::tool_projection::{ToolProjection, ToolProjectionSink, attempt_version, emit};

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

    /// Durably commits the canonical `failed/owner_fenced_after_dispatch`
    /// terminal for one running D-7 whose bound lease is definitively fenced.
    ///
    /// The default fails closed because only the production canonical store
    /// can prove the bound lease is no longer current under the Turn
    /// ownership lock; a composition without that proof must keep the
    /// attempt for reconciliation instead of writing a fenced terminal.
    ///
    /// # Errors
    ///
    /// Returns [`AttemptStoreError::Unavailable`] by default; the production
    /// store reports its conditional resolution.
    fn commit_fenced_after_dispatch(
        &mut self,
        _binding: &ExactActionBinding,
        _effect_state: EffectState,
        _terminal_at_millis: u64,
    ) -> Result<
        super::attempt_store::AttemptTerminalResolution,
        super::attempt_store::AttemptStoreError,
    > {
        Err(super::attempt_store::AttemptStoreError::Unavailable)
    }
}

/// Typed result of one C-6 foreground-generation validation.
///
/// `Unavailable` MUST NOT be treated as `Fenced`: a validator that panicked or
/// failed leaves ownership undetermined, so the attempt is retained for
/// reconciliation with zero dispatch instead of being closed as a fenced
/// cancellation (TC-07).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaseCheck {
    /// The bound owner is still the current foreground owner.
    Current,
    /// The bound owner was definitively fenced by a newer generation.
    Fenced,
    /// Validation itself failed; ownership is undetermined.
    Unavailable,
}

/// Consumer-owned port for C-6 foreground generation validation.
pub trait LeaseValidator {
    /// Reports the typed current-ownership state of the bound generation.
    ///
    /// Implementations return [`LeaseCheck::Unavailable`] — never a guessed
    /// `Current`/`Fenced` — when they cannot validate ownership, so callers
    /// can distinguish an undetermined validator from a fenced owner.
    fn check_current(&mut self, binding: &ExactActionBinding) -> LeaseCheck;
}

/// A rejected attempt preparation before any D-7 slot is consumed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionPreparationError {
    /// The exact binding no longer belongs to the current foreground generation.
    OwnerFenced,
    /// The current foreground generation could not be validated; ownership is
    /// undetermined and no D-7 slot was consumed.
    LeaseUnavailable,
    /// The Turn authority rejected the requested allocation.
    Rejected(ExecutionError),
}

/// Trusted C-7 port for the exact approval identity and `ai.tool.approve` scope.
pub(crate) trait ApprovalAuthorizer {
    /// Reports whether this authenticated principal owns the approval context and scope.
    fn can_resolve_tool_approval(
        &mut self,
        binding: &ExactActionBinding,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> bool;
}

/// Sole application service allowed to mutate one requested D-6.
pub(crate) struct ApprovalDecisionService<A> {
    authorizer: A,
}

impl<A> ApprovalDecisionService<A>
where
    A: ApprovalAuthorizer,
{
    /// Creates the decision service around the configured C-7 authorization adapter.
    #[must_use]
    pub(crate) const fn new(authorizer: A) -> Self {
        Self { authorizer }
    }

    /// Validates that `trust` may resolve approvals for `binding` without
    /// mutating state, before any D-6 request or D-7 allocation exists.
    ///
    /// An expired D-6 window does not waive this check (TC-05/TC-09): without
    /// it an unscoped principal could drain the Turn's attempt budget through
    /// allocate-then-cancel loops.
    ///
    /// # Errors
    ///
    /// Returns [`ApprovalError::NotAuthorized`] without mutation for invalid
    /// ownership or scope.
    pub(crate) fn validate_resolver_for_binding(
        &mut self,
        binding: &ExactActionBinding,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> Result<(), ApprovalError> {
        if binding.tenant_id() != &trust.tenant_id
            || binding.thread_id() != thread_id
            || !self
                .authorizer
                .can_resolve_tool_approval(binding, trust, thread_id)
        {
            return Err(ApprovalError::NotAuthorized);
        }
        Ok(())
    }

    /// Validates that `trust` may resolve `approval` without mutating state.
    ///
    /// Callers MUST run this check before exposing the D-6 to any decision
    /// provider, so an unauthorized principal can neither observe the approval
    /// nor trigger decision-provider side effects (TC-05).
    ///
    /// # Errors
    ///
    /// Returns [`ApprovalError::NotAuthorized`] without mutation for invalid
    /// ownership or scope.
    pub(crate) fn validate_resolver(
        &mut self,
        approval: &ApprovalRequest,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> Result<(), ApprovalError> {
        self.validate_resolver_for_binding(approval.binding(), trust, thread_id)
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
        self.validate_resolver(approval, trust, thread_id)?;
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
    C: AttemptCommitter + DurableAttemptTransitions,
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
        self.execute_projected(
            authority,
            approval,
            attempt,
            started_at_millis,
            now,
            &mut crate::application::NoToolProjections,
        )
    }

    /// Dispatches one exact D-7 result while appending the D-3 running
    /// projection after the canonical dispatch claim wins (TC-06).
    ///
    /// The projection is a durable view published only after its append; it
    /// never authorizes or redispatches the attempt.
    /// # Errors
    ///
    /// Returns [`ExecutionPending`] when the canonical dispatch claim was rejected
    /// or no canonical terminal write won. No error variant is a final Tool result.
    // One ordered lease-check, dispatch-claim, projection, executor, deadline,
    // and conditional-commit sequence whose ordering encodes TC-07; extracting
    // a phase would separate the claim from its projection and commit.
    #[allow(clippy::too_many_lines)]
    pub fn execute_projected(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        approval: Option<&ApprovalRequest>,
        attempt: &mut ExecutionAttempt,
        started_at_millis: u64,
        now: &mut dyn FnMut() -> u64,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ToolExecutionOutcome, ExecutionPending> {
        if attempt.status() != ExecutionStatus::Prepared {
            return Err(rejected_start(ExecutionError::AlreadyDispatched));
        }
        let binding = attempt.binding().clone();
        if let Some(cancelled) = self.pre_dispatch_lease(authority, attempt, &binding)? {
            return Ok(cancelled);
        }
        if let Err(error) = authority.claim_dispatch(attempt, approval, started_at_millis) {
            return Err(rejected_start(error));
        }
        // TC-12: only the won durable running claim permits an executor
        // dispatch, so the one-running-per-Turn and single-dispatch
        // guarantees hold across processes; a fenced or concurrent durable
        // slot closes this never-dispatched attempt or defers to
        // reconciliation with zero executor calls.
        if let Some(cancelled) =
            self.claim_canonical_dispatch(authority, attempt, started_at_millis)?
        {
            return Ok(cancelled);
        }
        // TC-06: the running projection immediately follows the won canonical
        // dispatch claim, before the post-claim lease check and any executor
        // call, so publication can never outrun the canonical running
        // transition and a post-claim fence cannot leave a terminal projection
        // without it.
        emit(
            projections,
            ToolProjection::ToolCall {
                descriptor_id: binding.action().descriptor_id().to_owned(),
                descriptor_version: binding.action().descriptor_version().to_owned(),
                target: binding.action().target().to_owned(),
                attempt_id: binding.attempt_id(),
                status: ExecutionStatus::Running,
                version: attempt_version(ExecutionStatus::Running),
            },
        );
        if let Some(cancelled) = self.post_claim_lease(authority, attempt, &binding)? {
            return Ok(cancelled);
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
            Ok(response) => response.effect_state(),
            Err(error) => error.effect_state(),
        };
        if let Err(pending) = self.post_dispatch_lease(&binding, effect_state) {
            // The executor already returned, so an external effect may exist.
            // An executor-confirmed `not_started` effect never started, so a
            // fenced owner may still close its D-7 as cancelled without
            // delivering any Tool result (ADR-0003 TC-07); `started` or
            // `unknown` effects stay held for reconciliation as
            // failed/owner_fenced_after_dispatch. When ownership is merely
            // undetermined rather than proven fenced, hold the running
            // attempt's terminal reservation for reconciliation; otherwise a
            // recovered lease would let the interruption boundary cancel an
            // already-executed effect (TC-07/TC-10).
            if matches!(
                pending,
                ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::OwnerFencedAfterDispatch,
                    ..
                }
            ) && effect_state == EffectState::NotStarted
            {
                return self.commit_terminal(
                    authority,
                    attempt,
                    ToolExecutionOutcome::Cancelled {
                        effect_state: EffectState::NotStarted,
                    },
                    ExecutionStatus::Cancelled,
                    DispatchPhase::AfterDispatch,
                );
            }
            if matches!(
                pending,
                ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::OwnerFencedAfterDispatch,
                    ..
                }
            ) && matches!(effect_state, EffectState::Started | EffectState::Unknown)
            {
                // The lease is definitively fenced after an effect may have
                // started, so the canonical row must carry
                // failed/owner_fenced_after_dispatch rather than staying
                // running for the lease-expiry fallback to relabel
                // timed_out/unknown (ADR-0003 lines 309-314). The dedicated
                // transition re-proves the fence under the ownership lock; a
                // lost or conflicted write stays held for reconciliation and
                // no Tool result reaches the model either way (TC-07).
                if authority.reserve_terminal(attempt).is_ok() {
                    let now_ms = (now)();
                    match self.committer.commit_fenced_after_dispatch(
                        &binding,
                        effect_state,
                        now_ms,
                    ) {
                        Ok(super::attempt_store::AttemptTerminalResolution::Won { .. }) => {
                            let _ = authority.mirror_terminal(attempt, ExecutionStatus::Failed);
                        }
                        Ok(super::attempt_store::AttemptTerminalResolution::ExistingTerminal(
                            _,
                        )) => {
                            authority.release_terminal_reservation(attempt);
                        }
                        _ => {}
                    }
                }
                return Err(pending);
            }
            if matches!(
                pending,
                ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::LeaseUnavailable,
                    ..
                }
            ) && authority.reserve_terminal(attempt).is_err()
            {
                return Err(ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::TerminalConflict,
                    effect_state,
                });
            }
            return Err(pending);
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
                        code: error.code(),
                        effect_state: error.effect_state(),
                    },
                    ExecutionStatus::Failed,
                    DispatchPhase::AfterDispatch,
                );
            }
        };
        let effect_state = response.effect_state();
        let outcome = ToolExecutionOutcome::Succeeded {
            output: response.into_output(),
            effect_state,
        };
        self.commit_terminal(
            authority,
            attempt,
            outcome,
            ExecutionStatus::Succeeded,
            DispatchPhase::AfterDispatch,
        )
    }

    /// Validates the lease immediately before dispatch (TC-07).
    ///
    /// A fenced owner closes the still-prepared D-7 as `cancelled/not_started`
    /// and the terminal is returned to the caller as the final outcome; an
    /// undetermined validator must not close the attempt as a fenced
    /// cancellation, so reconciliation owns the next transition with zero
    /// dispatch.
    pub(super) fn pre_dispatch_lease(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        attempt: &mut ExecutionAttempt,
        binding: &ExactActionBinding,
    ) -> Result<Option<ToolExecutionOutcome>, ExecutionPending> {
        match self.lease.check_current(binding) {
            LeaseCheck::Current => Ok(None),
            LeaseCheck::Fenced => self
                .commit_terminal(
                    authority,
                    attempt,
                    ToolExecutionOutcome::Cancelled {
                        effect_state: EffectState::NotStarted,
                    },
                    ExecutionStatus::Cancelled,
                    DispatchPhase::BeforeDispatch,
                )
                .map(Some),
            LeaseCheck::Unavailable => Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::LeaseUnavailable,
                effect_state: EffectState::NotStarted,
            }),
        }
    }

    /// Validates the lease after the dispatch claim and before the executor
    /// call (TC-07/TC-10).
    ///
    /// The claim has already marked the D-7 Running, so an undetermined
    /// validator must hold the terminal reservation for reconciliation:
    /// without it the interruption boundary would treat the never-dispatched
    /// attempt as running work and send an executor cancellation for an effect
    /// that was never requested. A fenced owner still closes the attempt as
    /// `cancelled/not_started` through the conditional commit, which reserves
    /// before writing.
    pub(super) fn post_claim_lease(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        attempt: &mut ExecutionAttempt,
        binding: &ExactActionBinding,
    ) -> Result<Option<ToolExecutionOutcome>, ExecutionPending> {
        match self.lease.check_current(binding) {
            LeaseCheck::Current => Ok(None),
            LeaseCheck::Fenced => self
                .commit_terminal(
                    authority,
                    attempt,
                    ToolExecutionOutcome::Cancelled {
                        effect_state: EffectState::NotStarted,
                    },
                    ExecutionStatus::Cancelled,
                    DispatchPhase::BeforeDispatch,
                )
                .map(Some),
            LeaseCheck::Unavailable => {
                if authority.reserve_terminal(attempt).is_err() {
                    return Err(ExecutionPending::ReconciliationRequired {
                        code: ExecutionFailure::TerminalConflict,
                        effect_state: EffectState::NotStarted,
                    });
                }
                Err(ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::LeaseUnavailable,
                    effect_state: EffectState::NotStarted,
                })
            }
        }
    }

    /// Validates the lease after an effect may have started (TC-07).
    ///
    /// A fenced owner commits no Tool result and an undetermined validator
    /// cannot prove ownership, so both defer to reconciliation with truthful
    /// effect evidence and distinct codes.
    pub(super) fn post_dispatch_lease(
        &mut self,
        binding: &ExactActionBinding,
        effect_state: EffectState,
    ) -> Result<(), ExecutionPending> {
        match self.lease.check_current(binding) {
            LeaseCheck::Current => Ok(()),
            LeaseCheck::Fenced => Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::OwnerFencedAfterDispatch,
                effect_state,
            }),
            LeaseCheck::Unavailable => Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::LeaseUnavailable,
                effect_state,
            }),
        }
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
