// ADR: koduck-ai/docs/adr/ADR-0005-effective-correction-projection.md

//! CAND-12 focused semantic tests for the scoped effective correction
//! projection (ADR-0005 AC-1 through AC-6). Every fixture calls the real pure
//! projection and the real raw-replay validator: no database, provider, or
//! runtime double is involved. Named fixtures carry explicit stable IDs and
//! positive Turn-local sequences unless that dimension is under test.

use std::sync::Barrier;

#[path = "cand_12_projection/fixtures.rs"]
mod fixtures;

use fixtures::*;
use koduck_ai::application::{
    ProjectionError, ProjectionScope, ScopedProjectionItem, project_corrections,
};
use koduck_ai::domain::item_correction::{
    ItemCorrection, RawReplayStructureError, validate_raw_replay,
};
use koduck_ai::domain::{
    Item, ItemId, ItemPayload, TenantId, TerminalOutcome, ThreadId, TurnId, Usage,
};

/// AC-1: empty and unchanged history retain identity, order, and content
/// deterministically (EP-01, EP-02, EP-04, EP-06).
#[test]
fn unchanged_history() {
    empty_input_projects_to_an_empty_deterministic_result();
    uncorrected_history_preserves_identity_order_and_payloads();
    projection_is_returnable_from_locally_built_wrapper_storage();
    projection_is_returnable_over_a_local_scope();
}

/// Returns the projection for caller-owned Items while the expected scope is
/// constructed locally: PR 30 round-2 review — the output must bind only to
/// the Item lifetime, never to the scope borrow.
fn projection_returned_over_local_scope(
    items: &[Item],
) -> Vec<koduck_ai::application::EffectiveItem<'_>> {
    let scope = scope_fixture("tenant-a", "subject-a");
    let mut wrappers = Vec::with_capacity(items.len());
    for item in items {
        wrappers.push(ScopedProjectionItem::new(item, &scope));
    }
    project_corrections(&scope, &wrappers).expect("history projects")
}

/// The returned projection keeps working when the scope was only a local
/// construction: identity and content still reference the caller's Items.
fn projection_is_returnable_over_a_local_scope() {
    let items = vec![user_item(1, "scoped locally")];
    let returned = projection_returned_over_local_scope(&items);
    assert_eq!(returned.len(), items.len());
    assert_eq!(returned[0].original().item_id, items[0].item_id);
    assert_eq!(returned[0].effective_content(), Some("scoped locally"));
}

/// Returns the projection built over a wrapper slice this helper owns
/// locally: PR 30 round-1 review — the output must borrow only the
/// underlying Items, never the temporary wrapper storage.
fn projection_returned_over_local_wrappers<'a>(
    scope: &ProjectionScope,
    items: &'a [Item],
) -> Vec<koduck_ai::application::EffectiveItem<'a>> {
    let wrappers: Vec<ScopedProjectionItem> = items
        .iter()
        .map(|item| ScopedProjectionItem::new(item, scope))
        .collect();
    project_corrections(scope, &wrappers).expect("history projects")
}

/// The returned projection keeps working after the local wrapper storage is
/// gone: identity and content still reference the longer-lived Items.
fn projection_is_returnable_from_locally_built_wrapper_storage() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let items = vec![user_item(1, "returned"), delta_item(2, "kept")];
    let returned = projection_returned_over_local_wrappers(&scope, &items);
    assert_eq!(returned.len(), items.len());
    assert_eq!(returned[0].original().item_id, items[0].item_id);
    assert_eq!(returned[0].effective_content(), Some("returned"));
    assert_eq!(returned[1].effective_content(), Some("kept"));
}

/// AC-2: linear corrections select exactly one last value at each original
/// position, including post-terminal corrections and exact bytes (EP-03,
/// EP-04, EP-05).
#[test]
fn chain_tip_substitution() {
    one_root_one_correction_substitutes_the_chain_tip();
    repeated_correction_moves_the_tip_to_the_last_link();
    interleaved_independent_roots_keep_separate_tips();
    adr_example_produces_four_effective_entries();
    appended_snapshot_changes_only_the_affected_root();
    corrected_content_preserves_exact_bytes();
}

/// AC-3: non-text payloads, delta granularity, and relative positions survive
/// projection unchanged (EP-04, EP-05).
#[test]
fn item_kind_preservation() {
    every_non_correction_variant_projects_with_self_provenance();
    adjacent_deltas_stay_separate_under_correction();
    tool_and_approval_items_keep_positions_and_payloads();
    terminal_before_later_corrections_keeps_its_outcome();
}

/// AC-4: invalid history produces the declared typed error with no output or
/// mutation, and raw faults precede ancestry faults (EP-02, EP-03, EP-06).
#[test]
fn corruption_rejection() {
    zero_equal_and_decreasing_sequences_fail_closed();
    duplicate_identity_fails_closed();
    correction_target_faults_fail_closed();
    branched_and_self_edges_fail_closed();
    forward_edges_and_cycles_fail_closed();
    every_non_text_kind_is_an_unsupported_root();
    valid_prefix_corruption_returns_no_partial_output();
    raw_faults_precede_ancestry_faults();
}

/// AC-5: scope rejection, failed-call recovery, and redacted diagnostics
/// satisfy EP-01 and EP-06.
#[test]
fn scope_and_diagnostics() {
    every_scope_component_drift_reports_the_first_mismatch_index();
    mixed_foreign_entry_reports_its_position_before_raw_faults();
    separate_scopes_stay_independent_and_recover();
    foreign_only_target_is_never_borrowed();
    error_diagnostics_carry_no_content_or_scope_text();
}

/// AC-6: long and repeated projections borrow sources and retain
/// deterministic bounded structure (EP-04, EP-06, EP-07).
#[test]
fn bounded_borrowed_projection() {
    long_chain_substitutes_the_final_tip();
    many_independent_chains_borrow_sources_above_one_mib();
    repeated_and_parallel_calls_share_immutable_input();
}

fn empty_input_projects_to_an_empty_deterministic_result() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let entries: Vec<ScopedProjectionItem> = Vec::new();
    let first = project_corrections(&scope, &entries).expect("empty input projects");
    assert!(
        first.is_empty(),
        "EP-01: empty input returns an empty projection"
    );
    let second = project_corrections(&scope, &entries).expect("repeat projects");
    assert_eq!(first, second, "EP-06 repeated calls are equal");
}

fn uncorrected_history_preserves_identity_order_and_payloads() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let items = vec![
        user_item(1, "  padded é root\n"),
        delta_item(3, "delta one"),
        usage_item(7),
        terminal_item(11, TerminalOutcome::Interrupted),
    ];
    let before = items.clone();
    let entries = scoped_entries(&items, &scope);
    let first = project_corrections(&scope, &entries).expect("uncorrected history projects");
    let second = project_corrections(&scope, &entries).expect("repeat projects");
    assert_eq!(first, second, "EP-06 deterministic replay");
    assert_eq!(first.len(), items.len(), "one view per non-correction Item");
    for (view, item) in first.iter().zip(&items) {
        assert_eq!(view.original().item_id, item.item_id, "identity preserved");
        assert_eq!(
            view.original().sequence,
            item.sequence,
            "sequence preserved"
        );
        assert_eq!(view.original(), item, "exact original payload retained");
        assert_eq!(
            view.source().item_id,
            item.item_id,
            "uncorrected source is self"
        );
    }
    assert_eq!(first[0].effective_content(), Some("  padded é root\n"));
    assert_eq!(first[1].effective_content(), Some("delta one"));
    assert_eq!(first[2].effective_content(), None, "usage is non-text");
    assert_eq!(first[3].effective_content(), None, "terminal is non-text");
    assert_eq!(items, before, "EP-06 input unchanged");
}

fn one_root_one_correction_substitutes_the_chain_tip() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let root = user_item(1, "old");
    let correction = correction_item(2, root.item_id, "new");
    let items = vec![root, correction];
    let entries = scoped_entries(&items, &scope);

    let projection = project_corrections(&scope, &entries).expect("one linear chain projects");

    assert_eq!(projection.len(), 1, "n minus corrections outputs");
    let view = &projection[0];
    assert_eq!(view.original().item_id, items[0].item_id);
    assert_eq!(view.original().sequence, 1);
    assert_eq!(view.source().item_id, items[1].item_id);
    assert_eq!(view.effective_content(), Some("new"));
}

fn repeated_correction_moves_the_tip_to_the_last_link() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let root = user_item(1, "old");
    let first = correction_item(2, root.item_id, "new");
    let second = correction_item(3, first.item_id, "latest");
    let items = vec![root, first, second];
    let entries = scoped_entries(&items, &scope);

    let projection = project_corrections(&scope, &entries).expect("linear chain projects");

    assert_eq!(projection.len(), 1, "corrections emit no separate output");
    let view = &projection[0];
    assert_eq!(
        view.source().item_id,
        items[2].item_id,
        "tip is the last link"
    );
    assert_eq!(view.effective_content(), Some("latest"));
}

fn interleaved_independent_roots_keep_separate_tips() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let root_one = user_item(1, "a");
    let root_two = user_item(2, "b");
    let correct_two = correction_item(3, root_two.item_id, "b2");
    let correct_one = correction_item(4, root_one.item_id, "a2");
    let items = vec![root_one, root_two, correct_two, correct_one];
    let entries = scoped_entries(&items, &scope);

    let projection = project_corrections(&scope, &entries).expect("independent chains project");

    let tips: Vec<(ItemId, ItemId, Option<&str>)> = projection
        .iter()
        .map(|view| {
            (
                view.original().item_id,
                view.source().item_id,
                view.effective_content(),
            )
        })
        .collect();
    assert_eq!(
        tips,
        vec![
            (items[0].item_id, items[3].item_id, Some("a2")),
            (items[1].item_id, items[2].item_id, Some("b2")),
        ],
        "original order with per-root tips"
    );
}

/// The worked example from ADR-0005 "Algorithm And Reuse Design": seven raw
/// entries produce exactly four effective entries with post-terminal
/// corrections applied.
fn adr_example_produces_four_effective_entries() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let root = user_item(1, "old");
    let delta = delta_item(2, "A");
    let untouched = delta_item(4, "B");
    let terminal = terminal_item(
        8,
        TerminalOutcome::Completed {
            usage: Usage::new(3, 5).expect("valid usage"),
        },
    );
    let first = correction_item(9, root.item_id, "new");
    let second = correction_item(10, first.item_id, "latest");
    let delta_fix = correction_item(11, delta.item_id, "X");
    let ids = (
        root.item_id,
        delta.item_id,
        untouched.item_id,
        terminal.item_id,
    );
    let items = vec![root, delta, untouched, terminal, first, second, delta_fix];
    let entries = scoped_entries(&items, &scope);

    let projection = project_corrections(&scope, &entries).expect("the example projects");

    assert_eq!(projection.len(), 4, "n minus corrections outputs");
    let (root_id, delta_id, untouched_id, terminal_id) = ids;
    assert_eq!(projection[0].original().item_id, root_id);
    assert_eq!(projection[0].source().item_id, items[5].item_id);
    assert_eq!(projection[0].effective_content(), Some("latest"));
    assert_eq!(projection[1].original().item_id, delta_id);
    assert_eq!(projection[1].source().item_id, items[6].item_id);
    assert_eq!(projection[1].effective_content(), Some("X"));
    assert_eq!(projection[2].original().item_id, untouched_id);
    assert_eq!(projection[2].source().item_id, untouched_id);
    assert_eq!(projection[2].effective_content(), Some("B"));
    assert_eq!(projection[3].original().item_id, terminal_id);
    assert_eq!(projection[3].source().item_id, terminal_id);
    assert_eq!(projection[3].effective_content(), None);
    assert_eq!(
        projection[3].original().payload,
        items[3].payload,
        "exact terminal payload preserved"
    );
}

fn appended_snapshot_changes_only_the_affected_root() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let root_one = user_item(1, "a");
    let root_two = user_item(2, "b");
    let correct_one = correction_item(3, root_one.item_id, "a2");
    let snapshot_one = vec![root_one.clone(), root_two.clone(), correct_one.clone()];
    let snapshot_one_entries = scoped_entries(&snapshot_one, &scope);
    let baseline =
        project_corrections(&scope, &snapshot_one_entries).expect("snapshot one projects");
    let appended = correction_item(4, correct_one.item_id, "a3");
    let snapshot_two = vec![root_one, root_two, correct_one, appended];
    let snapshot_two_entries = scoped_entries(&snapshot_two, &scope);
    let grown = project_corrections(&scope, &snapshot_two_entries).expect("snapshot two projects");

    assert_eq!(grown.len(), baseline.len(), "views keep one slot per root");
    assert_eq!(grown[0].effective_content(), Some("a3"));
    assert_eq!(grown[0].source().item_id, snapshot_two[3].item_id);
    assert_eq!(grown[1], baseline[1], "unaffected root view is identical");
    assert_eq!(grown[1].effective_content(), Some("b"));
}

fn corrected_content_preserves_exact_bytes() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let root = user_item(1, "old");
    let replacement = "  padded\treplacement é\n";
    let correction = correction_item(2, root.item_id, replacement);
    let items = vec![root, correction];
    let entries = scoped_entries(&items, &scope);

    let projection = project_corrections(&scope, &entries).expect("chain projects");

    assert_eq!(projection[0].effective_content(), Some(replacement));
    let source_text = correction_content(&items[1]);
    assert!(
        std::ptr::eq(
            projection[0].effective_content().expect("text").as_ptr(),
            source_text.as_ptr()
        ),
        "effective content borrows the selected source payload"
    );
}

fn every_non_correction_variant_projects_with_self_provenance() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let mut sequences = 0_u64;
    let items: Vec<Item> = non_correction_payload_fixtures()
        .into_iter()
        .map(|payload| {
            sequences += 1;
            Item::new(sequences, payload)
        })
        .collect();
    let before = items.clone();
    let entries = scoped_entries(&items, &scope);

    let projection = project_corrections(&scope, &entries).expect("fixtures project");

    assert_eq!(projection.len(), items.len());
    for (view, item) in projection.iter().zip(&items) {
        assert_eq!(view.original(), item, "exact payload and position retained");
        assert_eq!(view.source().item_id, item.item_id, "self provenance");
    }
    assert_eq!(projection[0].effective_content(), Some("user \"quoted\" é"));
    assert_eq!(projection[1].effective_content(), Some("delta \n\u{0001}"));
    for view in &projection[2..] {
        assert_eq!(view.effective_content(), None, "non-text accessor is None");
    }
    assert_eq!(items, before, "EP-06 input unchanged");
}

fn adjacent_deltas_stay_separate_under_correction() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let first = delta_item(1, "one");
    let second = delta_item(2, "two");
    let correction = correction_item(3, second.item_id, "two-fixed");
    let items = vec![first, second, correction];
    let entries = scoped_entries(&items, &scope);

    let projection = project_corrections(&scope, &entries).expect("adjacent deltas project");

    assert_eq!(projection.len(), 2, "deltas are never joined");
    assert_eq!(projection[0].effective_content(), Some("one"));
    assert_eq!(projection[0].source().item_id, items[0].item_id);
    assert_eq!(projection[1].effective_content(), Some("two-fixed"));
    assert_eq!(projection[1].source().item_id, items[2].item_id);
    assert_eq!(
        projection[1].original().payload,
        ItemPayload::AgentMessageDelta {
            content: "two".to_owned()
        },
        "corrected delta keeps its kind and original payload"
    );
}

fn tool_and_approval_items_keep_positions_and_payloads() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let root = user_item(1, "question");
    let call = tool_call_item(2);
    let approval = approval_item(3);
    let result = tool_result_item(4);
    let correction = correction_item(5, root.item_id, "answer");
    let terminal = terminal_item(6, TerminalOutcome::Cancelled);
    let items = vec![root, call, approval, result, correction, terminal];
    let entries = scoped_entries(&items, &scope);

    let projection = project_corrections(&scope, &entries).expect("mixed history projects");

    let positions: Vec<ItemId> = projection
        .iter()
        .map(|view| view.original().item_id)
        .collect();
    assert_eq!(
        positions,
        vec![
            items[0].item_id,
            items[1].item_id,
            items[2].item_id,
            items[3].item_id,
            items[5].item_id,
        ],
        "every non-correction Item keeps its relative position"
    );
    assert_eq!(projection[0].effective_content(), Some("answer"));
    for view in &projection[1..] {
        assert_eq!(view.effective_content(), None);
        assert_eq!(view.source().item_id, view.original().item_id);
    }
    assert_eq!(projection[1].original().payload, items[1].payload);
    assert_eq!(projection[2].original().payload, items[2].payload);
    assert_eq!(projection[3].original().payload, items[3].payload);
}

fn terminal_before_later_corrections_keeps_its_outcome() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let root = user_item(1, "before");
    let terminal = terminal_item(
        2,
        TerminalOutcome::Failed {
            code: "provider_failed".to_owned(),
        },
    );
    let correction = correction_item(3, root.item_id, "after");
    let items = vec![root, terminal, correction];
    let entries = scoped_entries(&items, &scope);

    let projection = project_corrections(&scope, &entries).expect("post-terminal chain projects");

    assert_eq!(projection.len(), 2);
    assert_eq!(
        projection[0].effective_content(),
        Some("after"),
        "EP-05 post-terminal correction applies"
    );
    assert_eq!(
        projection[1].original().payload,
        ItemPayload::Terminal(TerminalOutcome::Failed {
            code: "provider_failed".to_owned()
        }),
        "terminal outcome is never reinterpreted"
    );
    assert_eq!(projection[1].effective_content(), None);
}

fn zero_equal_and_decreasing_sequences_fail_closed() {
    let scope = scope_fixture("tenant-a", "subject-a");
    for sequences in [vec![0, 1], vec![1, 1], vec![2, 1]] {
        let items: Vec<Item> = sequences
            .into_iter()
            .map(|sequence| Item {
                item_id: ItemId::new(),
                sequence,
                payload: ItemPayload::UserMessage {
                    content: "text".to_owned(),
                },
            })
            .collect();
        let before = items.clone();
        let error = project_corrections(&scope, &scoped_entries(&items, &scope))
            .expect_err("sequence fault must reject");
        assert_eq!(
            error,
            ProjectionError::InvalidReplay(RawReplayStructureError::NonIncreasingSequence)
        );
        assert_eq!(
            validate_raw_replay(&items),
            Err(RawReplayStructureError::NonIncreasingSequence)
        );
        assert_eq!(items, before, "failed call leaves input unchanged");
    }
}

fn duplicate_identity_fails_closed() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let shared = ItemId::new();
    let items = vec![
        Item {
            item_id: shared,
            sequence: 1,
            payload: ItemPayload::UserMessage {
                content: "one".to_owned(),
            },
        },
        Item {
            item_id: shared,
            sequence: 2,
            payload: ItemPayload::UserMessage {
                content: "two".to_owned(),
            },
        },
    ];
    let error =
        project_corrections(&scope, &scoped_entries(&items, &scope)).expect_err("must reject");
    assert_eq!(
        error,
        ProjectionError::InvalidReplay(RawReplayStructureError::DuplicateItemIdentity)
    );
    assert_eq!(
        validate_raw_replay(&items),
        Err(RawReplayStructureError::DuplicateItemIdentity)
    );
}

fn correction_target_faults_fail_closed() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let root = user_item(1, "root");
    let absent = correction_item(2, ItemId::new(), "orphan");
    let items = vec![root, absent];
    let error =
        project_corrections(&scope, &scoped_entries(&items, &scope)).expect_err("must reject");
    assert_eq!(
        error,
        ProjectionError::InvalidReplay(RawReplayStructureError::UnknownCorrectionTarget)
    );
    assert_eq!(
        validate_raw_replay(&items),
        Err(RawReplayStructureError::UnknownCorrectionTarget)
    );
}

fn branched_and_self_edges_fail_closed() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let root = user_item(1, "root");
    let first = correction_item(2, root.item_id, "one");
    let second = correction_item(3, root.item_id, "two");
    let branched = vec![root.clone(), first, second];
    let error = project_corrections(&scope, &scoped_entries(&branched, &scope))
        .expect_err("branch must reject");
    assert_eq!(
        error,
        ProjectionError::InvalidReplay(RawReplayStructureError::DuplicateSuccessor)
    );

    let self_id = ItemId::new();
    let self_correction = Item {
        item_id: self_id,
        sequence: 1,
        payload: ItemPayload::Correction(
            ItemCorrection::new("self", self_id).expect("valid correction"),
        ),
    };
    let error = project_corrections(&scope, &scoped_entries(&[self_correction], &scope))
        .expect_err("self edge must reject");
    assert_eq!(
        error,
        ProjectionError::InvalidReplay(RawReplayStructureError::SelfCorrection)
    );
}

/// Forward edges and two-node or longer cycles pass the raw validator but
/// must fail the ordered ancestry pass (EP-03): every cycle contains a
/// forward edge.
fn forward_edges_and_cycles_fail_closed() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let earlier = delta_item(1, "earlier");
    let late_target = delta_item(3, "later");
    let forward = correction_item(2, late_target.item_id, "jumps ahead");
    let forward_items = vec![earlier, forward, late_target];
    assert_eq!(
        validate_raw_replay(&forward_items),
        Ok(()),
        "raw pass judges structure only"
    );
    assert_eq!(
        project_corrections(&scope, &scoped_entries(&forward_items, &scope)),
        Err(ProjectionError::ForwardReference)
    );

    let one = ItemId::new();
    let two = ItemId::new();
    let first_link = Item {
        item_id: one,
        sequence: 1,
        payload: correction_payload(two, "first"),
    };
    let second_link = Item {
        item_id: two,
        sequence: 2,
        payload: correction_payload(one, "second"),
    };
    let two_cycle = vec![first_link, second_link];
    assert_eq!(validate_raw_replay(&two_cycle), Ok(()));
    assert_eq!(
        project_corrections(&scope, &scoped_entries(&two_cycle, &scope)),
        Err(ProjectionError::ForwardReference)
    );

    let one = ItemId::new();
    let two = ItemId::new();
    let three = ItemId::new();
    let first_link = Item {
        item_id: one,
        sequence: 1,
        payload: correction_payload(two, "first"),
    };
    let second_link = Item {
        item_id: two,
        sequence: 2,
        payload: correction_payload(three, "second"),
    };
    let third_link = Item {
        item_id: three,
        sequence: 3,
        payload: correction_payload(one, "third"),
    };
    let longer_cycle = vec![first_link, second_link, third_link];
    assert_eq!(validate_raw_replay(&longer_cycle), Ok(()));
    assert_eq!(
        project_corrections(&scope, &scoped_entries(&longer_cycle, &scope)),
        Err(ProjectionError::ForwardReference),
        "a longer cycle contains a rejected forward edge"
    );
}

fn every_non_text_kind_is_an_unsupported_root() {
    let scope = scope_fixture("tenant-a", "subject-a");
    for payload_fixture in [
        usage_item(1),
        approval_item(1),
        tool_call_item(1),
        tool_result_item(1),
        terminal_item(1, TerminalOutcome::Interrupted),
    ] {
        let target_id = payload_fixture.item_id;
        let correction = correction_item(2, target_id, "rewrite");
        let items = vec![payload_fixture, correction];
        assert_eq!(
            validate_raw_replay(&items),
            Ok(()),
            "structure alone is valid"
        );
        assert_eq!(
            project_corrections(&scope, &scoped_entries(&items, &scope)),
            Err(ProjectionError::UnsupportedRoot),
            "every non-text kind is an unsupported root"
        );
    }
}

fn valid_prefix_corruption_returns_no_partial_output() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let root = user_item(1, "root");
    let valid = correction_item(2, root.item_id, "fixed");
    let late_target = delta_item(4, "late");
    let forward = correction_item(3, late_target.item_id, "forward");
    let items = vec![root, valid, forward, late_target];
    let before = items.clone();
    let entries = scoped_entries(&items, &scope);
    let result = project_corrections(&scope, &entries);

    assert_eq!(result, Err(ProjectionError::ForwardReference));
    assert_eq!(items, before, "no partial output and unchanged input");
}

fn raw_faults_precede_ancestry_faults() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let earlier = delta_item(2, "earlier");
    let correction = correction_item(3, earlier.item_id, "forward-free");
    let decreasing = Item {
        item_id: ItemId::new(),
        sequence: 1,
        payload: ItemPayload::UserMessage {
            content: "breaks order".to_owned(),
        },
    };
    let items = vec![earlier, correction, decreasing];
    let error =
        project_corrections(&scope, &scoped_entries(&items, &scope)).expect_err("must reject");
    assert_eq!(
        error,
        ProjectionError::InvalidReplay(RawReplayStructureError::NonIncreasingSequence),
        "EP-03: raw structure precedes the ordered ancestry pass"
    );
}

fn every_scope_component_drift_reports_the_first_mismatch_index() {
    let shared_thread = ThreadId::new();
    let shared_turn = TurnId::new();
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let local = ProjectionScope::new(tenant.clone(), "subject-a", shared_thread, shared_turn)
        .expect("valid scope");
    let root = user_item(1, "local");
    let items = vec![root];
    let entries = scoped_entries(&items, &local);
    let drifted = [
        scope_fixture("tenant-b", "subject-a"),
        ProjectionScope::new(tenant, "subject-b", shared_thread, shared_turn).expect("valid scope"),
        ProjectionScope::new(
            TenantId::new("tenant-a").expect("valid tenant"),
            "subject-a",
            ThreadId::new(),
            shared_turn,
        )
        .expect("valid scope"),
        ProjectionScope::new(
            TenantId::new("tenant-a").expect("valid tenant"),
            "subject-a",
            shared_thread,
            TurnId::new(),
        )
        .expect("valid scope"),
    ];
    for expected in drifted {
        assert_eq!(
            project_corrections(&expected, &entries),
            Err(ProjectionError::ScopeMismatch { index: 0 }),
            "EP-01 first mismatching entry at index 0"
        );
    }
}

fn mixed_foreign_entry_reports_its_position_before_raw_faults() {
    let local = scope_fixture("tenant-a", "subject-a");
    let foreign = scope_fixture("tenant-a", "subject-b");
    let first = user_item(1, "first");
    let second = user_item(2, "second");
    let intruder = user_item(9, "intruder");
    let decreasing = Item {
        item_id: ItemId::new(),
        sequence: 3,
        payload: ItemPayload::UserMessage {
            content: "breaks order".to_owned(),
        },
    };
    let items = [first, second, intruder, decreasing];
    let entries = vec![
        ScopedProjectionItem::new(&items[0], &local),
        ScopedProjectionItem::new(&items[1], &local),
        ScopedProjectionItem::new(&items[2], &foreign),
        ScopedProjectionItem::new(&items[3], &local),
    ];

    assert_eq!(
        project_corrections(&local, &entries),
        Err(ProjectionError::ScopeMismatch { index: 2 }),
        "EP-01 scope pass precedes structural validation"
    );
}

fn separate_scopes_stay_independent_and_recover() {
    let scope_one = scope_fixture("tenant-a", "subject-a");
    let scope_two = scope_fixture("tenant-a", "subject-b");
    let shared_identity = ItemId::new();
    let history_one = vec![
        Item {
            item_id: shared_identity,
            sequence: 1,
            payload: ItemPayload::UserMessage {
                content: "one".to_owned(),
            },
        },
        Item {
            item_id: ItemId::new(),
            sequence: 2,
            payload: ItemPayload::UserMessage {
                content: "tail".to_owned(),
            },
        },
    ];
    let history_two = vec![Item {
        item_id: shared_identity,
        sequence: 1,
        payload: ItemPayload::AgentMessageDelta {
            content: "two".to_owned(),
        },
    }];

    let first_entries = scoped_entries(&history_one, &scope_one);
    let first = project_corrections(&scope_one, &first_entries).expect("scope one projects");
    let second_entries = scoped_entries(&history_two, &scope_two);
    let second = project_corrections(&scope_two, &second_entries).expect("scope two projects");
    assert_eq!(first.len(), 2);
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].effective_content(), Some("two"));
}

fn foreign_only_target_is_never_borrowed() {
    let local = scope_fixture("tenant-a", "subject-a");
    let other = scope_fixture("tenant-a", "subject-b");
    let local_root = user_item(1, "local root");
    let foreign_root = user_item(1, "foreign root");
    let dangling = correction_item(2, foreign_root.item_id, "steals");
    let local_items = vec![local_root, dangling];
    let foreign_items = vec![foreign_root];

    assert_eq!(
        project_corrections(&local, &scoped_entries(&local_items, &local)),
        Err(ProjectionError::InvalidReplay(
            RawReplayStructureError::UnknownCorrectionTarget
        )),
        "a target outside the supplied scope is never recovered"
    );

    let recovered = correction_item(3, local_items[0].item_id, "local fix");
    let healed = vec![local_items[0].clone(), recovered];
    let healed_entries = scoped_entries(&healed, &local);
    let projection = project_corrections(&local, &healed_entries).expect("valid retry");
    assert_eq!(projection[0].effective_content(), Some("local fix"));
    let foreign_entries = scoped_entries(&foreign_items, &other);
    let foreign_view = project_corrections(&other, &foreign_entries).expect("other scope");
    assert_eq!(foreign_view[0].effective_content(), Some("foreign root"));
}

fn error_diagnostics_carry_no_content_or_scope_text() {
    let content_sentinel = "SENTINEL-CONTENT-007";
    let replacement_sentinel = "SENTINEL-REPLACEMENT-008";
    let tenant_sentinel = "SENTINEL-TENANT-009";
    let subject_sentinel = "SENTINEL-SUBJECT-010";
    let local = scope_fixture(tenant_sentinel, subject_sentinel);
    let foreign = scope_fixture("tenant-x", "subject-x");
    let root = user_item(1, content_sentinel);
    let orphan = correction_item(4, ItemId::new(), replacement_sentinel);
    let usage_target = usage_item(1);
    let rewrite = correction_item(2, usage_target.item_id, replacement_sentinel);

    let scope_fault = {
        let items = vec![root.clone()];
        project_corrections(&foreign, &scoped_entries(&items, &local)).expect_err("scope fault")
    };
    let raw_fault = {
        let items = vec![root.clone(), orphan];
        project_corrections(&local, &scoped_entries(&items, &local)).expect_err("raw fault")
    };
    let ancestry_fault = {
        let late = delta_item(3, content_sentinel);
        let forward = correction_item(2, late.item_id, replacement_sentinel);
        let items = vec![root, forward, late];
        project_corrections(&local, &scoped_entries(&items, &local)).expect_err("ancestry fault")
    };
    let root_fault = {
        let items = vec![usage_target, rewrite];
        project_corrections(&local, &scoped_entries(&items, &local)).expect_err("root fault")
    };
    assert_eq!(scope_fault, ProjectionError::ScopeMismatch { index: 0 });
    assert_eq!(
        raw_fault,
        ProjectionError::InvalidReplay(RawReplayStructureError::UnknownCorrectionTarget)
    );
    assert_eq!(ancestry_fault, ProjectionError::ForwardReference);
    assert_eq!(root_fault, ProjectionError::UnsupportedRoot);
    for error in [scope_fault, raw_fault, ancestry_fault, root_fault] {
        let display = format!("{error}");
        let debug = format!("{error:?}");
        for sentinel in [
            content_sentinel,
            replacement_sentinel,
            tenant_sentinel,
            subject_sentinel,
        ] {
            assert!(
                !display.contains(sentinel),
                "EP-06 Display redacts {sentinel}"
            );
            assert!(!debug.contains(sentinel), "EP-06 Debug redacts {sentinel}");
        }
    }
}

fn long_chain_substitutes_the_final_tip() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let mut items = Vec::with_capacity(4_097);
    items.push(user_item(1, "origin"));
    let mut previous = items[0].item_id;
    for index in 1..=4_096_u64 {
        let item = correction_item(index + 1, previous, &format!("correction-{index:04}"));
        previous = item.item_id;
        items.push(item);
    }
    let before = items.clone();
    let entries = scoped_entries(&items, &scope);

    let projection = project_corrections(&scope, &entries).expect("4,097-node chain projects");

    assert_eq!(projection.len(), 1, "n minus corrections outputs");
    let view = &projection[0];
    assert_eq!(view.original().item_id, items[0].item_id);
    let tip = items.last().expect("chain tip");
    assert_eq!(view.source().item_id, tip.item_id);
    assert_eq!(view.effective_content(), Some("correction-4096"));
    let tip_text = correction_content(tip);
    assert!(std::ptr::eq(
        view.effective_content().expect("text").as_ptr(),
        tip_text.as_ptr()
    ));
    assert_eq!(items, before);
}

fn many_independent_chains_borrow_sources_above_one_mib() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let roots = 4_096_usize;
    let mut items = Vec::with_capacity(roots * 2);
    for index in 0..roots {
        let filler = "x".repeat(148);
        let root = user_item(2 * index as u64 + 1, &format!("root-{index:04}-{filler}"));
        let fix = correction_item(
            2 * index as u64 + 2,
            root.item_id,
            &format!("fix-{index:04}-{filler}"),
        );
        items.push(root);
        items.push(fix);
    }
    let total_text: usize = items
        .iter()
        .map(|item| match &item.payload {
            ItemPayload::UserMessage { content } | ItemPayload::AgentMessageDelta { content } => {
                content.len()
            }
            ItemPayload::Correction(correction) => correction.content().len(),
            _ => 0,
        })
        .sum();
    assert!(
        total_text > 1_048_576,
        "fixture carries more than 1 MiB of source text"
    );
    let before = items.clone();
    let entries = scoped_entries(&items, &scope);

    let projection = project_corrections(&scope, &entries).expect("independent chains project");

    assert_eq!(projection.len(), roots, "n minus corrections outputs");
    for (index, view) in projection.iter().enumerate() {
        assert_eq!(view.original().item_id, items[2 * index].item_id);
        assert_eq!(view.source().item_id, items[2 * index + 1].item_id);
    }
    let sampled = projection.len() - 1;
    let tip_text = correction_content(&items[2 * sampled + 1]);
    assert!(std::ptr::eq(
        projection[sampled]
            .effective_content()
            .expect("text")
            .as_ptr(),
        tip_text.as_ptr()
    ));
    assert_eq!(items, before);
}

fn repeated_and_parallel_calls_share_immutable_input() {
    let scope = scope_fixture("tenant-a", "subject-a");
    let roots = 64_usize;
    let mut items = Vec::with_capacity(roots * 2);
    for index in 0..roots {
        let root = user_item(2 * index as u64 + 1, &format!("root-{index:04}"));
        let fix = correction_item(
            2 * index as u64 + 2,
            root.item_id,
            &format!("fix-{index:04}"),
        );
        items.push(root);
        items.push(fix);
    }
    let before = items.clone();
    let entries = scoped_entries(&items, &scope);
    let baseline = project_corrections(&scope, &entries).expect("baseline projects");
    let repeated = project_corrections(&scope, &entries).expect("repeat projects");
    assert_eq!(baseline, repeated, "EP-06 repeated calls are equal");

    let barrier = Barrier::new(4);
    let outcomes = std::thread::scope(|scoped| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                scoped.spawn(|| {
                    barrier.wait();
                    project_corrections(&scope, &entries).expect("parallel call projects")
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("no panic"))
            .collect::<Vec<_>>()
    });
    for outcome in outcomes {
        assert_eq!(outcome, baseline, "EP-06 parallel outcomes equal");
    }
    assert_eq!(items, before, "shared immutable input unchanged");
}
