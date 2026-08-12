// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use sqlx::postgres::PgPoolOptions;

use crate::application::{HistoryError, RecoveryHandoff, TurnLiveness};

use super::settle_commit_attempt;
use super::{BackgroundAdmission, LeaseRenewalGuard, ReconciliationWorker, SqlxPostgresExecutor};

#[test]
fn background_admission_rejects_saturation_and_releases_capacity() {
    let admission = Arc::new(BackgroundAdmission::new(1));
    let permit = admission.try_acquire().expect("first worker is admitted");

    assert!(matches!(
        admission.try_acquire(),
        Err(HistoryError::Unavailable)
    ));

    drop(permit);
    assert!(admission.try_acquire().is_ok());
}

#[test]
fn renewal_guard_drop_does_not_join_a_blocked_renewal() {
    let blocked = Arc::new((Mutex::new(true), Condvar::new()));
    let worker_blocked = Arc::clone(&blocked);
    let renewal = thread::spawn(move || {
        let (lock, ready) = &*worker_blocked;
        let mut is_blocked = lock.lock().expect("renewal gate lock");
        while *is_blocked {
            is_blocked = ready.wait(is_blocked).expect("renewal gate wait");
        }
    });
    let release_blocked = Arc::clone(&blocked);
    let release = thread::spawn(move || {
        thread::sleep(Duration::from_millis(250));
        let (lock, ready) = &*release_blocked;
        *lock.lock().expect("renewal gate lock") = false;
        ready.notify_all();
    });
    let guard = LeaseRenewalGuard {
        stop: Arc::new(AtomicBool::new(false)),
        thread: Some(renewal),
        permit_receiver: None,
        recovery: None,
    };

    let started = Instant::now();
    drop(guard);
    let elapsed = started.elapsed();
    release.join().expect("renewal release joins");

    assert!(
        elapsed < Duration::from_millis(100),
        "guard shutdown must not synchronously join an unbounded renewal: {elapsed:?}"
    );
}

#[test]
fn renewal_recovery_handoff_retains_permit_reservation() {
    let admission = Arc::new(BackgroundAdmission::new(1));
    let permit = admission.try_acquire().expect("renewal is admitted");
    let (permit_sender, permit_receiver) = std::sync::mpsc::sync_channel(1);
    let renewal = thread::spawn(move || {
        thread::park();
        permit_sender.send(permit).expect("handoff receiver exists");
    });
    let recovery_started = Arc::new(AtomicBool::new(false));
    let observed_recovery = Arc::clone(&recovery_started);
    let observed_admission = Arc::clone(&admission);
    let guard = Box::new(LeaseRenewalGuard {
        stop: Arc::new(AtomicBool::new(false)),
        thread: Some(renewal),
        permit_receiver: Some(permit_receiver),
        recovery: Some(Box::new(move |permit| {
            assert!(
                observed_admission.try_acquire().is_err(),
                "transferred permit must remain reserved while recovery starts"
            );
            observed_recovery.store(true, std::sync::atomic::Ordering::Release);
            drop(permit);
            Ok(())
        })),
    });

    let handoff = guard
        .handoff_to_recovery()
        .expect("reservation transfers to recovery");

    assert_eq!(handoff, RecoveryHandoff::Recovered);
    assert!(recovery_started.load(std::sync::atomic::Ordering::Acquire));
    assert!(
        admission.try_acquire().is_ok(),
        "capacity returns after recovery releases the transferred permit"
    );
}

#[test]
fn reconciliation_guard_drop_does_not_join_a_blocked_scan() {
    let blocked = Arc::new((Mutex::new(true), Condvar::new()));
    let worker_blocked = Arc::clone(&blocked);
    let scan = thread::spawn(move || {
        let (lock, ready) = &*worker_blocked;
        let mut is_blocked = lock.lock().expect("scan gate lock");
        while *is_blocked {
            is_blocked = ready.wait(is_blocked).expect("scan gate wait");
        }
    });
    let release_blocked = Arc::clone(&blocked);
    let release = thread::spawn(move || {
        thread::sleep(Duration::from_millis(250));
        let (lock, ready) = &*release_blocked;
        *lock.lock().expect("scan gate lock") = false;
        ready.notify_all();
    });
    let guard = ReconciliationWorker {
        stop: Arc::new(AtomicBool::new(false)),
        thread: Some(scan),
    };

    let started = Instant::now();
    drop(guard);
    let elapsed = started.elapsed();
    release.join().expect("scan release joins");

    assert!(
        elapsed < Duration::from_millis(100),
        "worker shutdown must not synchronously join an unbounded scan: {elapsed:?}"
    );
}

#[test]
fn database_attempt_deadline_stops_a_slow_recovery_operation() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .expect("test runtime");
    let pool = {
        let _runtime_context = runtime.enter();
        PgPoolOptions::new()
            .connect_lazy("postgresql://localhost/koduck")
            .expect("lazy test pool")
    };
    let executor = SqlxPostgresExecutor::new(pool, runtime.handle().clone());

    let result = executor.wait_with_deadline(Duration::from_millis(10), async {
        tokio::time::sleep(Duration::from_secs(1)).await;
        Ok(())
    });

    assert_eq!(result, Err(HistoryError::Unavailable));
}

#[tokio::test]
async fn timed_out_commit_returns_the_reconciled_durable_outcome() {
    let result = settle_commit_attempt(
        Duration::from_millis(1),
        async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok(7_u8)
        },
        async { Ok(Some(7_u8)) },
    )
    .await;

    assert_eq!(result, Ok(7));
}

#[tokio::test]
async fn timed_out_commit_reports_unavailable_only_after_absence_is_reconciled() {
    let result = settle_commit_attempt(
        Duration::from_millis(1),
        async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok(7_u8)
        },
        async { Ok(None) },
    )
    .await;

    assert_eq!(result, Err(HistoryError::Unavailable));
}

#[tokio::test]
async fn failed_commit_acknowledgement_returns_the_reconciled_durable_outcome() {
    let result = settle_commit_attempt(
        Duration::from_secs(1),
        async { Err(HistoryError::Unavailable) },
        async { Ok(Some(9_u8)) },
    )
    .await;

    assert_eq!(result, Ok(9));
}

#[tokio::test]
async fn commit_reconciliation_attempt_is_bounded_by_the_database_deadline() {
    let started = Instant::now();
    let result = settle_commit_attempt(
        Duration::from_millis(5),
        async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok(7_u8)
        },
        async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok(Some(7_u8))
        },
    )
    .await;

    assert_eq!(result, Err(HistoryError::Unavailable));
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "operation and reconciliation must each stop at their database attempt deadline"
    );
}
