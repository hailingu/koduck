// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md

//! AC-2: actual provider requests contain the ordered effective messages with
//! no correction duplicates. The runner drives the production
//! `PostgreSQL` history and the production `Reqwest` transport against a
//! deterministic loopback upstream; every case asserts the parsed request
//! JSON, typed provenance, and unchanged raw snapshots.

use koduck_ai::adapters::history::postgres::{
    PostgresExecutor, PostgresTurnHistory, SqlxPostgresExecutor,
};
use koduck_ai::adapters::provider::{OpenAiCompatibleProvider, ReqwestOpenAiTransport};
use koduck_ai::application::{
    AcceptedTurn, CorrectionCommand, CorrectionStore, NewItem, TurnCommand, TurnRunner,
    prepare_provider_history,
};
use koduck_ai::domain::{Item, ItemId, TerminalOutcome, ThreadId, TrustContext, Usage};

use super::support::*;

/// Number of provider requests the complete case matrix issues.
const REQUESTS: usize = 8;

/// Runs the complete AC-2 check against isolated migrated `PostgreSQL` and
/// the production transport.
pub(crate) fn run() {
    let (pool, runtime) = migrated_pool();
    let executor = SqlxPostgresExecutor::new(pool.clone(), runtime.handle().clone());
    let (tenant, subject) = unique_owner();
    let owner = owner_trust(&tenant, &subject);
    let responses: Vec<Vec<u8>> = (0..REQUESTS).map(|_| completion_frames("A")).collect();
    let (base_url, receiver) = scripted_sse_upstream(responses, None, "ac2", None);
    let transport = ReqwestOpenAiTransport::new(
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("test client"),
        runtime.handle().clone(),
        &base_url,
        "test-model",
        "test-key",
    );
    let mut runner = TurnRunner::new(
        OpenAiCompatibleProvider::new(transport),
        PostgresTurnHistory::new(executor.clone()),
    );

    let mut cases: Vec<(ThreadId, Vec<serde_json::Value>)> = Vec::new();
    cases.push(new_thread_carries_only_current_input(&mut runner, &owner));
    cases.push(no_correction_resume_matches_baseline(
        &executor,
        &mut runner,
        &owner,
    ));
    let (thread, expected, provenance) = single_correction_case(&executor, &mut runner, &owner);
    cases.push((thread, expected));
    cases.push(repeated_corrections_select_the_last(
        &executor,
        &mut runner,
        &owner,
    ));
    cases.push(independent_corrections_across_turns(
        &executor,
        &mut runner,
        &owner,
    ));
    cases.push(adjacent_deltas_concatenate_under_correction(
        &executor,
        &mut runner,
        &owner,
    ));
    cases.push(exact_replacement_bytes_survive_serialization(
        &executor,
        &mut runner,
        &owner,
    ));
    cases.push(corrected_sixty_four_kib_delta_passes_unchunked(
        &executor,
        &mut runner,
        &owner,
    ));
    assert_typed_provenance(&executor, &owner, &provenance);

    let bodies = captured_bodies(&receiver, REQUESTS);
    for (index, (thread_id, expected)) in cases.iter().enumerate() {
        assert_eq!(
            messages_of(&bodies[index]),
            *expected,
            "case {} of {}",
            index + 1,
            thread_id.as_uuid()
        );
    }
    runtime.block_on(pool.close());
}

/// Resumes (or starts) one Turn through the production runner.
fn execute(
    runner: &mut TurnRunnerType,
    owner: &TrustContext,
    thread: Option<ThreadId>,
    input: &str,
) {
    runner
        .execute(TurnCommand::new(owner.clone(), thread, input).expect("valid resumed command"))
        .expect("the production Turn completes");
}

/// The runner type under test.
type TurnRunnerType = TurnRunner<
    OpenAiCompatibleProvider<ReqwestOpenAiTransport>,
    PostgresTurnHistory<SqlxPostgresExecutor>,
>;

/// Seeds one terminal Turn and returns its acceptance.
fn seed_terminal_turn(
    executor: &SqlxPostgresExecutor,
    owner: &TrustContext,
    thread: Option<ThreadId>,
    input: &str,
    deltas: &[&str],
) -> AcceptedTurn {
    let accepted = executor
        .accept_initial(&TurnCommand::new(owner.clone(), thread, input).expect("valid command"))
        .expect("seed the source Turn");
    for delta in deltas {
        executor
            .append(
                &accepted,
                NewItem::AgentMessageDelta {
                    content: (*delta).to_owned(),
                },
            )
            .expect("seed assistant delta");
    }
    executor
        .append(
            &accepted,
            NewItem::Terminal(TerminalOutcome::Completed {
                usage: Usage::new(1, 1).expect("valid usage"),
            }),
        )
        .expect("seed terminal");
    accepted
}

/// Admits one correction of the target Item through the production boundary.
fn correct(
    executor: &SqlxPostgresExecutor,
    owner: &TrustContext,
    turn: &AcceptedTurn,
    target: ItemId,
    content: &str,
) -> Item {
    executor
        .correct(
            CorrectionCommand::new(
                owner.clone(),
                turn.thread_id,
                turn.turn_id,
                ItemId::new(),
                target,
                content,
            )
            .expect("valid correction command"),
        )
        .expect("admit correction")
}

/// Case: a brand-new Thread sends exactly one current user message.
fn new_thread_carries_only_current_input(
    runner: &mut TurnRunnerType,
    owner: &TrustContext,
) -> (ThreadId, Vec<serde_json::Value>) {
    let result = runner
        .execute(TurnCommand::new(owner.clone(), None, "hello-new").expect("valid command"))
        .expect("the new-Turn production execution completes");
    (
        result.thread_id,
        vec![serde_json::json!({ "role": "user", "content": "hello-new" })],
    )
}

/// Case: a resume without corrections equals the baseline message array.
fn no_correction_resume_matches_baseline(
    executor: &SqlxPostgresExecutor,
    runner: &mut TurnRunnerType,
    owner: &TrustContext,
) -> (ThreadId, Vec<serde_json::Value>) {
    let accepted = seed_terminal_turn(executor, owner, None, "draft", &["answer"]);
    execute(runner, owner, Some(accepted.thread_id), "second");
    (
        accepted.thread_id,
        vec![
            serde_json::json!({ "role": "user", "content": "draft" }),
            serde_json::json!({ "role": "assistant", "content": "answer" }),
            serde_json::json!({ "role": "user", "content": "second" }),
        ],
    )
}

/// Case: one post-terminal correction replaces exactly its root; also
/// returns the typed provenance later asserted against the read seam.
fn single_correction_case(
    executor: &SqlxPostgresExecutor,
    runner: &mut TurnRunnerType,
    owner: &TrustContext,
) -> (ThreadId, Vec<serde_json::Value>, (AcceptedTurn, Item)) {
    let accepted = seed_terminal_turn(executor, owner, None, "draft", &["answer"]);
    let correction = correct(
        executor,
        owner,
        &accepted,
        accepted.input.item_id,
        "revised",
    );
    let before = raw_items(executor, owner, &accepted);
    execute(runner, owner, Some(accepted.thread_id), "next");
    assert_eq!(
        raw_items(executor, owner, &accepted),
        before,
        "the prior Turn's canonical rows are unchanged by the resume"
    );
    (
        accepted.thread_id,
        vec![
            serde_json::json!({ "role": "user", "content": "revised" }),
            serde_json::json!({ "role": "assistant", "content": "answer" }),
            serde_json::json!({ "role": "user", "content": "next" }),
        ],
        (accepted, correction),
    )
}

/// Case: a correction chain selects exactly the last replacement.
fn repeated_corrections_select_the_last(
    executor: &SqlxPostgresExecutor,
    runner: &mut TurnRunnerType,
    owner: &TrustContext,
) -> (ThreadId, Vec<serde_json::Value>) {
    let accepted = seed_terminal_turn(executor, owner, None, "draft", &["answer"]);
    let first = correct(executor, owner, &accepted, accepted.input.item_id, "second");
    let _second = correct(executor, owner, &accepted, first.item_id, "third");
    execute(runner, owner, Some(accepted.thread_id), "next");
    (
        accepted.thread_id,
        vec![
            serde_json::json!({ "role": "user", "content": "third" }),
            serde_json::json!({ "role": "assistant", "content": "answer" }),
            serde_json::json!({ "role": "user", "content": "next" }),
        ],
    )
}

/// Case: independent corrections in different source Turns stay separate.
fn independent_corrections_across_turns(
    executor: &SqlxPostgresExecutor,
    runner: &mut TurnRunnerType,
    owner: &TrustContext,
) -> (ThreadId, Vec<serde_json::Value>) {
    let first = seed_terminal_turn(executor, owner, None, "q1", &["a1"]);
    correct(executor, owner, &first, first.input.item_id, "Q1");
    let second = seed_terminal_turn(executor, owner, Some(first.thread_id), "q2", &["a2"]);
    correct(executor, owner, &second, second.input.item_id, "A2-source");
    execute(runner, owner, Some(first.thread_id), "next");
    (
        first.thread_id,
        vec![
            serde_json::json!({ "role": "user", "content": "Q1" }),
            serde_json::json!({ "role": "assistant", "content": "a1" }),
            serde_json::json!({ "role": "user", "content": "A2-source" }),
            serde_json::json!({ "role": "assistant", "content": "a2" }),
            serde_json::json!({ "role": "user", "content": "next" }),
        ],
    )
}

/// Case: adjacent assistant deltas concatenate, and correcting one changes
/// only its own contribution.
fn adjacent_deltas_concatenate_under_correction(
    executor: &SqlxPostgresExecutor,
    runner: &mut TurnRunnerType,
    owner: &TrustContext,
) -> (ThreadId, Vec<serde_json::Value>) {
    let accepted = seed_terminal_turn(executor, owner, None, "u", &["A", "B"]);
    let second_delta = raw_items(executor, owner, &accepted)
        .into_iter()
        .find(|item| matches!(&item.payload, koduck_ai::domain::ItemPayload::AgentMessageDelta { content } if content.as_str() == "B"))
        .expect("the second delta exists");
    correct(executor, owner, &accepted, second_delta.item_id, "B2");
    execute(runner, owner, Some(accepted.thread_id), "next");
    (
        accepted.thread_id,
        vec![
            serde_json::json!({ "role": "user", "content": "u" }),
            serde_json::json!({ "role": "assistant", "content": "AB2" }),
            serde_json::json!({ "role": "user", "content": "next" }),
        ],
    )
}

/// Case: whitespace, control, and Unicode replacement bytes survive exactly.
fn exact_replacement_bytes_survive_serialization(
    executor: &SqlxPostgresExecutor,
    runner: &mut TurnRunnerType,
    owner: &TrustContext,
) -> (ThreadId, Vec<serde_json::Value>) {
    let accepted = seed_terminal_turn(
        executor,
        owner,
        None,
        "  padded\té\n\u{1}",
        &["back\\slash \"quoted\""],
    );
    let replacement = "  revised \t é \u{1} \"x\" \\y\n";
    correct(
        executor,
        owner,
        &accepted,
        accepted.input.item_id,
        replacement,
    );
    execute(runner, owner, Some(accepted.thread_id), "next");
    (
        accepted.thread_id,
        vec![
            serde_json::json!({ "role": "user", "content": replacement }),
            serde_json::json!({ "role": "assistant", "content": "back\\slash \"quoted\"" }),
            serde_json::json!({ "role": "user", "content": "next" }),
        ],
    )
}

/// Case: a 65,536-byte corrected assistant delta passes through unchunked.
fn corrected_sixty_four_kib_delta_passes_unchunked(
    executor: &SqlxPostgresExecutor,
    runner: &mut TurnRunnerType,
    owner: &TrustContext,
) -> (ThreadId, Vec<serde_json::Value>) {
    let accepted = seed_terminal_turn(executor, owner, None, "u", &["small"]);
    let delta = raw_items(executor, owner, &accepted)
        .into_iter()
        .find(|item| matches!(&item.payload, koduck_ai::domain::ItemPayload::AgentMessageDelta { content } if content.as_str() == "small"))
        .expect("the delta exists");
    let replacement = "x".repeat(65_536);
    correct(executor, owner, &accepted, delta.item_id, &replacement);
    execute(runner, owner, Some(accepted.thread_id), "next");
    (
        accepted.thread_id,
        vec![
            serde_json::json!({ "role": "user", "content": "u" }),
            serde_json::json!({ "role": "assistant", "content": replacement }),
            serde_json::json!({ "role": "user", "content": "next" }),
        ],
    )
}

/// Returns the canonical replay of one accepted Turn.
fn raw_items(
    executor: &SqlxPostgresExecutor,
    owner: &TrustContext,
    turn: &AcceptedTurn,
) -> Vec<Item> {
    executor
        .replay(&owner.tenant_id, turn.turn_id)
        .expect("source Turn replay")
}

/// Asserts the typed view provenance of the single-correction case directly
/// through the read seam (PC-04).
fn assert_typed_provenance(
    executor: &SqlxPostgresExecutor,
    owner: &TrustContext,
    provenance: &(AcceptedTurn, Item),
) {
    let (accepted, correction) = provenance;
    let groups = executor
        .prior_thread_turns(owner, accepted.thread_id)
        .expect("the production read returns the Thread's groups");
    let prepared = prepare_provider_history(owner, accepted.thread_id, &groups)
        .expect("the corrected history prepares");
    let root = &prepared[0];
    assert_eq!(
        root.item_id, accepted.input.item_id,
        "original identity kept"
    );
    assert_eq!(
        root.source_item_id, correction.item_id,
        "final source identity kept"
    );
    assert!(
        prepared
            .iter()
            .all(|view| view.source_item_id != correction.item_id
                || view.item_id == accepted.input.item_id),
        "the correction is never its own view"
    );
}
