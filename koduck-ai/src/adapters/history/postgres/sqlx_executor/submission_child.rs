// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! Identified-submission lookup, atomic acceptance, and bounded commit
//! reconciliation over the canonical `PostgreSQL` schema (SI-02, SI-03,
//! SI-06, SI-08).

use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::application::{
    AcceptedTurn, HistoryError, IdentifiedAcceptance, SubmissionObservation, TurnCommand,
    submission_lock_keys, validate_identified_command,
};
use crate::domain::{Item, ItemPayload, LeaseGeneration, ThreadId, TurnId};

use super::super::payload_codec::decode_original_user_input;
use super::super::settle_commit_attempt;
use super::{generation_i64, unavailable, write_initial_canonical_state};

/// One joined binding row with the original sequence-1 input payload.
struct BindingRow {
    thread_id: ThreadId,
    turn_id: TurnId,
    original_thread_id: Option<Uuid>,
    original_input: String,
}

/// The selector equality contract (SI-02b): an omitted or null original
/// selector matches only a selectorless retry; an explicit original selector
/// matches only the same explicit UUID, never the allocated Thread.
fn selector_matches(original: Option<Uuid>, requested: Option<ThreadId>) -> bool {
    match (original, requested) {
        (None, None) => true,
        (Some(original), Some(requested)) => original == requested.as_uuid(),
        _ => false,
    }
}

/// Compares one binding against the exact original request (SI-02b).
///
/// Returns the receipt for an exact match and the typed conflict for any
/// semantic drift in the input bytes or the requested selector.
fn compare_binding(
    binding: &BindingRow,
    command: &TurnCommand,
) -> Result<SubmissionObservation, HistoryError> {
    if !selector_matches(binding.original_thread_id, command.thread_id) {
        return Err(HistoryError::SubmissionConflict);
    }
    if binding.original_input != command.input {
        return Err(HistoryError::SubmissionConflict);
    }
    Ok(SubmissionObservation {
        submission_id: command.submission_id.ok_or(HistoryError::Unavailable)?,
        thread_id: binding.thread_id,
        turn_id: binding.turn_id,
    })
}

/// The single canonical binding read: one row only when the owned key exists,
/// with every joined canonical column intact.
///
/// A present binding whose joined Thread owner, Turn, lease, or sequence-1
/// input is missing, non-owned, or undecodable fails unavailable instead of
/// establishing absence (SI-08c). The optional-turn marker keeps that
/// distinction decidable: `joined` is false exactly when the binding exists
/// but its canonical structure is broken.
const BINDING_READ: &str = "SELECT s.thread_id, s.turn_id, s.original_thread_id, \
     i.item_type, i.payload, h.subject_id = $2 AS owned, \
     (t.turn_id IS NOT NULL AND l.generation IS NOT NULL \
      AND i.item_id IS NOT NULL) AS joined \
     FROM chat_submissions s \
     LEFT JOIN threads h ON h.tenant_id = s.tenant_id AND h.thread_id = s.thread_id \
     LEFT JOIN turns t ON t.tenant_id = s.tenant_id \
         AND t.thread_id = s.thread_id AND t.turn_id = s.turn_id \
     LEFT JOIN turn_leases l ON l.tenant_id = s.tenant_id \
         AND l.thread_id = s.thread_id AND l.turn_id = s.turn_id \
     LEFT JOIN turn_items i ON i.tenant_id = s.tenant_id \
         AND i.thread_id = s.thread_id AND i.turn_id = s.turn_id \
         AND i.sequence = 1 \
     WHERE s.tenant_id = $1 AND s.subject_id = $2 AND s.submission_id = $3";

/// Parses one complete binding row, failing closed on broken structure.
fn binding_from_row(row: &sqlx::postgres::PgRow) -> Result<Option<BindingRow>, HistoryError> {
    let owned: bool = row.try_get("owned").map_err(unavailable)?;
    let joined: bool = row.try_get("joined").map_err(unavailable)?;
    if !owned || !joined {
        // A present binding with missing or non-owned canonical data is
        // corrupt structure, never absence and never authority (SI-08c).
        return Err(HistoryError::Unavailable);
    }
    let thread_id: Uuid = row.try_get("thread_id").map_err(unavailable)?;
    let turn_id: Uuid = row.try_get("turn_id").map_err(unavailable)?;
    let original_thread_id: Option<Uuid> =
        row.try_get("original_thread_id").map_err(unavailable)?;
    let item_type: String = row.try_get("item_type").map_err(unavailable)?;
    let payload: String = row.try_get("payload").map_err(unavailable)?;
    let payload: serde_json::Value =
        serde_json::from_str(&payload).map_err(|_| HistoryError::Unavailable)?;
    Ok(Some(BindingRow {
        thread_id: ThreadId::from_uuid(thread_id),
        turn_id: TurnId::from_uuid(turn_id),
        original_thread_id,
        original_input: decode_original_user_input(&item_type, &payload)?,
    }))
}

/// Reads one owned binding without any advisory or row lock, observing only
/// committed state (SI-07a).
pub(super) async fn observation_async(
    pool: &PgPool,
    command: &TurnCommand,
) -> Result<Option<SubmissionObservation>, HistoryError> {
    let trust = &command.trust;
    let submission_id = command.submission_id.ok_or(HistoryError::Unavailable)?;
    let row = sqlx::query(BINDING_READ)
        .bind(trust.tenant_id.as_str())
        .bind(trust.subject_id.as_str())
        .bind(submission_id.as_uuid())
        .fetch_optional(pool)
        .await
        .map_err(unavailable)?;
    match row {
        None => Ok(None),
        Some(row) => binding_from_row(&row)?.map_or(Ok(None), |binding| {
            compare_binding(&binding, command).map(Some)
        }),
    }
}

/// Rechecks the full owned key under the submission lock and, when absent,
/// writes the binding and the complete canonical acceptance in one
/// transaction (SI-03a, SI-03b).
async fn accept_with_submission_async(
    pool: &PgPool,
    command: &TurnCommand,
    creator_operation_id: Uuid,
    thread_id: ThreadId,
    turn_id: TurnId,
    input: Item,
) -> Result<IdentifiedAcceptance, HistoryError> {
    let trust = &command.trust;
    let submission_id = command.submission_id.ok_or(HistoryError::Unavailable)?;
    let keys = submission_lock_keys(trust, submission_id);
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    // SI-03b: the submission lock precedes every Item lock this transaction
    // later takes, in the separate two-int4 key space, and the full owned key
    // is rechecked before any canonical state is allocated.
    sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
        .bind(keys.0)
        .bind(keys.1)
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
    let row = sqlx::query(BINDING_READ)
        .bind(trust.tenant_id.as_str())
        .bind(trust.subject_id.as_str())
        .bind(submission_id.as_uuid())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(unavailable)?;
    if let Some(row) = row {
        let binding = binding_from_row(&row)?;
        // Authoritative transaction recheck: an exact committed key is an
        // observation with zero canonical mutation; drift is the typed
        // conflict (SI-02d, SI-04).
        if let Some(binding) = binding {
            return compare_binding(&binding, command).map(IdentifiedAcceptance::Existing);
        }
    }
    if command.thread_id.is_some() {
        // SI-02e/SI-03a: an explicit Thread must already exist and remain
        // owned at final acceptance. Only the selectorless case allocates a
        // new Thread; the identified path never creates a caller-selected
        // one, so an unknown or foreign selector is the indistinguishable
        // typed `NotFound` without claiming the key.
        let owned = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM threads WHERE tenant_id = $1 \
             AND subject_id = $2 AND thread_id = $3)",
        )
        .bind(trust.tenant_id.as_str())
        .bind(trust.subject_id.as_str())
        .bind(thread_id.as_uuid())
        .fetch_one(&mut *transaction)
        .await
        .map_err(unavailable)?;
        if !owned {
            return Err(HistoryError::NotFound);
        }
    }
    write_initial_canonical_state(&mut transaction, command, thread_id, turn_id, &input).await?;
    sqlx::query(
        "INSERT INTO chat_submissions \
         (tenant_id, subject_id, submission_id, thread_id, turn_id, \
          original_thread_id, creator_operation_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(trust.tenant_id.as_str())
    .bind(trust.subject_id.as_str())
    .bind(submission_id.as_uuid())
    .bind(thread_id.as_uuid())
    .bind(turn_id.as_uuid())
    .bind(command.thread_id.map(ThreadId::as_uuid))
    .bind(creator_operation_id)
    .execute(&mut *transaction)
    .await
    .map_err(unavailable)?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(IdentifiedAcceptance::Created(AcceptedTurn::new(
        trust.tenant_id.clone(),
        thread_id,
        turn_id,
        LeaseGeneration::initial(),
        input,
    )))
}

/// Reconstructs this invocation's acceptance when the write acknowledgement
/// became uncertain (SI-06a through SI-06e).
///
/// The proof reads the complete committed binding under the same submission
/// lock and returns `Created` only when the binding matches this
/// creator-operation identity, original selector, and original input, and
/// the Turn is still started with its unfenced, unexpired generation-1
/// lease. A binding from another invocation, or this creator's terminal,
/// fenced, or expired Turn, is the observation receipt without authority.
async fn reconciled_acceptance_async(
    pool: &PgPool,
    command: &TurnCommand,
    creator_operation_id: Uuid,
    thread_id: ThreadId,
    turn_id: TurnId,
    input: &Item,
) -> Result<Option<IdentifiedAcceptance>, HistoryError> {
    let trust = &command.trust;
    let submission_id = command.submission_id.ok_or(HistoryError::Unavailable)?;
    let keys = submission_lock_keys(trust, submission_id);
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
        .bind(keys.0)
        .bind(keys.1)
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
    let row = sqlx::query(
        "SELECT s.thread_id, s.turn_id, s.original_thread_id, s.creator_operation_id, \
         i.item_type, i.payload, h.subject_id = $2 AS owned, \
         (t.turn_id IS NOT NULL AND l.generation IS NOT NULL \
          AND i.item_id IS NOT NULL) AS joined, \
         t.status, l.generation, l.fenced, \
         (l.expires_at > CURRENT_TIMESTAMP) AS lease_live \
         FROM chat_submissions s \
         LEFT JOIN threads h ON h.tenant_id = s.tenant_id AND h.thread_id = s.thread_id \
         LEFT JOIN turns t ON t.tenant_id = s.tenant_id \
             AND t.thread_id = s.thread_id AND t.turn_id = s.turn_id \
         LEFT JOIN turn_leases l ON l.tenant_id = s.tenant_id \
             AND l.thread_id = s.thread_id AND l.turn_id = s.turn_id \
         LEFT JOIN turn_items i ON i.tenant_id = s.tenant_id \
             AND i.thread_id = s.thread_id AND i.turn_id = s.turn_id \
             AND i.sequence = 1 \
         WHERE s.tenant_id = $1 AND s.subject_id = $2 AND s.submission_id = $3",
    )
    .bind(trust.tenant_id.as_str())
    .bind(trust.subject_id.as_str())
    .bind(submission_id.as_uuid())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(unavailable)?;
    let Some(row) = row else {
        // Nothing committed: the write provably rolled back or never ran.
        return Ok(None);
    };
    let committed_creator: Uuid = row.try_get("creator_operation_id").map_err(unavailable)?;
    let committed_thread: Uuid = row.try_get("thread_id").map_err(unavailable)?;
    let committed_turn: Uuid = row.try_get("turn_id").map_err(unavailable)?;
    let status: String = row.try_get("status").map_err(unavailable)?;
    let generation: Option<i64> = row.try_get("generation").map_err(unavailable)?;
    let fenced: Option<bool> = row.try_get("fenced").map_err(unavailable)?;
    let lease_live: Option<bool> = row.try_get("lease_live").map_err(unavailable)?;
    let binding = binding_from_row(&row)?;
    let Some(binding) = binding else {
        // The proof cannot be completed over a broken structure; the outcome
        // stays unavailable with no identity publication (SI-06e, SI-08c).
        return Err(HistoryError::Unavailable);
    };
    // Proven drift conflicts even during reconciliation (SI-06e).
    let receipt = compare_binding(&binding, command)?;
    let creator_is_live = committed_creator == creator_operation_id
        && committed_thread == thread_id.as_uuid()
        && committed_turn == turn_id.as_uuid()
        && status == "started"
        && generation == Some(generation_i64(LeaseGeneration::initial())?)
        && fenced == Some(false)
        && lease_live == Some(true);
    if creator_is_live {
        // SI-06c: the complete matching live proof grants this invocation the
        // created owner result, including inside its own settlement.
        return Ok(Some(IdentifiedAcceptance::Created(AcceptedTurn::new(
            trust.tenant_id.clone(),
            thread_id,
            turn_id,
            LeaseGeneration::initial(),
            input.clone(),
        ))));
    }
    // SI-06d: another invocation's binding, or this creator's terminal,
    // fenced, or expired Turn, is observation without authority.
    Ok(Some(IdentifiedAcceptance::Existing(receipt)))
}

/// Runs the identified acceptance through the shared bounded settlement: one
/// write attempt at its full reserved budget and, when unacknowledged, at
/// most one read-only reconciliation at its own full budget (SI-06b,
/// SI-07c).
///
/// The direct-port guard revalidates the owned input byte bound and the
/// non-nil submission identity before any database I/O (SI-01d).
pub(super) async fn settle_identified_acceptance(
    pool: &PgPool,
    command: &TurnCommand,
    attempt_budget: std::time::Duration,
) -> Result<IdentifiedAcceptance, HistoryError> {
    validate_identified_command(command).map_err(|_| HistoryError::Unavailable)?;
    // SI-06a: one private creator-operation identity per fresh invocation,
    // retained with its proposed Thread, Turn, and input identities through
    // settlement.
    let creator_operation_id = Uuid::new_v4();
    let thread_id = command.thread_id.unwrap_or_default();
    let turn_id = TurnId::new();
    let input = Item::new(
        1,
        ItemPayload::UserMessage {
            content: command.input.clone(),
        },
    );
    settle_commit_attempt(
        attempt_budget,
        Box::pin(accept_with_submission_async(
            pool,
            command,
            creator_operation_id,
            thread_id,
            turn_id,
            input.clone(),
        )),
        Box::pin(reconciled_acceptance_async(
            pool,
            command,
            creator_operation_id,
            thread_id,
            turn_id,
            &input,
        )),
    )
    .await
}
