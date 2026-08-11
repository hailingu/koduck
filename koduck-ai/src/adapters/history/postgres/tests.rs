// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::LeaseRenewalGuard;

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
