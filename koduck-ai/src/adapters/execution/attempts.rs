// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! `SQLx`-backed canonical D-7 execution-attempt persistence.

use std::future::Future;
use std::time::Duration;

use sqlx::{PgPool, Row};
use tokio::runtime::Handle;

use crate::application::{
    AppendPolicy, AttemptCommitError, AttemptCommitResult, AttemptCommitter,
    AttemptInsertResolution, AttemptStoreError, AttemptTerminalResolution,
    CanonicalAttemptTerminal, DispatchClaimResolution, DurableAttemptTerminal, EffectState,
    ExecutionAttemptInterruptionGuard, ExecutionAttemptLiveness, ExecutionAttemptStore,
    ExecutionFailure, ToolExecutionOutcome,
};
use crate::domain::execution::{ExactActionBinding, ExecutionStatus};
use crate::domain::{TenantId, ThreadId, TurnId};

/// Production D-7 store using one `SQLx` pool and its owning Tokio runtime.
///
/// Every operation is one conditional durable write or read on the canonical
/// `tool_execution_attempts` table, so competing dispatchers, terminal
/// results, and reconcilers converge on a single canonical outcome
/// (ADR-0003 TC-12): exactly one conditional `prepared -> running` claim and
/// one terminal transition win per attempt identity, every loser reads the
/// already-committed canonical row, and the durable boundary keeps at most
/// one running D-7 per Turn.
#[derive(Clone)]
pub struct SqlxExecutionAttemptStore {
    pool: PgPool,
    runtime: Handle,
}

impl SqlxExecutionAttemptStore {
    /// Creates a store whose synchronous port calls drive `SQLx` on `runtime`.
    #[must_use]
    pub const fn new(pool: PgPool, runtime: Handle) -> Self {
        Self { pool, runtime }
    }

    fn wait<T>(
        &self,
        operation: impl Future<Output = Result<T, AttemptStoreError>>,
    ) -> Result<T, AttemptStoreError> {
        let deadline: Duration = AppendPolicy::cand_1().deadline();
        self.runtime.block_on(async {
            tokio::time::timeout(deadline, operation)
                .await
                .map_err(|_| AttemptStoreError::Unavailable)?
        })
    }
}

impl ExecutionAttemptStore for SqlxExecutionAttemptStore {
    fn insert_prepared(
        &mut self,
        binding: &ExactActionBinding,
        prepared_at_millis: u64,
    ) -> Result<AttemptInsertResolution, AttemptStoreError> {
        let action = binding.action();
        self.wait(async {
            // The CTE holds the authenticated Turn and its lease through the
            // allocation. That serializes every prepared insert with the
            // interruption barrier and makes both the current-generation and
            // 16-attempt checks atomic with the D-7 write (TC-07/TC-09).
            let outcome = sqlx::query(
                "WITH locked_owner AS (
                     SELECT t.status, t.interrupting, l.generation, l.fenced, l.expires_at
                     FROM turns t JOIN turn_leases l
                       ON l.tenant_id = t.tenant_id
                      AND l.thread_id = t.thread_id
                      AND l.turn_id = t.turn_id
                     WHERE t.tenant_id = $1 AND t.thread_id = $3 AND t.turn_id = $4
                     FOR UPDATE OF t, l
                 ),
                 available_slot AS (
                     SELECT 1 FROM locked_owner
                     WHERE status = 'started' AND NOT interrupting
                       AND generation = $5 AND NOT fenced
                       AND expires_at + INTERVAL '2 seconds' > CURRENT_TIMESTAMP
                       AND (
                           SELECT COUNT(*) FROM tool_execution_attempts
                           WHERE tenant_id = $1 AND thread_id = $3 AND turn_id = $4
                       ) < 16
                 )
                 INSERT INTO tool_execution_attempts (
                    tenant_id, attempt_id, thread_id, turn_id, lease_generation,
                    descriptor_id, descriptor_version, effect, action_digest,
                    profile_id, profile_version, prepared_at_millis,
                    status, version
                ) SELECT
                    $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'prepared', 1
                  FROM available_slot
                ON CONFLICT (tenant_id, attempt_id) DO NOTHING",
            )
            .bind(binding.tenant_id().as_str())
            .bind(binding.attempt_id().as_uuid())
            .bind(binding.thread_id().as_uuid())
            .bind(binding.turn_id().as_uuid())
            .bind(millis(binding.lease_generation().get())?)
            .bind(action.descriptor_id())
            .bind(action.descriptor_version())
            .bind(effect_code(action.effect()))
            .bind(hex_digest(binding.action_digest().as_bytes()))
            .bind(binding.profile_id())
            .bind(binding.profile_version())
            .bind(millis(prepared_at_millis)?)
            .execute(&self.pool)
            .await
            .map_err(|_| AttemptStoreError::Unavailable)?;
            if outcome.rows_affected() == 1 {
                return Ok(AttemptInsertResolution::Inserted);
            }
            // A lost acknowledgement may replay after the Turn was
            // interrupted or fenced. Re-read the immutable canonical row
            // before treating a no-insert result as a rejected allocation.
            let existing = sqlx::query(
                "SELECT thread_id, turn_id, lease_generation, descriptor_id,
                        descriptor_version, effect, action_digest, profile_id,
                        profile_version, prepared_at_millis, status, version
                 FROM tool_execution_attempts
                 WHERE tenant_id = $1 AND attempt_id = $2",
            )
            .bind(binding.tenant_id().as_str())
            .bind(binding.attempt_id().as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| AttemptStoreError::Unavailable)?;
            let Some(row) = existing else {
                // The owner was no longer current, the interruption barrier
                // was set, or the attempt budget was exhausted. Each case
                // fails closed without creating a prepared D-7.
                return Err(AttemptStoreError::Unavailable);
            };
            if !immutable_fields_match(&row, binding, Some(prepared_at_millis)) {
                return Err(AttemptStoreError::IdentityConflict);
            }
            Ok(AttemptInsertResolution::Existing {
                status: row_status(&row)?,
                version: row_version(&row)?,
            })
        })
    }

    fn claim_running(
        &mut self,
        binding: &ExactActionBinding,
        started_at_millis: u64,
    ) -> Result<DispatchClaimResolution, AttemptStoreError> {
        self.wait(async {
            if claim_running_winner(&self.pool, binding, started_at_millis).await? {
                return Ok(DispatchClaimResolution::Claimed { version: 2 });
            }
            resolve_claim_loss(&self.pool, binding).await
        })
    }

    fn commit_terminal(
        &mut self,
        binding: &ExactActionBinding,
        terminal: &DurableAttemptTerminal,
        terminal_at_millis: u64,
    ) -> Result<AttemptTerminalResolution, AttemptStoreError> {
        // A cancellation may close a still-prepared attempt only when the
        // executor proves no effect started (a declined, cancelled, or
        // expired D-6); every other terminal requires the won dispatch claim
        // (ADR-0003 D-7 transitions). This mirrors
        // `DurableAttemptTerminal::legal_from` and the schema shape CHECK.
        let allowed_sources: Vec<&str> = match terminal.status() {
            ExecutionStatus::Cancelled if terminal.effect_state() == EffectState::NotStarted => {
                vec!["prepared", "running"]
            }
            _ => vec!["running"],
        };
        self.wait(async {
            if let Some(version) = commit_terminal_winner(
                &self.pool,
                binding,
                terminal,
                terminal_at_millis,
                &allowed_sources,
            )
            .await?
            {
                return Ok(AttemptTerminalResolution::Won { version });
            }
            resolve_terminal_loss(&self.pool, binding).await
        })
    }
}

impl ExecutionAttemptLiveness for SqlxExecutionAttemptStore {
    fn has_live_attempt(
        &mut self,
        tenant_id: &TenantId,
        thread_id: ThreadId,
        turn_id: TurnId,
    ) -> Result<bool, AttemptStoreError> {
        self.wait(async {
            sqlx::query_scalar(
                "SELECT EXISTS (
                     SELECT 1 FROM tool_execution_attempts
                     WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3
                       AND status IN ('prepared', 'running')
                 )",
            )
            .bind(tenant_id.as_str())
            .bind(thread_id.as_uuid())
            .bind(turn_id.as_uuid())
            .fetch_one(&self.pool)
            .await
            .map_err(|_| AttemptStoreError::Unavailable)
        })
    }
}

impl ExecutionAttemptInterruptionGuard for SqlxExecutionAttemptStore {
    fn begin_interruption(
        &mut self,
        tenant_id: &TenantId,
        thread_id: ThreadId,
        turn_id: TurnId,
    ) -> Result<(), AttemptStoreError> {
        self.wait(async {
            // Lock the exact same C-5 ownership rows used by prepared and
            // claim transitions. Once this update commits, their conditional
            // predicates reject every new D-7 before dispatch.
            let barrier = sqlx::query(
                "WITH locked_owner AS (
                     SELECT t.tenant_id, t.thread_id, t.turn_id,
                            t.status, l.fenced, l.expires_at
                     FROM turns t JOIN turn_leases l
                       ON l.tenant_id = t.tenant_id
                      AND l.thread_id = t.thread_id
                      AND l.turn_id = t.turn_id
                     WHERE t.tenant_id = $1 AND t.thread_id = $2 AND t.turn_id = $3
                     FOR UPDATE OF t, l
                 )
                 UPDATE turns t
                 SET interrupting = TRUE
                 FROM locked_owner
                 WHERE t.tenant_id = locked_owner.tenant_id
                   AND t.thread_id = locked_owner.thread_id
                   AND t.turn_id = locked_owner.turn_id
                   AND locked_owner.status = 'started'
                   AND NOT locked_owner.fenced
                   AND locked_owner.expires_at + INTERVAL '2 seconds' > CURRENT_TIMESTAMP",
            )
            .bind(tenant_id.as_str())
            .bind(thread_id.as_uuid())
            .bind(turn_id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|_| AttemptStoreError::Unavailable)?;
            if barrier.rows_affected() == 1 {
                Ok(())
            } else {
                Err(AttemptStoreError::Unavailable)
            }
        })
    }
}

impl AttemptCommitter for SqlxExecutionAttemptStore {
    fn commit_outcome(
        &mut self,
        binding: &ExactActionBinding,
        outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        let terminal = DurableAttemptTerminal::from_outcome(outcome);
        // The committer port carries no timestamp, so terminal evidence reads
        // the shared production wall clock.
        let terminal_at_millis = crate::adapters::history::postgres::unix_time_ms();
        match self.commit_terminal(binding, &terminal, terminal_at_millis) {
            Ok(AttemptTerminalResolution::Won { version: 3 }) => Ok(AttemptCommitResult::Won),
            // A won terminal at any other version is durable-state drift, and
            // a missing canonical row leaves the durable state undecidable:
            // the caller cannot reconcile either through this port.
            Ok(AttemptTerminalResolution::Won { .. } | AttemptTerminalResolution::NotFound) => {
                Err(AttemptCommitError::Unavailable)
            }
            Ok(AttemptTerminalResolution::ExistingTerminal(canonical)) => {
                Ok(AttemptCommitResult::Existing(canonical))
            }
            Ok(AttemptTerminalResolution::Fenced) => Err(AttemptCommitError::Fenced),
            // A conflicting or drifted canonical row already won this D-7
            // transition; reconciliation owns the next one.
            Ok(AttemptTerminalResolution::Conflict) | Err(AttemptStoreError::IdentityConflict) => {
                Err(AttemptCommitError::Conflict)
            }
            Err(AttemptStoreError::Unavailable) => Err(AttemptCommitError::Unavailable),
        }
    }
}

async fn claim_running_winner(
    pool: &PgPool,
    binding: &ExactActionBinding,
    started_at_millis: u64,
) -> Result<bool, AttemptStoreError> {
    let action = binding.action();
    // The conditional winner update binds the full immutable record, not just
    // the attempt key, so a replay whose ownership data drifted cannot claim
    // another canonical D-7 before loser-side validation (ADR-0003 TC-12).
    claim_running_result(
        sqlx::query(
            "WITH locked_owner AS (
             SELECT t.status, t.interrupting, l.generation, l.fenced, l.expires_at
             FROM turns t JOIN turn_leases l
               ON l.tenant_id = t.tenant_id
              AND l.thread_id = t.thread_id
              AND l.turn_id = t.turn_id
             WHERE t.tenant_id = $1 AND t.thread_id = $4 AND t.turn_id = $5
             FOR UPDATE OF t, l
         )
         UPDATE tool_execution_attempts
         SET status = 'running', started_at_millis = $3, version = 2
         WHERE tenant_id = $1 AND attempt_id = $2 AND status = 'prepared'
           AND thread_id = $4 AND turn_id = $5 AND lease_generation = $6
           AND descriptor_id = $7 AND descriptor_version = $8
           AND effect = $9 AND action_digest = $10
           AND profile_id = $11 AND profile_version = $12
           AND EXISTS (
               SELECT 1 FROM locked_owner
               WHERE status = 'started' AND NOT interrupting
                 AND generation = $6 AND NOT fenced
                 AND expires_at + INTERVAL '2 seconds' > CURRENT_TIMESTAMP
           )
           AND NOT EXISTS (
               SELECT 1 FROM tool_execution_attempts other
               WHERE other.tenant_id = $1 AND other.turn_id = $5
                 AND other.status = 'running' AND other.attempt_id <> $2
           )
         RETURNING version",
        )
        .bind(binding.tenant_id().as_str())
        .bind(binding.attempt_id().as_uuid())
        .bind(millis(started_at_millis)?)
        .bind(binding.thread_id().as_uuid())
        .bind(binding.turn_id().as_uuid())
        .bind(millis(binding.lease_generation().get())?)
        .bind(action.descriptor_id())
        .bind(action.descriptor_version())
        .bind(effect_code(action.effect()))
        .bind(hex_digest(binding.action_digest().as_bytes()))
        .bind(binding.profile_id())
        .bind(binding.profile_version())
        .fetch_optional(pool)
        .await,
    )
}

fn claim_running_result(
    result: Result<Option<sqlx::postgres::PgRow>, sqlx::Error>,
) -> Result<bool, AttemptStoreError> {
    match result {
        Ok(winner) => Ok(winner.is_some()),
        // Another attempt can win between the `NOT EXISTS` predicate and the
        // partial unique index. The loser re-read reports `Concurrent`.
        Err(error)
            if error
                .as_database_error()
                .is_some_and(sqlx::error::DatabaseError::is_unique_violation) =>
        {
            Ok(false)
        }
        Err(_) => Err(AttemptStoreError::Unavailable),
    }
}

async fn resolve_claim_loss(
    pool: &PgPool,
    binding: &ExactActionBinding,
) -> Result<DispatchClaimResolution, AttemptStoreError> {
    let existing = sqlx::query(
        "SELECT thread_id, turn_id, lease_generation, descriptor_id,
                descriptor_version, effect, action_digest, profile_id,
                profile_version, prepared_at_millis, status, version
         FROM tool_execution_attempts
         WHERE tenant_id = $1 AND attempt_id = $2",
    )
    .bind(binding.tenant_id().as_str())
    .bind(binding.attempt_id().as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(|_| AttemptStoreError::Unavailable)?;
    let Some(row) = existing else {
        return Ok(DispatchClaimResolution::NotFound);
    };
    // The claim port carries no prepared-at expectation, so validate every
    // immutable binding field it can observe before reporting canonical state.
    if !immutable_fields_match(&row, binding, None) {
        return Err(AttemptStoreError::IdentityConflict);
    }
    let status = row_status(&row)?;
    let version = row_version(&row)?;
    match status {
        ExecutionStatus::Running
        | ExecutionStatus::Succeeded
        | ExecutionStatus::Failed
        | ExecutionStatus::TimedOut
        | ExecutionStatus::Cancelled => Ok(DispatchClaimResolution::Existing { status, version }),
        ExecutionStatus::Prepared => {
            if bound_lease_is_not_current(pool, binding).await? {
                Ok(DispatchClaimResolution::Fenced)
            } else {
                Ok(DispatchClaimResolution::Concurrent)
            }
        }
    }
}

async fn commit_terminal_winner(
    pool: &PgPool,
    binding: &ExactActionBinding,
    terminal: &DurableAttemptTerminal,
    terminal_at_millis: u64,
    allowed_sources: &[&str],
) -> Result<Option<u64>, AttemptStoreError> {
    let action = binding.action();
    let winner = sqlx::query(
        "UPDATE tool_execution_attempts
         SET status = $3, effect_state = $4, failure_code = $5, output = $6,
             terminal_at_millis = $7, version = 3
         WHERE tenant_id = $1 AND attempt_id = $2 AND status = ANY($8)
           AND thread_id = $9 AND turn_id = $10 AND lease_generation = $11
           AND descriptor_id = $12 AND descriptor_version = $13
           AND effect = $14 AND action_digest = $15
           AND profile_id = $16 AND profile_version = $17
           AND EXISTS (
               SELECT 1
               FROM (
                   SELECT generation, fenced, expires_at
                   FROM turn_leases
                   WHERE tenant_id = $1 AND thread_id = $9 AND turn_id = $10
                   FOR UPDATE
               ) AS bound_lease
               WHERE bound_lease.generation = $11 AND NOT bound_lease.fenced
                 AND bound_lease.expires_at + INTERVAL '2 seconds' > CURRENT_TIMESTAMP
           )
         RETURNING version",
    )
    .bind(binding.tenant_id().as_str())
    .bind(binding.attempt_id().as_uuid())
    .bind(status_code(terminal.status()))
    .bind(terminal.effect_state().as_str())
    .bind(terminal.failure_code().map(ExecutionFailure::stable_code))
    .bind(terminal.output())
    .bind(millis(terminal_at_millis)?)
    .bind(allowed_sources)
    .bind(binding.thread_id().as_uuid())
    .bind(binding.turn_id().as_uuid())
    .bind(millis(binding.lease_generation().get())?)
    .bind(action.descriptor_id())
    .bind(action.descriptor_version())
    .bind(effect_code(action.effect()))
    .bind(hex_digest(binding.action_digest().as_bytes()))
    .bind(binding.profile_id())
    .bind(binding.profile_version())
    .fetch_optional(pool)
    .await
    .map_err(|_| AttemptStoreError::Unavailable)?;
    winner.map_or(Ok(None), |row| {
        let version = row_version(&row)?;
        if version == 3 {
            Ok(Some(version))
        } else {
            Err(AttemptStoreError::Unavailable)
        }
    })
}

async fn resolve_terminal_loss(
    pool: &PgPool,
    binding: &ExactActionBinding,
) -> Result<AttemptTerminalResolution, AttemptStoreError> {
    let existing = sqlx::query(
        "SELECT thread_id, turn_id, lease_generation, descriptor_id,
                descriptor_version, effect, action_digest, profile_id,
                profile_version, prepared_at_millis, status, version,
                effect_state, failure_code, output
         FROM tool_execution_attempts
         WHERE tenant_id = $1 AND attempt_id = $2",
    )
    .bind(binding.tenant_id().as_str())
    .bind(binding.attempt_id().as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(|_| AttemptStoreError::Unavailable)?;
    let Some(row) = existing else {
        return Ok(AttemptTerminalResolution::NotFound);
    };
    if !immutable_fields_match(&row, binding, None) {
        return Ok(AttemptTerminalResolution::Conflict);
    }
    let status = row_status(&row)?;
    if matches!(
        status,
        ExecutionStatus::Succeeded
            | ExecutionStatus::Failed
            | ExecutionStatus::TimedOut
            | ExecutionStatus::Cancelled
    ) {
        let version = row_version(&row)?;
        if version != 3 {
            return Ok(AttemptTerminalResolution::Conflict);
        }
        let canonical = CanonicalAttemptTerminal::from_persistence(
            binding.clone(),
            version,
            canonical_outcome(&row, status)?,
        )
        .map_err(|_| AttemptStoreError::Unavailable)?;
        return Ok(AttemptTerminalResolution::ExistingTerminal(Box::new(
            canonical,
        )));
    }
    if bound_lease_is_not_current(pool, binding).await? {
        return Ok(AttemptTerminalResolution::Fenced);
    }
    Ok(AttemptTerminalResolution::Conflict)
}

/// Reports whether the D-7's bound canonical C-6 lease is not current.
///
/// A missing lease leaves D-7 ownership unproven and therefore fails closed,
/// the same way a mismatched, fenced, or expired lease does.
async fn bound_lease_is_not_current(
    pool: &PgPool,
    binding: &ExactActionBinding,
) -> Result<bool, AttemptStoreError> {
    let generation = millis(binding.lease_generation().get())?;
    sqlx::query_scalar(
        "SELECT NOT EXISTS (
             SELECT 1 FROM turn_leases lease
             WHERE lease.tenant_id = $1 AND lease.thread_id = $2 AND lease.turn_id = $3
               AND lease.generation = $4 AND NOT lease.fenced
               AND lease.expires_at + INTERVAL '2 seconds' > CURRENT_TIMESTAMP
         )",
    )
    .bind(binding.tenant_id().as_str())
    .bind(binding.thread_id().as_uuid())
    .bind(binding.turn_id().as_uuid())
    .bind(generation)
    .fetch_one(pool)
    .await
    .map_err(|_| AttemptStoreError::Unavailable)
}

/// Verifies the immutable binding fields against one canonical row.
fn immutable_fields_match(
    row: &sqlx::postgres::PgRow,
    binding: &ExactActionBinding,
    expected_prepared_at: Option<u64>,
) -> bool {
    let action = binding.action();
    // Decode numeric canonical values fail-closed before comparing, so a
    // drifted negative durable value never compares equal to its expected
    // positive counterpart.
    (|| {
        let lease_generation = canonical_non_negative(row, "lease_generation").ok()?;
        let prepared_at = canonical_non_negative(row, "prepared_at_millis").ok()?;
        Some(
            row.try_get::<uuid::Uuid, _>("thread_id").ok()? == binding.thread_id().as_uuid()
                && row.try_get::<uuid::Uuid, _>("turn_id").ok()? == binding.turn_id().as_uuid()
                && lease_generation == binding.lease_generation().get()
                && expected_prepared_at.is_none_or(|expected| prepared_at == expected)
                && row.try_get::<String, _>("descriptor_id").ok()? == action.descriptor_id()
                && row.try_get::<String, _>("descriptor_version").ok()?
                    == action.descriptor_version()
                && row.try_get::<String, _>("effect").ok()? == effect_code(action.effect())
                && row.try_get::<String, _>("action_digest").ok()?
                    == hex_digest(binding.action_digest().as_bytes())
                && row.try_get::<String, _>("profile_id").ok()? == binding.profile_id()
                && row.try_get::<String, _>("profile_version").ok()? == binding.profile_version(),
        )
    })()
    .unwrap_or(false)
}

/// Rebuilds the canonical bounded outcome from one terminal row.
fn canonical_outcome(
    row: &sqlx::postgres::PgRow,
    status: ExecutionStatus,
) -> Result<ToolExecutionOutcome, AttemptStoreError> {
    let effect_state = row
        .try_get::<Option<String>, _>("effect_state")
        .map_err(|_| AttemptStoreError::Unavailable)?
        .as_deref()
        .and_then(EffectState::from_code)
        .ok_or(AttemptStoreError::Unavailable)?;
    match status {
        ExecutionStatus::Succeeded => Ok(ToolExecutionOutcome::Succeeded {
            output: row
                .try_get::<Option<Vec<u8>>, _>("output")
                .map_err(|_| AttemptStoreError::Unavailable)?
                .ok_or(AttemptStoreError::Unavailable)?,
            effect_state,
        }),
        ExecutionStatus::Failed => Ok(ToolExecutionOutcome::Failed {
            code: row
                .try_get::<Option<String>, _>("failure_code")
                .map_err(|_| AttemptStoreError::Unavailable)?
                .as_deref()
                .and_then(ExecutionFailure::from_stable_code)
                .ok_or(AttemptStoreError::Unavailable)?,
            effect_state,
        }),
        ExecutionStatus::TimedOut => Ok(ToolExecutionOutcome::TimedOut { effect_state }),
        ExecutionStatus::Cancelled => Ok(ToolExecutionOutcome::Cancelled { effect_state }),
        // A prepared or running row is not a terminal and carries no outcome.
        ExecutionStatus::Prepared | ExecutionStatus::Running => Err(AttemptStoreError::Unavailable),
    }
}

fn canonical_non_negative(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Result<u64, AttemptStoreError> {
    let value = row
        .try_get::<i64, _>(column)
        .map_err(|_| AttemptStoreError::Unavailable)?;
    u64::try_from(value).map_err(|_| AttemptStoreError::Unavailable)
}

fn row_status(row: &sqlx::postgres::PgRow) -> Result<ExecutionStatus, AttemptStoreError> {
    let status = row
        .try_get::<String, _>("status")
        .map_err(|_| AttemptStoreError::Unavailable)?;
    status_from_code(&status).ok_or(AttemptStoreError::Unavailable)
}

fn row_version(row: &sqlx::postgres::PgRow) -> Result<u64, AttemptStoreError> {
    let version = row
        .try_get::<i64, _>("version")
        .map_err(|_| AttemptStoreError::Unavailable)?;
    u64::try_from(version)
        .ok()
        .filter(|version| *version >= 1)
        .ok_or(AttemptStoreError::Unavailable)
}

/// Converts one non-negative millisecond timestamp to its durable binding.
///
/// # Errors
///
/// Returns [`AttemptStoreError::Unavailable`] when the timestamp exceeds the
/// durable column domain.
fn millis(value: u64) -> Result<i64, AttemptStoreError> {
    i64::try_from(value).map_err(|_| AttemptStoreError::Unavailable)
}

fn status_code(status: ExecutionStatus) -> &'static str {
    status.as_str()
}

fn status_from_code(code: &str) -> Option<ExecutionStatus> {
    match code {
        "prepared" => Some(ExecutionStatus::Prepared),
        "running" => Some(ExecutionStatus::Running),
        "succeeded" => Some(ExecutionStatus::Succeeded),
        "failed" => Some(ExecutionStatus::Failed),
        "timed_out" => Some(ExecutionStatus::TimedOut),
        "cancelled" => Some(ExecutionStatus::Cancelled),
        _ => None,
    }
}

fn effect_code(effect: crate::domain::tool::Effect) -> &'static str {
    match effect {
        crate::domain::tool::Effect::ReadData => "read_data",
        crate::domain::tool::Effect::ExternalWrite => "external_write",
        crate::domain::tool::Effect::FilesystemWrite => "filesystem_write",
        crate::domain::tool::Effect::ProcessExecute => "process_execute",
        crate::domain::tool::Effect::NetworkEgress => "network_egress",
        crate::domain::tool::Effect::CredentialUse => "credential_use",
        crate::domain::tool::Effect::Unknown => "unknown",
    }
}

fn hex_digest(bytes: &[u8; 32]) -> String {
    let mut text = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
}
