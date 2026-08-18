// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Process-wide migration guard for every env-gated test harness inside the
//! library test binary.
//!
//! Concurrent `CREATE TABLE IF NOT EXISTS` statements race in the `PostgreSQL`
//! catalog (`pg_type` duplicate key), so parallel test harnesses must never
//! apply the production migrations independently: every in-process harness
//! funnels through this single async guard. Asynchronous harnesses await
//! [`ensure`] directly; synchronous harnesses drive it on their own runtime.
//! The DDL always executes in the caller's runtime context, so no harness
//! ever nests runtimes or shares a pool across runtimes.

use sqlx::PgPool;
use tokio::sync::OnceCell;

static MIGRATIONS: OnceCell<()> = OnceCell::const_new();

/// Applies the complete production migration list once per test process.
///
/// Concurrent callers coalesce onto one initialization; the migrations are
/// idempotent by content, and every later call returns immediately.
pub(crate) async fn ensure(pool: &PgPool) {
    MIGRATIONS
        .get_or_init(|| async {
            for migration in [
                include_str!("../../migrations/0001_cand_1_history.sql"),
                include_str!("../../migrations/0002_cand_2_policy_execution.sql"),
                include_str!("../../migrations/0003_cand_2_requester_ownership.sql"),
                include_str!("../../migrations/0004_cand_2_tool_projections.sql"),
                include_str!("../../migrations/0005_cand_2_execution_attempts.sql"),
                include_str!("../../migrations/0006_cand_2_interrupt_barrier.sql"),
            ] {
                sqlx::raw_sql(migration)
                    .execute(pool)
                    .await
                    .expect("apply production migration");
            }
        })
        .await;
}
