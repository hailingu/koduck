// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Durable canonical D-7 preparation recording and dispatch-claim gate.
//!
//! These coordinator transitions hold the cross-instance half of TC-12: the
//! process-local authority still arbitrates within one process, but only a
//! won durable claim permits an executor dispatch, and only an idempotently
//! recorded prepared D-7 can ever be dispatched, cancelled, or terminalized.

use crate::domain::execution::{ExecutionAttempt, ExecutionStatus, TurnExecutionAuthority};

use super::attempt_store::{
    AttemptInsertResolution, AttemptStoreError, DispatchClaimResolution, DurableAttemptTransitions,
};
use super::execution::{
    AttemptCommitter, ExecutionCoordinator, ExecutionPending, IsolatedExecutor, LeaseValidator,
};
use super::executor_envelope::{EffectState, ExecutionFailure};

/// The canonical disposition of one local dispatch claim.
pub(super) enum CanonicalDispatchClaim {
    /// This coordinator durably changed the attempt from prepared to running.
    Won,
    /// A reread found the exact attempt already running at version two.
    ///
    /// The caller must restore the durable running view but must not dispatch
    /// the effect again, because another execution may own that transition.
    ReconciledRunning,
}

impl<E, L, C> ExecutionCoordinator<E, L, C>
where
    E: IsolatedExecutor,
    L: LeaseValidator,
    C: AttemptCommitter + DurableAttemptTransitions,
{
    /// Durably records one newly prepared D-7 before approval resolution,
    /// dispatch, or cancellation (TC-12).
    ///
    /// The canonical row this insert commits is the only target every later
    /// conditional transition — dispatch claim, cancellation, or terminal —
    /// can bind to, so a caller whose durable preparation failed closed never
    /// dispatches or terminalizes an invisible attempt. The process-local
    /// slot is already consumed at this point; a failed insert therefore
    /// fails the whole call closed rather than silently proceeding with
    /// process-local state only. An identity conflict proves this binding
    /// never committed under its exact identity, so the local attempt is
    /// closed `cancelled/not_started` before any effect instead of remaining
    /// orphan live work a later interruption must reconcile; every undecidable
    /// outcome — a lost acknowledgement that may have committed, or a row
    /// already progressed by another owner — keeps the fail-closed reservation.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionPending::ReconciliationRequired`] when the durable
    /// write did not provably commit as this exact binding, so reconciliation
    /// owns the next transition. No error variant is a final Tool result.
    pub(super) fn record_prepared_attempt(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        attempt: &mut ExecutionAttempt,
        prepared_at_millis: u64,
    ) -> Result<(), ExecutionPending> {
        match self
            .committer
            .insert_prepared(attempt.binding(), prepared_at_millis)
        {
            Ok(
                AttemptInsertResolution::Inserted
                | AttemptInsertResolution::Existing {
                    status: ExecutionStatus::Prepared,
                    ..
                },
            ) => Ok(()),
            // The canonical row for this exact identity already progressed
            // without this caller: dispatching or cancelling it locally could
            // contradict another owner's canonical state.
            Ok(AttemptInsertResolution::Existing { .. }) => {
                if authority.reserve_terminal(attempt).is_err() {
                    return Err(reconciliation_required_with_effect(
                        ExecutionFailure::TerminalConflict,
                        EffectState::Unknown,
                    ));
                }
                Err(reconciliation_required_with_effect(
                    ExecutionFailure::TerminalConflict,
                    EffectState::Unknown,
                ))
            }
            // The identity exists with different immutable fields, proving
            // this binding never committed: the local attempt can neither
            // dispatch nor terminalize, so close it cancelled now rather than
            // leaving orphan live work (TC-12/TC-13).
            Err(AttemptStoreError::IdentityConflict) => {
                if authority.reserve_terminal(attempt).is_ok() {
                    let _ = authority.mirror_terminal(attempt, ExecutionStatus::Cancelled);
                }
                Err(reconciliation_required(ExecutionFailure::TerminalConflict))
            }
            // A lost acknowledgement may hide a committed row that another
            // instance progresses before reconciliation observes it.
            Err(AttemptStoreError::Unavailable) => {
                if authority.reserve_terminal(attempt).is_err() {
                    return Err(reconciliation_required_with_effect(
                        ExecutionFailure::TerminalConflict,
                        EffectState::Unknown,
                    ));
                }
                Err(reconciliation_required_with_effect(
                    ExecutionFailure::DurabilityUnavailable,
                    EffectState::Unknown,
                ))
            }
            // The exact durable cap is a definitive rejection, so this
            // locally prepared attempt cannot ever obtain a canonical row.
            // Close it with the same best-effort cleanup as an immutable
            // identity conflict before reporting the typed rejection.
            Err(AttemptStoreError::AttemptLimit) => {
                if authority.reserve_terminal(attempt).is_ok() {
                    let _ = authority.mirror_terminal(attempt, ExecutionStatus::Cancelled);
                }
                Err(ExecutionPending::DispatchRejected {
                    code: ExecutionFailure::AttemptLimit,
                })
            }
        }
    }

    /// Claims the Turn's only durable running slot immediately after the
    /// process-local dispatch claim and before the D-3 running projection or
    /// any executor call (TC-06/TC-12).
    ///
    /// Only the single durable claim winner may dispatch: the running
    /// projection therefore never outruns the canonical running transition,
    /// and a post-claim fence cannot leave a terminal projection without its
    /// canonical cause. A fenced durable lease cannot close through this
    /// committer — every terminal write requires the bound lease to be
    /// current — so the never-dispatched attempt retains its reservation and
    /// defers to reconciliation with unknown effect evidence. The stale local
    /// mirror cannot prove the canonical row remained `prepared` before the
    /// fence (TC-07/AC-8).
    /// A concurrent durable slot owner closes its own still-`prepared`
    /// attempt through the prepared-only conditional close — a racing
    /// claimant that progressed the row keeps its canonical state — and then
    /// receives the typed `concurrent_attempt` rejection. Every undecidable
    /// durable outcome — a lost acknowledgement, another owner of this exact
    /// identity, a missing canonical row, or an identity conflict — retains
    /// the local terminal reservation and defers to reconciliation with zero
    /// executor calls.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionPending`] when the durable claim did not provably
    /// win. No error variant is a final Tool result.
    pub(super) fn claim_canonical_dispatch(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        attempt: &mut ExecutionAttempt,
        started_at_millis: u64,
    ) -> Result<CanonicalDispatchClaim, ExecutionPending> {
        let binding = attempt.binding().clone();
        match self.committer.claim_running(&binding, started_at_millis) {
            Ok(DispatchClaimResolution::Claimed { .. }) => Ok(CanonicalDispatchClaim::Won),
            Ok(DispatchClaimResolution::Interrupted) => {
                // The durable barrier prevents this claim from dispatching,
                // but this local mirror is not a canonical snapshot: another claimant may
                // have progressed this identity before the barrier became
                // visible. Retain unknown evidence and the reservation for
                // authenticated interruption reconciliation.
                if authority.reserve_terminal(attempt).is_err() {
                    return Err(reconciliation_required_with_effect(
                        ExecutionFailure::TerminalConflict,
                        EffectState::Unknown,
                    ));
                }
                Err(reconciliation_required_with_effect(
                    ExecutionFailure::InterruptionRequested,
                    EffectState::Unknown,
                ))
            }
            Ok(DispatchClaimResolution::Fenced) => {
                // A durably fenced owner can never close through this
                // committer: every terminal write requires the bound lease to
                // be current, so attempting the close is a guaranteed failure.
                // A separate loser-side read cannot prove this row remained
                // prepared before the fence, so retain unknown evidence with
                // the reservation for reconciliation (TC-07/AC-8).
                if authority.reserve_terminal(attempt).is_err() {
                    return Err(reconciliation_required_with_effect(
                        ExecutionFailure::TerminalConflict,
                        EffectState::Unknown,
                    ));
                }
                Err(reconciliation_required_with_effect(
                    ExecutionFailure::OwnerFencedBeforeDispatch,
                    EffectState::Unknown,
                ))
            }
            Ok(DispatchClaimResolution::Concurrent) => {
                // Close this never-dispatched attempt through the
                // prepared-only conditional close: if a racing claimant
                // already progressed this exact identity, its canonical state
                // survives and reconciliation owns this side (TC-10/TC-12).
                self.cancel_prepared_attempt(authority, attempt)?;
                Err(ExecutionPending::DispatchRejected {
                    code: ExecutionFailure::ConcurrentAttempt,
                })
            }
            Ok(DispatchClaimResolution::Existing {
                status: ExecutionStatus::Running,
                version: 2,
            }) => {
                // The conditional update may have committed while its
                // acknowledgement was lost. Restore the canonical D-3
                // running view, but keep the local reservation for
                // reconciliation rather than dispatching an effect twice.
                Ok(CanonicalDispatchClaim::ReconciledRunning)
            }
            Ok(DispatchClaimResolution::Existing { .. }) => {
                // The reservation keeps the undecidable attempt away from the
                // interruption boundary; the returned pending is the same
                // either way, so only the reservation outcome matters.
                let _ = authority.reserve_terminal(attempt);
                Err(reconciliation_required_with_effect(
                    ExecutionFailure::TerminalConflict,
                    EffectState::Unknown,
                ))
            }
            Err(AttemptStoreError::IdentityConflict) => {
                let _ = authority.reserve_terminal(attempt);
                Err(reconciliation_required(ExecutionFailure::TerminalConflict))
            }
            // A missing canonical row or impossible claim-time attempt limit
            // prevents this caller from proving an effect began.
            Ok(DispatchClaimResolution::NotFound) | Err(AttemptStoreError::AttemptLimit) => {
                if authority.reserve_terminal(attempt).is_err() {
                    return Err(reconciliation_required(ExecutionFailure::TerminalConflict));
                }
                Err(reconciliation_required(
                    ExecutionFailure::DurabilityUnavailable,
                ))
            }
            // A lost acknowledgement may have committed this claim while a
            // competing owner dispatched it, so reconciliation must retain
            // unknown effect evidence.
            Err(AttemptStoreError::Unavailable) => {
                if authority.reserve_terminal(attempt).is_err() {
                    return Err(reconciliation_required_with_effect(
                        ExecutionFailure::TerminalConflict,
                        EffectState::Unknown,
                    ));
                }
                Err(reconciliation_required_with_effect(
                    ExecutionFailure::DurabilityUnavailable,
                    EffectState::Unknown,
                ))
            }
        }
    }
}

/// Builds the pending outcome that hands the next transition to reconciliation.
pub(super) fn reconciliation_required(code: ExecutionFailure) -> ExecutionPending {
    reconciliation_required_with_effect(code, EffectState::NotStarted)
}

/// Builds a reconciliation outcome with the only executor evidence available.
pub(super) fn reconciliation_required_with_effect(
    code: ExecutionFailure,
    effect_state: EffectState,
) -> ExecutionPending {
    ExecutionPending::ReconciliationRequired { code, effect_state }
}
