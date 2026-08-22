// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! C-5 interruption boundary for truthful cancellation and timeout terminals.
//!
//! An authenticated Turn interruption closes this Turn's prepared D-7 without
//! dispatch, or sends exactly one bounded cancellation for its running D-7 and
//! commits the terminal the executor evidence proves: an acknowledged
//! `not_started` or `started` effect is `cancelled` with that reported state,
//! while a cancellation that no executor acknowledges before the 30-second
//! action deadline commits `timed_out` with `effect_state=unknown`. A requested
//! D-6 is closed through the authenticated cancelled-decision path of the
//! approval service, which already cancels its prepared D-7; transport-level
//! pending-D-6 projection cancellation is T-2/T-3 work.

use std::sync::Arc;

use crate::domain::execution::{
    ApprovalRequirement, ExactActionBinding, ExecutionAttempt, ExecutionStatus,
    TurnAuthorityCatalog, TurnExecutionAuthority,
};
use crate::domain::{TenantId, ThreadId, TurnId};

use super::attempt_store::DurableAttemptTransitions;
use super::audit::{ToolAuditRecord, ToolAuditTrail, record_audit};
use super::deadline::{ActionDeadline, MAX_ACTION_DURATION_MILLIS};
use super::execution::{
    AttemptCommitter, DispatchPhase, ExecutionCoordinator, ExecutionPending, IsolatedExecutor,
    LeaseCheck, LeaseValidator, ToolExecutionOutcome, ToolExecutionRuntime,
};
use super::executor_envelope::{EffectState, ExecutionFailure};
use super::terminal::TerminalReservationFailure;

/// Opaque single-call authority created only by the C-5 cancellation boundary.
pub struct CancelPermit {
    _private: (),
}

/// Executor-observed effect state for an acknowledged bounded cancellation.
///
/// Only a concrete `not_started` or `started` observation may commit a
/// `cancelled` terminal. A reachable executor that provides no acknowledgement
/// before the deadline reports `NotAcknowledged` and commits
/// `timed_out/unknown`; an unavailable cancellation boundary instead requires
/// reconciliation. `unknown` is never an acknowledged cancellation state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancelledEffectState {
    /// The executor proves that no effect started.
    NotStarted,
    /// The executor observed that the effect started.
    Started,
}

/// Executor response to exactly one bounded cancellation request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancelAcknowledgement {
    /// The executor reported the observed effect state before the deadline.
    Acknowledged(CancelledEffectState),
    /// No executor acknowledgement arrived before the 30-second deadline.
    NotAcknowledged,
    /// The cancellation boundary is unavailable and cannot await an outcome.
    Unavailable,
}

/// Canonical outcome of attempting to close a requested D-6 during interruption.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "T-2 approval transport wiring is not complete")
)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PendingApprovalCancellation {
    /// This interruption won the requested-to-cancelled D-6 transition.
    Cancelled,
    /// Another valid D-6 terminal, including `accepted`, already won.
    AlreadyResolved,
}

/// C-5 port that conditionally closes the requested D-6 bound to one D-7.
///
/// The persistence or approval-transport adapter owns the canonical D-6 record
/// and must fail rather than report cancellation when its guarded transition
/// loses. The exact binding prevents an interruption from closing another
/// attempt's approval.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "T-2 approval transport wiring is not complete")
)]
pub(crate) trait PendingApprovalCanceller {
    /// Closes the requested D-6 for this exact approval-required D-7.
    fn cancel_requested(
        &mut self,
        binding: &ExactActionBinding,
    ) -> Result<PendingApprovalCancellation, ExecutionPending>;
}

/// Independently reachable C-5 path for one guarded D-7 cancellation.
///
/// Runtime assembly supplies this port separately from the blocking dispatch
/// coordinator. That separation lets an authenticated interruption reach the
/// executor cancellation client while a dispatched action is still waiting for
/// its result.
pub(crate) trait AttemptCancellationService {
    /// Reports whether durable cancellation commits include the terminal audit.
    fn appends_terminal_audit_atomically(&self) -> bool {
        false
    }

    /// Cancels one cataloged prepared D-7 without executor dispatch.
    fn cancel_prepared(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        attempt: &mut ExecutionAttempt,
    ) -> Result<ToolExecutionOutcome, ExecutionPending>;

    /// Cancels one cataloged running D-7 through the bounded executor path.
    ///
    /// `now` supplies the C-5 clock and is re-read after the bounded executor
    /// cancellation returns: an acknowledgement that arrives after the 30-second
    /// action deadline commits `timed_out/unknown` rather than `cancelled`.
    fn cancel_running(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        attempt: &mut ExecutionAttempt,
        now: &mut dyn FnMut() -> u64,
    ) -> Result<ToolExecutionOutcome, ExecutionPending>;
}

/// Outcome of interrupting one identified Turn's live execution work.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "T-2 runtime interruption wiring is not complete")
)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum InterruptionOutcome {
    /// The Turn has no prepared or running D-7, so nothing was cancelled.
    NoLiveAttempt,
    /// The live D-7 closed with this committed terminal.
    Closed(ToolExecutionOutcome),
    /// Multiple prepared D-7s closed during one interruption.
    ClosedMany(Vec<ToolExecutionOutcome>),
    /// Some D-7s closed, but a later one failed and the remaining live D-7s
    /// require reconciliation. The caller must not treat this as a total
    /// success.
    PartiallyClosed {
        /// Outcomes of the D-7s that were durably closed before the failure.
        closed: Vec<ToolExecutionOutcome>,
        /// The pending error from the D-7 whose cancellation failed.
        pending: ExecutionPending,
    },
}

/// Handle that interrupts one Turn's live D-7 through the guarded coordinator
/// path (TC-10), sharing the runtime's process-owned authority catalog.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "T-2 runtime interruption wiring is not complete")
)]
#[derive(Clone, Debug)]
pub(crate) struct ExecutionInterrupter {
    catalog: Arc<TurnAuthorityCatalog>,
}

impl ToolExecutionRuntime {
    /// Returns the interruption handle sharing this runtime's authority catalog.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "T-2 runtime interruption wiring is not complete")
    )]
    pub(crate) fn interrupter(&self) -> ExecutionInterrupter {
        ExecutionInterrupter {
            catalog: Arc::clone(&self.catalog),
        }
    }
}

impl ExecutionInterrupter {
    /// Interrupts every prepared or running D-7 for the identified Turn.
    ///
    /// A prepared D-7 closes to `cancelled/not_started` without executor
    /// dispatch; a running D-7 receives exactly one bounded cancellation whose
    /// acknowledgement determines the committed terminal.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionPending`] when no canonical terminal write won; the
    /// reconciler owns the next transition and no fabricated terminal is
    /// returned.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "T-2 runtime interruption wiring is not complete")
    )]
    #[allow(clippy::collapsible_if)]
    #[allow(
        clippy::too_many_arguments,
        reason = "the cancellation ports, audit trail, and authenticated Turn dimensions are explicit"
    )]
    pub(crate) fn interrupt(
        &self,
        cancellations: &mut dyn AttemptCancellationService,
        audits: &mut dyn ToolAuditTrail,
        approvals: &mut dyn PendingApprovalCanceller,
        tenant: &TenantId,
        thread: ThreadId,
        turn: TurnId,
        now: &mut dyn FnMut() -> u64,
    ) -> Result<InterruptionOutcome, ExecutionPending> {
        self.interrupt_with_projections(cancellations, audits, approvals, tenant, thread, turn, now)
            .map(|(outcome, _)| outcome)
    }

    /// Interrupts live work and returns its canonical terminal projections.
    #[allow(clippy::collapsible_if)]
    #[allow(
        clippy::too_many_arguments,
        reason = "the cancellation ports, audit trail, and authenticated Turn dimensions are explicit"
    )]
    pub(crate) fn interrupt_with_projections(
        &self,
        cancellations: &mut dyn AttemptCancellationService,
        audits: &mut dyn ToolAuditTrail,
        approvals: &mut dyn PendingApprovalCanceller,
        tenant: &TenantId,
        thread: ThreadId,
        turn: TurnId,
        now: &mut dyn FnMut() -> u64,
    ) -> Result<(InterruptionOutcome, Vec<super::ToolProjection>), ExecutionPending> {
        let Some(mut authority) = self.catalog.request_interruption(tenant, thread, turn) else {
            return Ok((InterruptionOutcome::NoLiveAttempt, Vec::new()));
        };
        let (mut attempts, terminal_commit_in_flight) = authority.interruption_snapshot();
        if terminal_commit_in_flight {
            return Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::TerminalConflict,
                effect_state: EffectState::Unknown,
            });
        }
        if attempts.is_empty() {
            return Ok((InterruptionOutcome::NoLiveAttempt, Vec::new()));
        }
        let mut closed = Vec::with_capacity(attempts.len());
        let mut projections = Vec::with_capacity(attempts.len());
        for attempt in &mut attempts {
            let outcome = match attempt.status() {
                ExecutionStatus::Prepared => {
                    if matches!(
                        attempt.binding().approval_requirement(),
                        Some(ApprovalRequirement::Required)
                    ) {
                        if let Err(pending) = approvals.cancel_requested(attempt.binding()) {
                            return partial_or_error(closed, pending)
                                .map(|outcome| (outcome, projections));
                        }
                    }
                    match cancellations.cancel_prepared(&mut authority, attempt) {
                        Err(
                            pending @ ExecutionPending::ReconciliationRequired {
                                code: ExecutionFailure::TerminalConflict,
                                ..
                            },
                        ) => {
                            let Some(mut running_attempt) =
                                authority.live_attempts().into_iter().find(|candidate| {
                                    candidate.binding() == attempt.binding()
                                        && candidate.status() == ExecutionStatus::Running
                                })
                            else {
                                return partial_or_error(closed, pending)
                                    .map(|outcome| (outcome, projections));
                            };
                            cancellations.cancel_running(&mut authority, &mut running_attempt, now)
                        }
                        result => result,
                    }
                }
                // `live_attempts` only mirrors prepared or running catalog entries.
                ExecutionStatus::Running => {
                    cancellations.cancel_running(&mut authority, attempt, now)
                }
                _ => unreachable!("live attempt lookup filters terminal states"),
            };
            match outcome {
                Ok(value) => {
                    // Durable committers append this terminal audit in their
                    // own transaction; only non-atomic committers need the
                    // C-5 fallback record (TC-14).
                    if !cancellations.appends_terminal_audit_atomically() {
                        record_audit(
                            audits,
                            &ToolAuditRecord::execution_terminal(attempt.binding(), &value, now()),
                        );
                    }
                    projections.push(super::tool_execution_terminal::tool_result_projection(
                        attempt.binding().attempt_id(),
                        &value,
                    ));
                    closed.push(value);
                }
                Err(pending) => {
                    return partial_or_error(closed, pending).map(|outcome| (outcome, projections));
                }
            }
        }
        if closed.len() == 1 {
            Ok((InterruptionOutcome::Closed(closed.remove(0)), projections))
        } else {
            Ok((InterruptionOutcome::ClosedMany(closed), projections))
        }
    }
}

/// On a mid-loop interruption failure, earlier D-7s may already be durably
/// closed. Return those partial results so the caller observes the real durable
/// state instead of a misleading total error. If nothing closed yet, the failure
/// is the caller's to reconcile.
fn partial_or_error(
    closed: Vec<ToolExecutionOutcome>,
    pending: ExecutionPending,
) -> Result<InterruptionOutcome, ExecutionPending> {
    if closed.is_empty() {
        Err(pending)
    } else {
        Ok(InterruptionOutcome::PartiallyClosed { closed, pending })
    }
}

impl<E, L, C> AttemptCancellationService for ExecutionCoordinator<E, L, C>
where
    E: IsolatedExecutor,
    L: LeaseValidator,
    C: AttemptCommitter + DurableAttemptTransitions,
{
    fn appends_terminal_audit_atomically(&self) -> bool {
        ExecutionCoordinator::appends_terminal_audit_atomically(self)
    }

    fn cancel_prepared(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        attempt: &mut ExecutionAttempt,
    ) -> Result<ToolExecutionOutcome, ExecutionPending> {
        self.cancel_prepared_attempt(authority, attempt)
    }

    fn cancel_running(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        attempt: &mut ExecutionAttempt,
        now: &mut dyn FnMut() -> u64,
    ) -> Result<ToolExecutionOutcome, ExecutionPending> {
        self.cancel_running_attempt(authority, attempt, now)
    }
}

impl<E, L, C> ExecutionCoordinator<E, L, C>
where
    E: IsolatedExecutor,
    L: LeaseValidator,
    C: AttemptCommitter + DurableAttemptTransitions,
{
    /// Closes one prepared D-7 as `cancelled/not_started` without executor
    /// dispatch, through the durable prepared-only conditional close.
    ///
    /// Used when a requested D-6 is declined, cancelled, or expired, and by a
    /// concurrent durable-claim loser closing its never-dispatched attempt:
    /// the close is a compare-and-set from `prepared`, so a racing claimant
    /// that already claimed this exact identity keeps its canonical state and
    /// this side defers to reconciliation instead of rewriting a dispatched
    /// row (ADR-0003 TC-10/TC-12).
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionPending`] when the conditional durable close did
    /// not win. A locally prepared attempt has no dispatch claim and releases
    /// its unresolved reservation; a locally running durable-claim loser
    /// retains it for reconciliation because a lease failure cannot prove the
    /// canonical effect never began.
    pub(crate) fn cancel_prepared_attempt(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        attempt: &mut ExecutionAttempt,
    ) -> Result<ToolExecutionOutcome, ExecutionPending> {
        use super::attempt_store::{AttemptStoreError, PreparedCloseResolution};
        use super::canonical_dispatch::{
            reconciliation_required, reconciliation_required_with_effect,
        };
        let binding = attempt.binding().clone();
        let was_running = attempt.status() == ExecutionStatus::Running;
        match self.lease.check_current(&binding) {
            LeaseCheck::Current => {}
            LeaseCheck::Fenced => {
                if was_running && authority.reserve_terminal(attempt).is_err() {
                    return Err(reconciliation_required_with_effect(
                        ExecutionFailure::TerminalConflict,
                        EffectState::Unknown,
                    ));
                }
                return Err(reconciliation_required_with_effect(
                    ExecutionFailure::OwnerFencedBeforeDispatch,
                    if was_running {
                        EffectState::Unknown
                    } else {
                        EffectState::NotStarted
                    },
                ));
            }
            LeaseCheck::Unavailable => {
                if was_running && authority.reserve_terminal(attempt).is_err() {
                    return Err(reconciliation_required_with_effect(
                        ExecutionFailure::TerminalConflict,
                        EffectState::Unknown,
                    ));
                }
                return Err(reconciliation_required_with_effect(
                    ExecutionFailure::LeaseUnavailable,
                    if was_running {
                        EffectState::Unknown
                    } else {
                        EffectState::NotStarted
                    },
                ));
            }
        }
        if authority.reserve_terminal(attempt).is_err() {
            return Err(reconciliation_required(ExecutionFailure::TerminalConflict));
        }
        match self.committer.cancel_prepared_attempt(&binding) {
            Ok(PreparedCloseResolution::Won { .. }) => {
                if authority
                    .mirror_terminal(attempt, ExecutionStatus::Cancelled)
                    .is_err()
                {
                    return Err(reconciliation_required(ExecutionFailure::TerminalConflict));
                }
                Ok(ToolExecutionOutcome::Cancelled {
                    effect_state: EffectState::NotStarted,
                })
            }
            // A local prepared attempt has never passed a dispatch claim, so
            // it may release a failed close. A local running attempt reached
            // this path only after its process-local claim lost durable
            // authority; retain its reservation so interruption cannot send a
            // cancellation for work this coordinator never dispatched.
            outcome => {
                if !was_running {
                    authority.release_terminal_reservation(attempt);
                }
                let (code, effect_state) = match outcome {
                    Ok(PreparedCloseResolution::Progressed { .. }) => {
                        (ExecutionFailure::TerminalConflict, EffectState::Unknown)
                    }
                    Err(AttemptStoreError::IdentityConflict) => {
                        (ExecutionFailure::TerminalConflict, EffectState::NotStarted)
                    }
                    Ok(PreparedCloseResolution::Fenced) => (
                        ExecutionFailure::OwnerFencedBeforeDispatch,
                        if was_running {
                            EffectState::Unknown
                        } else {
                            EffectState::NotStarted
                        },
                    ),
                    // A close acknowledgement can be lost after another
                    // owner claimed and dispatched this exact D-7, even when
                    // this local mirror remains prepared.
                    Err(AttemptStoreError::Unavailable) => (
                        ExecutionFailure::DurabilityUnavailable,
                        EffectState::Unknown,
                    ),
                    Err(AttemptStoreError::AttemptLimit) => {
                        (ExecutionFailure::AttemptLimit, EffectState::NotStarted)
                    }
                    Ok(PreparedCloseResolution::Won { .. }) => unreachable!("handled above"),
                };
                Err(reconciliation_required_with_effect(code, effect_state))
            }
        }
    }

    /// Sends exactly one bounded cancellation for a running D-7 and commits the
    /// terminal its acknowledgement proves.
    ///
    /// The current lease generation is validated before the cancellation is
    /// sent; a fenced owner receives a reconciliation requirement rather than a
    /// fabricated terminal, and the conditional durable commit re-validates
    /// ownership exactly like a dispatch result.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionPending::DispatchRejected`] when the addressed D-7 is
    /// not running, and [`ExecutionPending::ReconciliationRequired`] when the
    /// owner was fenced or no canonical terminal write won.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "T-2 runtime interruption wiring is not complete")
    )]
    pub(crate) fn cancel_running_attempt(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        attempt: &mut ExecutionAttempt,
        now: &mut dyn FnMut() -> u64,
    ) -> Result<ToolExecutionOutcome, ExecutionPending> {
        if attempt.status() != ExecutionStatus::Running {
            return Err(ExecutionPending::DispatchRejected {
                code: ExecutionFailure::AttemptNotRunning,
            });
        }
        let binding = attempt.binding().clone();
        self.post_dispatch_lease(&binding, EffectState::Unknown)?;
        let Some(started_at_millis) = attempt.started_at_millis() else {
            return Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::TerminalConflict,
                effect_state: EffectState::Unknown,
            });
        };
        if authority.reserve_terminal(attempt).is_err() {
            return Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::TerminalConflict,
                effect_state: EffectState::Unknown,
            });
        }
        let deadline = ActionDeadline::from_started_at(started_at_millis, now());
        if deadline.remaining_millis() == 0 {
            return self.commit_reserved_terminal_with_ownership(
                authority,
                attempt,
                ToolExecutionOutcome::TimedOut {
                    effect_state: EffectState::Unknown,
                },
                ExecutionStatus::TimedOut,
                DispatchPhase::AfterDispatch,
                TerminalReservationFailure::HoldForReconciliation,
                true,
            );
        }
        let permit = CancelPermit { _private: () };
        let acknowledgement = self.executor.cancel(&permit, &binding, deadline);
        let (effect_state, acknowledged) = match acknowledgement {
            CancelAcknowledgement::Acknowledged(CancelledEffectState::NotStarted) => {
                (EffectState::NotStarted, true)
            }
            CancelAcknowledgement::Acknowledged(CancelledEffectState::Started) => {
                (EffectState::Started, true)
            }
            CancelAcknowledgement::NotAcknowledged => (EffectState::Unknown, false),
            CancelAcknowledgement::Unavailable => {
                return Err(ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::ExecutorUnavailable,
                    effect_state: EffectState::Unknown,
                });
            }
        };
        self.post_dispatch_lease(&binding, effect_state)?;
        // The bounded cancellation acknowledgement may arrive after the 30-second
        // action deadline. The deadline then dominates: the D-7 commits
        // `timed_out/unknown` rather than a `cancelled` terminal whose effect
        // evidence can no longer be trusted relative to the deadline.
        if now().saturating_sub(started_at_millis) >= MAX_ACTION_DURATION_MILLIS {
            return self.commit_reserved_terminal_with_ownership(
                authority,
                attempt,
                ToolExecutionOutcome::TimedOut {
                    effect_state: EffectState::Unknown,
                },
                ExecutionStatus::TimedOut,
                DispatchPhase::AfterDispatch,
                TerminalReservationFailure::HoldForReconciliation,
                true,
            );
        }
        let outcome = if acknowledged {
            ToolExecutionOutcome::Cancelled { effect_state }
        } else {
            ToolExecutionOutcome::TimedOut {
                effect_state: EffectState::Unknown,
            }
        };
        let status = outcome.status();
        self.commit_reserved_terminal_with_ownership(
            authority,
            attempt,
            outcome,
            status,
            DispatchPhase::AfterDispatch,
            TerminalReservationFailure::HoldForReconciliation,
            true,
        )
    }
}
