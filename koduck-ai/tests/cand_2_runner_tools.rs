// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md
// ADR: docs/adr/ADR-0004-provider-stream-completion-normalization.md
// ADR: koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md

//! Black-box runner integration harness for C-5 tool-call servicing.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use koduck_ai::adapters::provider::{
    OpenAiCompatibleProvider, OpenAiFrame, OpenAiFrameStream, OpenAiProtocolTransport,
    OpenAiTransportError,
};
use koduck_ai::application::{
    AcceptedTurn, HistoryError, ModelInput, ModelProvider, ModelToolResult, NewItem, ProviderError,
    ProviderEvent, ProviderStream, ToolCallError, ToolCallExecutor, ToolCallTurnContext,
    ToolProjection, ToolProjectionSink, TurnCommand, TurnHistory, TurnLiveness, TurnRunError,
    TurnRunner, output_digest,
};
use koduck_ai::domain::{
    Item, ItemPayload, LeaseGeneration, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId,
    TurnStatus,
};

mod runner_doubles;

use runner_doubles::{ScriptedProvider, emit_projection};

#[derive(Clone, Default)]
struct RecordingToolExecutor {
    calls: Arc<Mutex<Vec<String>>>,
    interruptions: Arc<Mutex<Vec<(ThreadId, TurnId)>>>,
    committed_terminals: Arc<Mutex<Vec<(TenantId, ThreadId, TurnId)>>>,
}

impl RecordingToolExecutor {
    fn interruptions(&self) -> Vec<(ThreadId, TurnId)> {
        self.interruptions
            .lock()
            .expect("executor interruptions lock")
            .clone()
    }

    fn committed_terminals(&self) -> Vec<(TenantId, ThreadId, TurnId)> {
        self.committed_terminals
            .lock()
            .expect("executor terminal notifications lock")
            .clone()
    }
}

impl ToolCallExecutor for RecordingToolExecutor {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        thread_id: ThreadId,
        turn_id: TurnId,
    ) -> Result<Vec<NewItem>, ToolCallError> {
        self.interruptions
            .lock()
            .expect("executor interruptions lock")
            .push((thread_id, turn_id));
        Ok(vec![NewItem::ToolResult {
            attempt_id: Some(koduck_ai::domain::execution::AttemptId::new()),
            status: koduck_ai::domain::execution::ExecutionStatus::Cancelled,
            code: None,
            effect_state: Some(koduck_ai::domain::ToolEffectState::NotStarted),
            output_bytes: 0,
            output_digest: None,
            version: Some(3),
        }])
    }

    fn turn_terminal_committed(
        &mut self,
        tenant_id: &TenantId,
        thread_id: ThreadId,
        turn_id: TurnId,
    ) {
        self.committed_terminals
            .lock()
            .expect("executor terminal notifications lock")
            .push((tenant_id.clone(), thread_id, turn_id));
    }

    fn execute_tool_call(
        &mut self,
        call: koduck_ai::application::ModelToolCall,
        context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        self.calls
            .lock()
            .expect("executor calls lock")
            .push(call.name.clone());
        assert_eq!(call.arguments, "{}");
        assert_eq!(context.tenant_id.as_str(), "tenant-a");
        let attempt_id = koduck_ai::domain::execution::AttemptId::new();
        emit_projection(
            projections,
            &ToolProjection::ToolCall {
                descriptor_id: call.name,
                descriptor_version: "v1".to_owned(),
                target: "fixture-target".to_owned(),
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Running,
                version: 2,
            },
        );
        emit_projection(
            projections,
            &ToolProjection::ToolResult {
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Succeeded,
                code: None,
                effect_state: koduck_ai::application::EffectState::Started,
                output_bytes: 2,
                output_digest: Some(output_digest(b"ok")),
                version: 3,
            },
        );
        Ok(ModelToolResult {
            content: "ok".to_owned(),
            is_error: false,
        })
    }
}

/// Executor double whose committed result exceeds the model-bound serialized
/// limit, violating the port's bounded-result contract (ADR-0003 TC-09), and
/// which records durable Turn-terminal notifications (ADR-0003 T-3).
#[derive(Clone, Default)]
struct OversizedResultToolExecutor {
    committed_terminals: Arc<Mutex<Vec<(TenantId, ThreadId, TurnId)>>>,
}

impl OversizedResultToolExecutor {
    fn committed_terminals(&self) -> Vec<(TenantId, ThreadId, TurnId)> {
        self.committed_terminals
            .lock()
            .expect("executor terminal notifications lock")
            .clone()
    }
}

impl ToolCallExecutor for OversizedResultToolExecutor {
    fn turn_terminal_committed(
        &mut self,
        tenant_id: &TenantId,
        thread_id: ThreadId,
        turn_id: TurnId,
    ) {
        self.committed_terminals
            .lock()
            .expect("executor terminal notifications lock")
            .push((tenant_id.clone(), thread_id, turn_id));
    }

    fn execute_tool_call(
        &mut self,
        call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let attempt_id = koduck_ai::domain::execution::AttemptId::new();
        emit_projection(
            projections,
            &ToolProjection::ToolCall {
                descriptor_id: call.name,
                descriptor_version: "v1".to_owned(),
                target: "fixture-target".to_owned(),
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Running,
                version: 2,
            },
        );
        emit_projection(
            projections,
            &ToolProjection::ToolResult {
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Succeeded,
                code: None,
                effect_state: koduck_ai::application::EffectState::Started,
                output_bytes: 1_048_576,
                output_digest: Some(output_digest(&vec![b'x'; 1_048_576])),
                version: 3,
            },
        );
        Ok(ModelToolResult {
            content: "x".repeat(1_048_577),
            is_error: false,
        })
    }
}

/// Executor double whose committed result serializes to exactly the
/// 1,048,576-byte model-bound limit, pinning the inclusive boundary.
struct MaxResultToolExecutor;

impl ToolCallExecutor for MaxResultToolExecutor {
    fn execute_tool_call(
        &mut self,
        call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let attempt_id = koduck_ai::domain::execution::AttemptId::new();
        emit_projection(
            projections,
            &ToolProjection::ToolCall {
                descriptor_id: call.name,
                descriptor_version: "v1".to_owned(),
                target: "fixture-target".to_owned(),
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Running,
                version: 2,
            },
        );
        emit_projection(
            projections,
            &ToolProjection::ToolResult {
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Succeeded,
                code: None,
                effect_state: koduck_ai::application::EffectState::Started,
                output_bytes: 1_048_576,
                output_digest: Some(output_digest(&vec![b'x'; 1_048_576])),
                version: 3,
            },
        );
        Ok(ModelToolResult {
            content: "x".repeat(1_048_576),
            is_error: false,
        })
    }
}

/// Executor double that falsely presents the non-UTF-8 model summary for a
/// success projection whose bytes are not available to the runner.
struct UnverifiableNonUtf8SuccessSummaryToolExecutor;

impl ToolCallExecutor for UnverifiableNonUtf8SuccessSummaryToolExecutor {
    fn execute_tool_call(
        &mut self,
        call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let attempt_id = koduck_ai::domain::execution::AttemptId::new();
        emit_projection(
            projections,
            &ToolProjection::ToolCall {
                descriptor_id: call.name,
                descriptor_version: "v1".to_owned(),
                target: "fixture-target".to_owned(),
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Running,
                version: 2,
            },
        );
        emit_projection(
            projections,
            &ToolProjection::ToolResult {
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Succeeded,
                code: None,
                effect_state: koduck_ai::application::EffectState::Started,
                output_bytes: 2,
                output_digest: Some(output_digest(b"ok")),
                version: 3,
            },
        );
        Ok(ModelToolResult {
            content: "output_invalid_utf8".to_owned(),
            is_error: true,
        })
    }
}

/// Executor double that mirrors a real C-5 success whose opaque bytes cannot
/// be placed directly in the UTF-8 model continuation.
struct NonUtf8CommittedSuccessToolExecutor;

impl ToolCallExecutor for NonUtf8CommittedSuccessToolExecutor {
    fn execute_tool_call(
        &mut self,
        call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let attempt_id = koduck_ai::domain::execution::AttemptId::new();
        let output = [0xff, 0xfe, 0xfd];
        emit_projection(
            projections,
            &ToolProjection::ToolCall {
                descriptor_id: call.name,
                descriptor_version: "v1".to_owned(),
                target: "fixture-target".to_owned(),
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Running,
                version: 2,
            },
        );
        emit_projection(
            projections,
            &ToolProjection::ToolResult {
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Succeeded,
                code: None,
                effect_state: koduck_ai::application::EffectState::Started,
                output_bytes: output.len() as u64,
                output_digest: Some(output_digest(&output)),
                version: 3,
            },
        );
        projections.bind_opaque_success_summary(&output);
        Ok(ModelToolResult {
            content: "output_invalid_utf8".to_owned(),
            is_error: true,
        })
    }
}

/// Executor double that returns different same-sized content than its durable
/// success projection, exercising the continuation-binding trust boundary.
struct SameLengthMismatchedSuccessToolExecutor;

impl ToolCallExecutor for SameLengthMismatchedSuccessToolExecutor {
    fn execute_tool_call(
        &mut self,
        call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let attempt_id = koduck_ai::domain::execution::AttemptId::new();
        emit_projection(
            projections,
            &ToolProjection::ToolCall {
                descriptor_id: call.name,
                descriptor_version: "v1".to_owned(),
                target: "fixture-target".to_owned(),
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Running,
                version: 2,
            },
        );
        emit_projection(
            projections,
            &ToolProjection::ToolResult {
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Succeeded,
                code: None,
                effect_state: koduck_ai::application::EffectState::Started,
                output_bytes: 2,
                output_digest: Some(output_digest(b"ok")),
                version: 3,
            },
        );
        Ok(ModelToolResult {
            content: "no".to_owned(),
            is_error: false,
        })
    }
}

/// Executor double that returns an error summary inconsistent with its
/// committed terminal code.
struct MismatchedFailureSummaryToolExecutor;

impl ToolCallExecutor for MismatchedFailureSummaryToolExecutor {
    fn execute_tool_call(
        &mut self,
        call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let attempt_id = koduck_ai::domain::execution::AttemptId::new();
        emit_projection(
            projections,
            &ToolProjection::ToolCall {
                descriptor_id: call.name,
                descriptor_version: "v1".to_owned(),
                target: "fixture-target".to_owned(),
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Running,
                version: 2,
            },
        );
        emit_projection(
            projections,
            &ToolProjection::ToolResult {
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Failed,
                code: Some(koduck_ai::application::ExecutionFailure::ExecutorUnavailable),
                effect_state: koduck_ai::application::EffectState::NotStarted,
                output_bytes: 0,
                output_digest: None,
                version: 3,
            },
        );
        Ok(ModelToolResult {
            content: "unrelated_error".to_owned(),
            is_error: true,
        })
    }
}

#[derive(Default)]
struct MemoryHistoryState {
    items: BTreeMap<TurnId, Vec<Item>>,
    interruption_thread: Option<ThreadId>,
    requested_interrupts: Vec<TurnId>,
    interrupt_error: Option<HistoryError>,
    interrupt_payloads: Vec<ItemPayload>,
    fail_provider_terminal_once: bool,
    fail_projection_once: bool,
}

#[derive(Clone, Default)]
struct MemoryHistory {
    state: Arc<Mutex<MemoryHistoryState>>,
}

impl MemoryHistory {
    fn with_interruption_thread(thread_id: ThreadId) -> Self {
        let history = Self::default();
        history
            .state
            .lock()
            .expect("history lock")
            .interruption_thread = Some(thread_id);
        history
    }

    fn with_recovered_terminal() -> Self {
        let history = Self::default();
        history
            .state
            .lock()
            .expect("history lock")
            .fail_provider_terminal_once = true;
        history
    }

    fn with_projection_append_failure() -> Self {
        let history = Self::default();
        history
            .state
            .lock()
            .expect("history lock")
            .fail_projection_once = true;
        history
    }

    fn with_interrupt_terminal_race(thread_id: ThreadId) -> Self {
        let history = Self::with_interruption_thread(thread_id);
        history.state.lock().expect("history lock").interrupt_error =
            Some(HistoryError::AlreadyTerminal);
        history
    }

    fn requested_interrupts(&self) -> Vec<TurnId> {
        self.state
            .lock()
            .expect("history lock")
            .requested_interrupts
            .clone()
    }

    fn interrupt_payloads(&self) -> Vec<ItemPayload> {
        self.state
            .lock()
            .expect("history lock")
            .interrupt_payloads
            .clone()
    }
}

impl TurnHistory for MemoryHistory {
    fn start_turn_liveness(
        &self,
        turn: &AcceptedTurn,
    ) -> Result<Box<dyn TurnLiveness>, HistoryError> {
        Ok(Box::new(RecoveredTerminalLiveness {
            state: Arc::clone(&self.state),
            turn_id: turn.turn_id,
        }))
    }

    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        turn_id: TurnId,
        tool_terminals: Vec<NewItem>,
    ) -> Result<(), HistoryError> {
        let mut state = self.state.lock().expect("history lock");
        state.requested_interrupts.push(turn_id);
        if let Some(error) = state.interrupt_error.clone() {
            return Err(error);
        }
        state
            .interrupt_payloads
            .extend(tool_terminals.into_iter().map(NewItem::into_payload));
        state
            .interrupt_payloads
            .push(ItemPayload::Terminal(TerminalOutcome::Interrupted));
        Ok(())
    }

    fn interruption_thread(
        &self,
        _trust: &TrustContext,
        _turn_id: TurnId,
    ) -> Result<Option<ThreadId>, HistoryError> {
        Ok(self.state.lock().expect("history lock").interruption_thread)
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
        let turn_id = TurnId::new();
        let input = Item::new(
            1,
            ItemPayload::UserMessage {
                content: command.input.clone(),
            },
        );
        self.state
            .lock()
            .expect("history lock")
            .items
            .insert(turn_id, vec![input.clone()]);
        Ok(AcceptedTurn::new(
            command.trust.tenant_id.clone(),
            command.thread_id.unwrap_or_default(),
            turn_id,
            LeaseGeneration::initial(),
            input,
        ))
    }

    fn append(&mut self, turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        let mut state = self.state.lock().expect("history lock");
        let items = state
            .items
            .get_mut(&turn.turn_id)
            .ok_or(HistoryError::NotFound)?;
        let durable = Item::new(items.len() as u64 + 1, item.into_payload());
        items.push(durable.clone());
        Ok(durable)
    }

    fn append_tool_projection(
        &mut self,
        turn: &AcceptedTurn,
        items: Vec<NewItem>,
    ) -> Result<Vec<Item>, HistoryError> {
        let mut state = self.state.lock().expect("history lock");
        if state.fail_projection_once {
            state.fail_projection_once = false;
            return Err(HistoryError::Unavailable);
        }
        let payloads = items
            .into_iter()
            .map(NewItem::into_payload)
            .collect::<Vec<_>>();
        let persisted = state
            .items
            .get_mut(&turn.turn_id)
            .ok_or(HistoryError::NotFound)?;
        let first_sequence = persisted.len() as u64 + 1;
        let batch = payloads
            .into_iter()
            .enumerate()
            .map(|(offset, payload)| Item::new(first_sequence + offset as u64, payload))
            .collect::<Vec<_>>();
        persisted.extend(batch.iter().cloned());
        Ok(batch)
    }

    fn append_provider_terminal(
        &mut self,
        turn: &AcceptedTurn,
        outcome: TerminalOutcome,
    ) -> Result<Item, HistoryError> {
        let mut state = self.state.lock().expect("history lock");
        if state.fail_provider_terminal_once {
            state.fail_provider_terminal_once = false;
            return Err(HistoryError::Unavailable);
        }
        let items = state
            .items
            .get_mut(&turn.turn_id)
            .ok_or(HistoryError::NotFound)?;
        let terminal = Item::new(items.len() as u64 + 1, ItemPayload::Terminal(outcome));
        items.push(terminal.clone());
        Ok(terminal)
    }

    fn replay(&self, _tenant_id: &TenantId, turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        self.state
            .lock()
            .expect("history lock")
            .items
            .get(&turn_id)
            .cloned()
            .ok_or(HistoryError::NotFound)
    }
}

struct RecoveredTerminalLiveness {
    state: Arc<Mutex<MemoryHistoryState>>,
    turn_id: TurnId,
}

impl TurnLiveness for RecoveredTerminalLiveness {
    fn handoff_to_recovery(
        self: Box<Self>,
    ) -> Result<koduck_ai::application::RecoveryHandoff, HistoryError> {
        let mut state = self.state.lock().expect("history lock");
        let items = state
            .items
            .get_mut(&self.turn_id)
            .ok_or(HistoryError::NotFound)?;
        items.push(Item::new(
            items.len() as u64 + 1,
            ItemPayload::Terminal(TerminalOutcome::Failed {
                code: "DURABILITY_UNAVAILABLE".to_owned(),
            }),
        ));
        Ok(koduck_ai::application::RecoveryHandoff::Recovered)
    }
}

fn command() -> TurnCommand {
    TurnCommand {
        trust: TrustContext::new(
            TenantId::new("tenant-a").expect("valid tenant"),
            "subject-a",
        )
        .expect("valid principal"),
        thread_id: None,
        input: "use the fixture tool".to_owned(),
    }
}

fn tool_call_event(name: &str) -> ProviderEvent {
    ProviderEvent::ToolCall {
        name: name.to_owned(),
        arguments: "{}".to_owned(),
    }
}

fn payload_kinds(items: &[Item]) -> Vec<&'static str> {
    items
        .iter()
        .map(|item| match &item.payload {
            ItemPayload::AgentMessageDelta { .. } => "agent_message_delta",
            ItemPayload::ApprovalStatus { .. } => "approval_status",
            ItemPayload::ToolCall { .. } => "tool_call",
            ItemPayload::ToolResult { .. } => "tool_result",
            ItemPayload::Usage(_) => "usage",
            ItemPayload::Terminal(TerminalOutcome::Completed { .. }) => "completed",
            ItemPayload::Terminal(TerminalOutcome::Failed { .. }) => "failed",
            ItemPayload::UserMessage { .. } => "user_message",
            ItemPayload::Correction(_) => "correction",
            other @ ItemPayload::Terminal(_) => {
                panic!("unexpected published payload: {other:?}")
            }
        })
        .collect()
}

#[test]
fn authenticated_interrupt_cancels_live_tool_work_before_terminalizing_the_turn() {
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let history = MemoryHistory::with_interruption_thread(thread_id);
    let executor = RecordingToolExecutor::default();
    let mut runner = TurnRunner::new(ScriptedProvider::default(), history.clone())
        .with_tool_executor(executor.clone());

    runner
        .request_interrupt(&command().trust, turn_id)
        .expect("the authenticated interruption is accepted");

    assert_eq!(executor.interruptions(), vec![(thread_id, turn_id)]);
    assert_eq!(history.requested_interrupts(), vec![turn_id]);
    assert!(matches!(
        history.interrupt_payloads().as_slice(),
        [
            ItemPayload::ToolResult {
                status: koduck_ai::domain::execution::ExecutionStatus::Cancelled,
                ..
            },
            ItemPayload::Terminal(TerminalOutcome::Interrupted)
        ]
    ));
    // The runner notifies the executor after the durable interrupt terminal,
    // so a C-5 boundary can reclaim its process-local authority against the
    // proven canonical terminal (ADR-0003 T-3).
    assert_eq!(
        executor.committed_terminals(),
        vec![(command().trust.tenant_id.clone(), thread_id, turn_id)],
        "the interrupt terminal notifies the executor once"
    );
}

#[test]
fn terminal_race_notifies_the_executor_after_interrupt_returns_already_terminal() {
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let history = MemoryHistory::with_interrupt_terminal_race(thread_id);
    let executor = RecordingToolExecutor::default();
    let mut runner = TurnRunner::new(ScriptedProvider::default(), history.clone())
        .with_tool_executor(executor.clone());

    assert!(matches!(
        runner.request_interrupt(&command().trust, turn_id),
        Err(TurnRunError::History(HistoryError::AlreadyTerminal))
    ));
    assert_eq!(executor.interruptions(), vec![(thread_id, turn_id)]);
    assert_eq!(history.requested_interrupts(), vec![turn_id]);
    assert_eq!(
        executor.committed_terminals(),
        vec![(command().trust.tenant_id.clone(), thread_id, turn_id)],
        "a post-lookup terminal race still gives C-5 a canonical reclamation probe"
    );
}

/// Executor double whose configured call leaves a durable live D-7 behind and
/// surfaces the canonical reconciliation requirement.
struct ReconciliationToolExecutor;

impl ToolCallExecutor for ReconciliationToolExecutor {
    fn execute_tool_call(
        &mut self,
        _call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let attempt_id = koduck_ai::domain::execution::AttemptId::new();
        emit_projection(
            projections,
            &ToolProjection::ToolCall {
                descriptor_id: "fixture.tool".to_owned(),
                descriptor_version: "v1".to_owned(),
                target: "fixture-target".to_owned(),
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Running,
                version: 2,
            },
        );
        Err(ToolCallError::Reconciliation(
            koduck_ai::application::ExecutionPending::ReconciliationRequired {
                code: koduck_ai::application::ExecutionFailure::DurabilityUnavailable,
                effect_state: koduck_ai::application::EffectState::Unknown,
            },
        ))
    }
}

/// Executor double that combines a failed D-3 append with an undecidable live
/// D-7, the ordering edge where reconciliation must keep the Turn open.
struct FailedProjectionReconciliationToolExecutor;

impl ToolCallExecutor for FailedProjectionReconciliationToolExecutor {
    fn execute_tool_call(
        &mut self,
        _call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let attempt_id = koduck_ai::domain::execution::AttemptId::new();
        let _ = projections.append(&ToolProjection::ToolCall {
            descriptor_id: "fixture.tool".to_owned(),
            descriptor_version: "v1".to_owned(),
            target: "fixture-target".to_owned(),
            attempt_id,
            status: koduck_ai::domain::execution::ExecutionStatus::Running,
            version: 2,
        });
        Err(ToolCallError::Reconciliation(
            koduck_ai::application::ExecutionPending::ReconciliationRequired {
                code: koduck_ai::application::ExecutionFailure::DurabilityUnavailable,
                effect_state: koduck_ai::application::EffectState::Unknown,
            },
        ))
    }
}

#[test]
fn a_reconciliation_tool_failure_keeps_the_turn_open_for_d7_reconciliation() {
    // The C-5 boundary intentionally retains a live D-7 when its canonical
    // terminal cannot be decided, so the runner must not immediately commit a
    // failed Turn terminal: a terminal Turn is invisible to expiry recovery
    // and interruption, stranding the D-7. The runner surfaces a typed
    // durability failure and leaves the Turn non-terminal so reconciliation
    // closes both (ADR-0003 TC-10/TC-12).
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let mut runner =
        TurnRunner::new(provider, history.clone()).with_tool_executor(ReconciliationToolExecutor);

    let result = runner.execute(command());

    assert!(
        matches!(
            result,
            Err(koduck_ai::application::TurnRunError::Durability(_))
        ),
        "a live-D-7 reconciliation requirement surfaces as a durability failure, found {result:?}"
    );
    let recorded = history.state.lock().expect("history lock");
    let items = recorded
        .items
        .values()
        .next()
        .expect("the accepted turn exists");
    let kinds = payload_kinds(items);
    assert_eq!(
        kinds,
        vec!["user_message", "tool_call"],
        "no Turn terminal is committed while the D-7 awaits canonical reconciliation, found {kinds:?}"
    );
}

#[test]
fn a_reconciliation_requirement_outranks_a_failed_projection_append() {
    let provider = ScriptedProvider::scripted(vec![vec![tool_call_event("fixture.tool")]]);
    let history = MemoryHistory::with_projection_append_failure();
    let mut runner = TurnRunner::new(provider, history.clone())
        .with_tool_executor(FailedProjectionReconciliationToolExecutor);

    let result = runner.execute(command());

    assert!(matches!(
        result,
        Err(koduck_ai::application::TurnRunError::Durability(_))
    ));
    let recorded = history.state.lock().expect("history lock");
    let items = recorded
        .items
        .values()
        .next()
        .expect("the accepted turn exists");
    assert_eq!(
        payload_kinds(items),
        vec!["user_message"],
        "the undecidable live D-7 keeps the Turn non-terminal even when its projection append failed"
    );
}

#[test]
fn a_completed_turn_notifies_the_executor_of_its_durable_terminal() {
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let executor = RecordingToolExecutor::default();
    let mut runner =
        TurnRunner::new(provider, MemoryHistory::default()).with_tool_executor(executor.clone());

    let result = runner.execute(command()).expect("the turn completes");

    assert_eq!(result.status, TurnStatus::Completed);
    assert_eq!(
        executor.committed_terminals(),
        vec![(
            command().trust.tenant_id.clone(),
            result.thread_id,
            result.turn_id
        )],
        "the runner notifies the executor exactly once after the durable terminal (ADR-0003 T-3)"
    );
}

#[test]
fn a_recovered_terminal_notifies_the_executor_of_its_durable_terminal() {
    // This fails if the recovery-pending branch replays the recovered terminal
    // but forgets to notify C-5, leaving its process-local authority retained.
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::with_recovered_terminal();
    let executor = RecordingToolExecutor::default();
    let mut runner = TurnRunner::new(provider, history).with_tool_executor(executor.clone());

    let result = runner.execute(command());

    assert!(matches!(result, Err(TurnRunError::Durability(_))));
    assert_eq!(
        executor.committed_terminals().len(),
        1,
        "a recovered canonical terminal notifies C-5 exactly once"
    );
}

#[test]
fn tool_calls_continue_the_model_with_the_committed_result() {
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let executor = RecordingToolExecutor::default();
    let mut runner =
        TurnRunner::new(provider.clone(), history.clone()).with_tool_executor(executor.clone());

    let result = runner.execute(command()).expect("the turn completes");

    assert_eq!(result.status, TurnStatus::Completed);
    assert_eq!(
        executor
            .calls
            .lock()
            .expect("executor calls lock")
            .as_slice(),
        ["fixture.tool"],
        "the model tool call was serviced through the executor port"
    );
    // The first request carries no Tool rounds; the continuation request
    // carries the bounded committed result, and completion is accepted only
    // from that continuation (TC-11).
    let inputs = provider.recorded_inputs();
    assert_eq!(
        inputs.len(),
        2,
        "the runner started one continuation request"
    );
    assert!(inputs[0].tool_rounds.is_empty());
    assert_eq!(inputs[1].tool_rounds.len(), 1);
    let round = &inputs[1].tool_rounds[0];
    assert_eq!(round.calls.len(), 1);
    assert_eq!(round.calls[0].call.name, "fixture.tool");
    assert_eq!(round.calls[0].result.content, "ok");
    assert!(!round.calls[0].result.is_error);
    // The D-3 items are durable and published in append order before the
    // terminal: tool_call then tool_result, then the completed terminal.
    assert_eq!(
        payload_kinds(&result.published),
        ["tool_call", "tool_result", "completed"]
    );
    assert_eq!(payload_kinds(&result.replay).len(), 4);
    let ItemPayload::ToolResult {
        status,
        output_bytes,
        ..
    } = &result.published[1].payload
    else {
        panic!("the tool result item is published second");
    };
    assert_eq!(
        *status,
        koduck_ai::domain::execution::ExecutionStatus::Succeeded
    );
    assert_eq!(*output_bytes, 2);
}

#[test]
fn tool_continuations_retain_assistant_text_from_the_tool_call_stream() {
    let provider = ScriptedProvider::scripted(vec![
        vec![
            ProviderEvent::Delta("I will check that now.".to_owned()),
            tool_call_event("fixture.tool"),
        ],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider.clone(), history)
        .with_tool_executor(RecordingToolExecutor::default());

    let result = runner.execute(command()).expect("the turn completes");

    assert_eq!(result.status, TurnStatus::Completed);
    let inputs = provider.recorded_inputs();
    assert_eq!(
        inputs[1].tool_rounds[0].assistant_content, "I will check that now.",
        "the continuation preserves assistant text emitted before its Tool call"
    );
}

#[test]
fn completion_on_a_stream_that_owes_a_tool_continuation_fails_closed() {
    // A provider that completes the Turn on the same stream as its Tool call
    // never delivered the committed result to the model; the runner fails
    // closed instead of closing the Turn (TC-11).
    let provider = ScriptedProvider::scripted(vec![vec![
        tool_call_event("fixture.tool"),
        ProviderEvent::Completed,
    ]]);
    let history = MemoryHistory::default();
    let executor = RecordingToolExecutor::default();
    let mut runner =
        TurnRunner::new(provider.clone(), history.clone()).with_tool_executor(executor.clone());

    let result = runner.execute(command()).expect("the turn fails closed");

    assert_eq!(result.status, TurnStatus::Failed);
    assert_eq!(
        provider.recorded_inputs().len(),
        1,
        "no continuation request starts after a premature completion"
    );
    assert_eq!(
        payload_kinds(&result.replay),
        ["user_message", "tool_call", "tool_result", "failed"]
    );
    let ItemPayload::Terminal(TerminalOutcome::Failed { code }) = &result.replay[3].payload else {
        panic!("the turn terminal is the typed failure");
    };
    assert_eq!(code, "PROVIDER_PREMATURE_COMPLETION");
}

#[test]
fn every_continuation_round_carries_all_committed_results() {
    // A continuation stream may itself raise another Tool call; the next
    // continuation carries every round committed so far.
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let executor = RecordingToolExecutor::default();
    let mut runner =
        TurnRunner::new(provider.clone(), history.clone()).with_tool_executor(executor.clone());

    let result = runner.execute(command()).expect("the turn completes");

    assert_eq!(result.status, TurnStatus::Completed);
    let inputs = provider.recorded_inputs();
    assert_eq!(inputs.len(), 3, "two continuation requests started");
    assert_eq!(inputs[1].tool_rounds.len(), 1);
    assert_eq!(inputs[2].tool_rounds.len(), 2);
    // Each continuation round stays its own batch, so the second call keeps
    // its causal order after the first call's committed result (TC-11).
    assert_eq!(inputs[2].tool_rounds[0].calls.len(), 1);
    assert_eq!(inputs[2].tool_rounds[1].calls.len(), 1);
    assert_eq!(
        inputs[2].tool_rounds[0].calls[0].result.content,
        inputs[1].tool_rounds[0].calls[0].result.content,
        "the first round is carried unchanged"
    );
    assert_eq!(
        payload_kinds(&result.published),
        [
            "tool_call",
            "tool_result",
            "tool_call",
            "tool_result",
            "completed"
        ]
    );
}

#[test]
fn unassembled_tool_execution_fails_closed_with_a_recorded_typed_result() {
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider.clone(), history.clone());

    let result = runner.execute(command()).expect("the turn still completes");

    // The unconfigured boundary is recorded as a typed unavailability instead
    // of executing, caching, or silently ignoring the call (TC-13), and the
    // continuation request still carries that committed result to the model.
    assert_eq!(result.status, TurnStatus::Completed);
    let ItemPayload::ToolResult { status, code, .. } = &result.published[1].payload else {
        panic!("the tool result item is published second");
    };
    assert_eq!(
        *status,
        koduck_ai::domain::execution::ExecutionStatus::Failed
    );
    assert_eq!(code.as_deref(), Some("tool_execution_unavailable"));
    assert_eq!(result.replay.len(), 4, "the typed result is durable");
    let inputs = provider.recorded_inputs();
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[1].tool_rounds.len(), 1);
    assert_eq!(
        inputs[1].tool_rounds[0].calls[0].result.content,
        "tool_execution_unavailable"
    );
    assert!(inputs[1].tool_rounds[0].calls[0].result.is_error);
}

#[test]
fn an_oversized_model_bound_result_is_rejected_at_the_executor_boundary() {
    // The executor port returns a committed result whose raw byte size
    // exceeds the 1 MiB model-bound limit — the same raw-byte definition the
    // executor boundary enforces (1,048,577 content bytes): the runner
    // terminalizes the Turn before the oversized result can reach a
    // continuation request. The already committed D-3 lifecycle remains
    // durable for audit; append-before-publish has no generic rollback
    // transaction (ADR-0003 TC-06/TC-09).
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let executor = OversizedResultToolExecutor::default();
    let mut runner =
        TurnRunner::new(provider.clone(), history.clone()).with_tool_executor(executor.clone());

    let result = runner.execute(command());

    let Err(koduck_ai::application::TurnRunError::Durability(failure)) = result else {
        panic!("an oversized model-bound result fails as a boundary violation");
    };
    assert!(failure.accepted);
    assert_eq!(
        payload_kinds(&failure.published),
        ["tool_call", "tool_result"],
        "the committed lifecycle remains visible before the failure terminal"
    );
    let recorded = &history.state.lock().expect("history lock").items;
    let items = recorded.values().next().expect("the accepted turn exists");
    assert_eq!(
        payload_kinds(items),
        ["user_message", "tool_call", "tool_result", "failed"],
        "the out-of-contract result is never continued, while its committed lifecycle is retained"
    );
    let ItemPayload::Terminal(TerminalOutcome::Failed { code }) =
        &items.last().expect("the terminal exists").payload
    else {
        panic!("the turn terminal is the boundary-limit failure");
    };
    assert_eq!(code, "DURABILITY_UNAVAILABLE");
    assert_eq!(
        provider.recorded_inputs().len(),
        1,
        "no continuation request starts after the boundary rejection"
    );
    assert_eq!(
        executor.committed_terminals().len(),
        1,
        "the committed DURABILITY_UNAVAILABLE terminal still notifies the executor, so its fail-closed probe can reclaim process-local authority (ADR-0003 T-3)"
    );
}

#[test]
fn a_model_bound_result_at_the_exact_serialized_limit_is_accepted() {
    // Exactly 1,048,576 raw content bytes — the inclusive TC-09 model-bound
    // limit under the single raw-byte definition shared with the executor
    // boundary.
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider.clone(), history.clone())
        .with_tool_executor(MaxResultToolExecutor);

    let result = runner.execute(command()).expect("the turn completes");

    assert_eq!(result.status, TurnStatus::Completed);
    let inputs = provider.recorded_inputs();
    assert_eq!(
        inputs.len(),
        2,
        "the runner started one continuation request"
    );
    let round = &inputs[1].tool_rounds[0];
    assert_eq!(round.calls[0].result.content.len(), 1_048_576);
    assert!(!round.calls[0].result.is_error);
}

#[test]
fn an_unverifiable_non_utf8_success_summary_fails_closed() {
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider.clone(), history)
        .with_tool_executor(UnverifiableNonUtf8SuccessSummaryToolExecutor);

    assert!(
        runner.execute(command()).is_err(),
        "the runner cannot verify the claimed encoding from a byte count alone"
    );
    assert_eq!(provider.recorded_inputs().len(), 1);
}

#[test]
fn a_non_utf8_committed_success_reaches_the_model_as_the_stable_summary() {
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider.clone(), history)
        .with_tool_executor(NonUtf8CommittedSuccessToolExecutor);

    let result = runner
        .execute(command())
        .expect("the committed encoding rejection continues the model");

    assert_eq!(result.status, TurnStatus::Completed);
    let inputs = provider.recorded_inputs();
    assert_eq!(inputs.len(), 2, "the runner starts the continuation");
    let result = &inputs[1].tool_rounds[0].calls[0].result;
    assert!(result.is_error);
    assert_eq!(result.content, "output_invalid_utf8");
}

#[test]
fn a_same_length_success_result_must_match_the_durable_output() {
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider.clone(), history)
        .with_tool_executor(SameLengthMismatchedSuccessToolExecutor);

    assert!(
        runner.execute(command()).is_err(),
        "a byte count alone must not authorize different same-sized continuation content"
    );
    assert_eq!(provider.recorded_inputs().len(), 1);
}

#[test]
fn an_error_summary_must_match_the_committed_terminal_code() {
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let mut runner =
        TurnRunner::new(provider, history).with_tool_executor(MismatchedFailureSummaryToolExecutor);

    let result = runner.execute(command());

    assert!(matches!(
        result,
        Err(koduck_ai::application::TurnRunError::Durability(_))
    ));
}

#[test]
fn usage_counters_accumulate_across_continuation_requests() {
    // Each request of the Turn reports its own counters; the completed
    // terminal carries the checked sum across the initial request and every
    // Tool-call continuation (TC-11).
    let provider = ScriptedProvider::scripted(vec![
        vec![
            ProviderEvent::Usage(koduck_ai::domain::Usage::new(10, 2).expect("valid usage")),
            tool_call_event("fixture.tool"),
        ],
        vec![
            ProviderEvent::Usage(koduck_ai::domain::Usage::new(20, 3).expect("valid usage")),
            ProviderEvent::Completed,
        ],
    ]);
    let history = MemoryHistory::default();
    let executor = RecordingToolExecutor::default();
    let mut runner =
        TurnRunner::new(provider.clone(), history.clone()).with_tool_executor(executor.clone());

    let result = runner.execute(command()).expect("the turn completes");

    assert_eq!(result.status, TurnStatus::Completed);
    assert_eq!(
        payload_kinds(&result.replay),
        [
            "user_message",
            "usage",
            "tool_call",
            "tool_result",
            "usage",
            "completed"
        ]
    );
    let ItemPayload::Terminal(TerminalOutcome::Completed { usage }) =
        &result.replay.last().expect("a terminal exists").payload
    else {
        panic!("the turn terminal is the completion");
    };
    assert_eq!(usage.input_tokens, 30);
    assert_eq!(usage.output_tokens, 5);
    assert_eq!(usage.total_tokens, 35);
}

#[test]
fn usage_counter_overflow_fails_closed() {
    let provider = ScriptedProvider::scripted(vec![
        vec![
            ProviderEvent::Usage(
                koduck_ai::domain::Usage::new(u64::MAX, 0).expect("valid maximal usage"),
            ),
            tool_call_event("fixture.tool"),
        ],
        vec![ProviderEvent::Usage(
            koduck_ai::domain::Usage::new(1, 0).expect("valid usage"),
        )],
    ]);
    let history = MemoryHistory::default();
    let executor = RecordingToolExecutor::default();
    let mut runner =
        TurnRunner::new(provider.clone(), history.clone()).with_tool_executor(executor.clone());

    let result = runner.execute(command()).expect("the turn fails closed");

    assert_eq!(result.status, TurnStatus::Failed);
    let ItemPayload::Terminal(TerminalOutcome::Failed { code }) =
        &result.replay.last().expect("a terminal exists").payload
    else {
        panic!("the turn terminal is the usage-overflow failure");
    };
    assert_eq!(code, "PROVIDER_USAGE_OVERFLOW");
}

/// Deterministic fixture sentinel standing in for one explicit transport
/// clean end (ADR-0004 PSC-1); appended after the final `data:` frame of a
/// scripted stream, it yields the ordered `OpenAiFrame::CleanEnd`.
const CLEAN_END: &str = "\u{0}clean-end";

/// Transport stub serving one scripted OpenAI-compatible frame stream per
/// request and recording every request input (ADR-0004).
#[derive(Clone, Default)]
struct OpenAiFrameTransport {
    scripts: Arc<Mutex<VecDeque<Vec<String>>>>,
    inputs: Arc<Mutex<Vec<ModelInput>>>,
}

impl OpenAiFrameTransport {
    fn scripted(scripts: Vec<Vec<&str>>) -> Self {
        Self {
            scripts: Arc::new(Mutex::new(
                scripts
                    .into_iter()
                    .map(|frames| frames.into_iter().map(str::to_owned).collect())
                    .collect(),
            )),
            inputs: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl OpenAiProtocolTransport for OpenAiFrameTransport {
    fn chat_completion_frames(
        &mut self,
        input: &ModelInput,
    ) -> Result<OpenAiFrameStream, OpenAiTransportError> {
        self.inputs
            .lock()
            .expect("frame inputs lock")
            .push(input.clone());
        let frames = self
            .scripts
            .lock()
            .expect("frame scripts lock")
            .pop_front()
            .expect("one scripted frame stream per provider request");
        Ok(Box::new(frames.into_iter().map(|frame| {
            if frame == CLEAN_END {
                Ok(OpenAiFrame::CleanEnd)
            } else {
                Ok(OpenAiFrame::Data(frame))
            }
        })))
    }
}

/// `ModelProvider` running the production Chat Completions protocol
/// translation over scripted frame streams (ADR-0004).
#[derive(Clone)]
struct FrameScriptedProvider {
    inner: OpenAiCompatibleProvider<OpenAiFrameTransport>,
}

impl ModelProvider for FrameScriptedProvider {
    fn stream(&mut self, input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        self.inner.stream(input)
    }
}

#[test]
fn clean_eof_tool_round_continues_once() {
    // ADR-0004 PSC-4: a validated `finish_reason: "tool_calls"` followed by
    // optional usage and an explicit clean end ends only the model round; the
    // runner starts exactly one continuation carrying the committed result and
    // accepts the sole Turn completion from that continuation.
    let transport = OpenAiFrameTransport::scripted(vec![
        vec![
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"fixture.tool","arguments":"{}"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            r#"data: {"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}"#,
            CLEAN_END,
        ],
        vec![
            r#"data: {"choices":[{"delta":{"content":"Done."},"finish_reason":"stop"}]}"#,
            CLEAN_END,
        ],
    ]);
    let inputs = Arc::clone(&transport.inputs);
    let provider = FrameScriptedProvider {
        inner: OpenAiCompatibleProvider::new(transport),
    };
    let executor = RecordingToolExecutor::default();
    let mut runner =
        TurnRunner::new(provider, MemoryHistory::default()).with_tool_executor(executor.clone());

    let result = runner
        .execute(command())
        .expect("the turn completes through its continuation");

    assert_eq!(result.status, TurnStatus::Completed);
    assert_eq!(
        executor
            .calls
            .lock()
            .expect("executor calls lock")
            .as_slice(),
        ["fixture.tool"],
        "the clean-end Tool round emitted exactly one assembled Tool call"
    );
    let recorded = inputs.lock().expect("frame inputs lock").clone();
    assert_eq!(
        recorded.len(),
        2,
        "the clean-end Tool round ends the model round and starts exactly one continuation"
    );
    assert_eq!(recorded[1].tool_rounds.len(), 1);
    let round = &recorded[1].tool_rounds[0];
    assert_eq!(round.calls.len(), 1);
    assert_eq!(round.calls[0].call.name, "fixture.tool");
    assert_eq!(round.calls[0].call.arguments, "{}");
    assert_eq!(round.calls[0].result.content, "ok");
    assert!(!round.calls[0].result.is_error);
    assert_eq!(
        result
            .replay
            .iter()
            .filter(|item| matches!(
                item.payload,
                ItemPayload::Terminal(TerminalOutcome::Completed { .. })
            ))
            .count(),
        1,
        "the continuation supplies the sole Turn completion"
    );
}
