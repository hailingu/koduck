// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Shared C-5 conditional commitment for an already-reserved D-7 terminal.

use crate::domain::execution::{ExecutionAttempt, ExecutionStatus, TurnExecutionAuthority};

use super::execution::{
    AttemptCommitError, AttemptCommitResult, AttemptCommitter, DispatchPhase, ExecutionCoordinator,
    ExecutionFailure, ExecutionPending, IsolatedExecutor, LeaseValidator, ToolExecutionOutcome,
};

/// Determines whether an unresolved terminal write reopens the cataloged D-7.
#[derive(Clone, Copy)]
pub(super) enum TerminalReservationFailure {
    /// No external effect has been requested, so another owner may recover it.
    ReleaseBeforeExternalEffect,
    /// An executor side effect was requested; only reconciliation may proceed.
    HoldForReconciliation,
}

impl<E, L, C> ExecutionCoordinator<E, L, C>
where
    E: IsolatedExecutor,
    L: LeaseValidator,
    C: AttemptCommitter,
{
    /// Commits a terminal whose D-7 reservation has already been claimed.
    ///
    /// Cancellation claims this reservation before its external side effect;
    /// dispatch commits use the same path after acquiring it immediately before
    /// their conditional canonical write.
    pub(super) fn commit_reserved_terminal(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        attempt: &mut ExecutionAttempt,
        outcome: ToolExecutionOutcome,
        status: ExecutionStatus,
        dispatch_phase: DispatchPhase,
        reservation_failure: TerminalReservationFailure,
    ) -> Result<ToolExecutionOutcome, ExecutionPending> {
        let binding = attempt.binding().clone();
        let (result, canonical_terminal_known) =
            match self.committer.commit_outcome(&binding, &outcome) {
                Ok(AttemptCommitResult::Won) => (
                    if authority.mirror_terminal(attempt, status).is_err() {
                        Err(ExecutionPending::ReconciliationRequired {
                            code: ExecutionFailure::TerminalConflict,
                            effect_state: outcome.effect_state(),
                        })
                    } else {
                        Ok(outcome)
                    },
                    true,
                ),
                Ok(AttemptCommitResult::Existing(existing)) => {
                    if existing.binding() != &binding {
                        return Err(ExecutionPending::ReconciliationRequired {
                            code: ExecutionFailure::TerminalConflict,
                            effect_state: outcome.effect_state(),
                        });
                    }
                    let existing = existing.outcome().clone();
                    (
                        if authority
                            .mirror_terminal(attempt, existing.status())
                            .is_err()
                        {
                            Err(ExecutionPending::ReconciliationRequired {
                                code: ExecutionFailure::TerminalConflict,
                                effect_state: existing.effect_state(),
                            })
                        } else {
                            Ok(existing)
                        },
                        true,
                    )
                }
                Err(error) => {
                    let canonical_terminal_known = matches!(error, AttemptCommitError::Conflict);
                    (
                        Err(ExecutionPending::ReconciliationRequired {
                            code: match error {
                                AttemptCommitError::Fenced => match dispatch_phase {
                                    DispatchPhase::BeforeDispatch => {
                                        ExecutionFailure::OwnerFencedBeforeDispatch
                                    }
                                    DispatchPhase::AfterDispatch => {
                                        ExecutionFailure::OwnerFencedAfterDispatch
                                    }
                                },
                                AttemptCommitError::Unavailable => {
                                    ExecutionFailure::DurabilityUnavailable
                                }
                                AttemptCommitError::Conflict => ExecutionFailure::TerminalConflict,
                            },
                            effect_state: outcome.effect_state(),
                        }),
                        canonical_terminal_known,
                    )
                }
            };
        if result.is_err()
            && !canonical_terminal_known
            && matches!(
                reservation_failure,
                TerminalReservationFailure::ReleaseBeforeExternalEffect
            )
        {
            authority.release_terminal_reservation(attempt);
        }
        result
    }
}
