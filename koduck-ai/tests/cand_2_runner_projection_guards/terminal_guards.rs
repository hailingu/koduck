// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Terminal and tuple guard cases split from the shared projection harness.

use super::*;

#[test]
fn the_result_must_match_the_committed_terminal_projection() {
    // The committed result crosses the untrusted port: its error flag and
    // byte count must agree with the terminal projection — a two-byte result
    // cannot claim a 999-byte committed output (runner contract, TC-11).
    let attempt_id = koduck_ai::domain::execution::AttemptId::new();
    let sequence = vec![
        ToolProjection::ToolCall {
            descriptor_id: "fixture.tool".to_owned(),
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
            output_bytes: 999,
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

    let Err(koduck_ai::application::TurnRunError::Durability(_)) = result else {
        panic!("a result disagreeing with its terminal fails closed");
    };
    let recorded = &history.state.lock().expect("history lock").items;
    let items = recorded.values().next().expect("the accepted turn exists");
    let ItemPayload::Terminal(TerminalOutcome::Failed { code }) =
        &items.last().expect("the terminal exists").payload
    else {
        panic!("the turn terminal is the durability failure");
    };
    assert_eq!(code, "DURABILITY_UNAVAILABLE");
}

#[test]
fn terminals_close_the_sequence_except_one_pre_effect_retry() {
    let running = |attempt_id| ToolProjection::ToolCall {
        descriptor_id: "fixture.tool".to_owned(),
        descriptor_version: "v1".to_owned(),
        target: "fixture-target".to_owned(),
        attempt_id,
        status: koduck_ai::domain::execution::ExecutionStatus::Running,
        version: 2,
    };
    let terminal = |attempt_id, status, effect_state, code| ToolProjection::ToolResult {
        attempt_id,
        status,
        code,
        effect_state,
        output_bytes: 0,
        output_digest: (status == koduck_ai::domain::execution::ExecutionStatus::Succeeded)
            .then(|| output_digest(b"")),
        version: 3,
    };
    let attempt = koduck_ai::domain::execution::AttemptId::new;
    // A succeeded terminal closes the sequence: a further lifecycle is
    // rejected instead of reopening the stage (TC-08).
    // A started-effect failure permits no retry (the effect may have run).
    for sequence in [
        vec![
            running(attempt()),
            terminal(
                attempt(),
                koduck_ai::domain::execution::ExecutionStatus::Succeeded,
                koduck_ai::application::EffectState::Started,
                None,
            ),
            running(attempt()),
        ],
        vec![
            running(attempt()),
            terminal(
                attempt(),
                koduck_ai::domain::execution::ExecutionStatus::Failed,
                koduck_ai::application::EffectState::Started,
                Some(koduck_ai::application::ExecutionFailure::ExecutorUnavailable),
            ),
            running(attempt()),
        ],
    ] {
        let provider = ScriptedProvider::scripted(vec![
            vec![tool_call_event("fixture.tool")],
            vec![ProviderEvent::Completed],
        ]);
        let history = MemoryHistory::default();
        let mut runner = TurnRunner::new(provider.clone(), history.clone())
            .with_tool_executor(NoncanonicalSequenceToolExecutor(sequence));

        let result = runner.execute(command());

        let Err(koduck_ai::application::TurnRunError::Durability(_)) = result else {
            panic!("a lifecycle after a closing terminal fails closed");
        };
    }
}

#[test]
fn one_pre_effect_failure_retry_is_permitted_and_the_retry_terminal_closes() {
    // The single TC-08 retry: a `Failed`/`not_started` terminal reopens the
    // sequence once; the retry's own terminal closes it, so a third
    // lifecycle is rejected.
    let attempt = koduck_ai::domain::execution::AttemptId::new;
    let first = attempt();
    let second = attempt();
    let running = |attempt_id| ToolProjection::ToolCall {
        descriptor_id: "fixture.tool".to_owned(),
        descriptor_version: "v1".to_owned(),
        target: "fixture-target".to_owned(),
        attempt_id,
        status: koduck_ai::domain::execution::ExecutionStatus::Running,
        version: 2,
    };
    let sequence = vec![
        running(first),
        ToolProjection::ToolResult {
            attempt_id: first,
            status: koduck_ai::domain::execution::ExecutionStatus::Failed,
            code: Some(koduck_ai::application::ExecutionFailure::ExecutorUnavailable),
            effect_state: koduck_ai::application::EffectState::NotStarted,
            output_bytes: 0,
            output_digest: None,
            version: 3,
        },
        running(second),
        ToolProjection::ToolResult {
            attempt_id: second,
            status: koduck_ai::domain::execution::ExecutionStatus::Succeeded,
            code: None,
            effect_state: koduck_ai::application::EffectState::Started,
            output_bytes: 2,
            output_digest: Some(output_digest(b"ok")),
            version: 3,
        },
        running(attempt()),
    ];
    let provider = ScriptedProvider::scripted(vec![
        vec![tool_call_event("fixture.tool")],
        vec![ProviderEvent::Completed],
    ]);
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider.clone(), history.clone())
        .with_tool_executor(NoncanonicalSequenceToolExecutor(sequence));

    let result = runner.execute(command());

    let Err(koduck_ai::application::TurnRunError::Durability(_)) = result else {
        panic!("a lifecycle after the retry's terminal fails closed");
    };
    let recorded = &history.state.lock().expect("history lock").items;
    let items = recorded.values().next().expect("the accepted turn exists");
    assert_eq!(
        payload_kinds(items),
        [
            "user_message",
            "tool_call",
            "tool_result",
            "tool_call",
            "tool_result",
            "failed"
        ],
        "exactly one retried lifecycle is durable before the failure"
    );
}

#[test]
fn terminal_projections_enforce_canonical_byte_counts() {
    // Only a success may carry output, capped at 1,048,576 bytes; a failed
    // terminal claiming output, or a success beyond the cap, is noncanonical
    // (TC-09).
    let attempt = koduck_ai::domain::execution::AttemptId::new;
    let running = |attempt_id| ToolProjection::ToolCall {
        descriptor_id: "fixture.tool".to_owned(),
        descriptor_version: "v1".to_owned(),
        target: "fixture-target".to_owned(),
        attempt_id,
        status: koduck_ai::domain::execution::ExecutionStatus::Running,
        version: 2,
    };
    for (status, code, output_bytes) in [
        (
            koduck_ai::domain::execution::ExecutionStatus::Failed,
            Some(koduck_ai::application::ExecutionFailure::ExecutorUnavailable),
            5,
        ),
        (
            koduck_ai::domain::execution::ExecutionStatus::Succeeded,
            None,
            2_000_000,
        ),
    ] {
        let attempt_id = attempt();
        let sequence = vec![
            running(attempt_id),
            ToolProjection::ToolResult {
                attempt_id,
                status,
                code,
                effect_state: koduck_ai::application::EffectState::Started,
                output_bytes,
                output_digest: (status == koduck_ai::domain::execution::ExecutionStatus::Succeeded)
                    .then(|| output_digest(b"")),
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

        let Err(koduck_ai::application::TurnRunError::Durability(_)) = result else {
            panic!("a noncanonical byte count fails closed: {status:?}/{output_bytes}");
        };
        let recorded = &history.state.lock().expect("history lock").items;
        let items = recorded.values().next().expect("the accepted turn exists");
        assert!(
            items
                .iter()
                .all(|item| !matches!(item.payload, ItemPayload::ToolResult { .. })),
            "the noncanonical terminal never became durable"
        );
    }
}

#[test]
fn projections_reuse_the_canonical_tool_value_validators() {
    // The dispatch view's descriptor/version/target fields cross an
    // untrusted port: empty, non-ASCII, and control-character values that the
    // domain validators reject are refused before persistence (ADR-0003
    // TC-06, trust-boundary rule).
    for descriptor_id in ["", "fixture\ttool", "工具"] {
        let attempt_id = koduck_ai::domain::execution::AttemptId::new();
        let sequence = [
            ToolProjection::ToolCall {
                descriptor_id: descriptor_id.to_owned(),
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
            .with_tool_executor(NoncanonicalSequenceToolExecutor(sequence.to_vec()));

        let result = runner.execute(command());

        let Err(koduck_ai::application::TurnRunError::Durability(failure)) = result else {
            panic!("an invalid field value fails closed: {descriptor_id:?}");
        };
        assert!(failure.accepted);
        let recorded = &history.state.lock().expect("history lock").items;
        let items = recorded.values().next().expect("the accepted turn exists");
        assert_only_durability_failure(items, 0);
    }
}

#[test]
fn terminal_projections_require_a_digest_only_for_success() {
    // The durable codec accepts a digest exactly for success. The sink must
    // reject either polarity mismatch before an invalid D-3 item can become
    // durable and later fail replay.
    for (status, code, output_bytes, output_digest) in [
        (
            koduck_ai::domain::execution::ExecutionStatus::Succeeded,
            None,
            2,
            None,
        ),
        (
            koduck_ai::domain::execution::ExecutionStatus::Failed,
            Some(koduck_ai::application::ExecutionFailure::ExecutorUnavailable),
            0,
            Some(output_digest(b"not-a-failure")),
        ),
    ] {
        let attempt_id = koduck_ai::domain::execution::AttemptId::new();
        let sequence = vec![
            ToolProjection::ToolCall {
                descriptor_id: "fixture.tool".to_owned(),
                descriptor_version: "v1".to_owned(),
                target: "fixture-target".to_owned(),
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Running,
                version: 2,
            },
            ToolProjection::ToolResult {
                attempt_id,
                status,
                code,
                effect_state: koduck_ai::application::EffectState::Started,
                output_bytes,
                output_digest,
                version: 3,
            },
        ];
        let provider = ScriptedProvider::scripted(vec![
            vec![tool_call_event("fixture.tool")],
            vec![ProviderEvent::Completed],
        ]);
        let history = MemoryHistory::default();
        let mut runner = TurnRunner::new(provider, history.clone())
            .with_tool_executor(NoncanonicalSequenceToolExecutor(sequence));

        let result = runner.execute(command());

        assert!(matches!(
            result,
            Err(koduck_ai::application::TurnRunError::Durability(_))
        ));
        let recorded = &history.state.lock().expect("history lock").items;
        let items = recorded.values().next().expect("the accepted turn exists");
        assert!(
            items
                .iter()
                .all(|item| !matches!(item.payload, ItemPayload::ToolResult { .. })),
            "a terminal with a mismatched digest shape must not become durable"
        );
    }
}
