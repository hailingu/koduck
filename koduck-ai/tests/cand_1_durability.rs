// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use koduck_ai::application::{
    AcceptedTurn, AppendPolicy, BufferLimitError, DurabilityFailure, HistoryError, ModelInput,
    ModelProvider, NewItem, ProviderError, ProviderEvent, ProviderStream, TurnCommand, TurnHistory,
    TurnLiveness, TurnRunError, TurnRunner,
};
use koduck_ai::domain::{
    Item, ItemPayload, LeaseGeneration, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId,
    Usage,
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
    assert_eq!(failure.published.len(), 1);
    assert!(matches!(
        &failure.published[0].payload,
        ItemPayload::AgentMessageDelta { content } if content == "A"
    ));
    assert_eq!(mid_calls.get(), 1);
    assert_eq!(mid_consumed.get(), 2);
    assert_eq!(mid_items.borrow().len(), 2);
}

#[test]
fn append_deadline_and_buffer_caps() {
    let policy = AppendPolicy::cand_1();
    assert_eq!(
        policy.check_deadline(Duration::from_millis(2_001)),
        Err(BufferLimitError::AppendDeadline)
    );
    assert_eq!(
        policy.check_item_count(65),
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
    for error in [
        BufferLimitError::AppendDeadline,
        BufferLimitError::ItemCount,
        BufferLimitError::PayloadBytes,
    ] {
        assert_eq!(error.problem_code(), "durability-unavailable");
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

    let Err(TurnRunError::Durability(failure)) = result else {
        panic!("oversized delta must fail as a durability boundary violation");
    };
    assert!(failure.accepted);
    assert!(failure.published.is_empty());
    assert_eq!(consumed.get(), 1);
    assert_eq!(items.borrow().len(), 2);
    assert!(matches!(
        items.borrow().last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Failed { code }))
            if code == "DURABILITY_UNAVAILABLE"
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

    let Err(TurnRunError::Durability(failure)) = result else {
        panic!("cumulative payload over one MiB must fail at the durability boundary");
    };
    assert!(failure.accepted);
    assert_eq!(failure.published.len(), 1);
    assert_eq!(consumed.get(), 2);
    assert_eq!(
        items
            .borrow()
            .iter()
            .filter(|item| matches!(item.payload, ItemPayload::AgentMessageDelta { .. }))
            .count(),
        1
    );
    assert!(matches!(
        items.borrow().last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Failed { code }))
            if code == "DURABILITY_UNAVAILABLE"
    ));
}

struct ExcessItemProvider {
    consumed: Rc<Cell<usize>>,
}

impl ModelProvider for ExcessItemProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        let consumed = Rc::clone(&self.consumed);
        let mut events = (0..65)
            .map(|_| ProviderEvent::Delta("A".to_owned()))
            .collect::<Vec<_>>();
        events.push(ProviderEvent::Completed);
        Ok(Box::new(
            events
                .into_iter()
                .inspect(move |_| consumed.set(consumed.get() + 1)),
        ))
    }
}

#[test]
fn execution_rejects_item_65_before_append() {
    let items = Rc::new(RefCell::new(Vec::new()));
    let consumed = Rc::new(Cell::new(0));
    let result = TurnRunner::new(
        ExcessItemProvider {
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

    let Err(TurnRunError::Durability(failure)) = result else {
        panic!("item 65 must fail as a durability boundary violation");
    };
    assert!(failure.accepted);
    assert_eq!(failure.published.len(), 64);
    assert_eq!(consumed.get(), 65);
    assert_eq!(
        items
            .borrow()
            .iter()
            .filter(|item| matches!(item.payload, ItemPayload::AgentMessageDelta { .. }))
            .count(),
        64
    );
    assert!(matches!(
        items.borrow().last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Failed { code }))
            if code == "DURABILITY_UNAVAILABLE"
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
