// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md

//! One ordered, provenance-preserving prior-Turn history read (ADR-0006
//! PC-01/PC-03): a single tuple-ordered `SELECT` streams every canonical row
//! of the subject-owned Thread through the unchanged aggregate raw admission,
//! then decodes each row and groups consecutive rows by their actual Turn
//! identity. One statement supplies the complete raw Items of every returned
//! Turn — including rows after a terminal — with no per-Turn read.

use sqlx::{PgPool, Row};
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::application::{HistoryError, PriorTurnHistory, PriorTurnRow, ProjectionScope};
use crate::domain::{Item, ThreadId, TrustContext, TurnId};

use super::super::commit_reconciliation::{MAX_PROVIDER_HISTORY_QUERY_ROWS, push_bounded_history};
use super::super::payload_codec::row_to_item;
use super::unavailable;

/// Reads the complete bounded prior-Turn groups of one subject-owned Thread.
pub(super) async fn read(
    pool: &PgPool,
    trust: &TrustContext,
    thread_id: ThreadId,
) -> Result<Vec<PriorTurnHistory>, HistoryError> {
    let rows = sqlx::query(
        "SELECT turn_items.turn_id, turn_items.item_id, turn_items.sequence, \
         turn_items.item_type, turn_items.payload, turn_items.corrects_item_id FROM turn_items \
         JOIN turns ON turns.tenant_id = turn_items.tenant_id \
         AND turns.thread_id = turn_items.thread_id \
         AND turns.turn_id = turn_items.turn_id JOIN threads \
         ON threads.tenant_id = turn_items.tenant_id \
         AND threads.thread_id = turn_items.thread_id \
         WHERE turn_items.tenant_id = $1 AND turn_items.thread_id = $2 \
         AND threads.subject_id = $3 ORDER BY turns.created_at, \
         turn_items.turn_id, turn_items.sequence LIMIT $4",
    )
    .bind(trust.tenant_id.as_str())
    .bind(thread_id.as_uuid())
    .bind(trust.subject_id.as_str())
    .bind(MAX_PROVIDER_HISTORY_QUERY_ROWS)
    .fetch(pool);
    tokio::pin!(rows);
    let mut history = Vec::new();
    let mut row_turns = Vec::new();
    let mut payload_bytes = 0_usize;
    while let Some(row) = rows.next().await {
        let row = row.map_err(unavailable)?;
        let turn_id = TurnId::from_uuid(row.try_get::<Uuid, _>("turn_id").map_err(unavailable)?);
        push_bounded_history(&mut history, &mut payload_bytes, row_to_item(&row)?)?;
        row_turns.push(turn_id);
    }
    if history.is_empty() {
        ensure_owned_thread(pool, trust, thread_id).await?;
        return Ok(Vec::new());
    }
    group_admitted_rows(history, row_turns, trust, thread_id)
}

/// Fails closed when the data query returned nothing for a foreign Thread.
///
/// The existence query runs only after the data `SELECT` was empty, so an
/// owned Thread with no rows returns empty history while an unknown or
/// non-owned Thread returns [`HistoryError::NotFound`] without borrowing any
/// foreign row.
async fn ensure_owned_thread(
    pool: &PgPool,
    trust: &TrustContext,
    thread_id: ThreadId,
) -> Result<(), HistoryError> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM threads WHERE tenant_id = $1 \
         AND thread_id = $2 AND threads.subject_id = $3)",
    )
    .bind(trust.tenant_id.as_str())
    .bind(thread_id.as_uuid())
    .bind(trust.subject_id.as_str())
    .fetch_one(pool)
    .await
    .map_err(unavailable)?;
    if !exists {
        return Err(HistoryError::NotFound);
    }
    Ok(())
}

/// Groups the admitted rows by their actual Turn identity. Rows arrive in
/// `created_at`, `turn_id`, `sequence` order, so rows of one Turn are
/// consecutive; each row keeps the scope its authenticated source reported.
fn group_admitted_rows(
    history: Vec<Item>,
    row_turns: Vec<TurnId>,
    trust: &TrustContext,
    thread_id: ThreadId,
) -> Result<Vec<PriorTurnHistory>, HistoryError> {
    let mut groups: Vec<PriorTurnHistory> = Vec::new();
    for (item, turn_id) in history.into_iter().zip(row_turns) {
        let row = PriorTurnRow::new(item, reported_scope(trust, thread_id, turn_id)?);
        match groups.last_mut() {
            Some(group) if group.source_turn == turn_id => group.rows.push(row),
            _ => groups.push(PriorTurnHistory::new(turn_id, vec![row])),
        }
    }
    Ok(groups)
}

/// Builds the per-row scope from the validated command identity and the
/// Turn the row's own `turn_items.turn_id` column reported.
fn reported_scope(
    trust: &TrustContext,
    thread_id: ThreadId,
    turn_id: TurnId,
) -> Result<ProjectionScope, HistoryError> {
    ProjectionScope::new(
        trust.tenant_id.clone(),
        trust.subject_id.as_str(),
        thread_id,
        turn_id,
    )
    .map_err(|_| HistoryError::Unavailable)
}
