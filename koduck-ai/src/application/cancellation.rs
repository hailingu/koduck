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
    pub(crate) fn interrupt(
        &self,
        cancellations: &mut dyn AttemptCancellationService,
        approvals: &mut dyn PendingApprovalCanceller,
        tenant: &TenantId,
        thread: ThreadId,
        turn: TurnId,
        now: &mut dyn FnMut() -> u64,
    ) -> Result<InterruptionOutcome, ExecutionPending> {
        let Some(mut authority) = self.catalog.request_interruption(tenant, thread, turn) else {
            return Ok(InterruptionOutcome::NoLiveAttempt);
        };
        let (mut attempts, terminal_commit_in_flight) = authority.interruption_snapshot();
        if terminal_commit_in_flight {
            return Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::TerminalConflict,
                effect_state: EffectState::Unknown,
            });
        }
        if attempts.is_empty() {
            return Ok(InterruptionOutcome::NoLiveAttempt);
        }
        let mut closed = Vec::with_capacity(attempts.len());
        for attempt in &mut attempts {
            let outcome = match attempt.status() {
                ExecutionStatus::Prepared => {
                    if matches!(
                        attempt.binding().approval_requirement(),
                        Some(ApprovalRequirement::Required)
                    ) {
                        if let Err(pending) = approvals.cancel_requested(attempt.binding()) {
                            return partial_or_error(closed, pending);
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
                                return partial_or_error(closed, pending);
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
                Ok(value) => closed.push(value),
                Err(pending) => return partial_or_error(closed, pending),
            }
        }
        if closed.len() == 1 {
            Ok(InterruptionOutcome::Closed(closed.remove(0)))
        } else {
            Ok(InterruptionOutcome::ClosedMany(closed))
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
    C: AttemptCommitter,
{
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
    C: AttemptCommitter,
{
    /// Commits a cancelled terminal for one prepared D-7 without executor dispatch.
    ///
    /// Used when a requested D-6 is declined, cancelled, or expired: the prepared
    /// D-7 must close to `cancelled/not_started` rather than remain prepared or
    /// dispatch against a non-accepted approval.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionPending`] when the conditional durable write did not win.
    pub(crate) fn cancel_prepared_attempt(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        attempt: &mut ExecutionAttempt,
    ) -> Result<ToolExecutionOutcome, ExecutionPending> {
        let binding = attempt.binding().clone();
        match self.lease.check_current(&binding) {
            LeaseCheck::Current => {}
            LeaseCheck::Fenced => {
                return Err(ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::OwnerFencedBeforeDispatch,
                    effect_state: EffectState::NotStarted,
                });
            }
            LeaseCheck::Unavailable => {
                return Err(ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::LeaseUnavailable,
                    effect_state: EffectState::NotStarted,
                });
            }
        }
        self.commit_terminal(
            authority,
            attempt,
            ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::NotStarted,
            },
            ExecutionStatus::Cancelled,
            DispatchPhase::BeforeDispatch,
        )
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
            return self.commit_reserved_terminal(
                authority,
                attempt,
                ToolExecutionOutcome::TimedOut {
                    effect_state: EffectState::Unknown,
                },
                ExecutionStatus::TimedOut,
                DispatchPhase::AfterDispatch,
                TerminalReservationFailure::HoldForReconciliation,
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
            return self.commit_reserved_terminal(
                authority,
                attempt,
                ToolExecutionOutcome::TimedOut {
                    effect_state: EffectState::Unknown,
                },
                ExecutionStatus::TimedOut,
                DispatchPhase::AfterDispatch,
                TerminalReservationFailure::HoldForReconciliation,
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
        self.commit_reserved_terminal(
            authority,
            attempt,
            outcome,
            status,
            DispatchPhase::AfterDispatch,
            TerminalReservationFailure::HoldForReconciliation,
        )
    }
}
