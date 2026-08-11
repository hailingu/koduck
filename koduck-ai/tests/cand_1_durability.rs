// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use koduck_ai::application::{
    AcceptedTurn, AppendPolicy, BufferLimitError, DurabilityFailure, HistoryError, ModelInput,
    ModelProvider, NewItem, ProviderError, ProviderEvent, ProviderStream, TurnCommand, TurnHistory,
    TurnRunError, TurnRunner, UnpublishedBuffer,
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
    let mut deadline_buffer = UnpublishedBuffer::new(policy);
    assert_eq!(
        deadline_buffer.observe_append_elapsed(Duration::from_millis(2_001)),
        Err(BufferLimitError::AppendDeadline)
    );
    assert!(deadline_buffer.is_stopped());

    let mut item_buffer = UnpublishedBuffer::new(policy);
    for _ in 0..64 {
        item_buffer
            .push(NewItem::AgentMessageDelta {
                content: "A".to_owned(),
            })
            .expect("first 64 items fit");
    }
    assert_eq!(
        item_buffer.push(NewItem::AgentMessageDelta {
            content: "B".to_owned(),
        }),
        Err(BufferLimitError::ItemCount)
    );
    assert!(item_buffer.is_stopped());

    let mut payload_buffer = UnpublishedBuffer::new(policy);
    assert_eq!(
        payload_buffer.push(NewItem::AgentMessageDelta {
            content: "x".repeat(1_048_577),
        }),
        Err(BufferLimitError::PayloadBytes)
    );
    assert!(payload_buffer.is_stopped());
    for error in [
        BufferLimitError::AppendDeadline,
        BufferLimitError::ItemCount,
        BufferLimitError::PayloadBytes,
    ] {
        assert_eq!(error.problem_code(), "durability-unavailable");
    }
    assert!(deadline_buffer.take_durable_prefix().is_empty());
    assert!(item_buffer.take_durable_prefix().is_empty());
    assert!(payload_buffer.take_durable_prefix().is_empty());
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
