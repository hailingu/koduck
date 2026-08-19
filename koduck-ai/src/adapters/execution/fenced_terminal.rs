// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Durable `failed/owner_fenced_after_dispatch` terminal for a running D-7
//! whose bound lease is definitively fenced (ADR-0003 lines 309-314).

use sqlx::PgPool;

use crate::application::{AttemptStoreError, AttemptTerminalResolution, EffectState};

use super::attempts::{effect_code, hex_digest, millis, resolve_terminal_loss};
use crate::domain::execution::ExactActionBinding;

/// Commits the fenced post-dispatch failure and classifies its loss.
///
/// The ownership guard inverts the current-generation terminal write: the
/// write wins only when the bound lease row exists and the same generation is
/// fenced, superseded, or past its arbitration window, so a still-current
/// lease can never be relabelled through this transition (TC-07/TC-12).
pub(super) async fn commit_fenced_failure(
    pool: &PgPool,
    binding: &ExactActionBinding,
    effect_state: EffectState,
    terminal_at_millis: u64,
) -> Result<AttemptTerminalResolution, AttemptStoreError> {
    // Only the executor-observed states of a post-dispatch fence may persist
    // as the canonical failure; every other combination keeps the
    // current-generation transitions.
    if !matches!(effect_state, EffectState::Started | EffectState::Unknown) {
        return Ok(AttemptTerminalResolution::Conflict);
    }
    if fenced_failure_winner(pool, binding, effect_state, terminal_at_millis).await? {
        return Ok(AttemptTerminalResolution::Won { version: 3 });
    }
    resolve_terminal_loss(pool, binding).await
}

/// Commits `failed/owner_fenced_after_dispatch` for one running D-7 whose
/// bound lease exists and is definitively not current.
///
/// The ownership guard mirrors the current-generation terminal write, but
/// inverted: the write wins only when the bound lease row exists and the same
/// generation is fenced, superseded, or past its arbitration window, so a
/// recovered or still-current lease can never be relabelled through this
/// transition.
async fn fenced_failure_winner(
    pool: &PgPool,
    binding: &ExactActionBinding,
    effect_state: EffectState,
    terminal_at_millis: u64,
) -> Result<bool, AttemptStoreError> {
    let action = binding.action();
    sqlx::query(
        "UPDATE tool_execution_attempts
         SET status = 'failed', effect_state = $3,
             failure_code = 'owner_fenced_after_dispatch',
             terminal_at_millis = $4, version = 3
         WHERE tenant_id = $1 AND attempt_id = $2 AND status = 'running'
           AND thread_id = $5 AND turn_id = $6 AND lease_generation = $7
           AND descriptor_id = $8 AND descriptor_version = $9
           AND effect = $10 AND action_digest = $11
           AND profile_id = $12 AND profile_version = $13
           AND EXISTS (
               SELECT 1 FROM turn_leases bound
               WHERE bound.tenant_id = $1 AND bound.thread_id = $5
                 AND bound.turn_id = $6 AND bound.generation = $7
           )
           AND NOT EXISTS (
               SELECT 1 FROM turn_leases bound
               WHERE bound.tenant_id = $1 AND bound.thread_id = $5
                 AND bound.turn_id = $6 AND bound.generation = $7
                 AND NOT bound.fenced
                 AND bound.expires_at + INTERVAL '2 seconds' > CURRENT_TIMESTAMP
           )
         RETURNING version",
    )
    .bind(binding.tenant_id().as_str())
    .bind(binding.attempt_id().as_uuid())
    .bind(effect_state.as_str())
    .bind(millis(terminal_at_millis)?)
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
    .map(|winner| winner.is_some())
    .map_err(|_| AttemptStoreError::Unavailable)
}
