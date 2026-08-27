// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use koduck_ai::adapters::history::postgres::{
    LeaseKey, LeaseTiming, PostgresExecutor, PostgresTurnHistory, ReconcileOutcome,
    RecoveryOutcome, TurnTerminalObserver,
};
use koduck_ai::application::{
    AcceptedTurn, HistoryError, ModelInput, ModelProvider, NewItem, ProviderError, ProviderEvent,
    ProviderStream, TurnCommand, TurnHistory, TurnRunner,
};
use koduck_ai::domain::{
    Item, ItemPayload, LeaseGeneration, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId,
    TurnStatus, Usage,
};

#[derive(Clone)]
struct SimulatedPostgres {
    state: Arc<Mutex<SimulatedState>>,
}

#[derive(Clone, Copy, Default)]
enum ReconciliationRace {
    #[default]
    None,
    TerminalWonElsewhere,
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
    reconciliation_race: ReconciliationRace,
}

#[derive(Clone, Default)]
struct RecordingTerminalObserver {
    terminals: Arc<Mutex<Vec<(TenantId, ThreadId, TurnId)>>>,
}

impl RecordingTerminalObserver {
    fn terminals(&self) -> Vec<(TenantId, ThreadId, TurnId)> {
        self.terminals.lock().expect("observer lock").clone()
    }
}

impl TurnTerminalObserver for RecordingTerminalObserver {
    fn terminal_may_have_committed(
        &self,
        tenant_id: &TenantId,
        thread_id: ThreadId,
        turn_id: TurnId,
    ) {
        self.terminals
            .lock()
            .expect("observer lock")
            .push((tenant_id.clone(), thread_id, turn_id));
    }
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
                    reconciliation_race: ReconciliationRace::None,
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

    fn lose_reconciliation_race(&self) {
        self.state.lock().expect("state lock").reconciliation_race =
            ReconciliationRace::TerminalWonElsewhere;
    }
}

impl PostgresExecutor for SimulatedPostgres {
    fn request_interrupt(
        &self,
        _trust: &TrustContext,
        _turn_id: TurnId,
        _tool_terminals: Vec<koduck_ai::application::NewItem>,
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
        if matches!(
            state.reconciliation_race,
            ReconciliationRace::TerminalWonElsewhere
        ) {
            state.reconciliation_race = ReconciliationRace::None;
            state.terminal = true;
            return Err(HistoryError::AlreadyTerminal);
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
fn in_request_recovery_handoff_leaves_the_terminal_notification_to_the_runner() {
    // The runner already notifies the tool boundary after a Recovered
    // handoff, so the in-request recovery closure must not fire the
    // history-side terminal observer as well: both paths share one canonical
    // probe, and a duplicated notification doubles the durable query and
    // reclamation work — up to two extra two-second probes per recovery
    // while the database is degraded. The history observer stays reserved
    // for recovery the runner does not own: the background worker and the
    // scheduled jobs.
    let (executor, _, accepted) = SimulatedPostgres::seeded();
    let observer = Arc::new(RecordingTerminalObserver::default());
    let history = PostgresTurnHistory::new(executor).with_terminal_observer(observer.clone());
    let guard = history
        .start_turn_liveness(&accepted)
        .expect("renewal worker starts");

    let handoff = guard
        .handoff_to_recovery()
        .expect("the in-request recovery handoff completes");

    assert_eq!(
        handoff,
        koduck_ai::application::RecoveryHandoff::Recovered,
        "the seeded fixture recovers in-request"
    );
    assert!(
        observer.terminals().is_empty(),
        "the in-request handoff must not duplicate the runner's terminal notification"
    );
}

#[test]
fn direct_reconcile_expired_notifies_the_terminal_observer() {
    // The public reconciliation entry must behave like the background worker:
    // every outcome that may have durably terminalized the Turn gives the
    // observer a chance to prove it and release local C-5 authority, while
    // TooEarly and Fenced — which never commit a terminal — stay silent.
    let (executor, key, _) = SimulatedPostgres::seeded();
    let observer = Arc::new(RecordingTerminalObserver::default());
    let mut history = PostgresTurnHistory::new(executor).with_terminal_observer(observer.clone());

    // A stale key fences without terminalizing and stays silent.
    let stale = LeaseKey::new(
        key.tenant_id.clone(),
        key.thread_id,
        TurnId::new(),
        LeaseGeneration::initial(),
    );
    assert_eq!(
        history.reconcile_expired(&stale, 22_000),
        Err(HistoryError::Fenced),
        "a stale key fences without terminalizing"
    );
    assert!(
        observer.terminals().is_empty(),
        "Fenced commits no terminal and notifies nobody"
    );

    assert_eq!(
        history
            .reconcile_expired(&key, 21_999)
            .expect("early reconciliation is a typed result"),
        ReconcileOutcome::TooEarly
    );
    assert!(
        observer.terminals().is_empty(),
        "TooEarly commits no terminal and notifies nobody"
    );

    assert_eq!(
        history
            .reconcile_expired(&key, 22_000)
            .expect("reconciliation at exact boundary succeeds"),
        ReconcileOutcome::Cancelled
    );
    assert_eq!(
        observer.terminals(),
        vec![(key.tenant_id.clone(), key.thread_id, key.turn_id)],
        "the direct Cancelled reconciliation notifies the observer exactly like the worker"
    );
}

#[test]
fn direct_reconcile_expired_notifies_after_losing_the_terminal_race() {
    // A caller that loses the race to another reconciler still durably
    // terminalized its Turn, so the direct path must give the observer the
    // same AlreadyTerminal notification the background worker sends.
    let (executor, key, _) = SimulatedPostgres::seeded();
    executor.lose_reconciliation_race();
    let observer = Arc::new(RecordingTerminalObserver::default());
    let mut history = PostgresTurnHistory::new(executor).with_terminal_observer(observer.clone());

    assert_eq!(
        history.reconcile_expired(&key, 22_000),
        Err(HistoryError::AlreadyTerminal),
        "the competing reconciler owns the terminal"
    );
    assert_eq!(
        observer.terminals(),
        vec![(key.tenant_id.clone(), key.thread_id, key.turn_id)],
        "the direct AlreadyTerminal loss notifies the observer exactly like the worker"
    );
}

#[test]
fn reconciliation_worker_notifies_terminal_observer_after_closing_a_turn() {
    // This fails if the global reconciler commits a terminal but never gives
    // C-5 a second chance to release its process-local Turn authority.
    let (executor, key, _) = SimulatedPostgres::seeded();
    let observer = Arc::new(RecordingTerminalObserver::default());
    let history = PostgresTurnHistory::new(executor).with_terminal_observer(observer.clone());
    let worker = history
        .start_reconciliation_worker()
        .expect("reconciliation worker starts");
    let deadline = Instant::now() + Duration::from_millis(500);

    loop {
        if observer.terminals() == vec![(key.tenant_id.clone(), key.thread_id, key.turn_id)] {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the terminal observer did not receive the reconciler-owned completion"
        );
        thread::sleep(Duration::from_millis(5));
    }
    drop(worker);
}

#[test]
fn reconciliation_worker_notifies_terminal_observer_after_losing_the_terminal_race() {
    // This catches the competing-reconciler path: the scan observes a live
    // expired key, another instance closes it, and this worker then receives
    // AlreadyTerminal. The observer must still get a chance to independently
    // prove the terminal and release any local C-5 authority.
    let (executor, key, _) = SimulatedPostgres::seeded();
    executor.lose_reconciliation_race();
    let observer = Arc::new(RecordingTerminalObserver::default());
    let history = PostgresTurnHistory::new(executor).with_terminal_observer(observer.clone());
    let worker = history
        .start_reconciliation_worker()
        .expect("reconciliation worker starts");
    let deadline = Instant::now() + Duration::from_millis(500);

    loop {
        if observer.terminals() == vec![(key.tenant_id.clone(), key.thread_id, key.turn_id)] {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the observer missed the terminal committed by the competing reconciler"
        );
        thread::sleep(Duration::from_millis(5));
    }
    drop(worker);
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

// ADR: docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md

/// One provider that buffers scripted deltas, then flips a shared flag and
/// idles with Pending frames, so interruption and cancellation land on a
/// non-empty accumulator (ADR-0005 AC-6).
struct BufferedIdleProvider {
    deltas: Vec<String>,
    flag: Rc<Cell<bool>>,
    idle_sleep: Duration,
    flag_after: Option<Duration>,
    started: Option<Instant>,
}

impl ModelProvider for BufferedIdleProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        let deltas = self.deltas.clone();
        let flag = Rc::clone(&self.flag);
        let idle_sleep = self.idle_sleep;
        let flag_after = self.flag_after;
        let started = self.started.get_or_insert_with(Instant::now);
        let mut delta_index = 0;
        Ok(Box::new(std::iter::from_fn(move || {
            if delta_index < deltas.len() {
                let delta = deltas[delta_index].clone();
                delta_index += 1;
                return Some(ProviderEvent::Delta(delta));
            }
            if !idle_sleep.is_zero() {
                thread::sleep(idle_sleep);
            }
            if let Some(flag_after) = flag_after
                && started.elapsed() >= flag_after
                && !flag.get()
            {
                flag.set(true);
            }
            Some(ProviderEvent::Pending)
        })))
    }
}

/// One in-memory history whose persisted-interruption flag is externally
/// controlled by the fixture.
struct InterruptibleHistory {
    interrupt: Rc<Cell<bool>>,
    items: Rc<RefCell<Vec<Item>>>,
}

impl TurnHistory for InterruptibleHistory {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
        _tool_terminals: Vec<NewItem>,
    ) -> Result<(), HistoryError> {
        self.interrupt.set(true);
        Ok(())
    }

    fn interruption_requested(&self, _turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        Ok(self.interrupt.get())
    }

    fn prior_thread_items(
        &self,
        _trust: &TrustContext,
        _thread_id: ThreadId,
    ) -> Result<Vec<Item>, HistoryError> {
        Ok(Vec::new())
    }

    fn accept_initial(&mut self, command: &TurnCommand) -> Result<AcceptedTurn, HistoryError> {
        let input = Item::new(
            1,
            ItemPayload::UserMessage {
                content: command.input.clone(),
            },
        );
        self.items.borrow_mut().push(input.clone());
        Ok(AcceptedTurn::new(
            command.trust.tenant_id.clone(),
            ThreadId::new(),
            TurnId::new(),
            LeaseGeneration::initial(),
            input,
        ))
    }

    fn append(&mut self, _turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        let durable = Item::new(self.items.borrow().len() as u64 + 1, item.into_payload());
        self.items.borrow_mut().push(durable.clone());
        Ok(durable)
    }

    fn replay(&self, _tenant_id: &TenantId, _turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        Ok(self.items.borrow().clone())
    }
}

fn buffered_command() -> TurnCommand {
    TurnCommand::new(
        TrustContext::new(
            TenantId::new("tenant-a").expect("valid tenant"),
            "subject-a",
        )
        .expect("valid trust context"),
        None,
        "hello",
    )
    .expect("valid command")
}

fn buffered_deltas(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| match &item.payload {
            ItemPayload::AgentMessageDelta { content } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

/// PLB-3/AC-6: buffered coalesced text is durably flushed before an
/// authenticated interruption and before a dependency or disconnect
/// cancellation wins, and the timer flush at the 500-ms boundary still
/// precedes the winning terminal — each case ends with exactly one terminal
/// and no text after it.
#[test]
fn buffered_delta_interrupt_and_cancellation_arbitration() {
    // Authenticated interruption arrives with buffered text: the flush
    // precedes the Interrupted terminal.
    let interrupt = Rc::new(Cell::new(false));
    let items = Rc::new(RefCell::new(Vec::new()));
    let result = TurnRunner::new(
        BufferedIdleProvider {
            deltas: vec!["a".to_owned(), "b".to_owned()],
            flag: Rc::clone(&interrupt),
            idle_sleep: Duration::from_millis(5),
            flag_after: Some(Duration::ZERO),
            started: None,
        },
        InterruptibleHistory {
            interrupt: Rc::clone(&interrupt),
            items: Rc::clone(&items),
        },
    )
    .execute(buffered_command());

    assert_eq!(
        result
            .expect("the interrupted turn returns its durable replay")
            .status,
        TurnStatus::Interrupted
    );
    let durable = items.borrow();
    assert_eq!(
        buffered_deltas(&durable),
        vec!["ab".to_owned()],
        "the buffered text flushes as one coalesced delta before the terminal"
    );
    assert_eq!(durable.len(), 3);
    assert!(matches!(
        durable.last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Interrupted))
    ));

    // Dependency or disconnect cancellation arrives with buffered text: the
    // flush precedes the Cancelled terminal. The same runner signal serves
    // the disconnect path, whose end-to-end delivery AC-7 exercises.
    let cancel = Rc::new(Cell::new(false));
    let items = Rc::new(RefCell::new(Vec::new()));
    let result = TurnRunner::new(
        BufferedIdleProvider {
            deltas: vec!["a".to_owned(), "b".to_owned()],
            flag: Rc::clone(&cancel),
            idle_sleep: Duration::from_millis(5),
            flag_after: Some(Duration::ZERO),
            started: None,
        },
        InterruptibleHistory {
            interrupt: Rc::new(Cell::new(false)),
            items: Rc::clone(&items),
        },
    )
    .execute_with_observer_and_cancellation(buffered_command(), &mut |_| {}, &|| cancel.get());

    assert_eq!(
        result
            .expect("the cancelled turn returns its durable replay")
            .status,
        TurnStatus::Cancelled
    );
    let durable = items.borrow();
    assert_eq!(buffered_deltas(&durable), vec!["ab".to_owned()]);
    assert_eq!(durable.len(), 3);
    assert!(matches!(
        durable.last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Cancelled))
    ));

    // Timer race: both deltas buffer inside the latency window, the flush
    // becomes eligible at exactly 500 ms, and an interruption arriving after
    // that boundary still observes the flushed delta before its terminal.
    let interrupt = Rc::new(Cell::new(false));
    let items = Rc::new(RefCell::new(Vec::new()));
    let result = TurnRunner::new(
        BufferedIdleProvider {
            deltas: vec!["x1".to_owned(), "x2".to_owned()],
            flag: Rc::clone(&interrupt),
            idle_sleep: Duration::from_millis(25),
            flag_after: Some(Duration::from_millis(540)),
            started: None,
        },
        InterruptibleHistory {
            interrupt: Rc::clone(&interrupt),
            items: Rc::clone(&items),
        },
    )
    .execute(buffered_command());

    assert_eq!(
        result
            .expect("the timer-race interruption returns its durable replay")
            .status,
        TurnStatus::Interrupted
    );
    let durable = items.borrow();
    assert_eq!(
        buffered_deltas(&durable),
        vec!["x1x2".to_owned()],
        "the timer flush at 500 ms coalesces both buffered deltas"
    );
    assert_eq!(durable.len(), 3);
    assert!(matches!(
        durable.last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Interrupted))
    ));
}
