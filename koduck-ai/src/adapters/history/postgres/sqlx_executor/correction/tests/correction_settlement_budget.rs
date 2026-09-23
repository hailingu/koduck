// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md

//! AC-4: the deterministic two-second settlement budgets. Paused Tokio time
//! proves the exact arithmetic — at most one write attempt and one
//! reconciliation, each bounded at two seconds — without any database.

use std::future::{pending, poll_fn};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::WriteFailure;
use super::settle_correction_attempt;
use crate::application::CorrectionError;
use crate::domain::{Item, ItemPayload};

fn durable_item() -> Item {
    Item::new(
        1,
        ItemPayload::AgentMessageDelta {
            content: "settled".to_owned(),
        },
    )
}

#[tokio::test(start_paused = true)]
async fn a_committed_write_never_runs_reconciliation() {
    let item = durable_item();
    let started = tokio::time::Instant::now();
    let outcome = settle_correction_attempt(
        async { Ok(item.clone()) },
        poll_fn(|_| panic!("reconciliation must not run after a committed write")),
    )
    .await;
    assert_eq!(outcome, Ok(item));
    assert_eq!(started.elapsed(), Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn unknown_outcomes_consume_exactly_two_two_second_budgets() {
    let started = tokio::time::Instant::now();
    let outcome = settle_correction_attempt(pending(), pending()).await;
    assert_eq!(outcome, Err(CorrectionError::Unavailable));
    assert_eq!(started.elapsed(), Duration::from_secs(4));
}

#[tokio::test(start_paused = true)]
async fn a_failed_write_reconciles_once_and_proves_absence() {
    let reconciliations = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&reconciliations);
    let outcome = settle_correction_attempt(async { Err(WriteFailure::Ambiguous) }, async move {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    })
    .await;
    assert_eq!(outcome, Err(CorrectionError::NotApplied));
    assert_eq!(reconciliations.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn a_reconciled_exact_match_returns_the_durable_item() {
    let item = durable_item();
    let outcome = settle_correction_attempt(async { Err(WriteFailure::Ambiguous) }, async {
        Ok(Some(item.clone()))
    })
    .await;
    assert_eq!(outcome, Ok(item));
}

#[tokio::test(start_paused = true)]
async fn typed_reconciliation_conflicts_pass_through() {
    let outcome = settle_correction_attempt(async { Err(WriteFailure::Ambiguous) }, async {
        Err(CorrectionError::IdentityConflict)
    })
    .await;
    assert_eq!(outcome, Err(CorrectionError::IdentityConflict));
}

#[tokio::test(start_paused = true)]
async fn proven_rejections_never_reconcile() {
    let outcome = settle_correction_attempt(
        async { Err(WriteFailure::Resolved(CorrectionError::NotFound)) },
        poll_fn(|_| panic!("proven rejections must not reconcile")),
    )
    .await;
    assert_eq!(outcome, Err(CorrectionError::NotFound));
}

#[tokio::test(start_paused = true)]
async fn server_definitive_statement_failures_never_reconcile() {
    let outcome = settle_correction_attempt(
        async { Err(WriteFailure::Resolved(CorrectionError::Unavailable)) },
        poll_fn(|_| panic!("a server-rejected statement needs no reconciliation")),
    )
    .await;
    assert_eq!(outcome, Err(CorrectionError::Unavailable));
}
