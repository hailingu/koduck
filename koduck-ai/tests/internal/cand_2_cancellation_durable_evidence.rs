// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Ambiguous durable prepared-close evidence must remain truthful for
//! reconciliation when another instance may have progressed the D-7.

use super::*;

#[test]
fn unavailable_prepared_close_reports_unknown_effect_evidence() {
    // A timed-out prepared-only close can race a remote durable claim. The
    // local mirror remains prepared, but it cannot prove that the other owner
    // has not started the effect (ADR-0003 TC-10/TC-12).
    let harness = Harness::new();
    let (mut authority, mut attempt) = harness.prepared();
    let mut coordinator = ExecutionCoordinator::new(
        executor(CancelAcknowledgement::NotAcknowledged),
        AlwaysCurrentLease,
        UnavailableCommitter { calls: 0 },
    );

    assert_eq!(
        coordinator.cancel_prepared_attempt(&mut authority, &mut attempt),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::DurabilityUnavailable,
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(coordinator.executor().dispatches, 0);
    assert_eq!(coordinator.committer().calls, 1);
}
