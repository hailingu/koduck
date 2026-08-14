use super::*;

/// Lease that is Current for the pre-claim check and Unavailable right after
/// the dispatch claim, modelling ownership evidence lost mid-sequence.
struct UnavailablePostClaimLease {
    checks: usize,
}

impl LeaseValidator for UnavailablePostClaimLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        self.checks += 1;
        if self.checks == 1 {
            LeaseCheck::Current
        } else {
            LeaseCheck::Unavailable
        }
    }
}

#[test]
fn post_claim_lease_unavailability_holds_the_attempt_for_reconciliation() {
    let harness = Harness::new();
    let (mut authority, mut attempt) = harness.prepared();

    // Through the full driver path the lease reads Current at preparation and
    // at the pre-claim check, then loses ownership evidence right after the
    // dispatch claim; here the coordinator consumes the post-claim pair
    // directly: Current (pre-claim), Unavailable (post-claim).
    let mut dispatch_coordinator = ExecutionCoordinator::new(
        executor(CancelAcknowledgement::Acknowledged(
            CancelledEffectState::NotStarted,
        )),
        UnavailablePostClaimLease { checks: 0 },
        WinningCommitter { calls: 0 },
    );
    let result =
        dispatch_coordinator.execute(&mut authority, None, &mut attempt, 1_000, &mut || 1_000);
    assert!(
        matches!(
            result,
            Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::LeaseUnavailable,
                effect_state: EffectState::NotStarted,
            })
        ),
        "post-claim lease unavailability is a typed reconciliation: {result:?}"
    );
    assert_eq!(
        dispatch_coordinator.executor().dispatches,
        0,
        "the executor is never dispatched"
    );

    // The claim marked the D-7 Running; its held terminal reservation keeps the
    // never-dispatched attempt out of the cancellation flow once the lease
    // recovers, so no executor cancellation is sent for an effect that was
    // never requested (TC-10).
    let mut cancellations = coordinator(executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::NotStarted,
    )));
    let outcome = harness.interrupter().interrupt(
        &mut cancellations,
        &mut NoPendingApprovals,
        &harness.tenant,
        harness.thread,
        harness.turn,
        &mut || 1_000,
    );
    assert!(
        matches!(
            outcome,
            Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::TerminalConflict,
                ..
            })
        ),
        "the reserved attempt requires reconciliation, not cancellation: {outcome:?}"
    );
    assert_eq!(
        cancellations.executor().cancels,
        0,
        "no executor cancellation is sent for a never-dispatched effect"
    );
}

/// Lease that is Current through the dispatch claim, Unavailable right after
/// the executor returns, and Current again afterwards, modelling a validator
/// that transiently loses ownership evidence mid-flight.
struct UnavailableAfterResponseLease {
    checks: usize,
}

impl LeaseValidator for UnavailableAfterResponseLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        self.checks += 1;
        if self.checks == 3 {
            LeaseCheck::Unavailable
        } else {
            LeaseCheck::Current
        }
    }
}

#[test]
fn post_dispatch_lease_unavailability_holds_the_executed_attempt_for_reconciliation() {
    let harness = Harness::new();
    let (mut authority, mut attempt) = harness.prepared();
    let mut scripted = executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::Started,
    ));
    scripted.response = Ok(response(EffectState::Started, b"result"));
    let mut dispatch_coordinator = ExecutionCoordinator::new(
        scripted,
        UnavailableAfterResponseLease { checks: 0 },
        WinningCommitter { calls: 0 },
    );

    // Pre-claim Current, post-claim Current, post-dispatch Unavailable: the
    // executor has already returned an observed started effect when ownership
    // evidence is lost.
    let result =
        dispatch_coordinator.execute(&mut authority, None, &mut attempt, 1_000, &mut || 1_000);
    assert!(
        matches!(
            result,
            Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::LeaseUnavailable,
                effect_state: EffectState::Started,
            })
        ),
        "post-dispatch lease unavailability is a typed reconciliation: {result:?}"
    );
    assert_eq!(
        dispatch_coordinator.executor().dispatches,
        1,
        "the executor was dispatched before ownership evidence was lost"
    );

    // Once the lease recovers, the held terminal reservation keeps the
    // already-executed attempt out of the cancellation flow: interruption must
    // not send an executor cancellation nor commit a cancellation terminal for
    // an effect that reconciliation owns (TC-07/TC-10).
    let mut cancellations = coordinator(executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::Started,
    )));
    let outcome = harness.interrupter().interrupt(
        &mut cancellations,
        &mut NoPendingApprovals,
        &harness.tenant,
        harness.thread,
        harness.turn,
        &mut || 1_000,
    );
    assert!(
        matches!(
            outcome,
            Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::TerminalConflict,
                ..
            })
        ),
        "the reserved attempt requires reconciliation, not cancellation: {outcome:?}"
    );
    assert_eq!(
        cancellations.executor().cancels,
        0,
        "no executor cancellation is sent for an already-executed effect"
    );
    assert_eq!(
        cancellations.committer().calls,
        0,
        "no cancellation terminal is committed for an already-executed effect"
    );
}
