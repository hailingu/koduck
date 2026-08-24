// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Black-box runner integration harness for the durable projection sink's
//! guard contract: canonical tuple and lifecycle-identity validation,
//! complete-lifecycle budget reservation, fail-closed appends, and live
//! publish visibility.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use koduck_ai::application::{
    AcceptedTurn, HistoryError, ModelInput, ModelProvider, ModelToolResult, NewItem, ProviderError,
    ProviderEvent, ProviderStream, ToolCallError, ToolCallExecutor, ToolCallTurnContext,
    ToolProjection, ToolProjectionSink, TurnCommand, TurnHistory, TurnRunner, TurnStreamEvent,
    output_digest,
};
use koduck_ai::domain::{
    Item, ItemPayload, LeaseGeneration, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId,
    TurnStatus,
};

#[derive(Clone, Default)]
struct ScriptedProvider {
    scripts: Arc<Mutex<VecDeque<Vec<ProviderEvent>>>>,
    inputs: Arc<Mutex<Vec<ModelInput>>>,
}

impl ScriptedProvider {
    fn scripted(scripts: Vec<Vec<ProviderEvent>>) -> Self {
        Self {
            scripts: Arc::new(Mutex::new(scripts.into())),
            inputs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn recorded_inputs(&self) -> Vec<ModelInput> {
        self.inputs.lock().expect("inputs lock").clone()
    }
}

impl ModelProvider for ScriptedProvider {
    fn stream(&mut self, input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        self.inputs.lock().expect("inputs lock").push(input);
        let events = self
            .scripts
            .lock()
            .expect("scripts lock")
            .pop_front()
            .expect("one scripted stream per provider request");
        Ok(Box::new(events.into_iter()))
    }
}

/// Appends and publishes one projection, mirroring the production emit order.
fn emit_projection(sink: &mut dyn ToolProjectionSink, projection: &ToolProjection) {
    sink.append(projection).expect("fixture projection appends");
    sink.publish(projection);
}

#[derive(Clone, Default)]
struct RecordingToolExecutor {
    calls: Arc<Mutex<Vec<String>>>,
}

impl ToolCallExecutor for RecordingToolExecutor {
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

/// Executor double that emits one two-item denial projection, so the durable
/// sink's atomic sequence preflight decides whether the whole pair fits the
/// cumulative per-Turn budget.
struct DenyingToolExecutor;

impl ToolCallExecutor for DenyingToolExecutor {
    fn execute_tool_call(
        &mut self,
        call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let projection = ToolProjection::Denied {
            descriptor_id: call.name,
            descriptor_version: String::new(),
            target: String::new(),
            code: "descriptor_missing".to_owned(),
        };
        // The budget refusal is the behavior under test: observe it instead of
        // panicking, exactly like a well-behaved implementation must.
        if projections.append(&projection).is_ok() {
            projections.publish(&projection);
        }
        Ok(ModelToolResult {
            content: "descriptor_missing".to_owned(),
            is_error: true,
        })
    }
}

/// Executor double asserting that each projection is visible to the live
/// observer at its publish boundary, while the call is still being serviced.
struct LivePublishToolExecutor {
    observed: Arc<Mutex<Vec<&'static str>>>,
}

impl ToolCallExecutor for LivePublishToolExecutor {
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
        assert!(
            self.observed
                .lock()
                .expect("observed lock")
                .contains(&"tool_call"),
            "the running view became visible at its publish boundary, during servicing"
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
        assert!(
            self.observed
                .lock()
                .expect("observed lock")
                .contains(&"tool_result"),
            "the terminal view became visible at its publish boundary, during servicing"
        );
        Ok(ModelToolResult {
            content: "ok".to_owned(),
            is_error: false,
        })
    }
}

/// Executor double that services a real execution lifecycle — a running view
/// followed by its succeeded terminal — tolerating budget refusals like a
/// well-behaved implementation must.
struct ExecutionToolExecutor;

impl ToolCallExecutor for ExecutionToolExecutor {
    fn execute_tool_call(
        &mut self,
        call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let attempt_id = koduck_ai::domain::execution::AttemptId::new();
        let running = ToolProjection::ToolCall {
            descriptor_id: call.name,
            descriptor_version: "v1".to_owned(),
            target: "fixture-target".to_owned(),
            attempt_id,
            status: koduck_ai::domain::execution::ExecutionStatus::Running,
            version: 2,
        };
        if projections.append(&running).is_err() {
            return Ok(ModelToolResult {
                content: "executor_unavailable".to_owned(),
                is_error: true,
            });
        }
        projections.publish(&running);
        let terminal = ToolProjection::ToolResult {
            attempt_id,
            status: koduck_ai::domain::execution::ExecutionStatus::Succeeded,
            code: None,
            effect_state: koduck_ai::application::EffectState::Started,
            output_bytes: 2,
            output_digest: Some(output_digest(b"ok")),
            version: 3,
        };
        if projections.append(&terminal).is_ok() {
            projections.publish(&terminal);
        }
        Ok(ModelToolResult {
            content: "ok".to_owned(),
            is_error: false,
        })
    }
}

/// Executor double that services an approval-required lifecycle: requested and
/// accepted approval views, then the dispatch and terminal views.
struct ApprovingToolExecutor;

impl ToolCallExecutor for ApprovingToolExecutor {
    fn execute_tool_call(
        &mut self,
        call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let approval_id = koduck_ai::domain::execution::ApprovalId::new();
        let attempt_id = koduck_ai::domain::execution::AttemptId::new();
        let sequence = [
            ToolProjection::ApprovalStatus {
                approval_id,
                attempt_id,
                status: koduck_ai::domain::execution::ApprovalStatus::Requested,
                decision: None,
                version: 1,
            },
            ToolProjection::ApprovalStatus {
                approval_id,
                attempt_id,
                status: koduck_ai::domain::execution::ApprovalStatus::Accepted,
                decision: Some(koduck_ai::domain::execution::ApprovalDecision::Accepted),
                version: 2,
            },
            ToolProjection::ToolCall {
                descriptor_id: call.name,
                descriptor_version: "v1".to_owned(),
                target: "fixture-target".to_owned(),
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Running,
                version: 2,
            },
            ToolProjection::ToolResult {
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Succeeded,
                code: None,
                effect_state: koduck_ai::application::EffectState::Started,
                output_bytes: 2,
                output_digest: None,
                version: 3,
            },
        ];
        for projection in &sequence {
            if projections.append(projection).is_err() {
                break;
            }
            projections.publish(projection);
        }
        Ok(ModelToolResult {
            content: "ok".to_owned(),
            is_error: false,
        })
    }
}

/// Executor double that proves a retry of an approval-gated action can open a
/// fresh approval lifecycle after the first attempt failed before any effect.
struct ApprovalRetryToolExecutor;

impl ToolCallExecutor for ApprovalRetryToolExecutor {
    fn execute_tool_call(
        &mut self,
        call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let first_attempt = koduck_ai::domain::execution::AttemptId::new();
        let retry_approval = koduck_ai::domain::execution::ApprovalId::new();
        let retry_attempt = koduck_ai::domain::execution::AttemptId::new();
        let sequence = [
            ToolProjection::ToolCall {
                descriptor_id: call.name,
                descriptor_version: "v1".to_owned(),
                target: "fixture-target".to_owned(),
                attempt_id: first_attempt,
                status: koduck_ai::domain::execution::ExecutionStatus::Running,
                version: 2,
            },
            ToolProjection::ToolResult {
                attempt_id: first_attempt,
                status: koduck_ai::domain::execution::ExecutionStatus::Failed,
                code: Some(koduck_ai::application::ExecutionFailure::ExecutorUnavailable),
                effect_state: koduck_ai::application::EffectState::NotStarted,
                output_bytes: 0,
                output_digest: None,
                version: 3,
            },
            ToolProjection::ApprovalStatus {
                approval_id: retry_approval,
                attempt_id: retry_attempt,
                status: koduck_ai::domain::execution::ApprovalStatus::Requested,
                decision: None,
                version: 1,
            },
            ToolProjection::ApprovalStatus {
                approval_id: retry_approval,
                attempt_id: retry_attempt,
                status: koduck_ai::domain::execution::ApprovalStatus::Accepted,
                decision: Some(koduck_ai::domain::execution::ApprovalDecision::Accepted),
                version: 2,
            },
            ToolProjection::ToolCall {
                descriptor_id: "fixture.tool".to_owned(),
                descriptor_version: "v1".to_owned(),
                target: "fixture-target".to_owned(),
                attempt_id: retry_attempt,
                status: koduck_ai::domain::execution::ExecutionStatus::Running,
                version: 2,
            },
            ToolProjection::ToolResult {
                attempt_id: retry_attempt,
                status: koduck_ai::domain::execution::ExecutionStatus::Succeeded,
                code: None,
                effect_state: koduck_ai::application::EffectState::Started,
                output_bytes: 2,
                output_digest: Some(output_digest(b"ok")),
                version: 3,
            },
        ];
        for projection in &sequence {
            emit_projection(projections, projection);
        }
        Ok(ModelToolResult {
            content: "ok".to_owned(),
            is_error: false,
        })
    }
}

/// Executor double that records the retry budget exhaustion as the final
/// model-bound result after a committed pre-effect failure.
struct RetryAttemptLimitToolExecutor;

impl ToolCallExecutor for RetryAttemptLimitToolExecutor {
    fn execute_tool_call(
        &mut self,
        call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let attempt_id = koduck_ai::domain::execution::AttemptId::new();
        let sequence = [
            ToolProjection::ToolCall {
                descriptor_id: call.name,
                descriptor_version: "v1".to_owned(),
                target: "fixture-target".to_owned(),
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Running,
                version: 2,
            },
            ToolProjection::ToolResult {
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Failed,
                code: Some(koduck_ai::application::ExecutionFailure::ExecutorUnavailable),
                effect_state: koduck_ai::application::EffectState::NotStarted,
                output_bytes: 0,
                output_digest: None,
                version: 3,
            },
            ToolProjection::Denied {
                descriptor_id: "fixture.tool".to_owned(),
                descriptor_version: "v1".to_owned(),
                target: "fixture-target".to_owned(),
                code: "attempt_limit".to_owned(),
            },
        ];
        for projection in &sequence {
            emit_projection(projections, projection);
        }
        Ok(ModelToolResult {
            content: "attempt_limit".to_owned(),
            is_error: true,
        })
    }
}

/// Executor double that emits one arbitrary, noncanonical projection tuple.
struct NoncanonicalToolExecutor(ToolProjection);

impl ToolCallExecutor for NoncanonicalToolExecutor {
    fn execute_tool_call(
        &mut self,
        _call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        if projections.append(&self.0).is_ok() {
            projections.publish(&self.0);
        }
        Ok(ModelToolResult {
            content: "ok".to_owned(),
            is_error: false,
        })
    }
}

/// Executor double that emits an arbitrary projection sequence in order.
struct NoncanonicalSequenceToolExecutor(Vec<ToolProjection>);

impl ToolCallExecutor for NoncanonicalSequenceToolExecutor {
    fn execute_tool_call(
        &mut self,
        _call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        for projection in &self.0 {
            if projections.append(projection).is_err() {
                break;
            }
            projections.publish(projection);
        }
        Ok(ModelToolResult {
            content: "ok".to_owned(),
            is_error: false,
        })
    }
}

/// Executor double whose projection append fails and which also returns a
/// turn-level error, so the runner's failure precedence is observable.
struct FailingDenialToolExecutor;

impl ToolCallExecutor for FailingDenialToolExecutor {
    fn execute_tool_call(
        &mut self,
        call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let projection = ToolProjection::Denied {
            descriptor_id: call.name,
            descriptor_version: String::new(),
            target: String::new(),
            code: "descriptor_missing".to_owned(),
        };
        // Over the cumulative budget: the append fails, then the executor
        // also reports a turn-level error.
        let _ = projections.append(&projection);
        Err(ToolCallError::Denied(
            koduck_ai::application::DenialCode::DescriptorMissing,
        ))
    }
}

/// Executor double that reports a turn-level failure without attempting a
/// projection, so the runner's terminal-code ownership remains observable.
struct UnprojectedPortFailureToolExecutor;

impl ToolCallExecutor for UnprojectedPortFailureToolExecutor {
    fn execute_tool_call(
        &mut self,
        _call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        _projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        Err(ToolCallError::TenantMismatch)
    }
}

/// Executor double that keeps emitting after a refused append, proving the
/// sink fails closed instead of resuming an incomplete lifecycle.
struct PersistentAfterFailureToolExecutor;

impl ToolCallExecutor for PersistentAfterFailureToolExecutor {
    fn execute_tool_call(
        &mut self,
        _call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let requested = ToolProjection::ApprovalStatus {
            approval_id: koduck_ai::domain::execution::ApprovalId::new(),
            attempt_id: koduck_ai::domain::execution::AttemptId::new(),
            status: koduck_ai::domain::execution::ApprovalStatus::Requested,
            decision: None,
            version: 1,
        };
        // Over the cumulative budget: the lifecycle reservation fails.
        if projections.append(&requested).is_ok() {
            projections.publish(&requested);
        }
        // The sink is now failed: this cancelled terminal must not resume the
        // incomplete lifecycle from the unchanged Open stage.
        let cancelled = ToolProjection::ToolResult {
            attempt_id: koduck_ai::domain::execution::AttemptId::new(),
            status: koduck_ai::domain::execution::ExecutionStatus::Cancelled,
            code: None,
            effect_state: koduck_ai::application::EffectState::NotStarted,
            output_bytes: 0,
            output_digest: None,
            version: 3,
        };
        if projections.append(&cancelled).is_ok() {
            projections.publish(&cancelled);
        }
        Ok(ModelToolResult {
            content: "ok".to_owned(),
            is_error: false,
        })
    }
}

/// Executor double whose terminal view references a different D-7 attempt
/// than its dispatch view.
struct MismatchedAttemptToolExecutor;

impl ToolCallExecutor for MismatchedAttemptToolExecutor {
    fn execute_tool_call(
        &mut self,
        call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let running = ToolProjection::ToolCall {
            descriptor_id: call.name,
            descriptor_version: "v1".to_owned(),
            target: "fixture-target".to_owned(),
            attempt_id: koduck_ai::domain::execution::AttemptId::new(),
            status: koduck_ai::domain::execution::ExecutionStatus::Running,
            version: 2,
        };
        if projections.append(&running).is_ok() {
            projections.publish(&running);
        }
        let terminal = ToolProjection::ToolResult {
            attempt_id: koduck_ai::domain::execution::AttemptId::new(),
            status: koduck_ai::domain::execution::ExecutionStatus::Succeeded,
            code: None,
            effect_state: koduck_ai::application::EffectState::Started,
            output_bytes: 2,
            output_digest: None,
            version: 3,
        };
        if projections.append(&terminal).is_ok() {
            projections.publish(&terminal);
        }
        Ok(ModelToolResult {
            content: "ok".to_owned(),
            is_error: false,
        })
    }
}

/// Executor double whose resolution view references a different D-6 approval
/// than its requested view.
struct MismatchedApprovalToolExecutor;

impl ToolCallExecutor for MismatchedApprovalToolExecutor {
    fn execute_tool_call(
        &mut self,
        _call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let attempt_id = koduck_ai::domain::execution::AttemptId::new();
        let requested = ToolProjection::ApprovalStatus {
            approval_id: koduck_ai::domain::execution::ApprovalId::new(),
            attempt_id,
            status: koduck_ai::domain::execution::ApprovalStatus::Requested,
            decision: None,
            version: 1,
        };
        if projections.append(&requested).is_ok() {
            projections.publish(&requested);
        }
        let resolved = ToolProjection::ApprovalStatus {
            approval_id: koduck_ai::domain::execution::ApprovalId::new(),
            attempt_id,
            status: koduck_ai::domain::execution::ApprovalStatus::Accepted,
            decision: Some(koduck_ai::domain::execution::ApprovalDecision::Accepted),
            version: 2,
        };
        if projections.append(&resolved).is_ok() {
            projections.publish(&resolved);
        }
        Ok(ModelToolResult {
            content: "ok".to_owned(),
            is_error: false,
        })
    }
}

/// Executor double whose accepted D-6 view is followed by an unrelated D-7.
struct MismatchedApprovedAttemptToolExecutor;

impl ToolCallExecutor for MismatchedApprovedAttemptToolExecutor {
    fn execute_tool_call(
        &mut self,
        call: koduck_ai::application::ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, ToolCallError> {
        let approval_id = koduck_ai::domain::execution::ApprovalId::new();
        let approved_attempt = koduck_ai::domain::execution::AttemptId::new();
        let unrelated_attempt = koduck_ai::domain::execution::AttemptId::new();
        for projection in [
            ToolProjection::ApprovalStatus {
                approval_id,
                attempt_id: approved_attempt,
                status: koduck_ai::domain::execution::ApprovalStatus::Requested,
                decision: None,
                version: 1,
            },
            ToolProjection::ApprovalStatus {
                approval_id,
                attempt_id: approved_attempt,
                status: koduck_ai::domain::execution::ApprovalStatus::Accepted,
                decision: Some(koduck_ai::domain::execution::ApprovalDecision::Accepted),
                version: 2,
            },
            ToolProjection::ToolCall {
                descriptor_id: call.name,
                descriptor_version: "v1".to_owned(),
                target: "fixture-target".to_owned(),
                attempt_id: unrelated_attempt,
                status: koduck_ai::domain::execution::ExecutionStatus::Running,
                version: 2,
            },
            ToolProjection::ToolResult {
                attempt_id: unrelated_attempt,
                status: koduck_ai::domain::execution::ExecutionStatus::Succeeded,
                code: None,
                effect_state: koduck_ai::application::EffectState::Started,
                output_bytes: 2,
                output_digest: Some(output_digest(b"ok")),
                version: 3,
            },
        ] {
            if projections.append(&projection).is_ok() {
                projections.publish(&projection);
            }
        }
        Ok(ModelToolResult {
            content: "ok".to_owned(),
            is_error: false,
        })
    }
}

#[derive(Default)]
struct MemoryHistoryState {
    items: BTreeMap<TurnId, Vec<Item>>,
    projection_batches: usize,
}

#[derive(Clone, Default)]
struct MemoryHistory {
    state: Arc<Mutex<MemoryHistoryState>>,
    fail_projection_batch_number: Option<usize>,
    fail_append_after_projection: bool,
    defer_failed_recovery: bool,
}

impl MemoryHistory {
    fn failing_second_projection_append() -> Self {
        Self {
            state: Arc::new(Mutex::new(MemoryHistoryState::default())),
            fail_projection_batch_number: Some(1),
            fail_append_after_projection: true,
            defer_failed_recovery: false,
        }
    }

    fn failing_terminal_projection_append() -> Self {
        Self {
            state: Arc::new(Mutex::new(MemoryHistoryState::default())),
            fail_projection_batch_number: Some(2),
            fail_append_after_projection: false,
            defer_failed_recovery: true,
        }
    }
}

impl TurnHistory for MemoryHistory {
    fn schedule_failed_recovery(&mut self, turn: &AcceptedTurn) -> Result<(), HistoryError> {
        if self.defer_failed_recovery {
            return Ok(());
        }
        self.append(
            turn,
            NewItem::Terminal(TerminalOutcome::Failed {
                code: "DURABILITY_UNAVAILABLE".to_owned(),
            }),
        )?;
        Ok(())
    }

    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
        _tool_terminals: Vec<koduck_ai::application::NewItem>,
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
        if self.fail_append_after_projection && items.len() >= 2 {
            return Err(HistoryError::Unavailable);
        }
        let durable = Item::new(items.len() as u64 + 1, item.into_payload());
        items.push(durable.clone());
        Ok(durable)
    }

    fn append_tool_projection(
        &mut self,
        turn: &AcceptedTurn,
        items: Vec<NewItem>,
    ) -> Result<Vec<Item>, HistoryError> {
        let payloads = items
            .into_iter()
            .map(NewItem::into_payload)
            .collect::<Vec<_>>();
        let mut state = self.state.lock().expect("history lock");
        state.projection_batches += 1;
        let projection_batch = state.projection_batches;
        let persisted = state
            .items
            .get_mut(&turn.turn_id)
            .ok_or(HistoryError::NotFound)?;
        if self.fail_projection_batch_number == Some(projection_batch) {
            return Err(HistoryError::Unavailable);
        }
        let first_sequence = persisted.len() as u64 + 1;
        let batch = payloads
            .into_iter()
            .enumerate()
            .map(|(offset, payload)| Item::new(first_sequence + offset as u64, payload))
            .collect::<Vec<_>>();
        persisted.extend(batch.iter().cloned());
        Ok(batch)
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
            other @ ItemPayload::Terminal(_) => {
                panic!("unexpected published payload: {other:?}")
            }
        })
        .collect()
}

/// Asserts the recorded Turn history carries no tool-projection items and
/// ends in the `DURABILITY_UNAVAILABLE` terminal.
fn assert_only_durability_failure(items: &[Item], deltas: usize) {
    assert_eq!(
        items.len(),
        1 + deltas + 1,
        "only the user message, the deltas, and the failed terminal are durable"
    );
    assert!(
        items.iter().all(|item| !matches!(
            item.payload,
            ItemPayload::ApprovalStatus { .. }
                | ItemPayload::ToolCall { .. }
                | ItemPayload::ToolResult { .. }
        )),
        "no tool-projection item became durable"
    );
    let ItemPayload::Terminal(TerminalOutcome::Failed { code }) =
        &items.last().expect("the terminal exists").payload
    else {
        panic!("the turn terminal is the durability failure");
    };
    assert_eq!(code, "DURABILITY_UNAVAILABLE");
}

#[test]
fn a_projection_sequence_is_preflighted_atomically_against_the_cumulative_turn_budget() {
    // 63 provider deltas leave exactly one slot of the cumulative 64-item
    // per-Turn budget: the denial's two-item sequence (tool_call +
    // tool_result) does not fit, so neither part is appended or published — a
    // lone tool_call prefix would contradict the complete-sequence preflight
    // contract (ADR-0001 exact buffer contract, ADR-0003 TC-06).
    let mut stream: Vec<ProviderEvent> = (0..63)
        .map(|_| ProviderEvent::Delta("d".to_owned()))
        .collect();
    stream.push(tool_call_event("fixture.tool"));
    let provider = ScriptedProvider::scripted(vec![stream, vec![ProviderEvent::Completed]]);
    let history = MemoryHistory::default();
    let mut runner =
        TurnRunner::new(provider.clone(), history.clone()).with_tool_executor(DenyingToolExecutor);

    let result = runner.execute(command());

    let Err(koduck_ai::application::TurnRunError::Durability(failure)) = result else {
        panic!("an over-budget projection sequence fails as a durability boundary violation");
    };
    assert!(failure.accepted);
    assert!(
        failure.published.iter().all(|item| !matches!(
            item.payload,
            ItemPayload::ToolCall { .. } | ItemPayload::ToolResult { .. }
        )),
        "no part of the over-budget sequence was published"
    );
    let recorded = &history.state.lock().expect("history lock").items;
    let items = recorded.values().next().expect("the accepted turn exists");
    assert_eq!(
        items.len(),
        1 + 63 + 1,
        "only the user message, the deltas, and the failed terminal are durable"
    );
    assert!(
        items.iter().all(|item| !matches!(
            item.payload,
            ItemPayload::ToolCall { .. } | ItemPayload::ToolResult { .. }
        )),
        "the over-budget sequence left no partial prefix durable"
    );
    let ItemPayload::Terminal(TerminalOutcome::Failed { code }) =
        &items.last().expect("the terminal exists").payload
    else {
        panic!("the turn terminal is the buffer-limit failure");
    };
    assert_eq!(code, "DURABILITY_UNAVAILABLE");
    assert_eq!(
        provider.recorded_inputs().len(),
        1,
        "no continuation request starts after the sequence preflight fails"
    );
}

#[test]
fn projections_become_visible_at_their_publish_boundary_during_servicing() {
    // `publish` is the visibility step: a requested approval or running view
    // cannot stay invisible throughout the approval wait or executor call —
    // the live observer receives each projection as soon as its durable
    // append succeeds (projection contract, ADR-0003 TC-06). The executor
    // double asserts the mid-servicing visibility; this test proves the
    // observed stream still ends in exact append order with no duplicates.
    let observed = Arc::new(Mutex::new(Vec::new()));
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let mut runner =
        TurnRunner::new(provider, history).with_tool_executor(LivePublishToolExecutor {
            observed: observed.clone(),
        });

    let stream_observed = observed.clone();
    let result = runner
        .execute_with_observer(command(), &mut |event| {
            if let TurnStreamEvent::Item { item, .. } = event {
                let kind = match &item.payload {
                    ItemPayload::ToolCall { .. } => "tool_call",
                    ItemPayload::ToolResult { .. } => "tool_result",
                    ItemPayload::Terminal(TerminalOutcome::Completed { .. }) => "completed",
                    other => panic!("unexpected observed payload: {other:?}"),
                };
                stream_observed.lock().expect("observed lock").push(kind);
            }
        })
        .expect("the turn completes");

    assert_eq!(result.status, TurnStatus::Completed);
    assert_eq!(
        observed.lock().expect("observed lock").as_slice(),
        ["tool_call", "tool_result", "completed"],
        "each projection was observed exactly once, in append order"
    );
    assert_eq!(
        payload_kinds(&result.published),
        ["tool_call", "tool_result", "completed"],
        "the published record matches the observed stream"
    );
}

#[test]
fn a_running_projection_requires_reserved_capacity_for_its_terminal() {
    // 63 provider deltas leave exactly one slot of the cumulative per-Turn
    // budget: the running view alone would fit, but its guaranteed terminal
    // view would not — the complete call lifecycle is reserved before the
    // first projection, so the running view is never appended and no orphan
    // running view can be left durable (ADR-0001 exact buffer contract,
    // ADR-0003 TC-06).
    let mut stream: Vec<ProviderEvent> = (0..63)
        .map(|_| ProviderEvent::Delta("d".to_owned()))
        .collect();
    stream.push(tool_call_event("fixture.tool"));
    let provider = ScriptedProvider::scripted(vec![stream, vec![ProviderEvent::Completed]]);
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider.clone(), history.clone())
        .with_tool_executor(ExecutionToolExecutor);

    let result = runner.execute(command());

    let Err(koduck_ai::application::TurnRunError::Durability(failure)) = result else {
        panic!("an unreservable lifecycle fails as a durability boundary violation");
    };
    assert!(failure.accepted);
    let recorded = &history.state.lock().expect("history lock").items;
    let items = recorded.values().next().expect("the accepted turn exists");
    assert_only_durability_failure(items, 63);
    assert_eq!(
        provider.recorded_inputs().len(),
        1,
        "no continuation request starts after the reservation fails"
    );
}

#[test]
fn an_approval_lifecycle_reserves_its_complete_sequence_before_the_first_projection() {
    // 62 provider deltas leave two slots: an approval-required call needs up
    // to four (requested + resolution + running + terminal), so the lifecycle
    // reservation fails before the requested view is appended — no orphan
    // approval view can be left durable (ADR-0003 TC-06).
    let mut stream: Vec<ProviderEvent> = (0..62)
        .map(|_| ProviderEvent::Delta("d".to_owned()))
        .collect();
    stream.push(tool_call_event("fixture.tool"));
    let provider = ScriptedProvider::scripted(vec![stream, vec![ProviderEvent::Completed]]);
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider.clone(), history.clone())
        .with_tool_executor(ApprovingToolExecutor);

    let result = runner.execute(command());

    let Err(koduck_ai::application::TurnRunError::Durability(failure)) = result else {
        panic!("an unreservable approval lifecycle fails as a durability boundary violation");
    };
    assert!(failure.accepted);
    let recorded = &history.state.lock().expect("history lock").items;
    let items = recorded.values().next().expect("the accepted turn exists");
    assert_only_durability_failure(items, 62);
}

#[test]
fn lifecycle_reservations_release_so_calls_sharing_the_exact_budget_complete() {
    // 59 deltas plus two two-item execution lifecycles plus the completed
    // terminal equal exactly the 64-item budget: reservations are released as
    // their projections land, so exact-budget calls still complete.
    let mut first: Vec<ProviderEvent> = (0..59)
        .map(|_| ProviderEvent::Delta("d".to_owned()))
        .collect();
    first.push(tool_call_event("fixture.tool"));
    let provider = ScriptedProvider::scripted(vec![
        first,
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let executor = RecordingToolExecutor::default();
    let mut runner =
        TurnRunner::new(provider.clone(), history.clone()).with_tool_executor(executor.clone());

    let result = runner.execute(command()).expect("the turn completes");

    assert_eq!(result.status, TurnStatus::Completed);
    let mut expected = vec!["user_message"];
    expected.extend(std::iter::repeat_n("agent_message_delta", 59));
    expected.extend([
        "tool_call",
        "tool_result",
        "tool_call",
        "tool_result",
        "completed",
    ]);
    assert_eq!(payload_kinds(&result.replay), expected);
}

#[test]
fn noncanonical_projection_tuples_are_rejected_before_persistence() {
    // The projection port is untrusted: an accepted approval with a
    // mismatched decision, a terminal dispatch view, and a running terminal
    // view are all noncanonical tuples the sink rejects before append, so
    // they can never persist or publish (ADR-0003 TC-06).
    let attempt_id = koduck_ai::domain::execution::AttemptId::new();
    for projection in [
        ToolProjection::ApprovalStatus {
            approval_id: koduck_ai::domain::execution::ApprovalId::new(),
            attempt_id,
            status: koduck_ai::domain::execution::ApprovalStatus::Accepted,
            decision: Some(koduck_ai::domain::execution::ApprovalDecision::Declined),
            version: 2,
        },
        // A requested D-6 is canonically version 1 (D-6 state machine).
        ToolProjection::ApprovalStatus {
            approval_id: koduck_ai::domain::execution::ApprovalId::new(),
            attempt_id,
            status: koduck_ai::domain::execution::ApprovalStatus::Requested,
            decision: None,
            version: 9,
        },
        ToolProjection::ToolCall {
            descriptor_id: "fixture.tool".to_owned(),
            descriptor_version: "v1".to_owned(),
            target: "fixture-target".to_owned(),
            attempt_id,
            status: koduck_ai::domain::execution::ExecutionStatus::Succeeded,
            version: 3,
        },
        ToolProjection::ToolResult {
            attempt_id,
            status: koduck_ai::domain::execution::ExecutionStatus::Running,
            code: None,
            effect_state: koduck_ai::application::EffectState::Started,
            output_bytes: 0,
            output_digest: None,
            version: 2,
        },
    ] {
        let provider = ScriptedProvider::scripted(vec![
            vec![tool_call_event("fixture.tool")],
            vec![ProviderEvent::Completed],
        ]);
        let history = MemoryHistory::default();
        let mut runner = TurnRunner::new(provider.clone(), history.clone())
            .with_tool_executor(NoncanonicalToolExecutor(projection));

        let result = runner.execute(command());

        let Err(koduck_ai::application::TurnRunError::Durability(failure)) = result else {
            panic!("a noncanonical projection fails as a durability boundary violation");
        };
        assert!(failure.accepted);
        let recorded = &history.state.lock().expect("history lock").items;
        let items = recorded.values().next().expect("the accepted turn exists");
        assert_only_durability_failure(items, 0);
    }
}

#[test]
fn a_projection_durability_failure_takes_precedence_over_the_executor_error() {
    // The projection append fails AND the executor returns a turn-level
    // error: the missing durable projection is the more severe contract
    // violation, so the Turn enters the durability path rather than recording
    // the executor's normal tool-error terminal (ADR-0001, ADR-0003 TC-06).
    let mut stream: Vec<ProviderEvent> = (0..63)
        .map(|_| ProviderEvent::Delta("d".to_owned()))
        .collect();
    stream.push(tool_call_event("fixture.tool"));
    let provider = ScriptedProvider::scripted(vec![stream, vec![ProviderEvent::Completed]]);
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider.clone(), history.clone())
        .with_tool_executor(FailingDenialToolExecutor);

    let result = runner.execute(command());

    let Err(koduck_ai::application::TurnRunError::Durability(failure)) = result else {
        panic!("the projection durability failure outranks the executor error");
    };
    assert!(failure.accepted);
    let recorded = &history.state.lock().expect("history lock").items;
    let items = recorded.values().next().expect("the accepted turn exists");
    assert_only_durability_failure(items, 63);
}

#[test]
fn a_failed_multi_item_projection_leaves_no_durable_prefix() {
    let provider = ScriptedProvider::scripted(vec![vec![tool_call_event("fixture.tool")]]);
    let history = MemoryHistory::failing_second_projection_append();
    let mut runner =
        TurnRunner::new(provider, history.clone()).with_tool_executor(DenyingToolExecutor);

    let result = runner.execute(command());

    assert!(matches!(
        result,
        Err(koduck_ai::application::TurnRunError::Durability(_))
    ));
    let state = history.state.lock().expect("history lock");
    let items = state.items.values().next().expect("accepted turn exists");
    assert_eq!(
        payload_kinds(items),
        ["user_message", "failed"],
        "a failed denial batch leaves neither its ToolCall nor ToolResult durable before recovery"
    );
}

#[test]
fn a_terminal_projection_outage_keeps_the_turn_open_for_d7_recovery() {
    // Production commits the canonical D-7 terminal before emitting its D-3
    // ToolResult. If that second projection append is unavailable, a failed
    // Turn terminal would remove the Turn from every recovery scan and strand
    // replay at the preceding running view.
    let provider = ScriptedProvider::scripted(vec![vec![tool_call_event("fixture.tool")]]);
    let history = MemoryHistory::failing_terminal_projection_append();
    let mut runner =
        TurnRunner::new(provider, history.clone()).with_tool_executor(ExecutionToolExecutor);

    let result = runner.execute(command());

    assert!(matches!(
        result,
        Err(koduck_ai::application::TurnRunError::Durability(_))
    ));
    let state = history.state.lock().expect("history lock");
    let items = state.items.values().next().expect("accepted turn exists");
    assert_eq!(
        payload_kinds(items),
        ["user_message", "tool_call"],
        "the missing D-7 terminal projection must be recovered before any Turn terminal",
    );
}

#[path = "cand_2_runner_projection_guards/terminal_guards.rs"]
mod terminal_guards;

#[path = "cand_2_runner_projection_guards/lifecycle_guards.rs"]
mod lifecycle_guards;
