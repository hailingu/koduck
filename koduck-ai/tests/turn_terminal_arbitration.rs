// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use koduck_ai::application::{
    AcceptedTurn, HistoryError, ModelInput, ModelProvider, NewItem, ProviderError, ProviderEvent,
    ProviderStream, TurnCommand, TurnHistory, TurnRunner,
};
use koduck_ai::domain::{
    Item, ItemPayload, LeaseGeneration, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId,
    TurnStatus,
};

struct CompletingProvider;

impl ModelProvider for CompletingProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        Ok(Box::new([ProviderEvent::Completed].into_iter()))
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
