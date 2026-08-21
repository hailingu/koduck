// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! `SQLx`-backed canonical D-7 execution-attempt persistence.

use std::future::Future;
use std::time::Duration;

use sqlx::{PgPool, Row};
use tokio::runtime::Handle;

use crate::application::{
    AppendPolicy, AttemptCommitError, AttemptCommitResult, AttemptCommitter,
    AttemptInsertResolution, AttemptStoreError, AttemptTerminalResolution, CanonicalTurnTerminal,
    DispatchClaimResolution, DurableAttemptTerminal, DurableAttemptTransitions, EffectState,
    ExecutionAttemptInterruptionGuard, ExecutionAttemptLiveness, ExecutionAttemptStore,
    ExecutionFailure, PreparedCloseResolution, ToolExecutionOutcome,
};
use crate::domain::execution::{ExactActionBinding, ExecutionStatus};

use super::attempt_reconciliation::{
    bound_lease_is_not_current, immutable_fields_match, resolve_terminal_loss, row_status,
    row_version,
};
use super::prepared_close;
use super::prepared_insert;
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

impl SqlxExecutionAttemptStore {
    /// Commits a terminal with an explicit interruption-ownership marker:
    /// only an interruption-owned settlement may commit past the durable
    /// Turn barrier (ADR-0003 TC-10/TC-12).
    fn commit_terminal_with_ownership(
        &mut self,
        binding: &ExactActionBinding,
        terminal: &DurableAttemptTerminal,
        terminal_at_millis: u64,
        interruption_owned: bool,
    ) -> Result<AttemptTerminalResolution, AttemptStoreError> {
        Self::commit_terminal_impl(
            self,
            binding,
            terminal,
            terminal_at_millis,
            interruption_owned,
        )
    }

    /// Shared terminal transition body; `interruption_owned` gates the
    /// barrier bypass in the winner SQL (ADR-0003 TC-10/TC-12). Defined as
    /// an inherent method so the trait surface stays minimal.
    fn commit_terminal_impl(
        &mut self,
        binding: &ExactActionBinding,
        terminal: &DurableAttemptTerminal,
        terminal_at_millis: u64,
        interruption_owned: bool,
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
            // The terminal write and its correlated audit append commit
            // atomically: a committed D-7 can never permanently lack its
            // durable TC-14 evidence (ADR-0003).
            let mut transaction = self
                .pool
                .begin()
                .await
                .map_err(|_| AttemptStoreError::Unavailable)?;
            match commit_terminal_winner_tx(
                &mut transaction,
                binding,
                terminal,
                terminal_at_millis,
                &allowed_sources,
                interruption_owned,
            )
            .await
            {
                Ok(Some(version)) => {
                    append_terminal_audit(&mut transaction, binding, terminal, terminal_at_millis)
                        .await?;
                    // An ambiguous COMMIT acknowledgement — the transaction
                    // may have committed while its acknowledgement was lost —
                    // reconciles through the canonical reread like every
                    // other ambiguous write: a committed terminal surfaces
                    // as ExistingTerminal instead of withholding an
                    // already-committed result (ADR-0003 TC-12/TC-14).
                    if transaction.commit().await.is_err() {
                        return resolve_terminal_loss(&self.pool, binding).await;
                    }
                    Ok(AttemptTerminalResolution::Won { version })
                }
                // A lost write and an ambiguous write — the statement may
                // have committed while its acknowledgement was lost to a
                // timeout or connection failure — both reconcile through the
                // canonical re-read: a committed terminal surfaces as
                // ExistingTerminal so the caller still observes the terminal,
                // while a confirmed-absent row stays NotFound (mapped to the
                // undecidable unavailability the committer contract defines)
                // (ADR-0003 TC-12).
                lost_or_ambiguous => {
                    drop(transaction);
                    let _ = lost_or_ambiguous;
                    resolve_terminal_loss(&self.pool, binding).await
                }
            }
        })
    }
}

impl ExecutionAttemptStore for SqlxExecutionAttemptStore {
    fn insert_prepared(
        &mut self,
        binding: &ExactActionBinding,
        prepared_at_millis: u64,
    ) -> Result<AttemptInsertResolution, AttemptStoreError> {
        self.wait(prepared_insert::insert_prepared_row(
            &self.pool,
            binding,
            prepared_at_millis,
        ))
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
        Self::commit_terminal_with_ownership(self, binding, terminal, terminal_at_millis, false)
    }

    fn cancel_prepared_attempt(
        &mut self,
        binding: &ExactActionBinding,
    ) -> Result<PreparedCloseResolution, AttemptStoreError> {
        // The prepared-only conditional close and its loser classification
        // live in the sibling prepared_close module (TC-10/TC-12).
        self.wait(prepared_close::close_prepared_row(
            &self.pool,
            binding,
            crate::adapters::history::postgres::unix_time_ms(),
        ))
    }

    fn commit_fenced_after_dispatch(
        &mut self,
        binding: &ExactActionBinding,
        effect_state: EffectState,
        terminal_at_millis: u64,
    ) -> Result<AttemptTerminalResolution, AttemptStoreError> {
        // The guard and conditional SQL live in the sibling fenced_terminal
        // module (ADR-0003 lines 309-314, TC-07/TC-12).
        self.wait(super::fenced_terminal::commit_fenced_failure(
            &self.pool,
            binding,
            effect_state,
            terminal_at_millis,
        ))
    }
}

impl CanonicalTurnTerminal for SqlxExecutionAttemptStore {
    fn turn_is_terminal(
        &mut self,
        tenant_id: &TenantId,
        thread_id: ThreadId,
        turn_id: TurnId,
    ) -> Result<bool, AttemptStoreError> {
        self.wait(async {
            // A missing Turn row proves nothing, so it fails closed as
            // not-terminal rather than authorizing reclamation.
            sqlx::query_scalar(
                "SELECT status IN ('completed', 'failed', 'interrupted', 'cancelled') \
                 FROM turns \
                 WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
            )
            .bind(tenant_id.as_str())
            .bind(thread_id.as_uuid())
            .bind(turn_id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| AttemptStoreError::Unavailable)
            .map(|terminal| terminal.unwrap_or(false))
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
                return Ok(());
            }
            if super::interruption_barrier::lost_to_non_dispatchable_turn(
                &self.pool, tenant_id, thread_id, turn_id,
            )
            .await?
            {
                // The history boundary owns the precise endpoint result for
                // a concurrent terminal, fenced, expired, or missing Turn.
                // It must not be masked as a C-5 storage outage here.
                return Ok(());
            }
            Err(AttemptStoreError::Unavailable)
        })
    }
}

impl DurableAttemptTransitions for SqlxExecutionAttemptStore {
    fn insert_prepared(
        &mut self,
        binding: &ExactActionBinding,
        prepared_at_millis: u64,
    ) -> Result<AttemptInsertResolution, AttemptStoreError> {
        // The coordinator-side narrow port delegates to the same conditional
        // durable write the full store port exposes, so the C-5 boundary and
        // direct canonical callers cannot diverge (ADR-0003 TC-12).
        ExecutionAttemptStore::insert_prepared(self, binding, prepared_at_millis)
    }

    fn claim_running(
        &mut self,
        binding: &ExactActionBinding,
        started_at_millis: u64,
    ) -> Result<DispatchClaimResolution, AttemptStoreError> {
        ExecutionAttemptStore::claim_running(self, binding, started_at_millis)
    }

    fn cancel_prepared_attempt(
        &mut self,
        binding: &ExactActionBinding,
    ) -> Result<PreparedCloseResolution, AttemptStoreError> {
        ExecutionAttemptStore::cancel_prepared_attempt(self, binding)
    }
}

impl AttemptCommitter for SqlxExecutionAttemptStore {
    fn commit_outcome_as_interruption(
        &mut self,
        binding: &ExactActionBinding,
        outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        // The authenticated interruption's own settlement routes through the
        // ownership-flagged terminal transition (ADR-0003 TC-10/TC-12).
        let terminal = DurableAttemptTerminal::from_outcome(outcome);
        let terminal_at_millis = crate::adapters::history::postgres::unix_time_ms();
        match Self::commit_terminal_with_ownership(
            self,
            binding,
            &terminal,
            terminal_at_millis,
            true,
        ) {
            Ok(AttemptTerminalResolution::Won { version: 3 }) => Ok(AttemptCommitResult::Won),
            Ok(AttemptTerminalResolution::Won { .. } | AttemptTerminalResolution::NotFound)
            | Err(AttemptStoreError::Unavailable | AttemptStoreError::AttemptLimit) => {
                Err(AttemptCommitError::Unavailable)
            }
            Ok(AttemptTerminalResolution::ExistingTerminal(canonical)) => {
                Ok(AttemptCommitResult::Existing(canonical))
            }
            Ok(AttemptTerminalResolution::Fenced) => Err(AttemptCommitError::Fenced),
            Ok(AttemptTerminalResolution::Conflict) | Err(AttemptStoreError::IdentityConflict) => {
                Err(AttemptCommitError::Conflict)
            }
        }
    }

    fn commit_fenced_after_dispatch(
        &mut self,
        binding: &ExactActionBinding,
        effect_state: EffectState,
        terminal_at_millis: u64,
    ) -> Result<AttemptTerminalResolution, AttemptStoreError> {
        // The committer port carries no effect-state guard, so delegate to the
        // guarded store transition (ADR-0003 lines 309-314).
        ExecutionAttemptStore::commit_fenced_after_dispatch(
            self,
            binding,
            effect_state,
            terminal_at_millis,
        )
    }

    fn appends_terminal_audit_atomically(&self) -> bool {
        true
    }

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
            Ok(AttemptTerminalResolution::Won { .. } | AttemptTerminalResolution::NotFound)
            | Err(AttemptStoreError::Unavailable | AttemptStoreError::AttemptLimit) => {
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
        ExecutionStatus::Prepared => match turn_claim_state(pool, binding).await? {
            TurnClaimState::Interrupted => Ok(DispatchClaimResolution::Interrupted),
            TurnClaimState::Inactive => Ok(DispatchClaimResolution::Fenced),
            TurnClaimState::Active if bound_lease_is_not_current(pool, binding).await? => {
                Ok(DispatchClaimResolution::Fenced)
            }
            TurnClaimState::Active if another_attempt_is_running(pool, binding).await? => {
                Ok(DispatchClaimResolution::Concurrent)
            }
            // The winner update did not commit, but neither a durable
            // interruption nor a concurrent running owner explains its loss.
            // Do not fabricate a typed rejection from a stale loser read.
            TurnClaimState::Active => Err(AttemptStoreError::Unavailable),
        },
    }
}

/// Durable Turn state relevant to a lost conditional dispatch claim.
enum TurnClaimState {
    /// The Turn remains active and has no durable interruption barrier.
    Active,
    /// An authenticated interruption sealed the active Turn.
    Interrupted,
    /// The Turn is absent or no longer dispatchable.
    Inactive,
}

/// Reads the Turn barrier after a prepared claim loses.
async fn turn_claim_state(
    pool: &PgPool,
    binding: &ExactActionBinding,
) -> Result<TurnClaimState, AttemptStoreError> {
    let row = sqlx::query(
        "SELECT status, interrupting FROM turns \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
    )
    .bind(binding.tenant_id().as_str())
    .bind(binding.thread_id().as_uuid())
    .bind(binding.turn_id().as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(|_| AttemptStoreError::Unavailable)?;
    let Some(row) = row else {
        return Ok(TurnClaimState::Inactive);
    };
    let status = row
        .try_get::<String, _>("status")
        .map_err(|_| AttemptStoreError::Unavailable)?;
    let interrupting = row
        .try_get::<bool, _>("interrupting")
        .map_err(|_| AttemptStoreError::Unavailable)?;
    if status != "started" {
        Ok(TurnClaimState::Inactive)
    } else if interrupting {
        Ok(TurnClaimState::Interrupted)
    } else {
        Ok(TurnClaimState::Active)
    }
}

/// Reports whether another canonical D-7 owns this Turn's running slot.
async fn another_attempt_is_running(
    pool: &PgPool,
    binding: &ExactActionBinding,
) -> Result<bool, AttemptStoreError> {
    sqlx::query_scalar(
        "SELECT EXISTS ( \
             SELECT 1 FROM tool_execution_attempts \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
               AND status = 'running' AND attempt_id <> $4 \
         )",
    )
    .bind(binding.tenant_id().as_str())
    .bind(binding.thread_id().as_uuid())
    .bind(binding.turn_id().as_uuid())
    .bind(binding.attempt_id().as_uuid())
    .fetch_one(pool)
    .await
    .map_err(|_| AttemptStoreError::Unavailable)
}

async fn commit_terminal_winner_tx(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    binding: &ExactActionBinding,
    terminal: &DurableAttemptTerminal,
    terminal_at_millis: u64,
    allowed_sources: &[&str],
    interruption_owned: bool,
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
           AND EXISTS (
               SELECT 1 FROM turns barrier
               WHERE barrier.tenant_id = $1 AND barrier.thread_id = $9
                 AND barrier.turn_id = $10
                 AND barrier.status = 'started'
                 AND (NOT barrier.interrupting
                      OR ($18::boolean AND $3::text IN ('cancelled', 'timed_out')))
               FOR UPDATE
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
    .bind(interruption_owned)
    .fetch_optional(&mut **transaction)
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

/// Appends the bounded correlated audit record for one won D-7 terminal,
/// inside the terminal's own transaction (ADR-0003 TC-14).
pub(super) async fn append_terminal_audit_pub(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    binding: &ExactActionBinding,
    terminal: &DurableAttemptTerminal,
    terminal_at_millis: u64,
) -> Result<(), AttemptStoreError> {
    append_terminal_audit(transaction, binding, terminal, terminal_at_millis).await
}

async fn append_terminal_audit(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    binding: &ExactActionBinding,
    terminal: &DurableAttemptTerminal,
    terminal_at_millis: u64,
) -> Result<(), AttemptStoreError> {
    let outcome = match terminal.status() {
        ExecutionStatus::Succeeded => ToolExecutionOutcome::Succeeded {
            output: terminal
                .output()
                .ok_or(AttemptStoreError::Unavailable)?
                .to_vec(),
            effect_state: terminal.effect_state(),
        },
        ExecutionStatus::Failed => ToolExecutionOutcome::Failed {
            code: terminal
                .failure_code()
                .ok_or(AttemptStoreError::Unavailable)?,
            effect_state: terminal.effect_state(),
        },
        ExecutionStatus::TimedOut => ToolExecutionOutcome::TimedOut {
            effect_state: terminal.effect_state(),
        },
        ExecutionStatus::Cancelled => ToolExecutionOutcome::Cancelled {
            effect_state: terminal.effect_state(),
        },
        ExecutionStatus::Prepared | ExecutionStatus::Running => {
            return Err(AttemptStoreError::Unavailable);
        }
    };
    let record = crate::application::ToolAuditRecord::execution_terminal(
        binding,
        &outcome,
        terminal_at_millis,
    );
    let serialized = crate::adapters::audit::serialize_audit_record(&record)
        .map_err(|_| AttemptStoreError::Unavailable)?;
    sqlx::query(
        "INSERT INTO tool_audit_records \
         (tenant_id, thread_id, turn_id, at_millis, record) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(record.tenant_id())
    .bind(binding.thread_id().as_uuid())
    .bind(binding.turn_id().as_uuid())
    .bind(millis(terminal_at_millis)?)
    .bind(serialized)
    .execute(&mut **transaction)
    .await
    .map_err(|_| AttemptStoreError::Unavailable)?;
    Ok(())
}

/// Converts one non-negative millisecond timestamp to its durable binding.
///
/// # Errors
///
/// Returns [`AttemptStoreError::Unavailable`] when the timestamp exceeds the
/// durable column domain.
pub(super) fn millis(value: u64) -> Result<i64, AttemptStoreError> {
    i64::try_from(value).map_err(|_| AttemptStoreError::Unavailable)
}

fn status_code(status: ExecutionStatus) -> &'static str {
    status.as_str()
}

pub(super) fn effect_code(effect: crate::domain::tool::Effect) -> &'static str {
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

pub(super) fn hex_digest(bytes: &[u8; 32]) -> String {
    let mut text = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
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
