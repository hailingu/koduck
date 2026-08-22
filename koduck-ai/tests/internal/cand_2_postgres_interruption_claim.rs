// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Durable interruption-barrier claim regression (ADR-0003 TC-10/TC-12).

use koduck_ai::adapters::execution::DisabledExecutor;
use koduck_ai::application::{
    ExecutionAttemptInterruptionGuard, ExecutionAttemptStore, ExecutionCoordinator,
    ExecutionFailure, ExecutionPending, InterruptionBarrierResolution, LeaseCheck, LeaseValidator,
    ToolExecutionRuntimeRoot,
};
use koduck_ai::domain::execution::ExactActionBinding;

use super::attempts::attempt_store;
use super::harness;
use super::production_path::prepared_sealed_binding;

/// Keeps local preparation focused on the durable store's interruption barrier.
#[derive(Clone, Copy)]
struct AlwaysCurrentLease;

impl LeaseValidator for AlwaysCurrentLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        LeaseCheck::Current
    }
}

#[test]
fn durable_interruption_barrier_keeps_effect_evidence_unknown_for_reconciliation() {
    // The production claim winner rejects an interrupted Turn before any
    // dispatch. Its losing outcome must remain interruption_requested, not
    // concurrent_attempt, because no sibling D-7 owns the running slot. The
    // loser-side reads are not one canonical snapshot, so they also cannot
    // prove another claimant did not progress this identity before the
    // barrier became visible; reconciliation must retain unknown evidence.
    let Some(harness) = harness() else {
        return;
    };
    let binding = prepared_sealed_binding(&harness);
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    assert!(matches!(
        store.insert_prepared(&binding, 1_000),
        Ok(koduck_ai::application::AttemptInsertResolution::Inserted)
    ));
    store
        .begin_interruption(binding.tenant_id(), binding.thread_id(), binding.turn_id())
        .expect("the durable interruption barrier commits");

    let root = ToolExecutionRuntimeRoot::issue();
    let mut preparer = root.runtime().preparer(AlwaysCurrentLease);
    let (mut authority, mut attempt) = preparer
        .prepare(binding.clone())
        .expect("the local process can prepare the already-canonical identity");
    let mut coordinator = ExecutionCoordinator::new(
        DisabledExecutor,
        AlwaysCurrentLease,
        attempt_store(harness.pool.clone(), &harness.runtime),
    );
    let mut now = || 2_000_u64;

    assert_eq!(
        coordinator.execute(&mut authority, None, &mut attempt, 2_000, &mut now),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::InterruptionRequested,
            effect_state: koduck_ai::application::EffectState::Unknown,
        })
    );
    let status: String = harness
        .runtime
        .block_on(async {
            sqlx::query_scalar(
                "SELECT status FROM tool_execution_attempts WHERE tenant_id = $1 AND attempt_id = $2",
            )
            .bind(binding.tenant_id().as_str())
            .bind(binding.attempt_id().as_uuid())
            .fetch_one(&harness.pool)
            .await
        })
        .expect("canonical D-7 status is readable");
    assert_eq!(
        status, "prepared",
        "the losing claim must not rewrite the interrupted row"
    );
}

#[test]
fn interruption_barrier_loser_defers_a_terminal_turn_to_history_arbitration() {
    let Some(harness) = harness() else {
        return;
    };
    let binding = prepared_sealed_binding(&harness);
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turns SET status = 'completed' \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(binding.tenant_id().as_str())
        .bind(binding.thread_id().as_uuid())
        .bind(binding.turn_id().as_uuid())
        .execute(&harness.pool)
        .await
        .expect("a concurrent terminal wins before the interruption barrier");
    });

    assert_eq!(
        attempt_store(harness.pool.clone(), &harness.runtime).begin_interruption(
            binding.tenant_id(),
            binding.thread_id(),
            binding.turn_id(),
        ),
        Ok(InterruptionBarrierResolution::NonDispatchable),
        "the C-5 store leaves a terminal race for the history boundary to classify"
    );
}
