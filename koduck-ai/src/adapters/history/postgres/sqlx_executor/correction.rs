// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md

//! One guarded correction-admission transaction for the canonical
//! `PostgreSQL` history: authenticated ownership, exact caller-stable
//! retries, bounded ancestor validation, one atomic append, and a truthful
//! two-budget settlement (ADR-0004 CA-02 through CA-09).

use std::future::Future;
use std::time::Duration;

use sqlx::PgPool;
use sqlx::Row;
use uuid::Uuid;

use crate::application::{CorrectionCommand, CorrectionError, CorrectionStore};
use crate::domain::{Item, ItemId, ItemPayload};

use super::super::commit_reconciliation;
use super::super::payload_codec::DurableItemCodec;
use super::SqlxPostgresExecutor;
use super::is_terminal_status;

#[cfg(test)]
mod correction_settlement_budget;

/// The exact write and reconciliation budget of each attempt (CA-07).
const ATTEMPT_BUDGET: Duration = Duration::from_secs(2);

/// The inclusive ancestor-node admission limit, counting the predecessor
/// and the root (CA-06).
const MAX_ANCESTOR_NODES: usize = 4_096;

/// The inclusive per-row stored-payload read cap (CA-06).
const MAX_STORED_PAYLOAD_BYTES: i64 = 1_048_576;

impl CorrectionStore for SqlxPostgresExecutor {
    fn correct(&self, command: CorrectionCommand) -> Result<Item, CorrectionError> {
        let reconcile = command.clone();
        self.runtime.block_on(settle_correction_attempt(
            correct_async(&self.pool, command),
            reconcile_async(&self.pool, reconcile),
        ))
    }
}

/// Settles one write attempt like `settle_commit_attempt`, but with the
/// richer correction error contract: only unavailable transports and
/// exhausted budgets are ambiguous, so every typed rejection passes through
/// without a reconciliation attempt (CA-07).
async fn settle_correction_attempt(
    operation: impl Future<Output = Result<Item, WriteFailure>>,
    reconcile: impl Future<Output = Result<Option<Item>, CorrectionError>>,
) -> Result<Item, CorrectionError> {
    match tokio::time::timeout(ATTEMPT_BUDGET, operation).await {
        Ok(Ok(item)) => Ok(item),
        Ok(Err(WriteFailure::Ambiguous)) | Err(_) => {
            match tokio::time::timeout(ATTEMPT_BUDGET, reconcile).await {
                Ok(Ok(Some(item))) => Ok(item),
                Ok(Ok(None)) => Err(CorrectionError::NotApplied),
                Ok(Err(error)) => Err(error),
                Err(_) => Err(CorrectionError::Unavailable),
            }
        }
        Ok(Err(WriteFailure::Resolved(error))) => Err(error),
    }
}

/// The private write-attempt classification of one admission attempt
/// (ADR-0004 CA-07): transport loss and budget expiry leave the commit
/// outcome ambiguous and are reconciled, while a definitive server response
/// — a typed rejection or a rejected statement — is caller-final and is
/// never reconciled into a guessed outcome.
enum WriteFailure {
    /// The commit outcome is unknown; one read-only reconciliation decides.
    Ambiguous,
    /// A definitive caller-visible outcome, including unexpected server
    /// rejections typed as [`CorrectionError::Unavailable`].
    Resolved(CorrectionError),
}

fn resolved(error: CorrectionError) -> WriteFailure {
    WriteFailure::Resolved(error)
}

fn classify_write_error(error: sqlx::Error) -> WriteFailure {
    match error {
        sqlx::Error::Database(database) => match database.code().as_deref() {
            // Operator-intervention and connection-class rejections are
            // transport loss: the commit outcome is genuinely unknown.
            Some("57P01" | "57P02" | "57P03") => WriteFailure::Ambiguous,
            Some(code) if code.starts_with("08") => WriteFailure::Ambiguous,
            // Any other server response is definitive: the statement was
            // rejected and the transaction cannot commit (CA-07).
            _ => WriteFailure::Resolved(CorrectionError::Unavailable),
        },
        _ => WriteFailure::Ambiguous,
    }
}

/// Admits one correction inside one guarded transaction (CA-05).
///
/// The caller-stable identity lock serializes the operation and its
/// reconciliation before any Turn lock, matching the existing
/// write/reconciliation order. Error precedence follows the ADR: ownership,
/// then the stored identity, then terminal state, then sequence validity,
/// then ancestor validity and limits, then the tip successor.
async fn correct_async(pool: &PgPool, command: CorrectionCommand) -> Result<Item, WriteFailure> {
    let mut transaction = pool.begin().await.map_err(classify_write_error)?;
    commit_reconciliation::lock_operation(&mut transaction, command.item_id().as_uuid())
        .await
        .map_err(|_| WriteFailure::Ambiguous)?;
    let ownership = lock_owned_turn(&mut transaction, &command).await?;
    if let Some(item) = stored_retry(&mut transaction, &command, &ownership.status).await? {
        return Ok(item);
    }
    if !is_terminal_status(&ownership.status) {
        return Err(resolved(CorrectionError::TurnNotTerminal));
    }
    let next_sequence =
        validate_counter(&mut transaction, &command, ownership.next_sequence).await?;
    validate_ancestry(&mut transaction, &command).await?;
    if tip_has_successor(&mut transaction, &command).await? {
        return Err(resolved(CorrectionError::PredecessorConflict));
    }
    let item = Item {
        item_id: command.item_id(),
        sequence: next_sequence,
        payload: ItemPayload::Correction(command.correction().clone()),
    };
    // The insert is stated here instead of through the shared `insert_item`
    // helper because that helper flattens every failure to
    // `HistoryError::Unavailable`, which would hide the statement-rejected
    // versus transport-lost distinction CA-07 reconciles on. The durable
    // columns still come from the shared codec, and the correction
    // constraints remain the production ones (ADR-0004 CA-05).
    let columns = DurableItemCodec::encode(&item.payload);
    sqlx::query(
        "INSERT INTO turn_items \
         (tenant_id, thread_id, turn_id, sequence, item_id, item_type, payload, \
          is_terminal, corrects_item_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(command.trust().tenant_id.as_str())
    .bind(command.thread_id().as_uuid())
    .bind(command.turn_id().as_uuid())
    .bind(i64::try_from(next_sequence).map_err(|_| resolved(CorrectionError::CorruptHistory))?)
    .bind(item.item_id.as_uuid())
    .bind(columns.item_type)
    .bind(columns.payload)
    .bind(columns.is_terminal)
    .bind(columns.corrects_item_id)
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;
    let updated = sqlx::query(
        "UPDATE turns SET next_sequence = next_sequence + 1 \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 AND next_sequence = $4",
    )
    .bind(command.trust().tenant_id.as_str())
    .bind(command.thread_id().as_uuid())
    .bind(command.turn_id().as_uuid())
    .bind(i64::try_from(next_sequence).map_err(|_| resolved(CorrectionError::CorruptHistory))?)
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;
    if updated.rows_affected() != 1 {
        return Err(resolved(CorrectionError::CorruptHistory));
    }
    transaction.commit().await.map_err(classify_write_error)?;
    Ok(item)
}

/// Resolves the durable Item of an exact stored identity, or every identity
/// drift (CA-04). Stored payload bodies stay under the CA-06 read cap and
/// are decoded strictly before any content comparison.
async fn stored_retry(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    command: &CorrectionCommand,
    turn_status: &str,
) -> Result<Option<Item>, WriteFailure> {
    let row = sqlx::query(
        "SELECT item_id, sequence, thread_id, turn_id, item_type, corrects_item_id, \
         octet_length(payload)::BIGINT AS payload_bytes FROM turn_items \
         WHERE tenant_id = $1 AND item_id = $2",
    )
    .bind(command.trust().tenant_id.as_str())
    .bind(command.item_id().as_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(classify_write_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let payload_bytes: i64 = row.try_get("payload_bytes").map_err(classify_write_error)?;
    if payload_bytes > MAX_STORED_PAYLOAD_BYTES {
        return Err(resolved(CorrectionError::ResourceLimit));
    }
    let stored_thread: Uuid = row.try_get("thread_id").map_err(classify_write_error)?;
    let stored_turn: Uuid = row.try_get("turn_id").map_err(classify_write_error)?;
    let item_type: String = row.try_get("item_type").map_err(classify_write_error)?;
    let corrects: Option<Uuid> = row
        .try_get("corrects_item_id")
        .map_err(classify_write_error)?;
    if stored_thread != command.thread_id().as_uuid()
        || stored_turn != command.turn_id().as_uuid()
        || item_type != "correction"
        || corrects != Some(command.predecessor_item_id().as_uuid())
    {
        return Err(resolved(CorrectionError::IdentityConflict));
    }
    let payload_text: String =
        sqlx::query_scalar("SELECT payload FROM turn_items WHERE tenant_id = $1 AND item_id = $2")
            .bind(command.trust().tenant_id.as_str())
            .bind(command.item_id().as_uuid())
            .fetch_one(&mut **transaction)
            .await
            .map_err(classify_write_error)?;
    let payload = DurableItemCodec::decode(&item_type, &payload_text, corrects)
        .map_err(|_| resolved(CorrectionError::CorruptHistory))?;
    let ItemPayload::Correction(correction) = payload else {
        return Err(resolved(CorrectionError::CorruptHistory));
    };
    if correction.content().as_bytes() != command.content().as_bytes() {
        return Err(resolved(CorrectionError::IdentityConflict));
    }
    if !is_terminal_status(turn_status) {
        // Lawful admission only writes corrections after termination, so an
        // exact match beneath a live Turn is inconsistent durable state
        // rather than a new-write rejection (CA-04).
        return Err(resolved(CorrectionError::CorruptHistory));
    }
    let item_id: Uuid = row.try_get("item_id").map_err(classify_write_error)?;
    let sequence: i64 = row.try_get("sequence").map_err(classify_write_error)?;
    Ok(Some(Item {
        item_id: ItemId::from_uuid(item_id),
        sequence: u64::try_from(sequence).map_err(|_| resolved(CorrectionError::CorruptHistory))?,
        payload: ItemPayload::Correction(correction),
    }))
}

/// Validates the Turn counter: positive, greater than every existing Turn
/// sequence, and incrementable within `BIGINT`; otherwise the durable state
/// is corrupt (CA-05).
async fn validate_counter(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    command: &CorrectionCommand,
    next_sequence: i64,
) -> Result<u64, WriteFailure> {
    let highest = highest_sequence(transaction, command).await?;
    if next_sequence <= 0 || next_sequence <= highest || next_sequence == i64::MAX {
        return Err(resolved(CorrectionError::CorruptHistory));
    }
    u64::try_from(next_sequence).map_err(|_| resolved(CorrectionError::CorruptHistory))
}

async fn highest_sequence(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    command: &CorrectionCommand,
) -> Result<i64, WriteFailure> {
    sqlx::query_scalar(
        "SELECT COALESCE(MAX(sequence), 0) FROM turn_items \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
    )
    .bind(command.trust().tenant_id.as_str())
    .bind(command.thread_id().as_uuid())
    .bind(command.turn_id().as_uuid())
    .fetch_one(&mut **transaction)
    .await
    .map_err(classify_write_error)
}

/// The one-shot server-side ancestry summary: the bounded recursive walk
/// plus its count, payload-cap, ordering, root-kind, and branch checks
/// (ADR-0004 CA-03 and CA-06).
const SUMMARY_SQL: &str = "WITH RECURSIVE chain AS ( \
   SELECT i.item_id, i.corrects_item_id, i.sequence, i.item_type, \
          octet_length(i.payload)::BIGINT AS payload_bytes, 1 AS depth \
   FROM turn_items i \
   WHERE i.tenant_id = $1 AND i.thread_id = $2 AND i.turn_id = $3 \
     AND i.item_id = $4 \
   UNION ALL \
   SELECT n.item_id, n.corrects_item_id, n.sequence, n.item_type, \
          n.payload_bytes, c.depth + 1 \
   FROM chain c CROSS JOIN LATERAL ( \
     SELECT n2.item_id, n2.corrects_item_id, n2.sequence, n2.item_type, \
            octet_length(n2.payload)::BIGINT AS payload_bytes \
     FROM turn_items n2 \
     WHERE n2.tenant_id = $1 AND n2.thread_id = $2 AND n2.turn_id = $3 \
       AND n2.item_id = c.corrects_item_id \
     LIMIT 1 \
   ) n \
   WHERE c.depth < 4097 \
 ) \
 SELECT \
   (SELECT count(*) FROM chain), \
   (SELECT bool_or(payload_bytes > $5) FROM chain), \
   (SELECT bool_or(later_seq >= earlier_seq) FROM ( \
      SELECT sequence AS later_seq, \
             LAG(sequence) OVER (ORDER BY depth) AS earlier_seq \
      FROM chain) w), \
   (SELECT item_type FROM chain ORDER BY depth DESC LIMIT 1), \
   (SELECT bool_or(cnt > 1) FROM ( \
      SELECT count(*) AS cnt \
      FROM chain c JOIN turn_items s \
        ON s.tenant_id = $1 AND s.corrects_item_id = c.item_id \
      GROUP BY c.item_id) bc)";

/// Validates the bounded predecessor ancestry: cycle-free, strictly
/// earlier, terminating at a supported message root, branch-free, and
/// within the node and stored-payload caps (CA-03 and CA-06).
///
/// The whole summary is computed server-side in one bounded recursive query
/// (capped at 4,097 walked nodes by depth), so admission makes no
/// per-ancestor round trips and retains no chain rows on the application
/// side. A cycle can never pass the strictly-decreasing order check —
/// sequences cannot strictly decrease around a loop — so a dedicated cycle
/// detection is unnecessary.
async fn validate_ancestry(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    command: &CorrectionCommand,
) -> Result<(), WriteFailure> {
    // The bounded walk must plan per execution with the bound scope values:
    // a cached generic plan degenerates the recursive worktable join, so
    // this one-shot summary never caches its statement.
    let summary = sqlx::query_as::<
        _,
        (
            i64,
            Option<bool>,
            Option<bool>,
            Option<String>,
            Option<bool>,
        ),
    >(SUMMARY_SQL)
    .bind(command.trust().tenant_id.as_str())
    .bind(command.thread_id().as_uuid())
    .bind(command.turn_id().as_uuid())
    .bind(command.predecessor_item_id().as_uuid())
    .bind(MAX_STORED_PAYLOAD_BYTES)
    .persistent(false)
    .fetch_one(&mut **transaction)
    .await
    .map_err(classify_write_error)?;
    reject_invalid_summary(&summary, transaction, command).await
}

/// Applies the CA-03/CA-06 admission precedence to one walked summary: the
/// ordering violation (which also subsumes cycles) precedes the node cap,
/// so a truncated cycle is reported as corruption rather than a resource
/// bound.
async fn reject_invalid_summary(
    summary: &(
        i64,
        Option<bool>,
        Option<bool>,
        Option<String>,
        Option<bool>,
    ),
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    command: &CorrectionCommand,
) -> Result<(), WriteFailure> {
    let (node_count, oversized, order_violation, last_type, branched) = summary.clone();

    // A missing or out-of-scope predecessor admits no chain (CA-03).
    if node_count == 0 {
        return Err(resolved(CorrectionError::InvalidPredecessor));
    }
    // Every ancestor is strictly earlier than its descendant (CA-03). This
    // is checked before the node cap and also subsumes cycle rejection:
    // sequences cannot strictly decrease around a loop, so any cyclic
    // ancestry necessarily violates the order at its revisit — even when
    // the bounded walk truncates at the cap — and fails closed as corrupt
    // durable state rather than a resource bound.
    if order_violation == Some(true) {
        return Err(resolved(CorrectionError::CorruptHistory));
    }
    // Observing one node beyond the admission limit is a resource bound
    // (CA-06): the walk yields at most 4,097 rows.
    let node_cap = i64::try_from(MAX_ANCESTOR_NODES).expect("the admission node cap fits i64");
    if node_count > node_cap {
        return Err(resolved(CorrectionError::ResourceLimit));
    }
    // The chain terminates at a supported message root; a correction whose
    // own target did not join the walk has a broken ancestor link (CA-03).
    let Some(last_type) = last_type.as_deref() else {
        return Err(resolved(CorrectionError::InvalidPredecessor));
    };
    if last_type == "correction" {
        return Err(resolved(CorrectionError::CorruptHistory));
    }
    if !matches!(last_type, "user_message" | "agent_message_delta") {
        return Err(resolved(CorrectionError::InvalidPredecessor));
    }
    // Each stored payload is capped before its body is fetched (CA-06).
    if oversized == Some(true) {
        return Err(resolved(CorrectionError::ResourceLimit));
    }
    // Any predecessor with two direct successors is corrupt durable state;
    // the production one-successor index makes this impossible, and the
    // bounded check defends the fixture-only shapes the acceptance tests
    // seed (CA-03).
    if branched == Some(true) {
        return Err(resolved(CorrectionError::CorruptHistory));
    }
    reject_malformed_predecessor(transaction, command).await
}

/// Decodes exactly one ancestor payload — the direct predecessor's — so
/// admission never retains full-history content (CA-06); a malformed one is
/// corrupt durable state (CA-03). The payload validity of deeper ancestors
/// is owned by the CAND-3 raw-replay boundary, which fails closed on every
/// replayed row (CR-05).
async fn reject_malformed_predecessor(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    command: &CorrectionCommand,
) -> Result<(), WriteFailure> {
    let stored = sqlx::query_as::<_, (String, String, Option<Uuid>)>(
        "SELECT item_type, payload, corrects_item_id FROM turn_items \
         WHERE tenant_id = $1 AND item_id = $2",
    )
    .bind(command.trust().tenant_id.as_str())
    .bind(command.predecessor_item_id().as_uuid())
    .fetch_one(&mut **transaction)
    .await
    .map_err(classify_write_error)?;
    if DurableItemCodec::decode(&stored.0, &stored.1, stored.2).is_err() {
        return Err(resolved(CorrectionError::CorruptHistory));
    }
    Ok(())
}

/// Reports whether the otherwise valid predecessor tip already carries one
/// direct correction successor (CA-03).
async fn tip_has_successor(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    command: &CorrectionCommand,
) -> Result<bool, WriteFailure> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM turn_items \
         WHERE tenant_id = $1 AND corrects_item_id = $2)",
    )
    .bind(command.trust().tenant_id.as_str())
    .bind(command.predecessor_item_id().as_uuid())
    .fetch_one(&mut **transaction)
    .await
    .map_err(classify_write_error)
}

async fn lock_owned_turn(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    command: &CorrectionCommand,
) -> Result<TurnOwnership, WriteFailure> {
    let row = sqlx::query(
        "SELECT t.status, t.next_sequence FROM turns t JOIN threads h \
         ON h.tenant_id = t.tenant_id AND h.thread_id = t.thread_id \
         WHERE t.tenant_id = $1 AND h.subject_id = $2 AND t.thread_id = $3 \
         AND t.turn_id = $4 FOR UPDATE OF t",
    )
    .bind(command.trust().tenant_id.as_str())
    .bind(command.trust().subject_id.as_str())
    .bind(command.thread_id().as_uuid())
    .bind(command.turn_id().as_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(classify_write_error)?
    .ok_or(resolved(CorrectionError::NotFound))?;
    Ok(TurnOwnership {
        status: row.try_get("status").map_err(classify_write_error)?,
        next_sequence: row.try_get("next_sequence").map_err(classify_write_error)?,
    })
}

/// Repeats the ownership and exact-identity checks read-only after an
/// ambiguous acknowledgement, holding the same operation-identity lock so a
/// settled writer cannot be contradicted (CA-07).
async fn reconcile_async(
    pool: &PgPool,
    command: CorrectionCommand,
) -> Result<Option<Item>, CorrectionError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|_| CorrectionError::Unavailable)?;
    commit_reconciliation::lock_operation(&mut transaction, command.item_id().as_uuid())
        .await
        .map_err(|_| CorrectionError::Unavailable)?;
    let status: Option<String> = sqlx::query_scalar(
        "SELECT t.status FROM turns t JOIN threads h \
         ON h.tenant_id = t.tenant_id AND h.thread_id = t.thread_id \
         WHERE t.tenant_id = $1 AND h.subject_id = $2 AND t.thread_id = $3 \
         AND t.turn_id = $4",
    )
    .bind(command.trust().tenant_id.as_str())
    .bind(command.trust().subject_id.as_str())
    .bind(command.thread_id().as_uuid())
    .bind(command.turn_id().as_uuid())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| CorrectionError::Unavailable)?;
    let Some(turn_status) = status else {
        return Err(CorrectionError::NotFound);
    };
    match stored_retry(&mut transaction, &command, &turn_status).await {
        Ok(outcome) => Ok(outcome),
        Err(WriteFailure::Resolved(error)) => Err(error),
        Err(WriteFailure::Ambiguous) => Err(CorrectionError::Unavailable),
    }
}

struct TurnOwnership {
    status: String,
    next_sequence: i64,
}
