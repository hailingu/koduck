// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use koduck_ai::application::{
    AcceptedTurn, AppendPolicy, BufferLimitError, DeltaCoalescer, DurabilityFailure, HistoryError,
    ModelInput, ModelProvider, NewItem, ProviderError, ProviderEvent, ProviderStream, TurnCommand,
    TurnHistory, TurnLiveness, TurnRunError, TurnRunner,
};
use koduck_ai::domain::{
    Item, ItemPayload, LeaseGeneration, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId,
    TurnStatus, Usage,
};

struct CountingProvider {
    calls: Rc<Cell<usize>>,
    consumed: Rc<Cell<usize>>,
}

impl ModelProvider for CountingProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        self.calls.set(self.calls.get() + 1);
        let consumed = Rc::clone(&self.consumed);
        Ok(Box::new(
            vec![
                ProviderEvent::Delta("A".to_owned()),
                ProviderEvent::Delta("B".to_owned()),
                ProviderEvent::Usage(Usage::new(1, 2).expect("valid usage")),
                ProviderEvent::Completed,
            ]
            .into_iter()
            .inspect(move |_| consumed.set(consumed.get() + 1)),
        ))
    }
}

struct FaultHistory {
    fail_initial: bool,
    fail_append_at: Option<usize>,
    accepted: Rc<Cell<usize>>,
    append_calls: usize,
    items: Rc<RefCell<Vec<Item>>>,
}

impl TurnHistory for FaultHistory {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
        _tool_terminals: Vec<koduck_ai::application::NewItem>,
    ) -> Result<(), HistoryError> {
        Err(HistoryError::NotFound)
    }

    fn interruption_requested(&self, _turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        Ok(false)
    }

    fn prior_thread_items(
        &self,
        _trust: &TrustContext,
        _thread_id: ThreadId,
    ) -> Result<Vec<Item>, HistoryError> {
        Ok(Vec::new())
    }

    fn accept_initial(&mut self, command: &TurnCommand) -> Result<AcceptedTurn, HistoryError> {
        if self.fail_initial {
            return Err(HistoryError::Unavailable);
        }
        self.accepted.set(self.accepted.get() + 1);
        let input = Item::new(
            1,
            ItemPayload::UserMessage {
                content: command.input.clone(),
            },
        );
        self.items.borrow_mut().push(input.clone());
        Ok(AcceptedTurn::new(
            command.trust.tenant_id.clone(),
            command.thread_id.unwrap_or_default(),
            TurnId::new(),
            LeaseGeneration::initial(),
            input,
        ))
    }

    fn append(&mut self, _turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        self.append_calls += 1;
        if self
            .fail_append_at
            .is_some_and(|first_failure| self.append_calls >= first_failure)
        {
            return Err(HistoryError::Unavailable);
        }
        let durable = Item::new(self.items.borrow().len() as u64 + 1, item.into_payload());
        self.items.borrow_mut().push(durable.clone());
        Ok(durable)
    }

    fn append_tool_projection(
        &mut self,
        turn: &AcceptedTurn,
        items: Vec<NewItem>,
    ) -> Result<Vec<Item>, HistoryError> {
        let mut durable = Vec::new();
        for item in items {
            durable.push(self.append(turn, item)?);
        }
        Ok(durable)
    }

    fn replay(&self, _tenant_id: &TenantId, _turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        Ok(self.items.borrow().clone())
    }
}

fn trust() -> TrustContext {
    TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context")
}

fn run(
    history: FaultHistory,
    calls: Rc<Cell<usize>>,
    consumed: Rc<Cell<usize>>,
) -> Result<koduck_ai::application::TurnResult, TurnRunError> {
    TurnRunner::new(CountingProvider { calls, consumed }, history)
        .execute(TurnCommand::new(trust(), None, "hello").expect("valid command"))
}

#[test]
fn initial_and_mid_turn_outages_fail_closed() {
    let initial_calls = Rc::new(Cell::new(0));
    let initial_accepted = Rc::new(Cell::new(0));
    let initial_items = Rc::new(RefCell::new(Vec::new()));
    let initial = run(
        FaultHistory {
            fail_initial: true,
            fail_append_at: None,
            accepted: Rc::clone(&initial_accepted),
            append_calls: 0,
            items: Rc::clone(&initial_items),
        },
        Rc::clone(&initial_calls),
        Rc::new(Cell::new(0)),
    );
    assert!(matches!(
        initial,
        Err(TurnRunError::Durability(DurabilityFailure {
            accepted: false,
            ..
        }))
    ));
    assert_eq!(initial_calls.get(), 0);
    assert_eq!(initial_accepted.get(), 0);
    assert!(initial_items.borrow().is_empty());

    let mid_calls = Rc::new(Cell::new(0));
    let mid_consumed = Rc::new(Cell::new(0));
    let mid_items = Rc::new(RefCell::new(Vec::new()));
    let mid = run(
        FaultHistory {
            fail_initial: false,
            fail_append_at: Some(2),
            accepted: Rc::new(Cell::new(0)),
            append_calls: 0,
            items: Rc::clone(&mid_items),
        },
        Rc::clone(&mid_calls),
        Rc::clone(&mid_consumed),
    );
    let Err(TurnRunError::Durability(failure)) = mid else {
        panic!("mid-turn outage must be typed durability failure");
    };
    assert!(failure.accepted);
    // Both fragments buffer first; the usage boundary triggers one coalesced
    // flush whose append succeeds, then the usage append fails, so exactly
    // the committed coalesced item is published and nothing else.
    assert_eq!(failure.published.len(), 1);
    assert!(matches!(
        &failure.published[0].payload,
        ItemPayload::AgentMessageDelta { content } if content == "AB"
    ));
    assert_eq!(mid_calls.get(), 1);
    assert_eq!(mid_consumed.get(), 3);
    assert_eq!(mid_items.borrow().len(), 2);
}

#[test]
fn append_deadline_and_buffer_caps() {
    let policy = AppendPolicy::cand_1();
    assert_eq!(
        policy.check_deadline(Duration::from_millis(2_001)),
        Err(BufferLimitError::AppendDeadline)
    );
    assert_eq!(policy.check_item_count(512), Ok(()));
    assert_eq!(
        policy.check_item_count(513),
        Err(BufferLimitError::ItemCount)
    );

    assert_eq!(
        policy.check_item(&NewItem::AgentMessageDelta {
            content: "x".repeat(1_048_577),
        }),
        Err(BufferLimitError::PayloadBytes)
    );
    assert_eq!(
        policy.check_item(&NewItem::AgentMessageDelta {
            content: "\"".repeat(600_000),
        }),
        Err(BufferLimitError::PayloadBytes),
        "JSON escaping and payload object overhead count toward the serialized cap"
    );
    assert_eq!(
        BufferLimitError::AppendDeadline.problem_code(),
        "durability-unavailable"
    );
    for error in [BufferLimitError::ItemCount, BufferLimitError::PayloadBytes] {
        assert_eq!(error.problem_code(), "resource-limit-exceeded");
    }
}

struct OversizedProvider {
    consumed: Rc<Cell<usize>>,
}

impl ModelProvider for OversizedProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        let consumed = Rc::clone(&self.consumed);
        Ok(Box::new(
            vec![
                ProviderEvent::Delta("x".repeat(1_048_577)),
                ProviderEvent::Completed,
            ]
            .into_iter()
            .inspect(move |_| consumed.set(consumed.get() + 1)),
        ))
    }
}

#[test]
fn execution_rejects_an_oversized_provider_delta_before_append() {
    let items = Rc::new(RefCell::new(Vec::new()));
    let consumed = Rc::new(Cell::new(0));
    let result = TurnRunner::new(
        OversizedProvider {
            consumed: Rc::clone(&consumed),
        },
        FaultHistory {
            fail_initial: false,
            fail_append_at: None,
            accepted: Rc::new(Cell::new(0)),
            append_calls: 0,
            items: Rc::clone(&items),
        },
    )
    .execute(TurnCommand::new(trust(), None, "hello").expect("valid command"));

    // The fragment splits at UTF-8 boundaries into 64 capped chunks plus a
    // one-byte remainder; the 64th chunk crosses the exact 1-MiB payload cap
    // and is rejected before append or publication.
    let Err(TurnRunError::ResourceLimit(failure)) = result else {
        panic!("an over-cap cumulative payload must fail as a resource limit");
    };
    assert_eq!(
        failure
            .published
            .iter()
            .filter(|item| matches!(item.payload, ItemPayload::AgentMessageDelta { .. }))
            .count(),
        63
    );
    assert_eq!(consumed.get(), 1);
    let durable = items.borrow();
    assert_eq!(durable.len(), 1 + 63 + 1);
    assert!(matches!(
        durable.last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Failed { code }))
            if code == "RESOURCE_LIMIT_EXCEEDED"
    ));
}

struct CumulativePayloadProvider {
    consumed: Rc<Cell<usize>>,
}

impl ModelProvider for CumulativePayloadProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        let consumed = Rc::clone(&self.consumed);
        Ok(Box::new(
            vec![
                ProviderEvent::Delta("x".repeat(600_000)),
                ProviderEvent::Delta("y".repeat(600_000)),
                ProviderEvent::Completed,
            ]
            .into_iter()
            .inspect(move |_| consumed.set(consumed.get() + 1)),
        ))
    }
}

#[test]
fn execution_rejects_cumulative_provider_payload_over_one_mib_before_append() {
    let items = Rc::new(RefCell::new(Vec::new()));
    let consumed = Rc::new(Cell::new(0));
    let result = TurnRunner::new(
        CumulativePayloadProvider {
            consumed: Rc::clone(&consumed),
        },
        FaultHistory {
            fail_initial: false,
            fail_append_at: None,
            accepted: Rc::new(Cell::new(0)),
            append_calls: 0,
            items: Rc::clone(&items),
        },
    )
    .execute(TurnCommand::new(trust(), None, "hello").expect("valid command"));

    let Err(TurnRunError::ResourceLimit(failure)) = result else {
        panic!("cumulative payload over one MiB must fail as a resource limit");
    };
    // 36 chunks of the first fragment, its retained remainder, and 27 chunks
    // of the second fragment publish before the 28th crosses the exact cap.
    assert_eq!(
        failure
            .published
            .iter()
            .filter(|item| matches!(item.payload, ItemPayload::AgentMessageDelta { .. }))
            .count(),
        64
    );
    assert_eq!(consumed.get(), 2);
    let durable = items.borrow();
    assert_eq!(durable.len(), 1 + 64 + 1);
    assert!(matches!(
        durable.last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Failed { code }))
            if code == "RESOURCE_LIMIT_EXCEEDED"
    ));
}

/// 255 denied Tool calls plus one flushed delta occupy 511 counted slots,
/// so the usage nonterminal is the item that would starve the reserved
/// terminal slot at the 512 boundary.
struct ReserveBoundaryProvider {
    scripts: Vec<Vec<ProviderEvent>>,
    taken: usize,
}

impl ModelProvider for ReserveBoundaryProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        let script = self.scripts.get(self.taken).cloned().unwrap_or_default();
        self.taken += 1;
        Ok(Box::new(script.into_iter()))
    }
}

#[test]
fn execution_reserves_item_512_for_the_mandatory_terminal() {
    let (history, items) = fixture_history();
    let result = TurnRunner::new(
        ReserveBoundaryProvider {
            scripts: vec![
                (0..255).map(|_| denied_tool_call()).collect(),
                vec![
                    ProviderEvent::Delta("x".repeat(16_384)),
                    ProviderEvent::Usage(Usage::new(1, 2).expect("valid usage")),
                    ProviderEvent::Completed,
                ],
            ],
            taken: 0,
        },
        history,
    )
    .execute(TurnCommand::new(trust(), None, "hello").expect("valid command"));

    let Err(TurnRunError::ResourceLimit(failure)) = result else {
        panic!("the nonterminal item consuming the terminal reserve must fail closed");
    };
    // 510 projection items, the flushed delta, and the durable terminal
    // publish; the usage item that would starve the terminal reserve is
    // rejected before append.
    assert_eq!(failure.published.len(), 512);
    let durable = items.borrow();
    assert_eq!(durable.len(), 1 + 512);
    assert!(matches!(
        durable.last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Failed { code }))
            if code == "RESOURCE_LIMIT_EXCEEDED"
    ));
}

struct RecoverableHistory {
    attempts: Rc<Cell<usize>>,
    items: Rc<RefCell<Vec<Item>>>,
    recovery_error: Option<HistoryError>,
}

impl TurnHistory for RecoverableHistory {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
        _tool_terminals: Vec<koduck_ai::application::NewItem>,
    ) -> Result<(), HistoryError> {
        Err(HistoryError::NotFound)
    }

    fn interruption_requested(&self, _turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        Ok(false)
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
        let attempt = self.attempts.get() + 1;
        self.attempts.set(attempt);
        if attempt == 1 {
            return Err(HistoryError::Unavailable);
        }
        let durable = Item::new(self.items.borrow().len() as u64 + 1, item.into_payload());
        self.items.borrow_mut().push(durable.clone());
        Ok(durable)
    }

    fn replay(&self, _tenant_id: &TenantId, _turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        Ok(self.items.borrow().clone())
    }

    fn schedule_failed_recovery(&mut self, turn: &AcceptedTurn) -> Result<(), HistoryError> {
        if let Some(error) = self.recovery_error.clone() {
            return Err(error);
        }
        self.append(
            turn,
            NewItem::Terminal(TerminalOutcome::Failed {
                code: "DURABILITY_UNAVAILABLE".to_owned(),
            }),
        )?;
        Ok(())
    }
}

#[test]
fn accepted_append_outage_schedules_a_failed_terminal_recovery() {
    let attempts = Rc::new(Cell::new(0));
    let items = Rc::new(RefCell::new(Vec::new()));
    let result = TurnRunner::new(
        CountingProvider {
            calls: Rc::new(Cell::new(0)),
            consumed: Rc::new(Cell::new(0)),
        },
        RecoverableHistory {
            attempts: Rc::clone(&attempts),
            items: Rc::clone(&items),
            recovery_error: None,
        },
    )
    .execute(TurnCommand::new(trust(), None, "hello").expect("valid command"));

    assert!(matches!(result, Err(TurnRunError::Durability(_))));
    assert_eq!(attempts.get(), 2);
    assert!(matches!(
        items.borrow().last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Failed { code }))
            if code == "DURABILITY_UNAVAILABLE"
    ));
}

struct DropObservedLiveness {
    dropped: std::sync::Arc<std::sync::atomic::AtomicBool>,
    admission_released: std::sync::Arc<std::sync::atomic::AtomicBool>,
    recovery_scheduled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    items: std::sync::Arc<std::sync::Mutex<Vec<Item>>>,
}

impl TurnLiveness for DropObservedLiveness {
    fn handoff_to_recovery(
        self: Box<Self>,
    ) -> Result<koduck_ai::application::RecoveryHandoff, HistoryError> {
        self.admission_released
            .store(true, std::sync::atomic::Ordering::Release);
        self.recovery_scheduled
            .store(true, std::sync::atomic::Ordering::Release);
        self.items
            .lock()
            .expect("handoff history lock")
            .push(Item::new(
                2,
                ItemPayload::Terminal(TerminalOutcome::Failed {
                    code: "DURABILITY_UNAVAILABLE".to_owned(),
                }),
            ));
        Ok(koduck_ai::application::RecoveryHandoff::Recovered)
    }
}

impl Drop for DropObservedLiveness {
    fn drop(&mut self) {
        self.dropped
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

struct HandoffHistory {
    liveness_dropped: std::sync::Arc<std::sync::atomic::AtomicBool>,
    admission_released: std::sync::Arc<std::sync::atomic::AtomicBool>,
    recovery_scheduled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    control_unavailable: bool,
    items: std::sync::Arc<std::sync::Mutex<Vec<Item>>>,
}

impl TurnHistory for HandoffHistory {
    fn start_turn_liveness(
        &self,
        _turn: &AcceptedTurn,
    ) -> Result<Box<dyn TurnLiveness>, HistoryError> {
        Ok(Box::new(DropObservedLiveness {
            dropped: std::sync::Arc::clone(&self.liveness_dropped),
            admission_released: std::sync::Arc::clone(&self.admission_released),
            recovery_scheduled: std::sync::Arc::clone(&self.recovery_scheduled),
            items: std::sync::Arc::clone(&self.items),
        }))
    }

    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
        _tool_terminals: Vec<koduck_ai::application::NewItem>,
    ) -> Result<(), HistoryError> {
        Err(HistoryError::NotFound)
    }

    fn interruption_requested(&self, _turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        if self.control_unavailable {
            Err(HistoryError::Unavailable)
        } else {
            Ok(false)
        }
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
        self.items
            .lock()
            .expect("handoff history lock")
            .push(input.clone());
        Ok(AcceptedTurn::new(
            command.trust.tenant_id.clone(),
            ThreadId::new(),
            TurnId::new(),
            LeaseGeneration::initial(),
            input,
        ))
    }

    fn append(&mut self, _turn: &AcceptedTurn, _item: NewItem) -> Result<Item, HistoryError> {
        Err(HistoryError::Unavailable)
    }

    fn schedule_failed_recovery(&mut self, _turn: &AcceptedTurn) -> Result<(), HistoryError> {
        panic!("atomic liveness handoff must not reacquire history admission")
    }

    fn replay(&self, _tenant_id: &TenantId, _turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        Ok(self.items.lock().expect("handoff history lock").clone())
    }
}

#[test]
fn append_outage_confirms_admission_handoff_before_recovery() {
    let liveness_dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let admission_released = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let recovery_scheduled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut observed = Vec::new();
    let result = TurnRunner::new(
        CountingProvider {
            calls: Rc::new(Cell::new(0)),
            consumed: Rc::new(Cell::new(0)),
        },
        HandoffHistory {
            liveness_dropped: std::sync::Arc::clone(&liveness_dropped),
            admission_released: std::sync::Arc::clone(&admission_released),
            recovery_scheduled: std::sync::Arc::clone(&recovery_scheduled),
            control_unavailable: false,
            items: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        },
    )
    .execute_with_observer(
        TurnCommand::new(trust(), None, "hello").expect("valid command"),
        &mut |event| observed.push(event),
    );

    assert!(matches!(result, Err(TurnRunError::Durability(_))));
    assert!(liveness_dropped.load(std::sync::atomic::Ordering::Acquire));
    assert!(
        recovery_scheduled.load(std::sync::atomic::Ordering::Acquire),
        "recovery must retain the renewal reservation through scheduling"
    );
    assert!(matches!(
        observed.last(),
        Some(koduck_ai::application::TurnStreamEvent::Item {
            item: Item {
                payload: ItemPayload::Terminal(TerminalOutcome::Failed { .. }),
                ..
            },
            ..
        })
    ));
}

#[test]
fn control_read_outage_enters_failed_recovery_handoff() {
    let recovery_scheduled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let result = TurnRunner::new(
        CountingProvider {
            calls: Rc::new(Cell::new(0)),
            consumed: Rc::new(Cell::new(0)),
        },
        HandoffHistory {
            liveness_dropped: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            admission_released: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            recovery_scheduled: std::sync::Arc::clone(&recovery_scheduled),
            control_unavailable: true,
            items: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        },
    )
    .execute(TurnCommand::new(trust(), None, "hello").expect("valid command"));

    assert!(matches!(result, Err(TurnRunError::Durability(_))));
    assert!(
        recovery_scheduled.load(std::sync::atomic::Ordering::Acquire),
        "an accepted control-read outage must retain recovery ownership"
    );
}

#[test]
fn accepted_append_outage_propagates_non_unavailable_recovery_error() {
    let attempts = Rc::new(Cell::new(0));
    let result = TurnRunner::new(
        CountingProvider {
            calls: Rc::new(Cell::new(0)),
            consumed: Rc::new(Cell::new(0)),
        },
        RecoverableHistory {
            attempts: Rc::clone(&attempts),
            items: Rc::new(RefCell::new(Vec::new())),
            recovery_error: Some(HistoryError::NotFound),
        },
    )
    .execute(TurnCommand::new(trust(), None, "hello").expect("valid command"));

    assert!(matches!(
        result,
        Err(TurnRunError::History(HistoryError::NotFound))
    ));
    assert_eq!(attempts.get(), 1);
}

/// One provider whose successive streams replay one scripted event list each,
/// so budget fixtures can drive Tool-call rounds plus a completing stream.
struct ScriptedProvider {
    scripts: Vec<Vec<ProviderEvent>>,
    taken: usize,
}

fn denied_tool_call() -> ProviderEvent {
    ProviderEvent::ToolCall {
        name: "fixture.tool".to_owned(),
        arguments: "{}".to_owned(),
    }
}

impl ModelProvider for ScriptedProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        let script = self.scripts.get(self.taken).cloned().unwrap_or_default();
        self.taken += 1;
        Ok(Box::new(script.into_iter()))
    }
}

fn fixture_history() -> (FaultHistory, Rc<RefCell<Vec<Item>>>) {
    let items = Rc::new(RefCell::new(Vec::new()));
    (
        FaultHistory {
            fail_initial: false,
            fail_append_at: None,
            accepted: Rc::new(Cell::new(0)),
            append_calls: 0,
            items: Rc::clone(&items),
        },
        items,
    )
}

fn delta_contents(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| match &item.payload {
            ItemPayload::AgentMessageDelta { content } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

/// PLB-2/AC-2: the coalescer flushes at exactly 16,384 buffered UTF-8 bytes
/// or exactly 500 ms after the first buffered byte, splits an oversized
/// fragment only at UTF-8 scalar boundaries into the minimum ordered chunks,
/// and never emits an empty or oversized chunk.
#[test]
fn delta_coalescer_flushes_at_exact_byte_and_latency_boundaries() {
    let base = Instant::now();

    // ASCII byte cap: the 16,384th byte stays buffered, and the byte that
    // would cross the cap flushes the complete first chunk.
    let mut ascii = DeltaCoalescer::empty();
    assert!(ascii.push(&"a".repeat(16_384), base).is_empty());
    assert_eq!(ascii.push("b", base), vec!["a".repeat(16_384)]);
    assert_eq!(ascii.take_forced_flush().as_deref(), Some("b"));
    assert_eq!(ascii.take_forced_flush(), None);

    // One fragment one byte over the cap splits into one 16,384-byte chunk
    // plus the one-byte retained remainder.
    let mut oversized = DeltaCoalescer::empty();
    assert_eq!(
        oversized.push(&"c".repeat(16_385), base),
        vec!["c".repeat(16_384)]
    );
    assert_eq!(oversized.take_forced_flush().as_deref(), Some("c"));

    // 16,384 is not a multiple of the three-byte euro sign, so the split
    // backs off to the enclosing scalar boundary: a 16,383-byte chunk and a
    // three-byte remainder, both valid UTF-8, concatenating exactly.
    let mut multibyte = DeltaCoalescer::empty();
    let emitted = multibyte.push(&"€".repeat(5_462), base);
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0], "€".repeat(5_461));
    assert_eq!(multibyte.take_forced_flush().as_deref(), Some("€"));

    // Latency cap: no timer flush at 499 ms, exactly one eligible flush at
    // 500 ms, and the timer restarts from the next buffered byte.
    let mut timed = DeltaCoalescer::empty();
    assert!(timed.push("x", base).is_empty());
    assert_eq!(
        timed.take_due_flush(base + Duration::from_millis(499)),
        None
    );
    assert_eq!(
        timed.take_due_flush(base + Duration::from_millis(500)),
        Some("x".to_owned())
    );
    assert_eq!(
        timed.take_due_flush(base + Duration::from_millis(600)),
        None
    );
    assert!(
        timed
            .push("y", base + Duration::from_millis(600))
            .is_empty()
    );
    assert_eq!(
        timed.take_due_flush(base + Duration::from_millis(1_099)),
        None
    );
    assert_eq!(
        timed.take_due_flush(base + Duration::from_millis(1_100)),
        Some("y".to_owned())
    );
}

/// PLB-1/PLB-9/AC-1: 640 ordered one-byte provider deltas — including a
/// literal `<think>` tag sequence — arriving inside the latency window are
/// coalesced before the durable append, complete with exact concatenated
/// content, and never surface a durability failure.
#[test]
fn raw_provider_fragments_are_coalesced_before_durable_append() {
    let mut content = String::from("<think>reasoning</think>");
    content.push_str(&".".repeat(640 - content.len()));
    let (history, items) = fixture_history();
    let result = TurnRunner::new(
        ScriptedProvider {
            scripts: vec![
                content
                    .chars()
                    .map(|character| ProviderEvent::Delta(character.to_string()))
                    .chain(std::iter::once(ProviderEvent::Usage(
                        Usage::new(2, 4).expect("valid usage"),
                    )))
                    .chain(std::iter::once(ProviderEvent::Completed))
                    .collect(),
            ],
            taken: 0,
        },
        history,
    )
    .execute(TurnCommand::new(trust(), None, "hello").expect("valid command"));

    let Ok(result) = result else {
        panic!(
            "a high-fragment valid response must complete instead of exhausting the Turn budget"
        );
    };
    assert_eq!(result.status, TurnStatus::Completed);
    let durable = items.borrow();
    let deltas = delta_contents(&durable);
    assert_eq!(
        deltas,
        vec![content.clone()],
        "sub-cap, sub-latency completion flushes exactly one coalesced agent delta"
    );
    assert_eq!(
        result
            .published
            .iter()
            .filter(|item| matches!(item.payload, ItemPayload::AgentMessageDelta { .. }))
            .count(),
        1
    );
    assert_eq!(
        durable.len(),
        4,
        "user message plus one delta, one usage, and the terminal are durable"
    );
    assert!(durable.iter().all(|item| !matches!(
        &item.payload,
        ItemPayload::Terminal(TerminalOutcome::Failed { code })
            if code == "DURABILITY_UNAVAILABLE"
    )));
}

/// PLB-5/PLB-6/AC-4: the shared post-acceptance budget accepts exactly 512
/// counted Items with one terminal slot reserved, rejects the nonterminal
/// that would starve the terminal and Item 513 before append or publication,
/// and keeps the 1-MiB serialized payload cap exact and independent.
/// Builds the exact-count fixture: 254 denied Tool calls contribute 508
/// counted projection items and the legal tail completes the 512 counted
/// Items.
fn budget_count_fixture() -> Vec<Vec<ProviderEvent>> {
    vec![
        (0..254).map(|_| denied_tool_call()).collect(),
        vec![
            ProviderEvent::Delta("x".repeat(16_384)),
            ProviderEvent::Delta("y".repeat(16_384)),
            ProviderEvent::Usage(Usage::new(2, 4).expect("valid usage")),
            ProviderEvent::Completed,
        ],
    ]
}

#[test]
fn turn_budget_accepts_512_and_rejects_513() {
    let policy = AppendPolicy::cand_1();
    assert_eq!(policy.check_item_count(512), Ok(()));
    assert_eq!(
        policy.check_item_count(513),
        Err(BufferLimitError::ItemCount)
    );

    // 254 denied Tool calls contribute 508 counted projection items; the two
    // byte-cap deltas, the usage item, and the terminal reach exactly 512.
    let legal_stream = budget_count_fixture()[1].clone();
    let (history, items) = fixture_history();
    let result = TurnRunner::new(
        ScriptedProvider {
            scripts: vec![(0..254).map(|_| denied_tool_call()).collect(), legal_stream],
            taken: 0,
        },
        history,
    )
    .execute(TurnCommand::new(trust(), None, "hello").expect("valid command"));

    let Ok(result) = result else {
        panic!("the exact 512-Item budget must admit the legal maximum Turn");
    };
    assert_eq!(result.status, TurnStatus::Completed);
    assert_eq!(items.borrow().len(), 1 + 512);
    assert_eq!(
        delta_contents(&items.borrow()),
        vec!["x".repeat(16_384), "y".repeat(16_384)]
    );

    // One additional denied call leaves 510 projection items, so the second
    // byte-cap delta is the nonterminal that starves the terminal reserve:
    // it is rejected before append or publication and the Turn durably
    // closes as RESOURCE_LIMIT_EXCEEDED.
    let (history, items) = fixture_history();
    let mut starve_scripts = budget_count_fixture();
    starve_scripts[0].push(denied_tool_call());
    let result = TurnRunner::new(
        ScriptedProvider {
            scripts: starve_scripts,
            taken: 0,
        },
        history,
    )
    .execute(TurnCommand::new(trust(), None, "hello").expect("valid command"));

    let Err(TurnRunError::ResourceLimit(failure)) = result else {
        panic!("the terminal-starving nonterminal must fail closed as a resource limit");
    };
    assert_eq!(
        failure.published.len(),
        512,
        "the terminal is the counted Item 512; the starving delta itself is never published"
    );
    let starved_content = "y".repeat(16_384);
    assert!(!failure.published.iter().any(|item| matches!(
        &item.payload,
        ItemPayload::AgentMessageDelta { content } if content.as_str() == starved_content.as_str()
    )));
    let durable = items.borrow();
    assert_eq!(durable.len(), 1 + 512);
    let first_content = "x".repeat(16_384);
    assert_eq!(
        delta_contents(&durable).last(),
        Some(&first_content),
        "the starved second delta never became durable"
    );
    assert!(matches!(
        durable.last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Failed { code }))
            if code == "RESOURCE_LIMIT_EXCEEDED"
    ));

    // PLB-6: the 1-MiB payload cap stays exact and independent of the count
    // headroom — an exact-byte cumulative payload completes and one more
    // content byte is rejected with the same resource diagnostics.
    run_payload_boundary();
}

/// Drives the exact-byte and one-byte-over serialized payload cases against
/// the raised count headroom (ADR-0005 PLB-6).
fn run_payload_boundary() {
    // PLB-6: the 1-MiB payload cap stays exact and independent of the count
    // headroom — an exact-byte cumulative payload completes and one more
    // content byte is rejected with the same resource diagnostics.
    let policy = AppendPolicy::cand_1();
    let usage = Usage::new(2, 4).expect("valid usage");
    let usage_item = NewItem::Usage(usage);
    let terminal_item = NewItem::Terminal(TerminalOutcome::Completed { usage });
    let delta_overhead = policy
        .accumulate_payload_bytes(
            0,
            &NewItem::AgentMessageDelta {
                content: String::new(),
            },
        )
        .expect("the empty delta provides its object overhead");
    let terminal_and_usage_bytes = policy
        .accumulate_payload_bytes(0, &usage_item)
        .and_then(|total| policy.accumulate_payload_bytes(total, &terminal_item))
        .expect("the fixed tail is independently bounded");
    // 63 fragments at the 16,384-byte coalescing cap plus one sized remainder
    // fill the remaining allowance to the exact byte across 64 delta items.
    let content_total = 1_048_576 - terminal_and_usage_bytes - 64 * delta_overhead;
    let remainder = content_total - 63 * 16_384;
    let payload_stream = |extra: usize| {
        let mut events: Vec<ProviderEvent> = (0..63)
            .map(|_| ProviderEvent::Delta("x".repeat(16_384)))
            .collect();
        events.push(ProviderEvent::Delta("x".repeat(remainder + extra)));
        events.push(ProviderEvent::Usage(usage));
        events.push(ProviderEvent::Completed);
        events
    };
    let (history, items) = fixture_history();
    let result = TurnRunner::new(
        ScriptedProvider {
            scripts: vec![payload_stream(0)],
            taken: 0,
        },
        history,
    )
    .execute(TurnCommand::new(trust(), None, "hello").expect("valid command"));
    let Ok(result) = result else {
        panic!("the exact 1,048,576-byte payload must complete");
    };
    assert_eq!(result.status, TurnStatus::Completed);
    assert_eq!(delta_contents(&items.borrow()).len(), 64);

    // One content byte over the cap keeps every legal delta and usage item
    // but rejects the over-limit terminal: no over-limit Item is published
    // and the Turn durably closes as RESOURCE_LIMIT_EXCEEDED.
    let (history, items) = fixture_history();
    let result = TurnRunner::new(
        ScriptedProvider {
            scripts: vec![payload_stream(1)],
            taken: 0,
        },
        history,
    )
    .execute(TurnCommand::new(trust(), None, "hello").expect("valid command"));
    let Err(TurnRunError::ResourceLimit(failure)) = result else {
        panic!("one payload byte over the exact cap must fail closed as a resource limit");
    };
    assert_eq!(
        failure
            .published
            .iter()
            .filter(|item| matches!(item.payload, ItemPayload::AgentMessageDelta { .. }))
            .count(),
        64
    );
    let durable = items.borrow();
    assert_eq!(durable.len(), 1 + 64 + 1 + 1);
    assert!(matches!(
        durable.last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Failed { code }))
            if code == "RESOURCE_LIMIT_EXCEEDED"
    ));
}
