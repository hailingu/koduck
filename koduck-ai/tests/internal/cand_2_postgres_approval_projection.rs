// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Crash-window regressions for canonical D-6 terminal projection recovery.

use koduck_ai::adapters::history::postgres::{PostgresExecutor, SqlxPostgresExecutor};
use koduck_ai::application::{
    AcceptedTurn, ApprovalDecisionResolution, ApprovalInsertResolution, ApprovalRecordStore,
    NewItem, TurnHistory,
};
use koduck_ai::domain::execution::{ApprovalDecision, ApprovalRequest, ApprovalStatus};
use koduck_ai::domain::{Item, ItemPayload, TerminalOutcome, TrustContext};

use super::{Harness, approver, attempts, harness, requested_approval};

/// Seeds a requested projection, commits its canonical accepted terminal, and
/// deliberately omits the terminal projection to reproduce the crash window.
fn accepted_without_terminal_projection(
    harness: &mut Harness,
) -> (ApprovalRequest, SqlxPostgresExecutor) {
    let approval = requested_approval(1_000, 60_000);
    attempts::seed_owner_rows(
        harness,
        approval.tenant_id(),
        approval.binding().thread_id(),
        approval.binding().turn_id(),
        approval.binding().lease_generation(),
    );
    assert_eq!(
        harness.store.insert_requested(&approval, "requester"),
        Ok(ApprovalInsertResolution::Inserted),
    );
    let executor =
        SqlxPostgresExecutor::new(harness.pool.clone(), harness.runtime.handle().clone());
    let accepted = AcceptedTurn::new(
        approval.tenant_id().clone(),
        approval.binding().thread_id(),
        approval.binding().turn_id(),
        approval.binding().lease_generation(),
        Item::new(
            1,
            ItemPayload::UserMessage {
                content: "approval projection crash fixture".to_owned(),
            },
        ),
    );
    executor
        .append_tool_projection(
            &accepted,
            vec![NewItem::ApprovalStatus {
                approval_id: approval.approval_id(),
                attempt_id: approval.binding().attempt_id(),
                status: ApprovalStatus::Requested,
                decision: None,
                version: 1,
            }],
        )
        .expect("requested projection is durable");
    assert_eq!(
        harness.store.resolve_decision(
            approval.approval_id(),
            approval.tenant_id(),
            approval.binding().thread_id(),
            "requester",
            ApprovalDecision::Accepted,
            &approver("approver-a"),
            2_000,
        ),
        Ok(ApprovalDecisionResolution::Won {
            decision: ApprovalDecision::Accepted,
            version: 2,
        }),
    );
    (approval, executor)
}

/// Verifies the recovered canonical approval terminal immediately precedes
/// the supplied Turn terminal and retains the exact D-6 binding.
fn assert_accepted_projection_precedes(
    replay: &[Item],
    approval: &ApprovalRequest,
    terminal: &TerminalOutcome,
) {
    assert!(
        matches!(
            replay,
            [requested, accepted, recovered_terminal]
                if matches!(
                    requested.payload,
                    ItemPayload::ApprovalStatus {
                        approval_id,
                        attempt_id,
                        status: ApprovalStatus::Requested,
                        decision: None,
                        version: 1,
                    } if approval_id == approval.approval_id()
                        && attempt_id == approval.binding().attempt_id()
                ) && matches!(
                    accepted.payload,
                    ItemPayload::ApprovalStatus {
                        approval_id,
                        attempt_id,
                        status: ApprovalStatus::Accepted,
                        decision: Some(ApprovalDecision::Accepted),
                        version: 2,
                    } if approval_id == approval.approval_id()
                        && attempt_id == approval.binding().attempt_id()
                ) && recovered_terminal.payload == ItemPayload::Terminal(terminal.clone())
        ),
        "the missing accepted D-6 projection precedes the Turn terminal: {replay:?}",
    );
}

#[test]
fn lease_recovery_backfills_a_committed_approval_terminal_before_the_turn_terminal() {
    // Removing canonical terminal backfill from expiry recovery leaves the
    // append-only view permanently at requested after this Turn terminalizes.
    let Some(mut harness) = harness() else {
        return;
    };
    let (approval, executor) = accepted_without_terminal_projection(&mut harness);
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_leases SET renewed_at = CURRENT_TIMESTAMP - INTERVAL '1 hour',
                                    expires_at = CURRENT_TIMESTAMP - INTERVAL '55 minutes'
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(approval.tenant_id().as_str())
        .bind(approval.binding().thread_id().as_uuid())
        .bind(approval.binding().turn_id().as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture lease expires");
    });
    let key = koduck_ai::adapters::history::postgres::LeaseKey::new(
        approval.tenant_id().clone(),
        approval.binding().thread_id(),
        approval.binding().turn_id(),
        approval.binding().lease_generation(),
    );
    let mut history = koduck_ai::adapters::history::postgres::PostgresTurnHistory::new(executor);
    assert_eq!(
        history.reconcile_expired(&key, koduck_ai::adapters::history::postgres::unix_time_ms(),),
        Ok(koduck_ai::adapters::history::postgres::ReconcileOutcome::Cancelled),
    );
    let replay = history
        .replay(approval.tenant_id(), approval.binding().turn_id())
        .expect("recovered Turn history is readable");
    assert_accepted_projection_precedes(&replay, &approval, &TerminalOutcome::Cancelled);
}

#[test]
fn foreground_interruption_backfills_a_committed_approval_terminal() {
    // Restricting interruption recovery to cancellation-owned terminals drops
    // an accepted decision that committed before the interruption won.
    let Some(mut harness) = harness() else {
        return;
    };
    let (approval, executor) = accepted_without_terminal_projection(&mut harness);
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turns SET interrupting = TRUE
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(approval.tenant_id().as_str())
        .bind(approval.binding().thread_id().as_uuid())
        .bind(approval.binding().turn_id().as_uuid())
        .execute(&harness.pool)
        .await
        .expect("interruption barrier wins after the decision");
    });
    let trust = TrustContext::new(approval.tenant_id().clone(), "d7-attempt-fixture")
        .expect("valid fixture owner");
    executor
        .request_interrupt(&trust, approval.binding().turn_id(), Vec::new())
        .expect("foreground interruption commits");
    let replay = executor
        .replay(approval.tenant_id(), approval.binding().turn_id())
        .expect("interrupted Turn history is readable");
    assert_accepted_projection_precedes(&replay, &approval, &TerminalOutcome::Interrupted);
}
