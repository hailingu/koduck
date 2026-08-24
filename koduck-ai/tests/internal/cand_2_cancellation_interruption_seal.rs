// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

use super::*;

#[test]
fn interruption_seals_existing_turn_against_late_attempt_allocation() {
    let harness = Harness::new();
    let (_authority, _attempt) = harness.prepared();
    let mut coordinator = coordinator(executor(CancelAcknowledgement::NotAcknowledged));

    assert!(matches!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut koduck_ai::application::NoToolAudits,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Ok(InterruptionOutcome::Closed(_))
    ));

    let mut late_preparer = harness.runtime.preparer(AlwaysCurrentLease);
    assert!(
        late_preparer
            .prepare(sealed_binding(
                harness.tenant.clone(),
                harness.thread,
                harness.turn,
            ))
            .is_err(),
        "an interrupted Turn cannot allocate another D-7"
    );
}

#[test]
fn interruption_seals_unknown_turn_against_future_attempt_allocation() {
    let harness = Harness::new();
    let mut coordinator = coordinator(executor(CancelAcknowledgement::NotAcknowledged));

    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut koduck_ai::application::NoToolAudits,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Ok(InterruptionOutcome::NoLiveAttempt)
    );

    let mut late_preparer = harness.runtime.preparer(AlwaysCurrentLease);
    assert!(
        late_preparer
            .prepare(sealed_binding(
                harness.tenant.clone(),
                harness.thread,
                harness.turn,
            ))
            .is_err(),
        "an interrupted unknown Turn must retain a tombstone against future D-7 allocation"
    );
}
