// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

use super::*;
use koduck_ai::adapters::execution::DisabledExecutor;

#[test]
fn disabled_executor_does_not_report_timeout_before_the_deadline() {
    let harness = Harness::new();
    let (retained_authority, _attempt) = harness.running(1_000);
    let mut coordinator = ExecutionCoordinator::new(
        DisabledExecutor,
        AlwaysCurrentLease,
        WinningCommitter { calls: 0 },
    );

    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::ExecutorUnavailable,
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(coordinator.committer().calls, 0);
    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
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
    assert!(retained_authority.live_attempts().is_empty());
}
