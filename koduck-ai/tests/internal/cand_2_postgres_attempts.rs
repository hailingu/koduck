// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! D-7 dispatch-claim and terminal-commit legs of the canonical `PostgreSQL`
//! harness (ADR-0003 AC-12, TC-12/TC-09).

use std::sync::Arc;
use std::sync::Barrier;

use koduck_ai::adapters::execution::SqlxExecutionAttemptStore;
use koduck_ai::application::{
    AttemptInsertResolution, AttemptStoreError, AttemptTerminalResolution,
    CanonicalAttemptTerminal, DispatchClaimResolution, DurableAttemptTerminal, EffectState,
    ExecutionAttemptStore, ExecutionFailure, ToolExecutionOutcome,
};
use koduck_ai::domain::execution::{AttemptId, ExactActionBinding, ExecutionStatus};
use koduck_ai::domain::tool::{Action, Effect};
use koduck_ai::domain::{LeaseGeneration, TenantId};
use uuid::Uuid;

use super::harness;

/// One canonical tuple probe: (`status`, `started_at`, `effect_state`,
/// `failure_code`, `output`, `terminal_at`, `version`).
type IllegalTuple<'a> = (
    &'a str,
    Option<i64>,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a [u8]>,
    Option<i64>,
    i64,
);

pub(super) fn prepared_binding(effect: Effect) -> ExactActionBinding {
    ExactActionBinding::new(
        TenantId::new(format!("ci-{}", Uuid::new_v4())).expect("valid tenant"),
        koduck_ai::domain::ThreadId::new(),
        koduck_ai::domain::TurnId::new(),
        LeaseGeneration::initial(),
        ("profile-default", "v1"),
        AttemptId::new(),
        Action::new(
            "fixture.tool",
            "v1",
            effect,
            "fixture-target",
            koduck_ai::adapters::tool::parse_action_parameters(r#"{"value":1}"#)
                .expect("valid parameters"),
        )
        .expect("valid action"),
    )
    .expect("valid binding")
}

pub(super) fn attempt_store(
    pool: sqlx::PgPool,
    runtime: &tokio::runtime::Runtime,
) -> SqlxExecutionAttemptStore {
    SqlxExecutionAttemptStore::new(pool, runtime.handle().clone())
}

/// Creates the canonical C-6 ownership rows required by a durable D-7
/// preparation, claim, or terminal transition. D-7 rows intentionally have no
/// foreign key to the lease table, so persistence tests must seed ownership
/// explicitly rather than depending on the old missing-lease fail-open
/// behavior.
pub(super) fn seed_owner_rows(
    harness: &super::Harness,
    tenant_id: &TenantId,
    thread_id: koduck_ai::domain::ThreadId,
    turn_id: koduck_ai::domain::TurnId,
    lease_generation: LeaseGeneration,
) {
    harness.runtime.block_on(async {
        sqlx::query(
            "INSERT INTO threads (tenant_id, subject_id, thread_id) \
             VALUES ($1, 'd7-attempt-fixture', $2) ON CONFLICT DO NOTHING",
        )
        .bind(tenant_id.as_str())
        .bind(thread_id.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture thread exists");
        sqlx::query(
            "INSERT INTO turns (tenant_id, thread_id, turn_id, status, next_sequence) \
             VALUES ($1, $2, $3, 'started', 1) ON CONFLICT DO NOTHING",
        )
        .bind(tenant_id.as_str())
        .bind(thread_id.as_uuid())
        .bind(turn_id.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture turn exists");
        sqlx::query(
            "INSERT INTO turn_leases \
             (tenant_id, thread_id, turn_id, generation, renewed_at, expires_at, fenced) \
             VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP, \
                     CURRENT_TIMESTAMP + INTERVAL '1 hour', FALSE) \
             ON CONFLICT DO NOTHING",
        )
        .bind(tenant_id.as_str())
        .bind(thread_id.as_uuid())
        .bind(turn_id.as_uuid())
        .bind(i64::try_from(lease_generation.get()).expect("lease fits i64"))
        .execute(&harness.pool)
        .await
        .expect("fixture current lease exists");
    });
}

/// Seeds the canonical C-6 ownership rows for one binding's exact identity.
fn seed_current_lease(harness: &super::Harness, binding: &ExactActionBinding) {
    seed_owner_rows(
        harness,
        binding.tenant_id(),
        binding.thread_id(),
        binding.turn_id(),
        binding.lease_generation(),
    );
}

/// Seeds authenticated ownership before inserting the canonical prepared D-7.
pub(super) fn insert_prepared(
    harness: &super::Harness,
    store: &mut SqlxExecutionAttemptStore,
    binding: &ExactActionBinding,
    prepared_at_millis: u64,
) -> Result<AttemptInsertResolution, AttemptStoreError> {
    seed_current_lease(harness, binding);
    store.insert_prepared(binding, prepared_at_millis)
}

fn succeeded_terminal() -> DurableAttemptTerminal {
    DurableAttemptTerminal::from_outcome(&ToolExecutionOutcome::Succeeded {
        output: b"committed".to_vec(),
        effect_state: EffectState::Started,
    })
}

/// Builds the bindings that drift exactly one immutable field from the seeded
/// canonical record while keeping its (`tenant_id`, `attempt_id`) identity.
///
/// Reusing an attempt identity with a drifted Thread, Turn, lease generation,
/// action, or profile must never claim or terminalize the canonical D-7 of
/// the unmodified binding (ADR-0003 D-7/TC-12).
fn drifted_bindings(binding: &ExactActionBinding) -> Vec<(&'static str, ExactActionBinding)> {
    let mut bindings = drifted_identity_bindings(binding);
    bindings.extend(drifted_action_bindings(binding));
    bindings
}

/// Rebuilds one binding of `binding`'s (`tenant_id`, `attempt_id`) identity
/// with the given fields.
fn drifted_binding(
    binding: &ExactActionBinding,
    thread: koduck_ai::domain::ThreadId,
    turn: koduck_ai::domain::TurnId,
    lease: LeaseGeneration,
    profile: (&'static str, &'static str),
    action: Action,
) -> ExactActionBinding {
    ExactActionBinding::new(
        binding.tenant_id().clone(),
        thread,
        turn,
        lease,
        profile,
        binding.attempt_id(),
        action,
    )
    .expect("valid drifted binding")
}

/// The Thread, Turn, lease-generation, and profile drifts.
fn drifted_identity_bindings(
    binding: &ExactActionBinding,
) -> Vec<(&'static str, ExactActionBinding)> {
    use koduck_ai::domain::{ThreadId, TurnId};
    let thread = binding.thread_id();
    let turn = binding.turn_id();
    let lease = binding.lease_generation();
    let profile = ("profile-default", "v1");
    let action = || binding.action().clone();
    let drifted_lease = LeaseGeneration::from_persisted(lease.get() + 1).expect("valid lease");
    vec![
        (
            "thread",
            drifted_binding(binding, ThreadId::new(), turn, lease, profile, action()),
        ),
        (
            "turn",
            drifted_binding(binding, thread, TurnId::new(), lease, profile, action()),
        ),
        (
            "lease generation",
            drifted_binding(binding, thread, turn, drifted_lease, profile, action()),
        ),
        (
            "profile ID",
            drifted_binding(
                binding,
                thread,
                turn,
                lease,
                ("profile-drifted", "v1"),
                action(),
            ),
        ),
        (
            "profile version",
            drifted_binding(
                binding,
                thread,
                turn,
                lease,
                ("profile-default", "v2"),
                action(),
            ),
        ),
    ]
}

/// The descriptor ID/version, effect, and action-digest drifts.
fn drifted_action_bindings(
    binding: &ExactActionBinding,
) -> Vec<(&'static str, ExactActionBinding)> {
    let effect = binding.action().effect();
    let act = |descriptor_id, version, effect, target, parameters: &str| {
        Action::new(
            descriptor_id,
            version,
            effect,
            target,
            koduck_ai::adapters::tool::parse_action_parameters(parameters)
                .expect("valid drifted parameters"),
        )
        .expect("valid drifted action")
    };
    let keep = |action: Action| {
        drifted_binding(
            binding,
            binding.thread_id(),
            binding.turn_id(),
            binding.lease_generation(),
            ("profile-default", "v1"),
            action,
        )
    };
    // The effect drift must differ from the seeded record's own effect.
    let drifted_effect = match effect {
        Effect::ExternalWrite => Effect::ReadData,
        _ => Effect::ExternalWrite,
    };
    vec![
        (
            "descriptor ID",
            keep(act(
                "drifted.tool",
                "v1",
                effect,
                "fixture-target",
                r#"{"value":1}"#,
            )),
        ),
        (
            "descriptor version",
            keep(act(
                "fixture.tool",
                "v2",
                effect,
                "fixture-target",
                r#"{"value":1}"#,
            )),
        ),
        (
            "effect",
            keep(act(
                "fixture.tool",
                "v1",
                drifted_effect,
                "fixture-target",
                r#"{"value":1}"#,
            )),
        ),
        (
            "action digest",
            keep(act(
                "fixture.tool",
                "v1",
                effect,
                "drifted-target",
                r#"{"value":2}"#,
            )),
        ),
    ]
}

#[test]
fn dispatch_claim_binds_the_full_immutable_record() {
    let Some(harness) = harness() else {
        return;
    };
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    let binding = prepared_binding(Effect::ReadData);
    assert_eq!(
        insert_prepared(&harness, &mut store, &binding, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );

    // Every single-field drift is a typed identity conflict that changes no
    // state; none may win the conditional prepared -> running transition.
    for (field, drifted) in drifted_bindings(&binding) {
        assert_eq!(
            store.claim_running(&drifted, 2_000),
            Err(AttemptStoreError::IdentityConflict),
            "drifted {field} must not claim another canonical D-7",
        );
    }
    // No drifted claim transitioned the canonical row: the exact binding
    // still wins the claim.
    assert_eq!(
        store.claim_running(&binding, 2_000),
        Ok(DispatchClaimResolution::Claimed { version: 2 }),
    );
}

#[test]
fn terminal_commit_binds_the_full_immutable_record() {
    let Some(harness) = harness() else {
        return;
    };
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    let binding = prepared_binding(Effect::ExternalWrite);
    assert_eq!(
        insert_prepared(&harness, &mut store, &binding, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );
    assert_eq!(
        store.claim_running(&binding, 2_000),
        Ok(DispatchClaimResolution::Claimed { version: 2 }),
    );

    // Every single-field drift is a typed conflict against the running row;
    // none may terminalize the canonical D-7.
    for (field, drifted) in drifted_bindings(&binding) {
        assert_eq!(
            store.commit_terminal(&drifted, &succeeded_terminal(), 3_000),
            Ok(AttemptTerminalResolution::Conflict),
            "drifted {field} must not terminalize another canonical D-7",
        );
    }
    // The canonical row is still running: the exact binding wins the
    // terminal, and a drifted replay against the committed terminal is the
    // same typed conflict.
    assert_eq!(
        store.commit_terminal(&binding, &succeeded_terminal(), 3_000),
        Ok(AttemptTerminalResolution::Won { version: 3 }),
    );
    let (_, drifted) = drifted_bindings(&binding)
        .into_iter()
        .next()
        .expect("one drifted binding");
    assert_eq!(
        store.commit_terminal(&drifted, &succeeded_terminal(), 4_000),
        Ok(AttemptTerminalResolution::Conflict),
    );
}

#[test]
fn terminal_commit_without_a_current_lease_does_not_win() {
    use sqlx::Row as _;

    let Some(harness) = harness() else {
        return;
    };
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    let binding = prepared_binding(Effect::ExternalWrite);
    // The D-7 migration intentionally has no FK to `turn_leases`; model the
    // durable orphan directly so the terminal guard must fail closed rather
    // than treating an absent lease as current.
    harness.runtime.block_on(async {
        sqlx::query(
            "INSERT INTO tool_execution_attempts (
                 tenant_id, attempt_id, thread_id, turn_id, lease_generation,
                 descriptor_id, descriptor_version, effect, action_digest,
                 profile_id, profile_version, prepared_at_millis, status, version
             ) VALUES (
                 $1, $2, $3, $4, $5, $6, $7, 'external_write', $8, $9, $10, 1000,
                 'prepared', 1
             )",
        )
        .bind(binding.tenant_id().as_str())
        .bind(binding.attempt_id().as_uuid())
        .bind(binding.thread_id().as_uuid())
        .bind(binding.turn_id().as_uuid())
        .bind(i64::try_from(binding.lease_generation().get()).expect("lease fits i64"))
        .bind(binding.action().descriptor_id())
        .bind(binding.action().descriptor_version())
        .bind(format!("{:x}", binding.action_digest()))
        .bind(binding.profile_id())
        .bind(binding.profile_version())
        .execute(&harness.pool)
        .await
        .expect("orphan prepared attempt exists");
        sqlx::query(
            "UPDATE tool_execution_attempts \
             SET status = 'running', started_at_millis = 2000, version = 2 \
             WHERE tenant_id = $1 AND attempt_id = $2",
        )
        .bind(binding.tenant_id().as_str())
        .bind(binding.attempt_id().as_uuid())
        .execute(&harness.pool)
        .await
        .expect("orphan attempt becomes running");
    });

    assert_eq!(
        store.commit_terminal(&binding, &succeeded_terminal(), 3_000),
        Ok(AttemptTerminalResolution::Fenced),
        "a D-7 without a current lease must not commit a model-visible terminal",
    );
    let row = harness.runtime.block_on(async {
        sqlx::query(
            "SELECT status, version FROM tool_execution_attempts \
             WHERE tenant_id = $1 AND attempt_id = $2",
        )
        .bind(binding.tenant_id().as_str())
        .bind(binding.attempt_id().as_uuid())
        .fetch_one(&harness.pool)
        .await
        .expect("orphan attempt remains readable")
    });
    assert_eq!(
        (
            row.try_get::<String, _>("status").expect("status"),
            row.try_get::<i64, _>("version").expect("version")
        ),
        ("running".to_owned(), 2),
    );
}

#[test]
fn prepared_cancellation_requires_not_started_effect_state() {
    let Some(harness) = harness() else {
        return;
    };
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    // A still-prepared D-7 may close only as cancelled/not_started: a
    // cancellation reporting started or unknown effect evidence is legal only
    // from the won running claim (ADR-0003 D-7 transitions).
    for effect_state in [EffectState::Started, EffectState::Unknown] {
        let binding = prepared_binding(Effect::ReadData);
        assert_eq!(
            insert_prepared(&harness, &mut store, &binding, 1_000),
            Ok(AttemptInsertResolution::Inserted),
        );
        let impossible =
            DurableAttemptTerminal::from_outcome(&ToolExecutionOutcome::Cancelled { effect_state });
        assert_eq!(
            store.commit_terminal(&binding, &impossible, 2_000),
            Ok(AttemptTerminalResolution::Conflict),
            "prepared cancellation with effect state {effect_state:?} must not commit",
        );
        // The row remains prepared: the truthful not_started cancellation
        // still wins the transition.
        let truthful = DurableAttemptTerminal::from_outcome(&ToolExecutionOutcome::Cancelled {
            effect_state: EffectState::NotStarted,
        });
        assert_eq!(
            store.commit_terminal(&binding, &truthful, 3_000),
            Ok(AttemptTerminalResolution::Won { version: 3 }),
        );
    }
}

#[test]
fn terminal_legality_mirrors_the_prepared_cancellation_invariant() {
    // The port-level legality predicate mirrors the durable invariant without
    // a database: a still-prepared cancellation is legal only with
    // not_started effect evidence, and every non-cancellation terminal
    // requires the won running claim.
    let not_started = DurableAttemptTerminal::from_outcome(&ToolExecutionOutcome::Cancelled {
        effect_state: EffectState::NotStarted,
    });
    assert!(not_started.legal_from(ExecutionStatus::Prepared));
    assert!(not_started.legal_from(ExecutionStatus::Running));
    for effect_state in [EffectState::Started, EffectState::Unknown] {
        let terminal =
            DurableAttemptTerminal::from_outcome(&ToolExecutionOutcome::Cancelled { effect_state });
        assert!(!terminal.legal_from(ExecutionStatus::Prepared));
        assert!(terminal.legal_from(ExecutionStatus::Running));
    }
    assert!(!succeeded_terminal().legal_from(ExecutionStatus::Prepared));
    assert!(succeeded_terminal().legal_from(ExecutionStatus::Running));
    assert!(!not_started.legal_from(ExecutionStatus::Succeeded));
}

/// Builds two prepared D-7 bindings of one fresh Turn.
fn sibling_bindings(
    action: &koduck_ai::domain::tool::Action,
) -> (ExactActionBinding, ExactActionBinding) {
    let tenant = TenantId::new(format!("ci-{}", Uuid::new_v4())).expect("valid tenant");
    let thread = koduck_ai::domain::ThreadId::new();
    let turn = koduck_ai::domain::TurnId::new();
    let binding = |attempt_id| {
        ExactActionBinding::new(
            tenant.clone(),
            thread,
            turn,
            LeaseGeneration::initial(),
            ("profile-default", "v1"),
            attempt_id,
            action.clone(),
        )
        .expect("valid binding")
    };
    (binding(AttemptId::new()), binding(AttemptId::new()))
}

#[test]
fn concurrently_racing_claims_of_one_turn_never_report_unavailable() {
    let Some(harness) = harness() else {
        return;
    };
    let action = koduck_ai::domain::tool::Action::new(
        "fixture.tool",
        "v1",
        Effect::ReadData,
        "fixture-target",
        koduck_ai::adapters::tool::parse_action_parameters(r#"{"value":1}"#)
            .expect("valid parameters"),
    )
    .expect("valid action");
    // Each round races two prepared D-7 identities of one fresh Turn, so no
    // prior running row exists and both claims genuinely contend the Turn's
    // single running slot: the loser must observe the typed Concurrent
    // rejection — never an Unavailability masquerading as a store outage
    // (ADR-0003 TC-09/TC-12).
    for round in 0..50 {
        let (first, second) = sibling_bindings(&action);
        let mut seeder = attempt_store(harness.pool.clone(), &harness.runtime);
        assert_eq!(
            insert_prepared(&harness, &mut seeder, &first, 1_000),
            Ok(AttemptInsertResolution::Inserted),
        );
        assert_eq!(
            insert_prepared(&harness, &mut seeder, &second, 1_000),
            Ok(AttemptInsertResolution::Inserted),
        );
        let barrier = Arc::new(Barrier::new(2));
        let outcomes = std::thread::scope(|scope| {
            let handles: Vec<_> = [&first, &second]
                .into_iter()
                .map(|binding| {
                    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
                    let barrier = Arc::clone(&barrier);
                    let binding = binding.clone();
                    scope.spawn(move || {
                        barrier.wait();
                        store
                            .claim_running(&binding, 2_000)
                            .expect("racing claim completes")
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("claim thread completes"))
                .collect::<Vec<_>>()
        });
        let claimed = outcomes
            .iter()
            .filter(|outcome| matches!(outcome, DispatchClaimResolution::Claimed { .. }))
            .count();
        assert_eq!(claimed, 1, "round {round}: exactly one claim wins");
        for outcome in &outcomes {
            assert!(
                matches!(
                    outcome,
                    DispatchClaimResolution::Concurrent | DispatchClaimResolution::Claimed { .. }
                ),
                "round {round}: a lost claim must be the typed Concurrent rejection, not {outcome:?}"
            );
        }
    }
}

#[test]
fn prepared_insert_is_idempotent_and_identity_typed() {
    let Some(harness) = harness() else {
        return;
    };
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    let binding = prepared_binding(Effect::ReadData);

    assert_eq!(
        insert_prepared(&harness, &mut store, &binding, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );
    // Lost-acknowledgement replay reconciles as the canonical prepared row.
    assert_eq!(
        insert_prepared(&harness, &mut store, &binding, 1_000),
        Ok(AttemptInsertResolution::Existing {
            status: ExecutionStatus::Prepared,
            version: 1,
        }),
    );
    // The same identity with drifted immutable fields is a typed conflict
    // that changes no state.
    let drifted = ExactActionBinding::new(
        binding.tenant_id().clone(),
        binding.thread_id(),
        koduck_ai::domain::TurnId::new(),
        binding.lease_generation(),
        ("profile-default", "v1"),
        binding.attempt_id(),
        binding.action().clone(),
    )
    .expect("valid drifted binding");
    assert_eq!(
        insert_prepared(&harness, &mut store, &drifted, 1_000),
        Err(AttemptStoreError::IdentityConflict),
    );

    // Claim and terminate the attempt, then replay the insert: the canonical
    // projection reports the terminal, not the superseded prepared view.
    assert_eq!(
        store.claim_running(&binding, 2_000),
        Ok(DispatchClaimResolution::Claimed { version: 2 }),
    );
    assert_eq!(
        store.commit_terminal(&binding, &succeeded_terminal(), 3_000),
        Ok(AttemptTerminalResolution::Won { version: 3 }),
    );
    assert_eq!(
        insert_prepared(&harness, &mut store, &binding, 1_000),
        Ok(AttemptInsertResolution::Existing {
            status: ExecutionStatus::Succeeded,
            version: 3,
        }),
    );

    // Unknown and cross-tenant identities expose no attempt existence.
    let unknown = prepared_binding(Effect::ReadData);
    assert_eq!(
        store.claim_running(&unknown, 2_000),
        Ok(DispatchClaimResolution::NotFound),
    );
    assert_eq!(
        store.commit_terminal(&unknown, &succeeded_terminal(), 3_000),
        Ok(AttemptTerminalResolution::NotFound),
    );
}

#[test]
fn prepared_insert_replay_survives_a_later_owner_fence() {
    // A committed insert whose acknowledgement was lost remains readable for
    // reconciliation even when the C-6 owner is later fenced: the replay is a
    // read of the immutable canonical row, not a new allocation.
    let Some(harness) = harness() else {
        return;
    };
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    let binding = prepared_binding(Effect::ReadData);
    assert_eq!(
        insert_prepared(&harness, &mut store, &binding, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_leases SET fenced = TRUE \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(binding.tenant_id().as_str())
        .bind(binding.thread_id().as_uuid())
        .bind(binding.turn_id().as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture lease is fenced");
    });
    assert_eq!(
        store.insert_prepared(&binding, 1_000),
        Ok(AttemptInsertResolution::Existing {
            status: ExecutionStatus::Prepared,
            version: 1,
        }),
        "a fenced owner must not hide a previously committed attempt"
    );
}

#[test]
fn prepared_insert_requires_a_current_lease() {
    let Some(harness) = harness() else {
        return;
    };
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    let binding = prepared_binding(Effect::ExternalWrite);

    // A D-7 preparation is not allowed to create an orphan durable row. The
    // prior implementation inserted this row even though its C-6 ownership
    // never existed, leaving interruption unable to close it.
    assert_eq!(
        store.insert_prepared(&binding, 1_000),
        Err(AttemptStoreError::Unavailable),
    );
}

#[test]
fn prepared_insert_rejects_the_seventeenth_attempt_for_one_turn() {
    let Some(harness) = harness() else {
        return;
    };
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    let first = prepared_binding(Effect::ExternalWrite);
    seed_current_lease(&harness, &first);

    for _ in 0..16 {
        let binding = ExactActionBinding::new(
            first.tenant_id().clone(),
            first.thread_id(),
            first.turn_id(),
            first.lease_generation(),
            (first.profile_id(), first.profile_version()),
            AttemptId::new(),
            first.action().clone(),
        )
        .expect("valid sibling D-7 binding");
        assert_eq!(
            store.insert_prepared(&binding, 1_000),
            Ok(AttemptInsertResolution::Inserted),
        );
    }

    let seventeenth = ExactActionBinding::new(
        first.tenant_id().clone(),
        first.thread_id(),
        first.turn_id(),
        first.lease_generation(),
        (first.profile_id(), first.profile_version()),
        AttemptId::new(),
        first.action().clone(),
    )
    .expect("valid seventeenth D-7 binding");
    assert_eq!(
        store.insert_prepared(&seventeenth, 1_000),
        Err(AttemptStoreError::AttemptLimit),
    );
}

#[test]
fn dispatch_claim_and_terminal_commit_are_single_winner() {
    let Some(harness) = harness() else {
        return;
    };
    let binding = prepared_binding(Effect::ExternalWrite);
    let mut seeder = attempt_store(harness.pool.clone(), &harness.runtime);
    assert_eq!(
        insert_prepared(&harness, &mut seeder, &binding, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );

    let contenders = 32;
    let barrier = Arc::new(Barrier::new(contenders));
    let mut handles = Vec::new();
    for _ in 0..contenders {
        let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
        let barrier = Arc::clone(&barrier);
        let binding = binding.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            store
                .claim_running(&binding, 2_000)
                .expect("contender claim completes")
        }));
    }
    let mut claimed = 0;
    let mut existing = 0;
    for handle in handles {
        match handle.join().expect("claim thread completes") {
            DispatchClaimResolution::Claimed { version } => {
                assert_eq!(version, 2);
                claimed += 1;
            }
            DispatchClaimResolution::Existing { status, version } => {
                assert_eq!(status, ExecutionStatus::Running);
                assert_eq!(version, 2);
                existing += 1;
            }
            other => panic!("unexpected claim resolution: {other:?}"),
        }
    }
    assert_eq!(claimed, 1, "exactly one dispatch claim wins");
    assert_eq!(existing, contenders - 1);

    let barrier = Arc::new(Barrier::new(contenders));
    let mut handles = Vec::new();
    for _ in 0..contenders {
        let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
        let barrier = Arc::clone(&barrier);
        let binding = binding.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            store
                .commit_terminal(&binding, &succeeded_terminal(), 3_000)
                .expect("contender terminal completes")
        }));
    }
    let mut won = 0;
    let mut replayed = 0;
    for handle in handles {
        match handle.join().expect("terminal thread completes") {
            AttemptTerminalResolution::Won { version } => {
                assert_eq!(version, 3);
                won += 1;
            }
            AttemptTerminalResolution::ExistingTerminal(canonical) => {
                assert_eq!(canonical.binding(), &binding);
                assert_eq!(canonical.version(), 3);
                assert_eq!(
                    canonical.outcome(),
                    &ToolExecutionOutcome::Succeeded {
                        output: b"committed".to_vec(),
                        effect_state: EffectState::Started,
                    },
                );
                replayed += 1;
            }
            other => panic!("unexpected terminal resolution: {other:?}"),
        }
    }
    assert_eq!(won, 1, "exactly one terminal commit wins");
    assert_eq!(replayed, contenders - 1);

    // The idempotent replay observes the same single canonical terminal.
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    assert_eq!(
        store.commit_terminal(&binding, &succeeded_terminal(), 4_000),
        Ok(AttemptTerminalResolution::ExistingTerminal(Box::new(
            CanonicalAttemptTerminal::from_persistence(
                binding.clone(),
                3,
                ToolExecutionOutcome::Succeeded {
                    output: b"committed".to_vec(),
                    effect_state: EffectState::Started,
                },
            )
            .expect("canonical terminal validates"),
        ))),
    );
}

#[test]
fn one_running_attempt_per_turn_and_prepared_cancellation_commit() {
    let Some(harness) = harness() else {
        return;
    };
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    // Two D-7 identities for one Turn.
    let first = ExactActionBinding::new(
        TenantId::new(format!("ci-{}", Uuid::new_v4())).expect("valid tenant"),
        koduck_ai::domain::ThreadId::new(),
        koduck_ai::domain::TurnId::new(),
        LeaseGeneration::initial(),
        ("profile-default", "v1"),
        AttemptId::new(),
        Action::new(
            "fixture.tool",
            "v1",
            Effect::ReadData,
            "fixture-target",
            koduck_ai::adapters::tool::parse_action_parameters(r#"{"value":1}"#)
                .expect("valid parameters"),
        )
        .expect("valid action"),
    )
    .expect("valid binding");
    let second = ExactActionBinding::new(
        first.tenant_id().clone(),
        first.thread_id(),
        first.turn_id(),
        first.lease_generation(),
        ("profile-default", "v1"),
        AttemptId::new(),
        first.action().clone(),
    )
    .expect("valid sibling binding");
    assert_eq!(
        insert_prepared(&harness, &mut store, &first, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );
    assert_eq!(
        insert_prepared(&harness, &mut store, &second, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );

    assert_eq!(
        store.claim_running(&first, 2_000),
        Ok(DispatchClaimResolution::Claimed { version: 2 }),
    );
    // The durable boundary keeps the Turn's single running slot: a second
    // prepared D-7 of the same Turn cannot claim while the first runs.
    assert_eq!(
        store.claim_running(&second, 2_000),
        Ok(DispatchClaimResolution::Concurrent),
    );

    // A still-prepared D-7 commits cancelled/not_started (declined, cancelled,
    // or expired D-6 path), while a success terminal from prepared state is a
    // conflict because no dispatch claim ever won.
    let cancelled = DurableAttemptTerminal::from_outcome(&ToolExecutionOutcome::Cancelled {
        effect_state: EffectState::NotStarted,
    });
    assert_eq!(
        store.commit_terminal(&second, &cancelled, 3_000),
        Ok(AttemptTerminalResolution::Won { version: 3 }),
    );
    let third = ExactActionBinding::new(
        first.tenant_id().clone(),
        first.thread_id(),
        first.turn_id(),
        first.lease_generation(),
        ("profile-default", "v1"),
        AttemptId::new(),
        first.action().clone(),
    )
    .expect("valid sibling binding");
    assert_eq!(
        insert_prepared(&harness, &mut store, &third, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );
    assert_eq!(
        store.commit_terminal(&third, &succeeded_terminal(), 3_000),
        Ok(AttemptTerminalResolution::Conflict),
    );
    // The failed terminal retains its stable failure code and effect state.
    let failed = DurableAttemptTerminal::from_outcome(&ToolExecutionOutcome::Failed {
        code: ExecutionFailure::OutputLimitExceeded,
        effect_state: EffectState::Unknown,
    });
    assert_eq!(
        store.commit_terminal(&first, &failed, 4_000),
        Ok(AttemptTerminalResolution::Won { version: 3 }),
    );
}

/// Builds the parameterized insert for one canonical tuple probe: every
/// column outside the probed tuple is a legal literal, so acceptance or
/// rejection is decided by the probed values alone.
fn illegal_tuple_insert() -> &'static str {
    "
        INSERT INTO tool_execution_attempts (
            tenant_id, attempt_id, thread_id, turn_id, lease_generation,
            descriptor_id, descriptor_version, effect, action_digest,
            profile_id, profile_version, prepared_at_millis,
            status, started_at_millis, effect_state, failure_code, output,
            terminal_at_millis, version
        ) VALUES (
            'schema-ci', $1, '00000000-0000-0000-0000-000000000001',
            '00000000-0000-0000-0000-000000000002', 1,
            'fixture.tool', 'v1', 'read_data', '00',
            'profile-default', 'v1', 1000,
            $2, $3, $4, $5, $6, $7, $8
        )
    "
}

/// The accepted control tuples: each proves the probe fixture binds every
/// parameter and satisfies the shape CHECK, so every rejection below is
/// decided by its targeted condition alone. Tuple fields are (`status`,
/// `started_at`, `effect_state`, `failure_code`, `output`, `terminal_at`,
/// `version`).
fn legal_terminal_tuples() -> [IllegalTuple<'static>; 2] {
    [
        (
            "succeeded",
            Some(2_000),
            Some("started"),
            None,
            Some(&b"ok"[..]),
            Some(2_000),
            3,
        ),
        // A still-prepared cancellation commits as cancelled/not_started.
        (
            "cancelled",
            None,
            Some("not_started"),
            None,
            None,
            Some(2_000),
            3,
        ),
    ]
}

/// Every tuple below violates exactly one canonical shape invariant of the
/// `tool_execution_attempts` schema while otherwise satisfying its status
/// arm, mirroring the D-6 illegal-terminal table: they must be rejected by
/// the database itself.
fn illegal_terminal_tuples(over_limit_output: &[u8]) -> [IllegalTuple<'_>; 9] {
    [
        // Running requires a started-at timestamp.
        ("running", None, None, None, None, None, 2),
        // A failed terminal requires a non-blank stable failure code.
        (
            "failed",
            Some(2_000),
            Some("unknown"),
            Some("   "),
            None,
            Some(2_000),
            3,
        ),
        // A succeeded terminal requires its committed output.
        (
            "succeeded",
            Some(2_000),
            Some("started"),
            None,
            None,
            Some(2_000),
            3,
        ),
        // Every terminal requires effect-state evidence.
        ("timed_out", Some(2_000), None, None, None, Some(2_000), 3),
        // A success terminal may not carry a failure code.
        (
            "succeeded",
            Some(2_000),
            Some("started"),
            Some("output_limit_exceeded"),
            Some(&b"ok"[..]),
            Some(2_000),
            3,
        ),
        // Succeeded output beyond 1,048,576 bytes violates the bound.
        (
            "succeeded",
            Some(2_000),
            Some("started"),
            None,
            Some(over_limit_output),
            Some(2_000),
            3,
        ),
        // A terminal may not predate the dispatch it terminates.
        (
            "succeeded",
            Some(3_000),
            Some("started"),
            None,
            Some(&b"ok"[..]),
            Some(2_000),
            3,
        ),
        // A cancellation with no started-at timestamp is the still-prepared
        // close and may carry only not_started effect evidence.
        (
            "cancelled",
            None,
            Some("started"),
            None,
            None,
            Some(2_000),
            3,
        ),
        (
            "cancelled",
            None,
            Some("unknown"),
            None,
            None,
            Some(2_000),
            3,
        ),
    ]
}

#[test]
fn schema_rejects_illegal_attempt_tuples() {
    let Some(harness) = harness() else {
        return;
    };
    // Heap allocation: the over-limit leg binds 1,048,577 bytes without a
    // 1 MiB stack array.
    let over_limit_output = vec![0_u8; 1_048_577];
    let legal = legal_terminal_tuples();
    let illegal = illegal_terminal_tuples(&over_limit_output);
    for (label, tuples, expected_accepted) in [
        ("legal", &legal[..], true),
        ("illegal", &illegal[..], false),
    ] {
        for tuple in tuples {
            let (status, started_at, effect_state, failure_code, output, terminal_at, version) =
                tuple;
            let result = harness.runtime.block_on(
                sqlx::query(illegal_tuple_insert())
                    .bind(uuid::Uuid::new_v4())
                    .bind(status)
                    .bind(started_at)
                    .bind(effect_state)
                    .bind(failure_code)
                    .bind(output)
                    .bind(terminal_at)
                    .bind(version)
                    .execute(&harness.pool),
            );
            assert_eq!(
                result.is_ok(),
                expected_accepted,
                "{label} tuple ({status}, {started_at:?}, {effect_state:?}, {failure_code:?}, terminal {terminal_at:?}, version {version:?}) accepted={}",
                result.is_ok()
            );
        }
    }
}

#[test]
fn a_won_terminal_commit_appends_its_audit_record_atomically() {
    // The terminal transition and its correlated audit record commit in one
    // transaction, so a committed D-7 can never permanently lack its durable
    // TC-14 evidence — even if the process exits before any driver-side
    // emission (ADR-0003 TC-14).
    let Some(harness) = harness() else {
        return;
    };
    let binding = prepared_binding(koduck_ai::domain::tool::Effect::ReadData);
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    assert_eq!(
        insert_prepared(&harness, &mut store, &binding, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::claim_running(&mut store, &binding, 2_000),
        Ok(koduck_ai::application::DispatchClaimResolution::Claimed { version: 2 }),
    );
    let terminal = koduck_ai::application::DurableAttemptTerminal::from_outcome(
        &koduck_ai::application::ToolExecutionOutcome::Succeeded {
            output: b"ok".to_vec(),
            effect_state: koduck_ai::application::EffectState::Started,
        },
    );
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::commit_terminal(
            &mut store, &binding, &terminal, 3_000
        ),
        Ok(koduck_ai::application::AttemptTerminalResolution::Won { version: 3 }),
    );
    let audit: Option<String> = harness
        .runtime
        .block_on(async {
            sqlx::query_scalar(
                "SELECT record FROM tool_audit_records \
                 WHERE tenant_id = $1 AND turn_id = $2",
            )
            .bind(binding.tenant_id().as_str())
            .bind(binding.turn_id().as_uuid())
            .fetch_optional(&harness.pool)
            .await
        })
        .expect("audit rows are readable");
    let audit = audit.expect("the won terminal appends its audit record atomically");
    assert!(
        audit.contains(&binding.attempt_id().as_uuid().to_string()),
        "the audit record correlates the attempt"
    );
    assert!(audit.contains("succeeded"), "found {audit}");
}
