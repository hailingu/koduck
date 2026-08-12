// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use sqlx::postgres::PgPoolOptions;

use crate::application::HistoryError;

use super::{LeaseRenewalGuard, ReconciliationWorker, SqlxPostgresExecutor};

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
