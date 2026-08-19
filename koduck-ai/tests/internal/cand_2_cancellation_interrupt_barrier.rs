// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Regression coverage for the durable interruption barrier across C-5 and
//! CAND-1 terminal arbitration.

use std::time::{SystemTime, UNIX_EPOCH};

use koduck_ai::adapters::history::postgres::{LeaseKey, ReconcileOutcome};
use koduck_ai::application::{
    AttemptCommitError, AttemptCommitResult, AttemptCommitter, AttemptInsertResolution,
    AttemptStoreError, DispatchClaimResolution, ExecutionAttemptInterruptionGuard,
    ExecutionAttemptLiveness, ExecutionAttemptStore, HistoryError, LeaseCheck, LeaseValidator,
    ToolCallError, ToolCallExecutor, ToolConfigurationSnapshot, ToolExecutionRuntimeRoot,
    TurnCommand, TurnHistory,
};
use koduck_ai::domain::{ItemPayload, TenantId, TerminalOutcome, TrustContext, Usage};
use koduck_ai::runtime::tool_executor::BoundaryToolCallExecutor;

use super::*;
use crate::test_support::process_local_durable_claims;

/// Current C-6 answer for the process-local C-5 catalog exercised below.
#[derive(Clone, Copy)]
struct CurrentLease;

impl LeaseValidator for CurrentLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        LeaseCheck::Current
    }
}

/// Store double with a local cancellation winner and a different live D-7 in
/// another process. The durable lookup must prevent the runner from treating
/// the local close as a complete interruption.
#[derive(Clone, Copy)]
struct LocalCloseWithRemoteLive;

impl AttemptCommitter for LocalCloseWithRemoteLive {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        Ok(AttemptCommitResult::Won)
    }
}

impl ExecutionAttemptLiveness for LocalCloseWithRemoteLive {
    fn has_live_attempt(
        &mut self,
        _tenant_id: &TenantId,
        _thread_id: ThreadId,
        _turn_id: TurnId,
    ) -> Result<bool, AttemptStoreError> {
        Ok(true)
    }
}

impl ExecutionAttemptInterruptionGuard for LocalCloseWithRemoteLive {
    fn begin_interruption(
        &mut self,
        _tenant_id: &TenantId,
        _thread_id: ThreadId,
        _turn_id: TurnId,
    ) -> Result<(), AttemptStoreError> {
        Ok(())
    }
}

process_local_durable_claims!(LocalCloseWithRemoteLive);

#[test]
fn local_close_with_remote_live_attempt_requires_reconciliation() {
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust = TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let root = ToolExecutionRuntimeRoot::issue();
    let harness = Harness::with_runtime(root.runtime(), tenant, thread_id, turn_id);
    let _local_attempt = harness.prepared();
    let mut executor = BoundaryToolCallExecutor::new(
        &root,
        ToolConfigurationSnapshot::empty(),
        LocalCloseWithRemoteLive,
        CurrentLease,
        koduck_ai::application::NoToolAudits,
        koduck_ai::application::NoCanonicalTurnTerminal,
    );

    let result = executor.request_interrupt(&trust, thread_id, turn_id);

    assert!(matches!(result, Err(ToolCallError::Reconciliation(_))));
}

#[test]
fn interruption_barrier_blocks_provider_terminal_before_interrupt_commit() {
    let Some((mut durable, mut history, _pool, _runtime)) =
        super::turn_terminal::durable_backends()
    else {
        return;
    };
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust = TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let accepted = history
        .accept_initial(&TurnCommand::new(trust.clone(), None, "interrupt race").expect("command"))
        .expect("initial acceptance");

    durable
        .begin_interruption(&tenant, accepted.thread_id, accepted.turn_id)
        .expect("interruption barrier commits");

    assert_eq!(
        history.append_provider_terminal(
            &accepted,
            TerminalOutcome::Completed {
                usage: Usage::new(1, 2).expect("usage"),
            },
        ),
        Err(HistoryError::Fenced),
        "the barrier reserves terminal arbitration until C-5 closes or reconciles live D-7 work",
    );
}

#[test]
fn expired_interruption_barrier_recovers_the_turn_as_interrupted() {
    let Some((mut durable, mut history, pool, runtime)) = super::turn_terminal::durable_backends()
    else {
        return;
    };
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust = TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let accepted = history
        .accept_initial(
            &TurnCommand::new(trust.clone(), None, "interruption recovery").expect("command"),
        )
        .expect("initial acceptance");

    durable
        .begin_interruption(&tenant, accepted.thread_id, accepted.turn_id)
        .expect("interruption barrier commits before the interrupted terminal");
    runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_leases SET renewed_at = CURRENT_TIMESTAMP - INTERVAL '2 minutes', expires_at = CURRENT_TIMESTAMP - INTERVAL '1 minute' WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(tenant.as_str())
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .execute(&pool)
        .await
        .expect("expire the owner after the barrier is durable");
    });
    let now_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after the Unix epoch")
        .as_millis()
        .try_into()
        .expect("millisecond time fits u64");

    assert_eq!(
        history.reconcile_expired(
            &LeaseKey::new(
                tenant,
                accepted.thread_id,
                accepted.turn_id,
                accepted.generation,
            ),
            now_millis,
        ),
        Ok(ReconcileOutcome::Interrupted),
        "an orphaned barrier must converge to the authenticated interruption terminal",
    );
    let replayed = history
        .replay(&trust.tenant_id, accepted.turn_id)
        .expect("canonical replay");
    assert!(
        replayed
            .iter()
            .any(|item| { item.payload == ItemPayload::Terminal(TerminalOutcome::Interrupted) })
    );
}

#[test]
fn expired_interruption_barrier_closes_a_running_attempt_before_turn_terminal() {
    let Some((mut durable, mut history, pool, runtime)) = super::turn_terminal::durable_backends()
    else {
        return;
    };
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust = TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let accepted = history
        .accept_initial(
            &TurnCommand::new(trust.clone(), None, "running interruption recovery")
                .expect("command"),
        )
        .expect("initial acceptance");
    let binding = sealed_binding(tenant.clone(), accepted.thread_id, accepted.turn_id);
    assert_eq!(
        ExecutionAttemptStore::insert_prepared(&mut durable, &binding, 1_000),
        Ok(AttemptInsertResolution::Inserted)
    );
    assert_eq!(
        ExecutionAttemptStore::claim_running(&mut durable, &binding, 2_000),
        Ok(DispatchClaimResolution::Claimed { version: 2 })
    );
    durable
        .begin_interruption(&tenant, accepted.thread_id, accepted.turn_id)
        .expect("interruption barrier commits before recovery");
    runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_leases SET renewed_at = CURRENT_TIMESTAMP - INTERVAL '2 minutes', expires_at = CURRENT_TIMESTAMP - INTERVAL '1 minute' WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(tenant.as_str())
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .execute(&pool)
        .await
        .expect("expire the owner after the barrier is durable");
    });
    let now_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after the Unix epoch")
        .as_millis()
        .try_into()
        .expect("millisecond time fits u64");

    assert_eq!(
        history.reconcile_expired(
            &LeaseKey::new(
                tenant.clone(),
                accepted.thread_id,
                accepted.turn_id,
                accepted.generation,
            ),
            now_millis,
        ),
        Ok(ReconcileOutcome::Interrupted),
        "recovery must close the live D-7 before closing the Turn",
    );
    let (status, effect_state) = runtime.block_on(async {
        use sqlx::Row as _;

        let row = sqlx::query(
            "SELECT status, effect_state FROM tool_execution_attempts WHERE tenant_id = $1 AND attempt_id = $2",
        )
        .bind(tenant.as_str())
        .bind(binding.attempt_id().as_uuid())
        .fetch_one(&pool)
        .await
        .expect("the canonical attempt remains queryable");
        (
            row.try_get::<String, _>("status").expect("status"),
            row.try_get::<Option<String>, _>("effect_state")
                .expect("effect state"),
        )
    });
    assert_eq!(
        (status.as_str(), effect_state.as_deref()),
        ("timed_out", Some("unknown")),
        "an orphaned running effect has unknown outcome after lease recovery",
    );
}

#[test]
fn expired_ordinary_lease_closes_a_running_attempt_before_cancelled_turn_terminal() {
    let Some((mut durable, mut history, pool, runtime)) = super::turn_terminal::durable_backends()
    else {
        return;
    };
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust = TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let accepted = history
        .accept_initial(
            &TurnCommand::new(trust.clone(), None, "ordinary expiry with running attempt")
                .expect("command"),
        )
        .expect("initial acceptance");
    let binding = sealed_binding(tenant.clone(), accepted.thread_id, accepted.turn_id);
    assert_eq!(
        ExecutionAttemptStore::insert_prepared(&mut durable, &binding, 1_000),
        Ok(AttemptInsertResolution::Inserted)
    );
    assert_eq!(
        ExecutionAttemptStore::claim_running(&mut durable, &binding, 2_000),
        Ok(DispatchClaimResolution::Claimed { version: 2 })
    );
    runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_leases SET renewed_at = CURRENT_TIMESTAMP - INTERVAL '2 minutes', expires_at = CURRENT_TIMESTAMP - INTERVAL '1 minute' WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(tenant.as_str())
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .execute(&pool)
        .await
        .expect("expire the ordinary owner");
    });
    let now_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after the Unix epoch")
        .as_millis()
        .try_into()
        .expect("millisecond time fits u64");

    assert_eq!(
        history.reconcile_expired(
            &LeaseKey::new(
                tenant.clone(),
                accepted.thread_id,
                accepted.turn_id,
                accepted.generation,
            ),
            now_millis,
        ),
        Ok(ReconcileOutcome::Cancelled),
        "ordinary lease expiry cancels the Turn",
    );
    let (status, effect_state) = runtime.block_on(async {
        use sqlx::Row as _;

        let row = sqlx::query(
            "SELECT status, effect_state FROM tool_execution_attempts WHERE tenant_id = $1 AND attempt_id = $2",
        )
        .bind(tenant.as_str())
        .bind(binding.attempt_id().as_uuid())
        .fetch_one(&pool)
        .await
        .expect("the canonical attempt remains queryable");
        (
            row.try_get::<String, _>("status").expect("status"),
            row.try_get::<Option<String>, _>("effect_state")
                .expect("effect state"),
        )
    });
    assert_eq!(
        (status.as_str(), effect_state.as_deref()),
        ("timed_out", Some("unknown")),
        "ordinary lease recovery must not leave a running D-7 behind a cancelled Turn",
    );
}
