// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use std::sync::{Arc, Mutex};

use koduck_ai::application::{
    AcceptedTurn, HistoryError, ModelInput, ModelProvider, NewItem, ProviderError, ProviderEvent,
    ProviderStream, TurnCommand, TurnHistory, TurnLiveness, TurnRunError, TurnRunner,
    TurnStreamEvent,
};
use koduck_ai::domain::{
    Item, ItemPayload, LeaseGeneration, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId,
    TurnStatus, Usage,
};

struct CompletingProvider;

impl ModelProvider for CompletingProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        Ok(Box::new([ProviderEvent::Completed].into_iter()))
    }
}

struct FailingProvider;

impl ModelProvider for FailingProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        Ok(Box::new(
            [ProviderEvent::Error {
                code: "UPSTREAM_RESET".to_owned(),
            }]
            .into_iter(),
        ))
    }
}

struct SetupFailingProvider;

impl ModelProvider for SetupFailingProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        Err(ProviderError {
            code: "UPSTREAM_UNAVAILABLE".to_owned(),
        })
    }
}

struct EmptyProvider;

impl ModelProvider for EmptyProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        Ok(Box::new(std::iter::empty()))
    }
}

struct OversizedDeltaProvider;

impl ModelProvider for OversizedDeltaProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        Ok(Box::new(
            [ProviderEvent::Delta("x".repeat(1_048_576))].into_iter(),
        ))
    }
}

#[derive(Default)]
struct InterruptedHistory {
    items: Vec<Item>,
}

impl TurnHistory for InterruptedHistory {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
    ) -> Result<(), HistoryError> {
        Ok(())
    }

    fn interruption_requested(&self, _turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        Ok(true)
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
        self.items.push(input.clone());
        Ok(AcceptedTurn::new(
            command.trust.tenant_id.clone(),
            ThreadId::new(),
            TurnId::new(),
            LeaseGeneration::initial(),
            input,
        ))
    }

    fn append(&mut self, _turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        let durable = Item::new(self.items.len() as u64 + 1, item.into_payload());
        self.items.push(durable.clone());
        Ok(durable)
    }

    fn replay(&self, _tenant_id: &TenantId, _turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        Ok(self.items.clone())
    }
}

#[test]
fn accepted_interrupt_wins_over_provider_completion() {
    let trust = TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context");
    let result = TurnRunner::new(CompletingProvider, InterruptedHistory::default())
        .execute(TurnCommand::new(trust, None, "hello").expect("valid command"))
        .expect("accepted interrupt terminalizes normally");

    assert_eq!(result.status, TurnStatus::Interrupted);
    assert!(matches!(
        result.replay.last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Interrupted))
    ));
}

#[test]
fn accepted_interrupt_wins_over_provider_failure() {
    let trust = TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context");
    let result = TurnRunner::new(FailingProvider, InterruptedHistory::default())
        .execute(TurnCommand::new(trust, None, "hello").expect("valid command"))
        .expect("accepted interrupt terminalizes normally");

    assert_eq!(result.status, TurnStatus::Interrupted);
    assert!(matches!(
        result.replay.last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Interrupted))
    ));
}

#[test]
fn accepted_interrupt_wins_before_provider_payload_validation() {
    let trust = TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context");
    let result = TurnRunner::new(OversizedDeltaProvider, InterruptedHistory::default())
        .execute(TurnCommand::new(trust, None, "hello").expect("valid command"))
        .expect("accepted interrupt bypasses the oversized provider delta");

    assert_eq!(result.status, TurnStatus::Interrupted);
    assert!(matches!(
        result.replay.last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Interrupted))
    ));
}

#[derive(Default)]
struct LimitRaceHistory {
    items: Vec<Item>,
}

impl TurnHistory for LimitRaceHistory {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
    ) -> Result<(), HistoryError> {
        Ok(())
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
        self.items.push(input.clone());
        Ok(AcceptedTurn::new(
            command.trust.tenant_id.clone(),
            ThreadId::new(),
            TurnId::new(),
            LeaseGeneration::initial(),
            input,
        ))
    }

    fn append(&mut self, _turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        let durable = Item::new(self.items.len() as u64 + 1, item.into_payload());
        self.items.push(durable.clone());
        Ok(durable)
    }

    fn append_provider_terminal(
        &mut self,
        _turn: &AcceptedTurn,
        _outcome: TerminalOutcome,
    ) -> Result<Item, HistoryError> {
        let durable = Item::new(
            self.items.len() as u64 + 1,
            ItemPayload::Terminal(TerminalOutcome::Interrupted),
        );
        self.items.push(durable.clone());
        Ok(durable)
    }

    fn replay(&self, _tenant_id: &TenantId, _turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        Ok(self.items.clone())
    }
}

#[test]
fn accepted_interrupt_wins_when_payload_validation_races_with_the_request() {
    let trust = TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context");
    let result = TurnRunner::new(OversizedDeltaProvider, LimitRaceHistory::default())
        .execute(TurnCommand::new(trust, None, "hello").expect("valid command"))
        .expect("atomic terminal arbitration preserves the accepted interrupt");

    assert_eq!(result.status, TurnStatus::Interrupted);
    assert!(matches!(
        result.replay.last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Interrupted))
    ));
}

struct OutputAfterInterruptProvider;

impl ModelProvider for OutputAfterInterruptProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        Ok(Box::new(
            [
                ProviderEvent::Delta("late".to_owned()),
                ProviderEvent::Usage(Usage::new(1, 1).expect("valid usage")),
                ProviderEvent::Completed,
            ]
            .into_iter(),
        ))
    }
}

#[derive(Default)]
struct InterruptArbitratingHistory {
    items: Vec<Item>,
    terminal: bool,
}

impl TurnHistory for InterruptArbitratingHistory {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
    ) -> Result<(), HistoryError> {
        Ok(())
    }

    fn interruption_requested(&self, _turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        Ok(true)
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
        self.items.push(input.clone());
        Ok(AcceptedTurn::new(
            command.trust.tenant_id.clone(),
            ThreadId::new(),
            TurnId::new(),
            LeaseGeneration::initial(),
            input,
        ))
    }

    fn append(&mut self, _turn: &AcceptedTurn, _item: NewItem) -> Result<Item, HistoryError> {
        if self.terminal {
            return Err(HistoryError::AlreadyTerminal);
        }
        let durable = Item::new(
            self.items.len() as u64 + 1,
            ItemPayload::Terminal(TerminalOutcome::Interrupted),
        );
        self.items.push(durable.clone());
        self.terminal = true;
        Ok(durable)
    }

    fn replay(&self, _tenant_id: &TenantId, _turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        Ok(self.items.clone())
    }
}

#[test]
fn accepted_interrupt_suppresses_the_next_provider_output() {
    let trust = TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context");
    let result = TurnRunner::new(
        OutputAfterInterruptProvider,
        InterruptArbitratingHistory::default(),
    )
    .execute(TurnCommand::new(trust, None, "hello").expect("valid command"))
    .expect("the atomic interrupt terminal stops provider output");

    assert_eq!(result.status, TurnStatus::Interrupted);
    assert_eq!(result.replay.len(), 2);
    assert!(matches!(
        result.replay.last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Interrupted))
    ));
    assert!(!result.replay.iter().any(|item| matches!(
        item.payload,
        ItemPayload::AgentMessageDelta { .. } | ItemPayload::Usage(_)
    )));
}

struct PendingProvider;

impl ModelProvider for PendingProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        Ok(Box::new([ProviderEvent::Pending].into_iter()))
    }
}

#[derive(Default)]
struct ReconciledHistory {
    items: Arc<Mutex<Vec<Item>>>,
}

impl TurnHistory for ReconciledHistory {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
    ) -> Result<(), HistoryError> {
        Ok(())
    }

    fn interruption_requested(&self, _turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        let mut items = self.items.lock().expect("items lock");
        let sequence = items.len() as u64 + 1;
        items.push(Item::new(
            sequence,
            ItemPayload::Terminal(TerminalOutcome::Cancelled),
        ));
        Err(HistoryError::Fenced)
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
        self.items.lock().expect("items lock").push(input.clone());
        Ok(AcceptedTurn::new(
            command.trust.tenant_id.clone(),
            ThreadId::new(),
            TurnId::new(),
            LeaseGeneration::initial(),
            input,
        ))
    }

    fn append(&mut self, _turn: &AcceptedTurn, _item: NewItem) -> Result<Item, HistoryError> {
        panic!("a fenced owner must not append after reconciliation")
    }

    fn replay(&self, _tenant_id: &TenantId, _turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        Ok(self.items.lock().expect("items lock").clone())
    }
}

#[test]
fn fencing_replays_and_publishes_the_durable_cancellation() {
    let trust = TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context");
    let mut observed = Vec::new();
    let result = TurnRunner::new(PendingProvider, ReconciledHistory::default())
        .execute_with_observer(
            TurnCommand::new(trust, None, "hello").expect("valid command"),
            &mut |event| observed.push(event),
        )
        .expect("durable reconciliation terminal closes the fenced stream normally");

    assert_eq!(result.status, TurnStatus::Cancelled);
    assert!(matches!(
        result.published.as_slice(),
        [Item {
            payload: ItemPayload::Terminal(TerminalOutcome::Cancelled),
            ..
        }]
    ));
    assert!(matches!(
        observed.last(),
        Some(TurnStreamEvent::Item {
            item: Item {
                payload: ItemPayload::Terminal(TerminalOutcome::Cancelled),
                ..
            },
            ..
        })
    ));
}

struct AppendFencedHistory {
    items: Vec<Item>,
    terminal_error: HistoryError,
}

impl Default for AppendFencedHistory {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            terminal_error: HistoryError::Fenced,
        }
    }
}

impl TurnHistory for AppendFencedHistory {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
    ) -> Result<(), HistoryError> {
        Ok(())
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
        self.items.push(input.clone());
        Ok(AcceptedTurn::new(
            command.trust.tenant_id.clone(),
            ThreadId::new(),
            TurnId::new(),
            LeaseGeneration::initial(),
            input,
        ))
    }

    fn append(&mut self, _turn: &AcceptedTurn, _item: NewItem) -> Result<Item, HistoryError> {
        let terminal = Item::new(
            self.items.len() as u64 + 1,
            ItemPayload::Terminal(TerminalOutcome::Cancelled),
        );
        self.items.push(terminal);
        Err(self.terminal_error.clone())
    }

    fn replay(&self, _tenant_id: &TenantId, _turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        Ok(self.items.clone())
    }
}

#[test]
fn append_already_terminal_replays_and_publishes_the_durable_terminal() {
    let trust = TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context");
    let history = AppendFencedHistory {
        terminal_error: HistoryError::AlreadyTerminal,
        ..AppendFencedHistory::default()
    };
    let result = TurnRunner::new(OutputAfterInterruptProvider, history)
        .execute(TurnCommand::new(trust, None, "hello").expect("valid command"))
        .expect("an existing durable terminal wins the append race");

    assert_eq!(result.status, TurnStatus::Cancelled);
    assert!(matches!(
        result.published.as_slice(),
        [Item {
            payload: ItemPayload::Terminal(TerminalOutcome::Cancelled),
            ..
        }]
    ));
}

#[test]
fn append_fencing_replays_and_publishes_the_durable_terminal() {
    let trust = TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context");
    let mut observed = Vec::new();
    let result = TurnRunner::new(OutputAfterInterruptProvider, AppendFencedHistory::default())
        .execute_with_observer(
            TurnCommand::new(trust, None, "hello").expect("valid command"),
            &mut |event| observed.push(event),
        )
        .expect("append fencing publishes the durable terminal normally");

    assert_eq!(result.status, TurnStatus::Cancelled);
    assert!(matches!(
        result.published.as_slice(),
        [Item {
            payload: ItemPayload::Terminal(TerminalOutcome::Cancelled),
            ..
        }]
    ));
    assert!(matches!(
        observed.last(),
        Some(TurnStreamEvent::Item {
            item: Item {
                payload: ItemPayload::Terminal(TerminalOutcome::Cancelled),
                ..
            },
            ..
        })
    ));
}

#[derive(Default)]
struct ProviderTerminalFencedHistory {
    items: Vec<Item>,
    interruption_requested: bool,
}

impl TurnHistory for ProviderTerminalFencedHistory {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
    ) -> Result<(), HistoryError> {
        Ok(())
    }

    fn interruption_requested(&self, _turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        Ok(self.interruption_requested)
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
        self.items.push(input.clone());
        Ok(AcceptedTurn::new(
            command.trust.tenant_id.clone(),
            ThreadId::new(),
            TurnId::new(),
            LeaseGeneration::initial(),
            input,
        ))
    }

    fn append(&mut self, _turn: &AcceptedTurn, _item: NewItem) -> Result<Item, HistoryError> {
        assert!(
            self.interruption_requested,
            "provider completion uses atomic terminal append"
        );
        let terminal = Item::new(
            self.items.len() as u64 + 1,
            ItemPayload::Terminal(TerminalOutcome::Cancelled),
        );
        self.items.push(terminal);
        Err(HistoryError::Fenced)
    }

    fn append_provider_terminal(
        &mut self,
        _turn: &AcceptedTurn,
        _outcome: TerminalOutcome,
    ) -> Result<Item, HistoryError> {
        let terminal = Item::new(
            self.items.len() as u64 + 1,
            ItemPayload::Terminal(TerminalOutcome::Cancelled),
        );
        self.items.push(terminal);
        Err(HistoryError::Fenced)
    }

    fn replay(&self, _tenant_id: &TenantId, _turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        Ok(self.items.clone())
    }
}

#[test]
fn provider_terminal_fencing_replays_and_publishes_the_durable_terminal() {
    let trust = TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context");
    let mut observed = Vec::new();
    let result = TurnRunner::new(CompletingProvider, ProviderTerminalFencedHistory::default())
        .execute_with_observer(
            TurnCommand::new(trust, None, "hello").expect("valid command"),
            &mut |event| observed.push(event),
        )
        .expect("provider terminal fencing publishes the durable terminal normally");

    assert_eq!(result.status, TurnStatus::Cancelled);
    assert!(matches!(
        result.published.as_slice(),
        [Item {
            payload: ItemPayload::Terminal(TerminalOutcome::Cancelled),
            ..
        }]
    ));
    assert!(matches!(
        observed.last(),
        Some(TurnStreamEvent::Item {
            item: Item {
                payload: ItemPayload::Terminal(TerminalOutcome::Cancelled),
                ..
            },
            ..
        })
    ));
}

#[test]
fn interrupt_terminal_fencing_replays_the_durable_terminal() {
    let trust = TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context");
    let history = ProviderTerminalFencedHistory {
        interruption_requested: true,
        ..ProviderTerminalFencedHistory::default()
    };
    let result = TurnRunner::new(CompletingProvider, history)
        .execute(TurnCommand::new(trust, None, "hello").expect("valid command"))
        .expect("interrupt append fencing replays the durable terminal");

    assert_eq!(result.status, TurnStatus::Cancelled);
}

#[test]
fn disconnect_terminal_fencing_replays_the_durable_terminal() {
    let trust = TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context");
    let result = TurnRunner::new(PendingProvider, ProviderTerminalFencedHistory::default())
        .execute_with_observer_and_cancellation(
            TurnCommand::new(trust, None, "hello").expect("valid command"),
            &mut |_| {},
            &|| true,
        )
        .expect("disconnect append fencing replays the durable terminal");

    assert_eq!(result.status, TurnStatus::Cancelled);
}

#[test]
fn provider_setup_terminal_fencing_replays_the_durable_terminal() {
    let trust = TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context");
    let mut observed = Vec::new();
    let result = TurnRunner::new(
        SetupFailingProvider,
        ProviderTerminalFencedHistory::default(),
    )
    .execute_with_observer(
        TurnCommand::new(trust, None, "hello").expect("valid command"),
        &mut |event| observed.push(event),
    )
    .expect("setup-failure fencing publishes the durable terminal normally");

    assert_eq!(result.status, TurnStatus::Cancelled);
    assert!(matches!(
        result.published.as_slice(),
        [Item {
            payload: ItemPayload::Terminal(TerminalOutcome::Cancelled),
            ..
        }]
    ));
}

#[test]
fn ended_provider_terminal_fencing_replays_the_durable_terminal() {
    let trust = TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context");
    let mut observed = Vec::new();
    let result = TurnRunner::new(EmptyProvider, ProviderTerminalFencedHistory::default())
        .execute_with_observer(
            TurnCommand::new(trust, None, "hello").expect("valid command"),
            &mut |event| observed.push(event),
        )
        .expect("end-of-stream fencing publishes the durable terminal normally");

    assert_eq!(result.status, TurnStatus::Cancelled);
    assert!(matches!(
        result.published.as_slice(),
        [Item {
            payload: ItemPayload::Terminal(TerminalOutcome::Cancelled),
            ..
        }]
    ));
}

#[derive(Clone, Default)]
struct LivenessStartFailingHistory {
    items: Arc<Mutex<Vec<Item>>>,
}

impl TurnHistory for LivenessStartFailingHistory {
    fn start_turn_liveness(
        &self,
        _turn: &AcceptedTurn,
    ) -> Result<Box<dyn TurnLiveness>, HistoryError> {
        Err(HistoryError::Unavailable)
    }

    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
    ) -> Result<(), HistoryError> {
        Ok(())
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
        self.items.lock().expect("items lock").push(input.clone());
        Ok(AcceptedTurn::new(
            command.trust.tenant_id.clone(),
            ThreadId::new(),
            TurnId::new(),
            LeaseGeneration::initial(),
            input,
        ))
    }

    fn append(&mut self, _turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        let mut items = self.items.lock().expect("items lock");
        let durable = Item::new(items.len() as u64 + 1, item.into_payload());
        items.push(durable.clone());
        Ok(durable)
    }

    fn replay(&self, _tenant_id: &TenantId, _turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        Ok(self.items.lock().expect("items lock").clone())
    }
}

#[test]
fn liveness_start_failure_closes_the_accepted_turn() {
    let trust = TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context");
    let history = LivenessStartFailingHistory::default();
    let items = Arc::clone(&history.items);
    let result = TurnRunner::new(CompletingProvider, history)
        .execute(TurnCommand::new(trust, None, "hello").expect("valid command"));

    assert!(matches!(
        result,
        Err(TurnRunError::Durability(ref failure)) if failure.accepted
    ));
    let items = items.lock().expect("items lock");
    assert!(matches!(
        items.last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Failed { code }))
            if code == "DURABILITY_UNAVAILABLE"
    ));
}
