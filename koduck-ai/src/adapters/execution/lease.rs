// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! `SQLx`-backed canonical C-6 lease validation for C-5 dispatch and interruption.

use sqlx::{PgPool, Row};
use tokio::runtime::Handle;

use crate::application::{AppendPolicy, LeaseCheck, LeaseValidator};
use crate::domain::execution::ExactActionBinding;

/// Production C-6 lease validator answering from the canonical `turn_leases`
/// table.
///
/// Both C-5 dispatch and authenticated interruption read the durable lease
/// row through this validator before any D-7 mutation. A fenced or expired
/// generation therefore cannot dispatch or commit a terminal (ADR-0003
/// TC-07). The expiry check mirrors the two-second arbitration window of the
/// authenticated interrupt write, so a lease near its boundary is treated
/// consistently by both paths.
#[derive(Clone)]
pub struct SqlxTurnLeaseValidator {
    pool: PgPool,
    runtime: Handle,
}

impl SqlxTurnLeaseValidator {
    /// Creates a validator whose synchronous checks drive `SQLx` on `runtime`.
    #[must_use]
    pub const fn new(pool: PgPool, runtime: Handle) -> Self {
        Self { pool, runtime }
    }
}

impl LeaseValidator for SqlxTurnLeaseValidator {
    fn check_current(&mut self, binding: &ExactActionBinding) -> LeaseCheck {
        let Ok(generation) = i64::try_from(binding.lease_generation().get()) else {
            return LeaseCheck::Unavailable;
        };
        let query = sqlx::query(
            "SELECT generation, fenced, \
             expires_at + INTERVAL '2 seconds' > CURRENT_TIMESTAMP AS within_window \
             FROM turn_leases \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(binding.tenant_id().as_str())
        .bind(binding.thread_id().as_uuid())
        .bind(binding.turn_id().as_uuid())
        .fetch_optional(&self.pool);
        let deadline: std::time::Duration = AppendPolicy::cand_1().deadline();
        // A missing row or a failed check leaves ownership undetermined:
        // report Unavailable rather than guessing Current or Fenced, exactly
        // like the bounded D-7 store reads.
        let row = self
            .runtime
            .block_on(async { tokio::time::timeout(deadline, query).await.ok()?.ok() });
        let Some(row) = row.and_then(|row| row) else {
            return LeaseCheck::Unavailable;
        };
        let (Ok(current), Ok(fenced), Ok(within_window)) = (
            row.try_get::<i64, _>("generation"),
            row.try_get::<bool, _>("fenced"),
            row.try_get::<bool, _>("within_window"),
        ) else {
            return LeaseCheck::Unavailable;
        };
        // A differing generation means a newer generation owns the Turn, and
        // a fenced or expired lease means the bound generation no longer does;
        // the authenticated interrupt write maps the same states to `Fenced`.
        if current != generation || fenced || !within_window {
            return LeaseCheck::Fenced;
        }
        LeaseCheck::Current
    }
}
