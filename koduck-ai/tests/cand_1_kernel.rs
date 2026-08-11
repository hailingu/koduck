// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use koduck_ai::application::{
    AcceptedTurn, HistoryError, ModelInput, ModelProvider, NewItem, ProviderEvent, ProviderStream,
    TurnCommand, TurnHistory, TurnRunner,
};
use koduck_ai::domain::{
    Item, ItemPayload, LeaseGeneration, TenantId, TerminalOutcome, TrustContext, TurnId,
    TurnStatus, Usage,
};

struct DeterministicProvider {
    events: Vec<ProviderEvent>,
}

impl ModelProvider for DeterministicProvider {
    fn stream(
        &mut self,
        _input: ModelInput,
    ) -> Result<ProviderStream<'_>, koduck_ai::application::ProviderError> {
        Ok(Box::new(self.events.clone().into_iter()))
    }
}

#[derive(Default)]
struct RecordingHistory {
    items: Vec<Item>,
}

impl TurnHistory for RecordingHistory {
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
        _tenant_id: &TenantId,
        _thread_id: koduck_ai::domain::ThreadId,
    ) -> Result<Vec<Item>, HistoryError> {
        Ok(Vec::new())
    }

    fn accept_initial(&mut self, command: &TurnCommand) -> Result<AcceptedTurn, HistoryError> {
        let thread_id = command.thread_id.unwrap_or_default();
        let turn_id = TurnId::new();
        let input = Item::new(
            1,
            ItemPayload::UserMessage {
                content: command.input.clone(),
            },
        );
        self.items.push(input.clone());

        Ok(AcceptedTurn::new(
            thread_id,
            turn_id,
            LeaseGeneration::initial(),
            input,
        ))
    }

    fn append(&mut self, _turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        let item = Item::new(self.items.len() as u64 + 1, item.into_payload());
        self.items.push(item.clone());
        Ok(item)
    }

    fn replay(&self, _tenant_id: &TenantId, _turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        Ok(self.items.clone())
    }
}

fn command() -> TurnCommand {
    TurnCommand::new(
        TrustContext::new(
            TenantId::new("tenant-a").expect("valid tenant"),
            "subject-a",
        )
        .expect("valid trust context"),
        None,
        "hello",
    )
    .expect("valid turn command")
}

#[test]
fn tool_free_turn_completes_with_ordered_items() {
    let provider = DeterministicProvider {
        events: vec![
            ProviderEvent::Delta("A".to_owned()),
            ProviderEvent::Delta("B".to_owned()),
            ProviderEvent::Usage(Usage::new(3, 2).expect("valid usage")),
            ProviderEvent::Completed,
        ],
    };
    let history = RecordingHistory::default();
    let mut runner = TurnRunner::new(provider, history);

    let result = runner.execute(command()).expect("turn completes");

    assert_eq!(result.status, TurnStatus::Completed);
    assert_eq!(result.replay.len(), 5);
    assert!(matches!(
        &result.replay[0].payload,
        ItemPayload::UserMessage { content } if content == "hello"
    ));
    assert!(matches!(
        &result.replay[1].payload,
        ItemPayload::AgentMessageDelta { content } if content == "A"
    ));
    assert!(matches!(
        &result.replay[2].payload,
        ItemPayload::AgentMessageDelta { content } if content == "B"
    ));
    assert!(matches!(result.replay[3].payload, ItemPayload::Usage(_)));
    assert!(matches!(
        result.replay[4].payload,
        ItemPayload::Terminal(TerminalOutcome::Completed { .. })
    ));
    assert_eq!(
        result
            .published
            .iter()
            .map(|item| item.sequence)
            .collect::<Vec<_>>(),
        vec![2, 3, 5]
    );
    assert!(result.published.iter().all(|published| {
        result
            .replay
            .iter()
            .any(|durable| durable.sequence == published.sequence && durable == published)
    }));
}

#[test]
fn provider_error_is_failed_terminal() {
    let provider = DeterministicProvider {
        events: vec![
            ProviderEvent::Delta("A".to_owned()),
            ProviderEvent::Error {
                code: "UPSTREAM_RESET".to_owned(),
            },
        ],
    };
    let history = RecordingHistory::default();
    let mut runner = TurnRunner::new(provider, history);

    let result = runner
        .execute(command())
        .expect("provider failure is terminal output");

    assert_eq!(result.status, TurnStatus::Failed);
    assert_eq!(result.replay.len(), 3);
    assert!(matches!(
        &result.replay[2].payload,
        ItemPayload::Terminal(TerminalOutcome::Failed { code }) if code == "UPSTREAM_RESET"
    ));
    assert_eq!(
        result
            .replay
            .iter()
            .filter(|item| matches!(item.payload, ItemPayload::Terminal(_)))
            .count(),
        1
    );
    assert!(!result.replay.iter().any(|item| matches!(
        item.payload,
        ItemPayload::Terminal(TerminalOutcome::Completed { .. })
    )));
}
