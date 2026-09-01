// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Shared C-5 conditional commitment for an already-reserved D-7 terminal.

use crate::domain::execution::{
    ExactActionBinding, ExecutionAttempt, ExecutionStatus, TurnExecutionAuthority,
};

use super::attempt_store::DurableAttemptTransitions;
use super::execution::{
    AttemptCommitError, AttemptCommitResult, AttemptCommitter, CanonicalAttemptTerminal,
    DispatchPhase, ExecutionCoordinator, ExecutionPending, IsolatedExecutor, LeaseValidator,
    ToolExecutionOutcome,
};
use super::executor_envelope::ExecutionFailure;
use super::tool_projection::attempt_version;

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
    C: AttemptCommitter + DurableAttemptTransitions,
{
    /// Commits a terminal whose D-7 reservation has already been claimed.
    ///
    /// Cancellation claims this reservation before its external side effect;
    /// dispatch commits use the same path after acquiring it immediately before
    /// their conditional canonical write.
    #[allow(
        clippy::too_many_arguments,
        reason = "each parameter is one independently validated settlement dimension"
    )]
    pub(super) fn commit_reserved_terminal_with_ownership(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        attempt: &mut ExecutionAttempt,
        outcome: ToolExecutionOutcome,
        status: ExecutionStatus,
        dispatch_phase: DispatchPhase,
        reservation_failure: TerminalReservationFailure,
        interruption_owned: bool,
    ) -> Result<ToolExecutionOutcome, ExecutionPending> {
        let binding = attempt.binding().clone();
        let committer_result = if interruption_owned {
            self.committer
                .commit_outcome_as_interruption(&binding, &outcome)
        } else {
            self.committer.commit_outcome(&binding, &outcome)
        };
        let (result, canonical_terminal_known) = match committer_result {
            Ok(AttemptCommitResult::Won) => {
                won_terminal_result(authority, attempt, outcome, status)
            }
            Ok(AttemptCommitResult::Existing(existing)) => {
                existing_terminal_result(authority, attempt, &binding, &outcome, &existing)
            }
            Err(error) => commit_error_result(error, &outcome, dispatch_phase),
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

/// Mirrors the won terminal into the local catalog and returns it; the
/// canonical terminal is known even when the mirror conflicts.
fn won_terminal_result(
    authority: &mut TurnExecutionAuthority,
    attempt: &mut ExecutionAttempt,
    outcome: ToolExecutionOutcome,
    status: ExecutionStatus,
) -> (Result<ToolExecutionOutcome, ExecutionPending>, bool) {
    if authority.mirror_terminal(attempt, status).is_err() {
        return (
            Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::TerminalConflict,
                effect_state: outcome.effect_state(),
            }),
            true,
        );
    }
    (Ok(outcome), true)
}

/// Adopts an idempotent canonical terminal that already existed: its binding
/// and persisted version must agree with the canonical D-7 transition before
/// it is mirrored and returned. A binding or version disagreement conflicts
/// without resolving the canonical terminal, so the caller retains the
/// reservation (reported as terminal-known, matching the immediate error
/// return this path replaces).
fn existing_terminal_result(
    authority: &mut TurnExecutionAuthority,
    attempt: &mut ExecutionAttempt,
    binding: &ExactActionBinding,
    outcome: &ToolExecutionOutcome,
    existing: &CanonicalAttemptTerminal,
) -> (Result<ToolExecutionOutcome, ExecutionPending>, bool) {
    if existing.binding() != binding {
        return (
            Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::TerminalConflict,
                effect_state: outcome.effect_state(),
            }),
            true,
        );
    }
    let persisted_version = existing.version();
    let existing = existing.outcome().clone();
    if persisted_version != attempt_version(existing.status()) {
        // A replayed or competing-writer terminal whose persisted version
        // contradicts the canonical D-7 transition version is a conflict:
        // projecting it would fabricate a canonical version, so
        // reconciliation owns the next transition.
        return (
            Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::TerminalConflict,
                effect_state: existing.effect_state(),
            }),
            true,
        );
    }
    if authority
        .mirror_terminal(attempt, existing.status())
        .is_err()
    {
        return (
            Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::TerminalConflict,
                effect_state: existing.effect_state(),
            }),
            true,
        );
    }
    (Ok(existing), true)
}

/// Maps a failed terminal commit to its reconciliation requirement. The
/// canonical terminal is known only for a plain conflict, which proves another
/// writer's terminal existed.
fn commit_error_result(
    error: AttemptCommitError,
    outcome: &ToolExecutionOutcome,
    dispatch_phase: DispatchPhase,
) -> (Result<ToolExecutionOutcome, ExecutionPending>, bool) {
    let canonical_terminal_known = matches!(error, AttemptCommitError::Conflict);
    let code = match error {
        AttemptCommitError::Fenced => match dispatch_phase {
            DispatchPhase::BeforeDispatch => ExecutionFailure::OwnerFencedBeforeDispatch,
            DispatchPhase::AfterDispatch => ExecutionFailure::OwnerFencedAfterDispatch,
        },
        AttemptCommitError::Unavailable => ExecutionFailure::DurabilityUnavailable,
        AttemptCommitError::Conflict => ExecutionFailure::TerminalConflict,
    };
    (
        Err(ExecutionPending::ReconciliationRequired {
            code,
            effect_state: outcome.effect_state(),
        }),
        canonical_terminal_known,
    )
}
