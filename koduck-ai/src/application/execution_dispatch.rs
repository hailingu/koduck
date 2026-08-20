// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! The coordinator's projected dispatch flow: durable claim, running
//! projection, bounded executor call, and post-dispatch settlement
//! (ADR-0003 TC-06/TC-07/TC-12).

use crate::domain::execution::{
    ApprovalRequest, ExactActionBinding, ExecutionAttempt, ExecutionStatus, TurnExecutionAuthority,
};

use crate::domain::execution::ExecutionError;

use super::attempt_store::DurableAttemptTransitions;
use super::deadline::{ActionDeadline, MAX_ACTION_DURATION_MILLIS};
use super::execution::{
    AttemptCommitter, DispatchPermit, DispatchPhase, ExecutionCoordinator, ExecutionPending,
    IsolatedExecutor, LeaseValidator, ToolExecutionOutcome, rejected_start,
};
use super::executor_envelope::{EffectState, ExecutionFailure};
use super::tool_projection::{ToolProjection, ToolProjectionSink, attempt_version, emit};

impl<E, L, C> ExecutionCoordinator<E, L, C>
where
    E: IsolatedExecutor,
    L: LeaseValidator,
    C: AttemptCommitter + DurableAttemptTransitions,
{
    /// Dispatches one exact D-7 result while appending the D-3 running
    /// projection after the canonical dispatch claim wins (TC-06).
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionPending`] when the canonical dispatch claim was
    /// rejected or no canonical terminal write won; no error variant is a
    /// final Tool result.
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
        let permit = DispatchPermit::issue();
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
            return self.settle_post_dispatch_fence(
                authority,
                attempt,
                &binding,
                pending,
                effect_state,
                now,
            );
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

    /// Settles the executor's return against the post-dispatch lease check.
    ///
    /// The three pending classes stay exactly as the inline contract
    /// recorded: an executor-confirmed `not_started` effect under a proven
    /// fence closes cancelled; a `started` or `unknown` effect persists the
    /// canonical `failed/owner_fenced_after_dispatch` terminal through the
    /// dedicated transition before surfacing the reconciliation error; an
    /// undetermined lease holds the reservation for reconciliation
    /// (ADR-0003 TC-07/TC-10, lines 309-314).
    fn settle_post_dispatch_fence(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        attempt: &mut ExecutionAttempt,
        binding: &ExactActionBinding,
        pending: ExecutionPending,
        effect_state: EffectState,
        now: &mut dyn FnMut() -> u64,
    ) -> Result<ToolExecutionOutcome, ExecutionPending> {
        // The settlement consumes the pending outcome its caller already
        // observed; re-checking the lease here could race a different answer.
        {
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
                    match self
                        .committer
                        .commit_fenced_after_dispatch(binding, effect_state, now_ms)
                    {
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
            Err(pending)
        }
    }
}
