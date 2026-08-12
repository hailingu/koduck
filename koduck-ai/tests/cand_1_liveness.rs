// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use koduck_ai::adapters::history::postgres::{
    LeaseKey, LeaseTiming, PostgresExecutor, PostgresTurnHistory, ReconcileOutcome, RecoveryOutcome,
};
use koduck_ai::application::{AcceptedTurn, HistoryError, NewItem, TurnCommand, TurnHistory};
use koduck_ai::domain::{
    Item, ItemPayload, LeaseGeneration, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId,
    Usage,
};

#[derive(Clone)]
struct SimulatedPostgres {
    state: Arc<Mutex<SimulatedState>>,
}

struct SimulatedState {
    available: bool,
    tenant_id: TenantId,
    accepted: AcceptedTurn,
    items: Vec<Item>,
    last_renewal_ms: u64,
    fenced: bool,
    terminal: bool,
    renewal_attempts: usize,
    transient_renewal_failures: usize,
    interrupt_checks: usize,
    interrupt_read_error: Option<HistoryError>,
}

impl SimulatedPostgres {
    fn seeded() -> (Self, LeaseKey, AcceptedTurn) {
        let tenant_id = TenantId::new("tenant-a").expect("valid tenant");
        let thread_id = ThreadId::new();
        let turn_id = TurnId::new();
        let input = Item::new(
            1,
            ItemPayload::UserMessage {
                content: "hello".to_owned(),
            },
        );
        let accepted = AcceptedTurn::new(
            tenant_id.clone(),
            thread_id,
            turn_id,
            LeaseGeneration::initial(),
            input.clone(),
        );
        let delta = Item::new(
            2,
            ItemPayload::AgentMessageDelta {
                content: "A".to_owned(),
            },
        );
        let key = LeaseKey::new(
            tenant_id.clone(),
            thread_id,
            turn_id,
            LeaseGeneration::initial(),
        );
        (
            Self {
                state: Arc::new(Mutex::new(SimulatedState {
                    available: true,
                    tenant_id,
                    accepted: accepted.clone(),
                    items: vec![input, delta],
                    last_renewal_ms: 0,
                    fenced: false,
                    terminal: false,
                    renewal_attempts: 0,
                    transient_renewal_failures: 0,
                    interrupt_checks: 0,
                    interrupt_read_error: None,
                })),
            },
            key,
            accepted,
        )
    }

    fn set_available(&self, available: bool) {
        self.state.lock().expect("state lock").available = available;
    }

    fn fail_next_renewals(&self, attempts: usize) {
        self.state
            .lock()
            .expect("state lock")
            .transient_renewal_failures = attempts;
    }

    fn renewal_attempts(&self) -> usize {
        self.state.lock().expect("state lock").renewal_attempts
    }

    fn fail_interrupt_reads(&self) {
        self.state.lock().expect("state lock").interrupt_read_error =
            Some(HistoryError::Unavailable);
    }

    fn interrupt_checks(&self) -> usize {
        self.state.lock().expect("state lock").interrupt_checks
    }
}

impl PostgresExecutor for SimulatedPostgres {
    fn request_interrupt(
        &self,
        _trust: &TrustContext,
        _turn_id: TurnId,
    ) -> Result<(), HistoryError> {
        Err(HistoryError::NotFound)
    }

    fn interruption_requested(&self, _turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        let mut state = self.state.lock().expect("state lock");
        state.interrupt_checks += 1;
        if let Some(error) = state.interrupt_read_error.clone() {
            return Err(error);
        }
        Ok(false)
    }

    fn prior_thread_items(
        &self,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> Result<Vec<Item>, HistoryError> {
        let state = self.state.lock().expect("state lock");
        if state.tenant_id == trust.tenant_id && state.accepted.thread_id == thread_id {
            Ok(state.items.clone())
        } else {
            Err(HistoryError::NotFound)
        }
    }

    fn accept_initial(&self, _command: &TurnCommand) -> Result<AcceptedTurn, HistoryError> {
        Err(HistoryError::Unavailable)
    }

    fn append(&self, turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        let mut state = self.state.lock().expect("state lock");
        if !state.available {
            return Err(HistoryError::Unavailable);
        }
        if state.fenced || turn.generation != state.accepted.generation {
            return Err(HistoryError::Fenced);
        }
        if state.terminal {
            return Err(HistoryError::AlreadyTerminal);
        }
        let terminal = matches!(item, NewItem::Terminal(_));
        let durable = Item::new(state.items.len() as u64 + 1, item.into_payload());
        state.items.push(durable.clone());
        state.terminal = terminal;
        Ok(durable)
    }

    fn replay(&self, tenant_id: &TenantId, turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        let state = self.state.lock().expect("state lock");
        if &state.tenant_id == tenant_id && state.accepted.turn_id == turn_id {
            Ok(state.items.clone())
        } else {
            Err(HistoryError::NotFound)
        }
    }

    fn renew_lease(&self, key: &LeaseKey, now_ms: u64) -> Result<(), HistoryError> {
        let mut state = self.state.lock().expect("state lock");
        state.renewal_attempts += 1;
        if state.transient_renewal_failures > 0 {
            state.transient_renewal_failures -= 1;
            return Err(HistoryError::Unavailable);
        }
        if !state.available {
            return Err(HistoryError::Unavailable);
        }
        if state.fenced || !key.matches(&state.tenant_id, &state.accepted) {
            return Err(HistoryError::Fenced);
        }
        state.last_renewal_ms = now_ms;
        Ok(())
    }

    fn reconcile_expired(
        &self,
        key: &LeaseKey,
        now_ms: u64,
        timing: LeaseTiming,
    ) -> Result<ReconcileOutcome, HistoryError> {
        let mut state = self.state.lock().expect("state lock");
        if !state.available {
            return Err(HistoryError::Unavailable);
        }
        if state.terminal {
            return Err(HistoryError::AlreadyTerminal);
        }
        if state.fenced || !key.matches(&state.tenant_id, &state.accepted) {
            return Err(HistoryError::Fenced);
        }
        if now_ms < state.last_renewal_ms + timing.reconcile_after_ms() {
            return Ok(ReconcileOutcome::TooEarly);
        }
        state.fenced = true;
        state.terminal = true;
        let terminal = Item::new(
            state.items.len() as u64 + 1,
            ItemPayload::Terminal(TerminalOutcome::Cancelled),
        );
        state.items.push(terminal);
        Ok(ReconcileOutcome::Cancelled)
    }

    fn expired_lease_keys(
        &self,
        now_ms: u64,
        timing: LeaseTiming,
    ) -> Result<Vec<LeaseKey>, HistoryError> {
        let state = self.state.lock().expect("state lock");
        if !state.available {
            return Err(HistoryError::Unavailable);
        }
        if state.terminal
            || state.fenced
            || now_ms < state.last_renewal_ms + timing.reconcile_after_ms()
        {
            return Ok(Vec::new());
        }
        Ok(vec![LeaseKey::new(
            state.tenant_id.clone(),
            state.accepted.thread_id,
            state.accepted.turn_id,
            state.accepted.generation,
        )])
    }

    fn recover_failed(
        &self,
        turn: &AcceptedTurn,
        _timing: LeaseTiming,
    ) -> Result<RecoveryOutcome, HistoryError> {
        self.append(
            turn,
            NewItem::Terminal(TerminalOutcome::Failed {
                code: "DURABILITY_UNAVAILABLE".to_owned(),
            }),
        )?;
        Ok(RecoveryOutcome::Failed)
    }
}

#[test]
fn postgres_completion_uses_one_atomic_append_operation() {
    let (executor, _key, accepted) = SimulatedPostgres::seeded();
    executor.fail_interrupt_reads();
    let mut history = PostgresTurnHistory::new(executor.clone());

    let terminal = history
        .append_completion(&accepted, Usage::zero())
        .expect("completion delegates directly to the bounded atomic append");

    assert!(matches!(
        terminal.payload,
        ItemPayload::Terminal(TerminalOutcome::Completed { .. })
    ));
    assert_eq!(executor.interrupt_checks(), 0);
}

#[test]
fn lease_renewal_retries_after_a_transient_store_failure() {
    let (executor, _, accepted) = SimulatedPostgres::seeded();
    executor.fail_next_renewals(1);
    let history = PostgresTurnHistory::new(executor.clone());
    let guard = history
        .start_turn_liveness(&accepted)
        .expect("renewal worker starts");

    thread::sleep(Duration::from_millis(10_500));
    drop(guard);

    assert!(
        executor.renewal_attempts() >= 2,
        "one transient failure must not stop all later heartbeat attempts"
    );
}

#[test]
fn production_reconciliation_worker_scans_expired_turns() {
    let (executor, key, _) = SimulatedPostgres::seeded();
    let history = PostgresTurnHistory::new(executor);
    let worker = history
        .start_reconciliation_worker()
        .expect("reconciliation worker starts");
    let deadline = Instant::now() + Duration::from_millis(500);

    loop {
        let replay = history
            .replay(&key.tenant_id, key.turn_id)
            .expect("replay remains readable");
        if replay.iter().any(|item| {
            matches!(
                item.payload,
                ItemPayload::Terminal(TerminalOutcome::Cancelled)
            )
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "reconciliation worker did not scan the expired generation"
        );
        thread::sleep(Duration::from_millis(5));
    }
    drop(worker);
}

#[test]
fn reconciliation_worker_startup_is_fallible_and_propagated() {
    let history = include_str!("../src/adapters/history/postgres.rs");
    let runtime = include_str!("../src/runtime/mod.rs");

    assert!(
        history.contains("thread::Builder::new()") && history.contains("koduck-ai-reconciliation"),
        "the global worker must use the fallible named-thread builder"
    );
    assert!(
        runtime.contains("RuntimeError::ReconciliationWorker")
            && runtime.contains("start_reconciliation_worker()")
            && runtime.contains("map_err(RuntimeError::ReconciliationWorker)?"),
        "runtime assembly must propagate reconciliation-worker spawn failure"
    );
}

#[test]
fn process_crash_fences_and_cancels_once() {
    let (executor, key, accepted) = SimulatedPostgres::seeded();
    let mut history = PostgresTurnHistory::new(executor.clone());

    assert_eq!(
        history
            .reconcile_expired(&key, 21_999)
            .expect("early reconciliation is a typed result"),
        ReconcileOutcome::TooEarly
    );
    assert_eq!(
        history
            .reconcile_expired(&key, 22_000)
            .expect("reconciliation at exact boundary succeeds"),
        ReconcileOutcome::Cancelled
    );
    let replay = history
        .replay(&key.tenant_id, key.turn_id)
        .expect("replay remains readable");
    assert_eq!(
        replay
            .iter()
            .filter(|item| matches!(
                item.payload,
                ItemPayload::Terminal(TerminalOutcome::Cancelled)
            ))
            .count(),
        1
    );
    assert_eq!(
        history.append(
            &accepted,
            NewItem::AgentMessageDelta {
                content: "late".to_owned(),
            },
        ),
        Err(HistoryError::Fenced)
    );
}

#[test]
fn concurrent_reconcilers_are_idempotent() {
    let (executor, key, accepted) = SimulatedPostgres::seeded();
    executor.set_available(false);
    let unavailable = race_reconcilers(&executor, &key);
    assert_eq!(
        unavailable
            .iter()
            .filter(|result| **result == Err(HistoryError::Unavailable))
            .count(),
        32
    );

    executor.set_available(true);
    let recovered = race_reconcilers(&executor, &key);
    assert_eq!(
        recovered
            .iter()
            .filter(|result| **result == Ok(ReconcileOutcome::Cancelled))
            .count(),
        1
    );
    assert_eq!(
        recovered
            .iter()
            .filter(|result| matches!(
                result,
                Err(HistoryError::AlreadyTerminal | HistoryError::Fenced)
            ))
            .count(),
        31
    );
    let mut history = PostgresTurnHistory::new(executor);
    assert!(matches!(
        history.append(
            &accepted,
            NewItem::Terminal(TerminalOutcome::Completed {
                usage: Usage::zero(),
            }),
        ),
        Err(HistoryError::AlreadyTerminal | HistoryError::Fenced)
    ));
}

fn race_reconcilers(
    executor: &SimulatedPostgres,
    key: &LeaseKey,
) -> Vec<Result<ReconcileOutcome, HistoryError>> {
    let mut handles = Vec::new();
    for _ in 0..32 {
        let executor = executor.clone();
        let key = key.clone();
        handles.push(thread::spawn(move || {
            PostgresTurnHistory::new(executor).reconcile_expired(&key, 22_000)
        }));
    }
    handles
        .into_iter()
        .map(|handle| handle.join().expect("reconciler thread"))
        .collect()
}
