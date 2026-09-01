// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Pinning legs for the interruption-side terminal-commit and cancellation
//! boundaries (ADR-0003 TC-10/TC-12).
//!
//! These tests pin the reservation-retention semantics of the reserved
//! terminal commit on the interruption path — an existing canonical terminal
//! for another identity conflicts while the local reservation stays held,
//! and a terminal-conflicting prepared close releases its reservation — and
//! the fenced-lease and acknowledgement failure classes of the C-5
//! cancellation boundary. They are characterization legs for behavior the
//! remediation refactors must preserve, not new behavior.

use std::collections::VecDeque;

use koduck_ai::application::{
    AttemptCommitError, AttemptCommitResult, CancelAcknowledgement, CanonicalAttemptTerminal,
    EffectState, ExecutionCoordinator, ExecutionFailure, ExecutionPending, InterruptionOutcome,
    LeaseCheck, LeaseValidator, PendingApprovalCancellation, PendingApprovalCanceller,
    ToolExecutionOutcome,
};
use koduck_ai::domain::execution::{ExactActionBinding, ExecutionAttempt, ExecutionStatus};
use koduck_ai::domain::{ThreadId, TurnId};

use super::{
    AlwaysCurrentLease, Harness, NoPendingApprovals, SequencedCommitter, SequencedLease,
    WinningCommitter, coordinator, executor, sealed_binding,
};

/// A prepared-close terminal conflict keeps the caller's reconciliation error
/// while releasing the failed reservation, so a later interruption can still
/// close the same prepared D-7 (ADR-0003 TC-10).
#[test]
fn terminal_conflicting_prepared_close_surfaces_and_releases_the_reservation() {
    let harness = Harness::new();
    let (_authority, _attempt) = harness.prepared();
    let mut conflicting = ExecutionCoordinator::new(
        executor(CancelAcknowledgement::NotAcknowledged),
        AlwaysCurrentLease,
        SequencedCommitter {
            calls: 0,
            results: VecDeque::from([Err(AttemptCommitError::Conflict)]),
        },
    );

    // Another owner progressed the durable row, so the prepared close loses
    // and the interruption surfaces the reconciliation requirement.
    assert_eq!(
        harness.interrupter().interrupt(
            &mut conflicting,
            &mut koduck_ai::application::NoToolAudits,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::TerminalConflict,
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(conflicting.committer().calls, 1);
    assert_eq!(conflicting.executor().cancels, 0);

    // The failed close released the prepared attempt's reservation: a later
    // interruption closes it instead of failing on a still-held reservation.
    let mut retry = coordinator(executor(CancelAcknowledgement::NotAcknowledged));
    assert_eq!(
        harness.interrupter().interrupt(
            &mut retry,
            &mut koduck_ai::application::NoToolAudits,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Ok(InterruptionOutcome::Closed(
            ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::NotStarted,
            }
        ))
    );
}

/// An existing canonical terminal for another identity conflicts on the
/// interruption path, and the unresolved reservation stays held: the retry
/// never reaches the executor or the committer again (ADR-0003 TC-10).
#[test]
fn interruption_existing_terminal_for_another_attempt_retains_the_reservation() {
    let harness = Harness::new();
    let (mut authority, mut running) = harness.running(1_000);
    let other = CanonicalAttemptTerminal::from_persistence(
        sealed_binding(harness.tenant.clone(), ThreadId::new(), TurnId::new()),
        2,
        ToolExecutionOutcome::Failed {
            code: ExecutionFailure::ExecutorUnavailable,
            effect_state: EffectState::Unknown,
        },
    )
    .expect("bounded terminal with a different identity");
    let mut coordinator = ExecutionCoordinator::new(
        executor(CancelAcknowledgement::NotAcknowledged),
        SequencedLease {
            decisions: VecDeque::from([true, true, true]),
        },
        SequencedCommitter {
            calls: 0,
            results: VecDeque::from([
                Ok(AttemptCommitResult::Existing(Box::new(other))),
                Ok(AttemptCommitResult::Won),
            ]),
        },
    );

    assert_eq!(
        coordinator.cancel_running_attempt(&mut authority, &mut running, &mut || 1_000),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::TerminalConflict,
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(coordinator.executor().cancels, 1);

    // The reservation stays held: the second attempt fails at the reserved
    // terminal slot without dispatching another cancellation or commit.
    assert_eq!(
        coordinator.cancel_running_attempt(&mut authority, &mut running, &mut || 1_000),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::TerminalConflict,
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(coordinator.executor().cancels, 1);
    assert_eq!(coordinator.committer().calls, 1);
    assert_eq!(running.status(), ExecutionStatus::Running);
}

/// A cancellation boundary that cannot await an outcome requires
/// reconciliation without committing any terminal (ADR-0003 TC-10).
#[test]
fn unavailable_cancellation_acknowledgement_requires_reconciliation_without_commit() {
    let harness = Harness::new();
    let (mut authority, mut running) = harness.running(1_000);
    let mut coordinator = ExecutionCoordinator::new(
        executor(CancelAcknowledgement::Unavailable),
        AlwaysCurrentLease,
        WinningCommitter { calls: 0 },
    );

    assert_eq!(
        coordinator.cancel_running_attempt(&mut authority, &mut running, &mut || 1_000),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::ExecutorUnavailable,
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(coordinator.executor().cancels, 1);
    assert_eq!(coordinator.committer().calls, 0);
    assert_eq!(running.status(), ExecutionStatus::Running);
}

/// A requested approval that cannot be cancelled fails the whole interruption
/// before any D-7 close runs (ADR-0003 TC-10).
#[test]
fn approval_cancellation_failure_surfaces_before_any_d7_close() {
    let harness = Harness::new();
    let (_authority, _attempt) = harness.approval_required_prepared();
    let mut coordinator = coordinator(executor(CancelAcknowledgement::NotAcknowledged));

    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut koduck_ai::application::NoToolAudits,
            &mut DenyingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Err(ExecutionPending::DispatchRejected {
            code: ExecutionFailure::AttemptNotRunning,
        })
    );
    assert_eq!(coordinator.committer().calls, 0);
    assert_eq!(coordinator.executor().cancels, 0);
}

/// Approval canceller whose requested D-6 transition cannot be closed.
struct DenyingApprovals;

impl PendingApprovalCanceller for DenyingApprovals {
    fn cancel_requested(
        &mut self,
        _binding: &ExactActionBinding,
    ) -> Result<PendingApprovalCancellation, ExecutionPending> {
        Err(ExecutionPending::DispatchRejected {
            code: ExecutionFailure::AttemptNotRunning,
        })
    }
}

/// A fenced prepared close reports a not-started effect without any commit;
/// its never-dispatched attempt proves the fence preceded every effect
/// (ADR-0003 TC-10).
#[test]
fn fenced_prepared_close_reports_not_started_without_any_commit() {
    let harness = Harness::new();
    let (_authority, _attempt) = harness.prepared();
    let mut fenced = ExecutionCoordinator::new(
        executor(CancelAcknowledgement::NotAcknowledged),
        SequencedLease {
            decisions: VecDeque::from([false]),
        },
        WinningCommitter { calls: 0 },
    );

    assert_eq!(
        harness.interrupter().interrupt(
            &mut fenced,
            &mut koduck_ai::application::NoToolAudits,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::OwnerFencedBeforeDispatch,
            effect_state: EffectState::NotStarted,
        })
    );
    assert_eq!(fenced.committer().calls, 0);
    assert_eq!(fenced.executor().cancels, 0);
}

/// A fenced running close reports an unknown effect and holds the running
/// attempt's reservation, so a competing owner cannot terminalize it while
/// reconciliation owns the next transition (ADR-0003 TC-10/TC-12).
#[test]
fn fenced_running_close_reports_unknown_and_holds_the_reservation() {
    let harness = Harness::new();
    let (mut authority, mut running) = harness.running(1_000);
    let mut fenced = ExecutionCoordinator::new(
        executor(CancelAcknowledgement::NotAcknowledged),
        SequencedLease {
            decisions: VecDeque::from([false, false]),
        },
        WinningCommitter { calls: 0 },
    );

    assert_eq!(
        fenced.cancel_prepared_attempt(&mut authority, &mut running),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::OwnerFencedBeforeDispatch,
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(fenced.committer().calls, 0);

    // The unresolved reservation stays held: the second fenced close surfaces
    // the reservation conflict rather than re-reserving the running attempt.
    assert_eq!(
        fenced.cancel_prepared_attempt(&mut authority, &mut running),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::TerminalConflict,
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(fenced.committer().calls, 0);
    assert_eq!(running.status(), ExecutionStatus::Running);
}

/// Lease validator whose ownership is merely undetermined, not proven fenced.
struct UnavailableLease;

impl LeaseValidator for UnavailableLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        LeaseCheck::Unavailable
    }
}

/// An undetermined lease fails the prepared close with its own failure class
/// and a not-started effect, because a prepared attempt never passed a
/// dispatch claim (ADR-0003 TC-10).
#[test]
fn undetermined_lease_fails_the_prepared_close_as_lease_unavailable() {
    let harness = Harness::new();
    let (_authority, _attempt) = harness.prepared();
    let mut undetermined = ExecutionCoordinator::new(
        executor(CancelAcknowledgement::NotAcknowledged),
        UnavailableLease,
        WinningCommitter { calls: 0 },
    );

    assert_eq!(
        harness.interrupter().interrupt(
            &mut undetermined,
            &mut koduck_ai::application::NoToolAudits,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::LeaseUnavailable,
            effect_state: EffectState::NotStarted,
        })
    );
    assert_eq!(undetermined.committer().calls, 0);
}

/// A prepared close the durable store fenced before dispatch reports the
/// owner-fenced failure with a not-started effect and releases its
/// reservation (ADR-0003 TC-10/TC-12).
#[test]
fn fenced_prepared_close_resolution_reports_owner_fenced_before_dispatch() {
    let harness = Harness::new();
    let (_authority, _attempt) = harness.prepared();
    let mut fenced_close = ExecutionCoordinator::new(
        executor(CancelAcknowledgement::NotAcknowledged),
        AlwaysCurrentLease,
        SequencedCommitter {
            calls: 0,
            results: VecDeque::from([Err(AttemptCommitError::Fenced)]),
        },
    );

    assert_eq!(
        harness.interrupter().interrupt(
            &mut fenced_close,
            &mut koduck_ai::application::NoToolAudits,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::OwnerFencedBeforeDispatch,
            effect_state: EffectState::NotStarted,
        })
    );
    assert_eq!(fenced_close.committer().calls, 1);
    assert_eq!(fenced_close.executor().cancels, 0);
}

/// A cataloged running attempt that cannot prove its start time cannot be
/// terminalized by cancellation; reconciliation owns the next transition
/// (ADR-0003 TC-10).
#[test]
fn running_attempt_without_a_start_time_requires_reconciliation() {
    let harness = Harness::new();
    let (mut authority, attempt) = harness.running(1_000);
    let mut handle = ExecutionAttempt::reconstruct(
        attempt.binding().clone(),
        ExecutionStatus::Running,
        None,
        &authority,
    );
    let mut coordinator = coordinator(executor(CancelAcknowledgement::NotAcknowledged));

    assert_eq!(
        coordinator.cancel_running_attempt(&mut authority, &mut handle, &mut || 1_000),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::TerminalConflict,
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(coordinator.executor().cancels, 0);
    assert_eq!(coordinator.committer().calls, 0);
    assert_eq!(handle.status(), ExecutionStatus::Running);
}
