use super::*;
use koduck_ai::domain::execution::ExecutionError;

#[test]
fn sealed_interruption_rejects_claim_dispatch_before_executor_call() {
    let harness = Harness::new();
    let (authority, attempt) = harness.prepared();

    // Interrupt the Turn with a cancellation service that blocks the prepared
    // close. request_interruption seals the Turn before any cancellation runs,
    // so the entry signal proves the sealed-but-still-Prepared window without
    // relying on thread scheduling.
    let interrupter = harness.interrupter();
    let (sealed_tx, sealed_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let mut blocking_cancellation = SignallingPreparedCancellation {
        entered: sealed_tx,
        release: release_rx,
        inner: coordinator(executor(CancelAcknowledgement::Acknowledged(
            CancelledEffectState::NotStarted,
        ))),
    };
    let tenant = harness.tenant.clone();
    let thread_id = harness.thread;
    let turn_id = harness.turn;
    let interrupt_thread = thread::spawn(move || {
        let mut approvals = NoPendingApprovals;
        interrupter.interrupt(
            &mut blocking_cancellation,
            &mut approvals,
            &tenant,
            thread_id,
            turn_id,
            &mut || 1_000,
        )
    });

    // The signal fires only after request_interruption has sealed the Turn, so
    // this bounded wait deterministically observes the sealed-but-Prepared
    // intermediate state.
    sealed_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("interruption seals the Turn and reaches the blocked prepared close");

    // The Turn is sealed but the D-7 is still Prepared (the close is blocked).
    // claim_dispatch must reject: the seal wins under the authority lock.
    let mut authority = authority;
    let mut attempt = attempt;
    let result = authority.claim_dispatch(&mut attempt, None, 1_000);
    assert!(
        matches!(result, Err(ExecutionError::InterruptionRequested)),
        "claim_dispatch must reject after interruption is sealed: {result:?}",
    );
    assert_eq!(
        attempt.status(),
        ExecutionStatus::Prepared,
        "the sealed attempt must remain Prepared and never reach the executor",
    );

    // Coordinator regression: a sealed dispatch claim rejection reports its own
    // interruption code instead of being misdiagnosed as an approval mismatch.
    let mut dispatch_coordinator = coordinator(executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::NotStarted,
    )));
    let dispatched =
        dispatch_coordinator.execute(&mut authority, None, &mut attempt, 1_000, &mut || 1_000);
    assert!(
        matches!(
            dispatched,
            Err(ExecutionPending::DispatchRejected {
                code: ExecutionFailure::InterruptionRequested,
            })
        ),
        "a sealed dispatch claim must report the interruption code: {dispatched:?}",
    );

    release_tx
        .send(())
        .expect("test releases the blocked prepared close");
    let outcome = interrupt_thread
        .join()
        .expect("blocking interruption joins");
    assert_eq!(
        outcome,
        Ok(InterruptionOutcome::Closed(
            ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::NotStarted,
            }
        )),
        "the released interruption closes the prepared D-7 without dispatch",
    );
}

#[test]
fn interruption_reaches_cancellation_while_dispatch_is_blocked() {
    let harness = Harness::new();
    let (authority, attempt) = harness.prepared();
    let interrupter = harness.interrupter();
    let tenant = harness.tenant.clone();
    let thread_id = harness.thread;
    let turn_id = harness.turn;
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let dispatch_coordinator = Arc::new(Mutex::new(ExecutionCoordinator::new(
        BlockingExecutor {
            entered: entered_tx,
            release: release_rx,
        },
        AlwaysCurrentLease,
        WinningCommitter { calls: 0 },
    )));
    let execution_coordinator = Arc::clone(&dispatch_coordinator);
    let execution = thread::spawn(move || {
        let mut authority = authority;
        let mut attempt = attempt;
        execution_coordinator
            .lock()
            .expect("execution coordinator lock is available")
            .execute(&mut authority, None, &mut attempt, 1_000, &mut || 1_000)
    });

    entered_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("dispatch entered the blocking executor");
    let (cancelled_tx, cancelled_rx) = mpsc::channel();
    let cancellation = thread::spawn(move || {
        let mut approvals = NoPendingApprovals;
        let mut cancellations = coordinator(executor(CancelAcknowledgement::Acknowledged(
            CancelledEffectState::NotStarted,
        )));
        let result = interrupter.interrupt(
            &mut cancellations,
            &mut approvals,
            &tenant,
            thread_id,
            turn_id,
            &mut || 1_000,
        );
        cancelled_tx
            .send((
                result,
                cancellations.executor().cancels,
                cancellations.committer().calls,
            ))
            .expect("test observes cancellation");
    });

    let cancellation_result = cancelled_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("independent cancellation completes before dispatch is released");
    release_tx
        .send(())
        .expect("dispatch is released for cleanup");
    let _ = execution.join().expect("blocked execution joins");
    cancellation.join().expect("cancellation joins");

    assert_eq!(
        cancellation_result,
        (
            Ok(InterruptionOutcome::Closed(
                ToolExecutionOutcome::Cancelled {
                    effect_state: EffectState::NotStarted,
                }
            )),
            1,
            1,
        ),
        "cancellation must reach its executor and durable-terminal paths before dispatch returns"
    );
}
