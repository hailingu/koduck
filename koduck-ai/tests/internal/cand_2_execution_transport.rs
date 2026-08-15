// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Transport-level AC-5 fencing and AC-11 isolated-envelope harness legs,
//! split from the parent execution harness to stay below the 1,200-line
//! exception limit while reusing its fixtures.

use koduck_ai::adapters::execution::DisabledExecutor;
use koduck_ai::adapters::tool::{
    ConfiguredCapability, translate_mcp_tool_call, translate_native_tool_call,
};
use koduck_ai::application::ExecutionFailure;
use koduck_ai::domain::tool::Effect;

use super::*;

/// Executor double that records the owned action carried by every envelope.
struct EnvelopeRecordingExecutor {
    calls: usize,
    actions: Vec<Action>,
    response: Result<ExecutionResponse, ExecutorError>,
}

impl IsolatedExecutor for EnvelopeRecordingExecutor {
    fn execute(
        &mut self,
        _permit: &DispatchPermit,
        binding: &ExactActionBinding,
        _deadline: ActionDeadline,
    ) -> Result<ExecutionResponse, ExecutorError> {
        self.calls += 1;
        self.actions.push(binding.action().clone());
        self.response.clone()
    }

    fn cancel(
        &mut self,
        _permit: &CancelPermit,
        _binding: &ExactActionBinding,
        _deadline: ActionDeadline,
    ) -> CancelAcknowledgement {
        CancelAcknowledgement::NotAcknowledged
    }
}

#[test]
fn stale_owner_never_commits_tool_result() {
    // AC-5 fence-before-prepare: a stale owner cannot even allocate the D-7.
    let (binding, _approval) = accepted();
    let runtime = new_runtime();
    let mut fenced_preparer = runtime.preparer(SequencedLease {
        decisions: VecDeque::from([false]),
    });
    assert!(matches!(
        fenced_preparer.prepare(binding),
        Err(ExecutionPreparationError::OwnerFenced)
    ));

    // AC-5 fence-before-dispatch: zero executor calls and a cancelled D-7
    // whose effect state is not_started; no Tool result is committed.
    let (binding, approval) = accepted();
    let (mut authority, mut attempt) = prepared(binding);
    let mut coordinator = ExecutionCoordinator::new(
        RecordingExecutor {
            calls: 0,
            response: Ok(response(EffectState::Started, b"result")),
        },
        SequencedLease {
            decisions: VecDeque::from([false]),
        },
        committer(Ok(())),
    );
    assert_eq!(
        coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2),
        Ok(ToolExecutionOutcome::Cancelled {
            effect_state: EffectState::NotStarted,
        })
    );
    assert_eq!(coordinator.executor().calls, 0);
    assert_eq!(
        coordinator.committer().calls,
        1,
        "the cancelled D-7 terminal commits, but no Tool result reaches the model"
    );

    // AC-5 fence-at-result-commit: the lease is current for the dispatch claim
    // and dispatch itself, then fences immediately before the result commit.
    // An executor-confirmed not_started attempt is cancelled; a started or
    // unknown effect is failed/owner_fenced_after_dispatch held for
    // reconciliation. In every leg the model receives no Tool result.
    for (effect_state, expected) in [
        (
            EffectState::NotStarted,
            Ok(ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::NotStarted,
            }),
        ),
        (
            EffectState::Started,
            Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::OwnerFencedAfterDispatch,
                effect_state: EffectState::Started,
            }),
        ),
        (
            EffectState::Unknown,
            Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::OwnerFencedAfterDispatch,
                effect_state: EffectState::Unknown,
            }),
        ),
    ] {
        let (binding, approval) = accepted();
        let (mut authority, mut attempt) = prepared(binding);
        let mut coordinator = ExecutionCoordinator::new(
            RecordingExecutor {
                calls: 0,
                response: Ok(response(effect_state, b"result")),
            },
            SequencedLease {
                decisions: VecDeque::from([true, true, false]),
            },
            committer(Ok(())),
        );
        let observed =
            coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2);
        assert_eq!(
            observed, expected,
            "a post-dispatch {effect_state:?} fence must never deliver a Tool result"
        );
        assert_eq!(
            coordinator.executor().calls,
            1,
            "the post-dispatch legs dispatched exactly once"
        );
        let committed_terminals = match effect_state {
            // The executor proved the effect never started, so the fenced
            // owner closes its D-7 as a cancelled terminal.
            EffectState::NotStarted => 1,
            // A started or unknown effect is held for reconciliation and no
            // terminal is committed by the fenced owner.
            EffectState::Started | EffectState::Unknown => 0,
        };
        assert_eq!(
            coordinator.committer().calls,
            committed_terminals,
            "a fenced owner commits no Tool result for a {effect_state:?} effect"
        );
    }
}

/// AC-11: the native Tool and MCP adapters address capabilities only through
/// one isolated executor envelope, and the disabled production executor
/// exposes no effect path or predecessor fallback.
#[test]
fn isolated_executor_is_only_effect_path() {
    let configured = ConfiguredCapability::new(
        "fixture.tool",
        "v1",
        Effect::ExternalWrite,
        "fixture-target",
    );

    // Both adapters translate the same configured capability into one
    // byte-identical owned action; the MCP declaration cannot relabel the
    // configured effect, target, or descriptor identity.
    let native =
        translate_native_tool_call(&configured, "{}").expect("native Tool call translates");
    let mcp = translate_mcp_tool_call(&configured, "fixture.tool", "{}")
        .expect("MCP call translates the same capability");
    assert_eq!(native, mcp, "one owned envelope for both adapter origins");

    // An MCP server declaration addressing a different capability is rejected
    // before any owned action exists.
    assert_eq!(
        translate_mcp_tool_call(&configured, "mcp.fixture.relabel", "{}"),
        Err(koduck_ai::adapters::tool::ToolAdapterError::CapabilityMismatch),
        "an MCP declaration cannot address another capability"
    );

    // Each origin dispatches exactly once through the isolated executor, and
    // the envelope observed at that single effect path is the identical owned
    // action — the executor cannot distinguish the adapter origin.
    let mut envelopes = Vec::new();
    for action in [native, mcp] {
        let (binding, approval) = accepted_for(action);
        let (mut authority, mut attempt) = prepared(binding);
        let mut coordinator = ExecutionCoordinator::new(
            EnvelopeRecordingExecutor {
                calls: 0,
                actions: Vec::new(),
                response: Ok(response(EffectState::Started, b"result")),
            },
            SequencedLease {
                decisions: VecDeque::from([true, true, true]),
            },
            committer(Ok(())),
        );
        let outcome =
            coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2);
        assert!(
            matches!(outcome, Ok(ToolExecutionOutcome::Succeeded { .. })),
            "the enabled synthetic call completes"
        );
        let executor = coordinator.executor();
        assert_eq!(
            executor.calls, 1,
            "exactly one executor dispatch per adapter origin"
        );
        envelopes.extend(executor.actions.iter().cloned());
    }
    assert_eq!(
        envelopes[0], envelopes[1],
        "both origins presented the identical owned envelope at the executor"
    );

    // The disabled production executor exposes no effect path: the outcome is
    // the typed executor-unavailable failure and no external dispatch or
    // fallback path exists (TC-13).
    let (binding, approval) = accepted();
    let (mut authority, mut attempt) = prepared(binding);
    let mut coordinator = ExecutionCoordinator::new(
        DisabledExecutor,
        SequencedLease {
            decisions: VecDeque::from([true, true, true]),
        },
        committer(Ok(())),
    );
    let outcome = coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2);
    assert!(
        matches!(
            outcome,
            Ok(ToolExecutionOutcome::Failed {
                code: ExecutionFailure::ExecutorUnavailable,
                effect_state: EffectState::NotStarted,
            })
        ),
        "the disabled runtime returns the typed unavailability without any fallback"
    );
}
