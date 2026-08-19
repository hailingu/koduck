// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Durable audit-trail legs of the canonical `PostgreSQL` harness: the
//! production sink appends each bounded serialized record with its Turn
//! correlation to the canonical `tool_audit_records` table, the migration is
//! idempotent, and the schema rejects over-bound records as defense in depth
//! (ADR-0003 TC-14/AC-13).

use koduck_ai::adapters::audit::{SqlxToolAuditSink, serialize_audit_record};
use koduck_ai::application::{DenialCode, PolicyDenialContext, ToolAuditRecord, ToolAuditSink};
use koduck_ai::domain::{LeaseGeneration, TenantId, ThreadId, TurnId};

use super::harness;

#[test]
fn durable_trail_appends_each_bounded_record_with_turn_correlation() {
    let Some(harness) = harness() else {
        return;
    };
    let tenant = TenantId::new(format!("ci-{}", uuid::Uuid::new_v4())).expect("valid tenant");
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let context = PolicyDenialContext::new(
        tenant.clone(),
        thread_id,
        turn_id,
        LeaseGeneration::initial(),
    );
    let record = ToolAuditRecord::policy_denial(&context, DenialCode::DescriptorMissing, 1_000);
    let serialized = serialize_audit_record(&record).expect("the fixture record is bounded");
    let mut sink = SqlxToolAuditSink::new(harness.pool.clone(), harness.runtime.handle().clone());
    sink.record(&record, &serialized)
        .expect("the bounded record appends durably");

    let (stored_record, at_millis): (String, i64) = harness
        .runtime
        .block_on(
            sqlx::query_as(
                "SELECT record, at_millis FROM tool_audit_records \
                 WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
            )
            .bind(tenant.as_str())
            .bind(thread_id.as_uuid())
            .bind(turn_id.as_uuid())
            .fetch_one(&harness.pool),
        )
        .expect("the appended record is Turn-correlated");
    assert_eq!(stored_record, serialized);
    assert_eq!(at_millis, 1_000);

    // The additive migration re-applies idempotently at every startup.
    harness
        .runtime
        .block_on(
            sqlx::raw_sql(include_str!("../../migrations/0007_cand_2_tool_audit.sql"))
                .execute(&harness.pool),
        )
        .expect("the audit migration is idempotent");

    // Defense in depth: the schema itself rejects a record beyond the TC-14
    // byte bound even if a future caller bypassed the application check.
    let oversized = "x".repeat(16_385);
    let rejected = harness.runtime.block_on(
        sqlx::query(
            "INSERT INTO tool_audit_records \
             (tenant_id, thread_id, turn_id, at_millis, record) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(tenant.as_str())
        .bind(thread_id.as_uuid())
        .bind(turn_id.as_uuid())
        .bind(1_000_i64)
        .bind(&oversized)
        .execute(&harness.pool),
    );
    assert!(
        rejected.is_err(),
        "the schema rejects an over-bound audit record as defense in depth"
    );

    // The bound is bytes, not characters: 10,000 three-byte characters are
    // 30,000 bytes while `length()` counts only 10,000, so a character-count
    // CHECK would admit a record that occupies nearly twice the TC-14 bound.
    let multibyte_oversized = "字".repeat(10_000);
    assert_eq!(multibyte_oversized.len(), 30_000);
    let rejected = harness.runtime.block_on(
        sqlx::query(
            "INSERT INTO tool_audit_records \
             (tenant_id, thread_id, turn_id, at_millis, record) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(tenant.as_str())
        .bind(thread_id.as_uuid())
        .bind(turn_id.as_uuid())
        .bind(1_000_i64)
        .bind(&multibyte_oversized)
        .execute(&harness.pool),
    );
    assert!(
        rejected.is_err(),
        "the schema rejects a record whose UTF-8 bytes exceed the bound even \
         when its character count is within it"
    );

    // A multibyte record within the byte bound still appends.
    let multibyte_bounded = "字".repeat(5_000);
    assert_eq!(multibyte_bounded.len(), 15_000);
    harness
        .runtime
        .block_on(
            sqlx::query(
                "INSERT INTO tool_audit_records \
                 (tenant_id, thread_id, turn_id, at_millis, record) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(tenant.as_str())
            .bind(thread_id.as_uuid())
            .bind(turn_id.as_uuid())
            .bind(1_000_i64)
            .bind(&multibyte_bounded)
            .execute(&harness.pool),
        )
        .expect("a multibyte record within 16,384 bytes appends");
}
