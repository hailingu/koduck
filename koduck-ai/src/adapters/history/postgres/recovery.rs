// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Bounded recovery ownership for accepted turns whose append became unavailable.

use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::application::{AcceptedTurn, HistoryError};

use super::{
    BackgroundAdmission, BackgroundPermit, LeaseTiming, PostgresExecutor, RecoveryOutcome,
};

/// Retains conditional recovery ownership until failure closes or fencing wins.
pub(super) fn schedule<E: PostgresExecutor + Send + 'static>(
    executor: E,
    accepted: AcceptedTurn,
    timing: LeaseTiming,
    admission: &Arc<BackgroundAdmission>,
) -> Result<(), HistoryError> {
    let permit = admission.try_acquire()?;
    schedule_job_with_permit(
        permit,
        Box::new(move || recover(&executor, &accepted, timing)),
        |receiver| {
            thread::Builder::new()
                .name("koduck-ai-turn-recovery".to_owned())
                .spawn(move || run_received_job(&receiver))
                .map(|_| ())
        },
    );
    Ok(())
}

/// Completes recovery while retaining an already-owned background reservation.
pub(super) fn recover_with_permit<E: PostgresExecutor>(
    executor: &E,
    accepted: &AcceptedTurn,
    timing: LeaseTiming,
    permit: BackgroundPermit,
) {
    let _permit = permit;
    recover(executor, accepted, timing);
}

type RecoveryJob = Box<dyn FnOnce() + Send + 'static>;
type RecoveryEnvelope = (RecoveryJob, BackgroundPermit);

fn schedule_job_with_permit(
    permit: BackgroundPermit,
    job: RecoveryJob,
    spawn: impl FnOnce(mpsc::Receiver<RecoveryEnvelope>) -> std::io::Result<()>,
) {
    let (sender, receiver) = mpsc::channel();
    if spawn(receiver).is_err() {
        let _permit = permit;
        job();
        return;
    }
    match sender.send((job, permit)) {
        Ok(()) => {}
        Err(mpsc::SendError((job, permit))) => {
            let _permit = permit;
            job();
        }
    }
}

fn run_received_job(receiver: &mpsc::Receiver<RecoveryEnvelope>) {
    if let Ok((job, permit)) = receiver.recv() {
        let _permit = permit;
        job();
    }
}

fn recover<E: PostgresExecutor>(executor: &E, accepted: &AcceptedTurn, timing: LeaseTiming) {
    let Some(deadline) =
        Instant::now().checked_add(Duration::from_millis(timing.reconcile_after_ms()))
    else {
        eprintln!("event=turn_recovery_deferred_to_reconciler");
        return;
    };
    loop {
        let Some(attempt_timeout) = remaining_attempt_timeout(deadline, Instant::now()) else {
            eprintln!("event=turn_recovery_deferred_to_reconciler");
            return;
        };
        match executor.recover_failed_with_deadline(accepted, timing, attempt_timeout) {
            Ok(RecoveryOutcome::Pending) => {}
            Ok(RecoveryOutcome::Failed)
            | Err(
                HistoryError::Fenced
                | HistoryError::AlreadyTerminal
                | HistoryError::NotFound
                | HistoryError::ContextLimit,
            ) => {
                return;
            }
            Err(HistoryError::Unavailable) => {
                let retry_delay = deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(100));
                thread::park_timeout(retry_delay);
            }
        }
    }
}

fn remaining_attempt_timeout(deadline: Instant, now: Instant) -> Option<Duration> {
    let remaining = deadline.checked_duration_since(now)?;
    (!remaining.is_zero()).then_some(remaining.min(Duration::from_secs(2)))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use super::{BackgroundAdmission, remaining_attempt_timeout, schedule_job_with_permit};

    #[test]
    fn final_recovery_attempt_is_capped_to_the_remaining_window() {
        let now = Instant::now();
        let remaining = Duration::from_millis(7);

        assert_eq!(
            remaining_attempt_timeout(now + remaining, now),
            Some(remaining)
        );
    }

    #[test]
    fn expired_recovery_window_starts_no_further_attempt() {
        let now = Instant::now();

        assert_eq!(remaining_attempt_timeout(now, now), None);
    }

    #[test]
    fn spawn_failure_runs_recovery_while_retaining_the_permit() {
        let admission = Arc::new(BackgroundAdmission::new(1));
        let permit = admission.try_acquire().expect("recovery is admitted");
        let ran = Arc::new(AtomicBool::new(false));
        let observed_ran = Arc::clone(&ran);
        let observed_admission = Arc::clone(&admission);

        schedule_job_with_permit(
            permit,
            Box::new(move || {
                assert!(
                    observed_admission.try_acquire().is_err(),
                    "synchronous fallback must retain the transferred permit"
                );
                observed_ran.store(true, Ordering::Release);
            }),
            |_| Err(std::io::Error::other("thread quota exhausted")),
        );

        assert!(ran.load(Ordering::Acquire));
        assert!(admission.try_acquire().is_ok());
    }
}
