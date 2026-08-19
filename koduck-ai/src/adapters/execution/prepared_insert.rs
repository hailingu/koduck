// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Durable prepared-D-7 insertion and idempotent replay SQL.

use sqlx::{PgPool, Postgres, Transaction};

use crate::application::{AttemptInsertResolution, AttemptStoreError};
use crate::domain::execution::ExactActionBinding;

use super::attempts::{
    effect_code, hex_digest, immutable_fields_match, millis, row_status, row_version,
};

/// Inserts one canonical prepared D-7 while serializing the Turn attempt budget.
pub(super) async fn insert_prepared_row(
    pool: &PgPool,
    binding: &ExactActionBinding,
    prepared_at_millis: u64,
) -> Result<AttemptInsertResolution, AttemptStoreError> {
    if let Some(existing) = replay_prepared_from_pool(pool, binding, prepared_at_millis).await? {
        return Ok(existing);
    }
    let mut transaction = pool
        .begin()
        .await
        .map_err(|_| AttemptStoreError::Unavailable)?;
    if !lock_current_owner(&mut transaction, binding).await? {
        drop(transaction);
        return replay_prepared_from_pool(pool, binding, prepared_at_millis)
            .await?
            .ok_or(AttemptStoreError::Unavailable);
    }
    if insert_locked_attempt(&mut transaction, binding, prepared_at_millis).await? {
        transaction
            .commit()
            .await
            .map_err(|_| AttemptStoreError::Unavailable)?;
        return Ok(AttemptInsertResolution::Inserted);
    }
    replay_prepared_from_transaction(&mut transaction, binding, prepared_at_millis)
        .await?
        .ok_or(AttemptStoreError::AttemptLimit)
}

async fn lock_current_owner(
    transaction: &mut Transaction<'_, Postgres>,
    binding: &ExactActionBinding,
) -> Result<bool, AttemptStoreError> {
    sqlx::query(
        "SELECT 1 FROM turns t JOIN turn_leases l
           ON l.tenant_id = t.tenant_id
          AND l.thread_id = t.thread_id
          AND l.turn_id = t.turn_id
         WHERE t.tenant_id = $1 AND t.thread_id = $2 AND t.turn_id = $3
           AND t.status = 'started' AND NOT t.interrupting
           AND l.generation = $4 AND NOT l.fenced
           AND l.expires_at + INTERVAL '2 seconds' > CURRENT_TIMESTAMP
         FOR UPDATE OF t, l",
    )
    .bind(binding.tenant_id().as_str())
    .bind(binding.thread_id().as_uuid())
    .bind(binding.turn_id().as_uuid())
    .bind(millis(binding.lease_generation().get())?)
    .fetch_optional(&mut **transaction)
    .await
    .map(|owner| owner.is_some())
    .map_err(|_| AttemptStoreError::Unavailable)
}

async fn insert_locked_attempt(
    transaction: &mut Transaction<'_, Postgres>,
    binding: &ExactActionBinding,
    prepared_at_millis: u64,
) -> Result<bool, AttemptStoreError> {
    let action = binding.action();
    sqlx::query(
        "INSERT INTO tool_execution_attempts (
            tenant_id, attempt_id, thread_id, turn_id, lease_generation,
            descriptor_id, descriptor_version, effect, action_digest,
            profile_id, profile_version, prepared_at_millis,
            status, version
        ) SELECT
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'prepared', 1
          WHERE (
              SELECT COUNT(*) FROM tool_execution_attempts
              WHERE tenant_id = $1 AND thread_id = $3 AND turn_id = $4
          ) < 16
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
    .execute(&mut **transaction)
    .await
    .map(|outcome| outcome.rows_affected() == 1)
    .map_err(|_| AttemptStoreError::Unavailable)
}

async fn replay_prepared_from_pool(
    pool: &PgPool,
    binding: &ExactActionBinding,
    prepared_at_millis: u64,
) -> Result<Option<AttemptInsertResolution>, AttemptStoreError> {
    replay_prepared(
        sqlx::query(
            "SELECT thread_id, turn_id, lease_generation, descriptor_id,
                    descriptor_version, effect, action_digest, profile_id,
                    profile_version, prepared_at_millis, status, version
             FROM tool_execution_attempts
             WHERE tenant_id = $1 AND attempt_id = $2",
        )
        .bind(binding.tenant_id().as_str())
        .bind(binding.attempt_id().as_uuid())
        .fetch_optional(pool),
        binding,
        prepared_at_millis,
    )
    .await
}

async fn replay_prepared_from_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    binding: &ExactActionBinding,
    prepared_at_millis: u64,
) -> Result<Option<AttemptInsertResolution>, AttemptStoreError> {
    replay_prepared(
        sqlx::query(
            "SELECT thread_id, turn_id, lease_generation, descriptor_id,
                    descriptor_version, effect, action_digest, profile_id,
                    profile_version, prepared_at_millis, status, version
             FROM tool_execution_attempts
             WHERE tenant_id = $1 AND attempt_id = $2",
        )
        .bind(binding.tenant_id().as_str())
        .bind(binding.attempt_id().as_uuid())
        .fetch_optional(&mut **transaction),
        binding,
        prepared_at_millis,
    )
    .await
}

async fn replay_prepared(
    query: impl std::future::Future<Output = Result<Option<sqlx::postgres::PgRow>, sqlx::Error>>,
    binding: &ExactActionBinding,
    prepared_at_millis: u64,
) -> Result<Option<AttemptInsertResolution>, AttemptStoreError> {
    let Some(row) = query.await.map_err(|_| AttemptStoreError::Unavailable)? else {
        return Ok(None);
    };
    if !immutable_fields_match(&row, binding, Some(prepared_at_millis)) {
        return Err(AttemptStoreError::IdentityConflict);
    }
    Ok(Some(AttemptInsertResolution::Existing {
        status: row_status(&row)?,
        version: row_version(&row)?,
    }))
}
