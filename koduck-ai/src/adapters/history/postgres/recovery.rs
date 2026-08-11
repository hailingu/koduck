// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Bounded recovery ownership for accepted turns whose append became unavailable.

use std::thread;
use std::time::{Duration, Instant};

use crate::application::{AcceptedTurn, HistoryError};

use super::{LeaseTiming, PostgresExecutor, RecoveryOutcome};

/// Retains conditional recovery ownership until failure closes or fencing wins.
pub(super) fn schedule<E: PostgresExecutor + Send + 'static>(
    executor: E,
    accepted: AcceptedTurn,
    timing: LeaseTiming,
) -> Result<(), HistoryError> {
    thread::Builder::new()
        .name("koduck-ai-turn-recovery".to_owned())
        .spawn(move || recover(&executor, &accepted, timing))
        .map_err(|_| HistoryError::Unavailable)?;
    Ok(())
}

fn recover<E: PostgresExecutor>(executor: &E, accepted: &AcceptedTurn, timing: LeaseTiming) {
    let deadline = Instant::now().checked_add(Duration::from_millis(timing.reconcile_after_ms()));
    loop {
        match executor.recover_failed(accepted, timing) {
            Ok(RecoveryOutcome::Pending) => {}
            Ok(RecoveryOutcome::Failed)
            | Err(HistoryError::Fenced | HistoryError::AlreadyTerminal | HistoryError::NotFound) => {
                return;
            }
            Err(HistoryError::Unavailable) => {
                if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                    eprintln!("event=turn_recovery_deferred_to_reconciler");
                    return;
                }
                thread::park_timeout(Duration::from_millis(100));
            }
        }
    }
}
