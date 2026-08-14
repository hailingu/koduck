// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

use super::*;

#[test]
fn failed_pre_dispatch_timeout_commit_cannot_trigger_executor_cancellation() {
    let harness = Harness::new();
    let (mut authority, mut attempt) = harness.prepared();
    let mut expiring = ExecutionCoordinator::new(
        executor(CancelAcknowledgement::NotAcknowledged),
        AlwaysCurrentLease,
        UnavailableCommitter { calls: 0 },
    );

    assert_eq!(
        expiring.execute(&mut authority, None, &mut attempt, 1_000, &mut || 31_000),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::DurabilityUnavailable,
            effect_state: EffectState::NotStarted,
        })
    );
    assert_eq!(expiring.executor().dispatches, 0);

    let mut later = coordinator(executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::NotStarted,
    )));
    assert_eq!(
        harness.interrupter().interrupt(
            &mut later,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 31_000,
        ),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::TerminalConflict,
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(later.executor().cancels, 0);
    assert_eq!(later.committer().calls, 0);
}
