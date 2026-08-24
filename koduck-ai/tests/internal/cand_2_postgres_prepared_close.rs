// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Prepared-only close legs of the canonical `PostgreSQL` harness: the
//! durable cancellation is a compare-and-set from `prepared`, so a racing
//! claimant that progressed the row keeps its canonical state (ADR-0003
//! TC-10/TC-12).

use koduck_ai::application::{
    AttemptInsertResolution, DispatchClaimResolution, ExecutionAttemptStore,
    PreparedCloseResolution,
};
use koduck_ai::domain::execution::ExecutionStatus;
use koduck_ai::domain::tool::Effect;

use super::attempts::{attempt_store, insert_prepared, prepared_binding};
use super::harness;

#[test]
fn prepared_only_close_wins_from_a_still_prepared_row() {
    let Some(harness) = harness() else {
        return;
    };
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    let binding = prepared_binding(Effect::ReadData);
    assert_eq!(
        insert_prepared(&harness, &mut store, &binding, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );

    // The compare-and-set closes the unclaimed row canonically.
    assert_eq!(
        ExecutionAttemptStore::cancel_prepared_attempt(&mut store, &binding),
        Ok(PreparedCloseResolution::Won { version: 3 }),
    );
    let row: (String, Option<String>, i64) = harness
        .runtime
        .block_on(async {
            sqlx::query_as(
                "SELECT status, effect_state, version FROM tool_execution_attempts \
             WHERE tenant_id = $1 AND attempt_id = $2",
            )
            .bind(binding.tenant_id().as_str())
            .bind(binding.attempt_id().as_uuid())
            .fetch_one(&harness.pool)
            .await
        })
        .expect("closed row is readable");
    assert_eq!(
        row,
        ("cancelled".to_owned(), Some("not_started".to_owned()), 3)
    );
}

#[test]
fn prepared_only_close_never_cancels_a_claimed_row() {
    // Another claimant that claimed this exact identity between the
    // coordinator's claim loss and its close must keep its canonical running
    // row untouched: the close reports Progressed instead of rewriting a
    // dispatched row to cancelled without an executor cancellation
    // (ADR-0003 TC-10/TC-12).
    let Some(harness) = harness() else {
        return;
    };
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    let binding = prepared_binding(Effect::ReadData);
    assert_eq!(
        insert_prepared(&harness, &mut store, &binding, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );
    assert_eq!(
        store.claim_running(&binding, 2_000),
        Ok(DispatchClaimResolution::Claimed { version: 2 }),
    );

    assert_eq!(
        ExecutionAttemptStore::cancel_prepared_attempt(&mut store, &binding),
        Ok(PreparedCloseResolution::Progressed {
            status: ExecutionStatus::Running,
            version: 2,
        }),
    );
    let row: (String, i64) = harness
        .runtime
        .block_on(async {
            sqlx::query_as(
                "SELECT status, version FROM tool_execution_attempts \
             WHERE tenant_id = $1 AND attempt_id = $2",
            )
            .bind(binding.tenant_id().as_str())
            .bind(binding.attempt_id().as_uuid())
            .fetch_one(&harness.pool)
            .await
        })
        .expect("claimed row is readable");
    assert_eq!(row, ("running".to_owned(), 2));
}

#[test]
fn prepared_only_close_reports_a_fenced_owner_without_mutation() {
    use sqlx::Row as _;

    let Some(harness) = harness() else {
        return;
    };
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    let binding = prepared_binding(Effect::ReadData);
    assert_eq!(
        insert_prepared(&harness, &mut store, &binding, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_leases SET fenced = TRUE \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(binding.tenant_id().as_str())
        .bind(binding.thread_id().as_uuid())
        .bind(binding.turn_id().as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture lease is fenced");
    });

    assert_eq!(
        ExecutionAttemptStore::cancel_prepared_attempt(&mut store, &binding),
        Ok(PreparedCloseResolution::Fenced),
    );
    let row = harness.runtime.block_on(async {
        sqlx::query(
            "SELECT status, version FROM tool_execution_attempts \
             WHERE tenant_id = $1 AND attempt_id = $2",
        )
        .bind(binding.tenant_id().as_str())
        .bind(binding.attempt_id().as_uuid())
        .fetch_one(&harness.pool)
        .await
        .expect("fenced row is readable")
    });
    assert_eq!(
        (
            row.try_get::<String, _>("status").expect("status"),
            row.try_get::<i64, _>("version").expect("version")
        ),
        ("prepared".to_owned(), 1),
        "a fenced owner must not mutate the canonical row",
    );
}

#[test]
fn a_won_prepared_close_appends_its_audit_record_atomically() {
    // The prepared-only cancellation terminal commits with its correlated
    // audit record in one transaction, matching every other D-7 terminal
    // (ADR-0003 TC-14).
    let Some(harness) = harness() else {
        return;
    };
    let binding = prepared_binding(koduck_ai::domain::tool::Effect::ReadData);
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    assert_eq!(
        insert_prepared(&harness, &mut store, &binding, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::cancel_prepared_attempt(
            &mut store, &binding
        ),
        Ok(PreparedCloseResolution::Won { version: 3 }),
    );
    let audit: Option<String> = harness
        .runtime
        .block_on(async {
            sqlx::query_scalar(
                "SELECT record FROM tool_audit_records \
                 WHERE tenant_id = $1 AND turn_id = $2",
            )
            .bind(binding.tenant_id().as_str())
            .bind(binding.turn_id().as_uuid())
            .fetch_optional(&harness.pool)
            .await
        })
        .expect("audit rows are readable");
    let audit = audit.expect("the prepared-only close appends its audit record");
    assert!(
        audit.contains(&binding.attempt_id().as_uuid().to_string()),
        "the audit record correlates the attempt"
    );
    assert!(audit.contains("cancelled"), "found {audit}");
}
