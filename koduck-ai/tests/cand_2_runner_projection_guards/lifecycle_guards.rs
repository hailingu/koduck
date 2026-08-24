// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Identity, lifecycle-completion, and retry guards split from the shared harness.

use super::*;

/// History double that falsely acknowledges each D-3 append with no items.
#[derive(Clone, Default)]
struct EmptyProjectionAcknowledgementHistory {
    inner: MemoryHistory,
}

impl TurnHistory for EmptyProjectionAcknowledgementHistory {
    fn request_interrupt(
        &mut self,
        trust: &TrustContext,
        turn_id: TurnId,
        tool_terminals: Vec<koduck_ai::application::NewItem>,
    ) -> Result<(), HistoryError> {
        self.inner.request_interrupt(trust, turn_id, tool_terminals)
    }

    fn interruption_requested(&self, turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        self.inner.interruption_requested(turn)
    }

    fn prior_thread_items(
        &self,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> Result<Vec<Item>, HistoryError> {
        self.inner.prior_thread_items(trust, thread_id)
    }

    fn accept_initial(&mut self, command: &TurnCommand) -> Result<AcceptedTurn, HistoryError> {
        self.inner.accept_initial(command)
    }

    fn append(&mut self, turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        self.inner.append(turn, item)
    }

    fn append_tool_projection(
        &mut self,
        _turn: &AcceptedTurn,
        _items: Vec<NewItem>,
    ) -> Result<Vec<Item>, HistoryError> {
        Ok(Vec::new())
    }

    fn replay(&self, tenant_id: &TenantId, turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        self.inner.replay(tenant_id, turn_id)
    }
}

#[test]
fn an_unprojected_port_failure_owns_its_terminal_code() {
    let provider = ScriptedProvider::scripted(vec![vec![tool_call_event("fixture.tool")]]);
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider, history.clone())
        .with_tool_executor(UnprojectedPortFailureToolExecutor);

    let _ = runner.execute(command());
    let recorded = &history.state.lock().expect("history lock").items;
    let items = recorded.values().next().expect("the accepted turn exists");
    let ItemPayload::Terminal(TerminalOutcome::Failed { code }) =
        &items.last().expect("the terminal exists").payload
    else {
        panic!("the turn ends with the executor-owned terminal");
    };
    assert_eq!(code, "TOOL_TENANT_MISMATCH");
}

#[test]
fn appends_are_rejected_after_the_first_projection_failure() {
    let mut stream: Vec<ProviderEvent> = (0..62)
        .map(|_| ProviderEvent::Delta("d".to_owned()))
        .collect();
    stream.push(tool_call_event("fixture.tool"));
    let provider = ScriptedProvider::scripted(vec![stream, vec![ProviderEvent::Completed]]);
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider.clone(), history.clone())
        .with_tool_executor(PersistentAfterFailureToolExecutor);
    let Err(koduck_ai::application::TurnRunError::Durability(failure)) = runner.execute(command())
    else {
        panic!("the failed sink terminalizes the turn as a durability violation");
    };
    assert!(failure.accepted);
    let recorded = &history.state.lock().expect("history lock").items;
    assert_only_durability_failure(
        recorded.values().next().expect("the accepted turn exists"),
        62,
    );
}

#[test]
fn lifecycle_transitions_are_bound_to_their_canonical_identities() {
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider.clone(), history.clone())
        .with_tool_executor(MismatchedAttemptToolExecutor);
    let result = runner.execute(command());
    let Err(koduck_ai::application::TurnRunError::Durability(failure)) = result else {
        panic!("a mismatched terminal identity fails as a durability violation");
    };
    assert!(failure.accepted);
    assert_eq!(payload_kinds(&failure.published), ["tool_call"]);
    let recorded = &history.state.lock().expect("history lock").items;
    assert_eq!(
        payload_kinds(recorded.values().next().expect("the accepted turn exists")),
        ["user_message", "tool_call", "failed"]
    );

    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider.clone(), history.clone())
        .with_tool_executor(MismatchedApprovalToolExecutor);
    let result = runner.execute(command());
    let Err(koduck_ai::application::TurnRunError::Durability(failure)) = result else {
        panic!("a mismatched resolution identity fails as a durability violation");
    };
    assert!(failure.accepted);
    let recorded = &history.state.lock().expect("history lock").items;
    assert_eq!(
        payload_kinds(recorded.values().next().expect("the accepted turn exists")),
        ["user_message", "approval_status", "failed"]
    );
}

#[test]
fn accepted_approval_cannot_be_paired_with_a_different_attempt() {
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider, history)
        .with_tool_executor(MismatchedApprovedAttemptToolExecutor);
    assert!(matches!(
        runner.execute(command()),
        Err(koduck_ai::application::TurnRunError::Durability(_))
    ));
}

#[test]
fn an_approval_resolution_must_advance_the_canonical_d6_version() {
    for resolution_version in [1, 3] {
        let approval_id = koduck_ai::domain::execution::ApprovalId::new();
        let attempt_id = koduck_ai::domain::execution::AttemptId::new();
        let sequence = vec![
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
                version: resolution_version,
            },
        ];
        let provider = ScriptedProvider::scripted(vec![
            vec![tool_call_event("fixture.tool")],
            vec![ProviderEvent::Completed],
        ]);
        let history = MemoryHistory::default();
        let mut runner = TurnRunner::new(provider.clone(), history.clone())
            .with_tool_executor(NoncanonicalSequenceToolExecutor(sequence));
        let Err(koduck_ai::application::TurnRunError::Durability(_)) = runner.execute(command())
        else {
            panic!("a resolution at version {resolution_version} fails closed");
        };
        let recorded = &history.state.lock().expect("history lock").items;
        assert_eq!(
            payload_kinds(recorded.values().next().expect("the accepted turn exists")),
            ["user_message", "approval_status", "failed"]
        );
    }
}

#[test]
fn a_result_without_a_completed_lifecycle_is_never_queued() {
    for sequence in [
        Vec::new(),
        vec![ToolProjection::ToolCall {
            descriptor_id: "fixture.tool".to_owned(),
            descriptor_version: "v1".to_owned(),
            target: "fixture-target".to_owned(),
            attempt_id: koduck_ai::domain::execution::AttemptId::new(),
            status: koduck_ai::domain::execution::ExecutionStatus::Running,
            version: 2,
        }],
    ] {
        let provider = ScriptedProvider::scripted(vec![
            vec![tool_call_event("fixture.tool")],
            vec![ProviderEvent::Completed],
        ]);
        let history = MemoryHistory::default();
        let mut runner = TurnRunner::new(provider.clone(), history.clone())
            .with_tool_executor(NoncanonicalSequenceToolExecutor(sequence));
        let Err(koduck_ai::application::TurnRunError::Durability(failure)) =
            runner.execute(command())
        else {
            panic!("an unproven result fails as a durability boundary violation");
        };
        assert!(failure.accepted);
        assert_eq!(provider.recorded_inputs().len(), 1);
        let recorded = &history.state.lock().expect("history lock").items;
        let items = recorded.values().next().expect("the accepted turn exists");
        let ItemPayload::Terminal(TerminalOutcome::Failed { code }) =
            &items.last().expect("the terminal exists").payload
        else {
            panic!("the turn terminal is the durability failure");
        };
        assert_eq!(code, "DURABILITY_UNAVAILABLE");
        assert!(
            items
                .iter()
                .all(|item| !matches!(item.payload, ItemPayload::ToolResult { .. }))
        );
    }
}

#[test]
fn a_projection_append_acknowledgement_must_match_the_planned_batch() {
    // A history adapter may incorrectly acknowledge a D-3 append without
    // returning the planned durable items. The runner must fail closed rather
    // than advance the projection lifecycle and send an unproven result in a
    // continuation request (ADR-0003 TC-06/TC-11).
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = EmptyProjectionAcknowledgementHistory::default();
    let mut runner = TurnRunner::new(provider.clone(), history.clone())
        .with_tool_executor(ExecutionToolExecutor);

    let result = runner.execute(command());

    let Err(koduck_ai::application::TurnRunError::Durability(failure)) = result else {
        panic!("an empty projection acknowledgement must fail as a durability violation");
    };
    assert!(failure.accepted);
    assert_eq!(
        provider.recorded_inputs().len(),
        1,
        "a continuation must not start after an unproven projection append"
    );
    let recorded = &history.inner.state.lock().expect("history lock").items;
    assert_only_durability_failure(
        recorded.values().next().expect("the accepted turn exists"),
        0,
    );
}

#[test]
fn a_prepared_projection_cannot_substitute_for_a_running_dispatch() {
    // The executor port is untrusted: `prepared` merely records allocation and
    // must never prove the dispatch that authorizes a terminal result to reach
    // the continuation request (ADR-0003 TC-06).
    let attempt_id = koduck_ai::domain::execution::AttemptId::new();
    let sequence = vec![
        ToolProjection::ToolCall {
            descriptor_id: "fixture.tool".to_owned(),
            descriptor_version: "v1".to_owned(),
            target: "fixture-target".to_owned(),
            attempt_id,
            status: koduck_ai::domain::execution::ExecutionStatus::Prepared,
            version: 1,
        },
        ToolProjection::ToolResult {
            attempt_id,
            status: koduck_ai::domain::execution::ExecutionStatus::Succeeded,
            code: None,
            effect_state: koduck_ai::application::EffectState::Started,
            output_bytes: 2,
            output_digest: Some(output_digest(b"ok")),
            version: 3,
        },
    ];
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider.clone(), history.clone())
        .with_tool_executor(NoncanonicalSequenceToolExecutor(sequence));

    let result = runner.execute(command());

    assert!(matches!(
        result,
        Err(koduck_ai::application::TurnRunError::Durability(_))
    ));
    assert_eq!(provider.recorded_inputs().len(), 1);
    let recorded = &history.state.lock().expect("history lock").items;
    assert_eq!(
        payload_kinds(recorded.values().next().expect("the accepted turn exists")),
        ["user_message", "failed"]
    );
}

#[test]
fn a_pre_effect_failure_can_retry_through_a_fresh_approval_lifecycle() {
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let mut runner =
        TurnRunner::new(provider.clone(), history).with_tool_executor(ApprovalRetryToolExecutor);
    let result = runner
        .execute(command())
        .expect("the approved retry completes");
    assert_eq!(result.status, TurnStatus::Completed);
    assert_eq!(provider.recorded_inputs().len(), 2);
    assert_eq!(
        payload_kinds(&result.published),
        [
            "tool_call",
            "tool_result",
            "approval_status",
            "approval_status",
            "tool_call",
            "tool_result",
            "completed"
        ]
    );
}

#[test]
fn retry_budget_exhaustion_is_durable_and_reaches_the_model() {
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider.clone(), history)
        .with_tool_executor(RetryAttemptLimitToolExecutor);
    let result = runner
        .execute(command())
        .expect("the turn completes after the typed failure");
    assert_eq!(result.status, TurnStatus::Completed);
    let inputs = provider.recorded_inputs();
    assert_eq!(inputs.len(), 2);
    assert_eq!(
        inputs[1].tool_rounds[0].calls[0].result,
        ModelToolResult {
            content: "attempt_limit".to_owned(),
            is_error: true
        }
    );
}
