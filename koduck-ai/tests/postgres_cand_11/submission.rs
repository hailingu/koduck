// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! Shared `CAND-18` `PostgreSQL` test helpers and the focused child modules.

use uuid::Uuid;

use koduck_ai::adapters::history::postgres::PostgresTurnHistory;
use koduck_ai::application::TurnCommand;
use koduck_ai::domain::{SubmissionId, TenantId, TrustContext};

use super::harness::{Harness, MIGRATIONS, MIGRATIONS_ONCE};

pub(crate) use crate::serialize_database_tests;

fn connected_history() -> (
    Harness,
    PostgresTurnHistory<koduck_ai::adapters::history::postgres::SqlxPostgresExecutor>,
    String,
) {
    let harness = Harness::connect(4);
    MIGRATIONS_ONCE.call_once(|| {
        harness.runtime.block_on(async {
            for migration in MIGRATIONS {
                sqlx::raw_sql(migration)
                    .execute(&harness.pool)
                    .await
                    .expect("apply production migration");
            }
        });
    });
    let history = PostgresTurnHistory::new(harness.executor());
    let tenant = format!("cand18-{}", Uuid::new_v4());
    (harness, history, tenant)
}

fn trust(tenant: &str, subject: &str) -> TrustContext {
    TrustContext::new(
        TenantId::new(tenant.to_owned()).expect("valid tenant"),
        subject,
    )
    .expect("valid trust")
}

fn identified_command(
    tenant: &str,
    subject: &str,
    submission: Uuid,
    thread: Option<koduck_ai::domain::ThreadId>,
    input: &str,
) -> TurnCommand {
    TurnCommand::new(trust(tenant, subject), thread, input.to_owned())
        .expect("valid input")
        .with_submission_id(SubmissionId::from_uuid(submission).expect("non-nil"))
        .expect("valid submission id")
}

/// Counts the durable rows one identified acceptance must produce.
async fn count_rows(
    pool: &sqlx::PgPool,
    tenant: &str,
    subject: &str,
    submission: Uuid,
) -> (i64, i64, i64, i64) {
    let binding: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_submissions \
         WHERE tenant_id = $1 AND subject_id = $2 AND submission_id = $3",
    )
    .bind(tenant)
    .bind(subject)
    .bind(submission)
    .fetch_one(pool)
    .await
    .expect("count bindings");
    let turn: (i64, i64, i64) = sqlx::query_as(
        "SELECT \
         (SELECT count(*) FROM turns t \
          JOIN chat_submissions s USING (tenant_id, thread_id, turn_id) \
          WHERE s.tenant_id = $1 AND s.subject_id = $2 AND s.submission_id = $3), \
         (SELECT count(*) FROM turn_items i \
          JOIN chat_submissions s USING (tenant_id, thread_id, turn_id) \
          WHERE s.tenant_id = $1 AND s.subject_id = $2 AND s.submission_id = $3 \
          AND i.sequence = 1 AND i.item_type = 'user_message'), \
         (SELECT count(*) FROM turn_leases l \
          JOIN chat_submissions s USING (tenant_id, thread_id, turn_id) \
          WHERE s.tenant_id = $1 AND s.subject_id = $2 AND s.submission_id = $3 \
          AND l.generation = 1 AND NOT l.fenced)",
    )
    .bind(tenant)
    .bind(subject)
    .bind(submission)
    .fetch_one(pool)
    .await
    .expect("count canonical rows");
    (binding, turn.0, turn.1, turn.2)
}

/// Acceptance-domain cases: atomic acceptance, equality and scope, and
/// concurrent creator selection (AC-2 through AC-4).
#[path = "submission/acceptance.rs"]
pub(crate) mod acceptance;

/// Runner-flow cases: the production runner's identified execution,
/// cancellation branches, and legacy-entry guards.
#[path = "submission/runner.rs"]
pub(crate) mod runner;

/// Deadline and port-default cases: the acceptance-clock gates and the
/// fail-closed port defaults (AC-6, SI-01d).
#[path = "submission/deadline.rs"]
pub(crate) mod deadline;

/// Integrity cases: migration idempotence, commit reconciliation, stored
/// payload bounding, and the runtime assembly (AC-5, AC-8, AC-9).
#[path = "submission/integrity.rs"]
pub(crate) mod integrity;

pub(crate) use acceptance::{
    cand_18_atomic_acceptance, cand_18_concurrent_identity, cand_18_equality_and_scope,
};
pub(crate) use deadline::{
    cand_18_deadline_gate, cand_18_default_port_fails_closed, cand_18_invalid_clock_fails_closed,
    cand_18_preparation_rejection,
};
pub(crate) use integrity::{
    cand_18_commit_reconciliation, cand_18_migration_and_integrity, cand_18_runtime_assembly,
};
pub(crate) use runner::{
    cand_18_runner_cancellation, cand_18_runner_flows, cand_18_runner_pre_acceptance_cancellation,
    cand_18_runner_stream_ended_without_terminal, cand_18_runner_tool_call_is_recorded_as_denial,
    cand_18_runner_tool_round_continuation,
};
