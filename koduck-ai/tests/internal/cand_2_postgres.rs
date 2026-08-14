// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Integration harness for canonical D-6 `PostgreSQL` persistence.

use std::sync::Arc;
use std::sync::Barrier;

use koduck_ai::adapters::execution::SqlxApprovalRecordStore;
use koduck_ai::adapters::tool::{parse_action_parameters, parse_input_schema};
use koduck_ai::application::{
    ApprovalDecisionResolution, ApprovalInsertResolution, ApprovalRecordStore, ApprovalStoreError,
    ToolAuthorizationService, ToolPolicyConfiguration,
};
use koduck_ai::domain::execution::{
    ApprovalDecision, ApprovalRequest, ApprovalStatus, AttemptId, ExactActionBinding,
};
use koduck_ai::domain::tool::{
    Action, CapabilityDescriptor, DescriptorState, Effect, PermissionProfile,
};
use koduck_ai::domain::{LeaseGeneration, TenantId};
use sqlx::postgres::{PgPool, PgPoolOptions};
use uuid::Uuid;

struct Harness {
    runtime: tokio::runtime::Runtime,
    pool: PgPool,
    store: SqlxApprovalRecordStore,
}

// Applied once per process: concurrent CREATE TABLE IF NOT EXISTS from
// parallel test sessions can itself race in PostgreSQL.
static MIGRATION: std::sync::OnceLock<()> = std::sync::OnceLock::new();

fn harness() -> Option<Harness> {
    let database_url = std::env::var("KODUCK_AI_TEST_DATABASE_URL").ok()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("PostgreSQL test runtime");
    let pool = runtime
        .block_on(
            PgPoolOptions::new()
                .max_connections(8)
                .connect(&database_url),
        )
        .expect("connect to disposable PostgreSQL");
    MIGRATION.get_or_init(|| {
        runtime
            .block_on(async {
                sqlx::raw_sql(include_str!(
                    "../../migrations/0002_cand_2_policy_execution.sql"
                ))
                .execute(&pool)
                .await
            })
            .expect("apply production migration");
    });
    let store = SqlxApprovalRecordStore::new(pool.clone(), runtime.handle().clone());
    Some(Harness {
        runtime,
        pool,
        store,
    })
}

fn requested_approval(requested_at_millis: u64, turn_deadline_millis: u64) -> ApprovalRequest {
    let binding = ExactActionBinding::new(
        TenantId::new(format!("ci-{}", Uuid::new_v4())).expect("valid tenant"),
        koduck_ai::domain::ThreadId::new(),
        koduck_ai::domain::TurnId::new(),
        LeaseGeneration::initial(),
        ("profile-default", "v1"),
        AttemptId::new(),
        Action::new(
            "fixture.tool",
            "v1",
            Effect::ExternalWrite,
            "fixture-target",
            parse_action_parameters(r#"{"value":1}"#).expect("valid parameters"),
        )
        .expect("valid action"),
    )
    .expect("valid binding");
    let descriptor = CapabilityDescriptor::new(
        "fixture.tool",
        "v1",
        Effect::ExternalWrite,
        DescriptorState::Active,
        parse_input_schema(
            r#"{"type":"object","properties":{"value":{"type":"number"}},"required":["value"],"additionalProperties":false}"#,
        )
        .expect("valid schema"),
    )
    .expect("valid descriptor");
    let profile = PermissionProfile::builder("profile-default", "v1")
        .expect("valid profile")
        .allow(
            "fixture.tool",
            "v1",
            Effect::ExternalWrite,
            "fixture-target",
        )
        .expect("valid profile entry")
        .build();
    let sealed = ToolAuthorizationService::new(FixturePolicyConfiguration {
        descriptor,
        profile,
    })
    .authorize_binding(binding)
    .expect("fixture binding is policy-authorized");
    ApprovalRequest::new(sealed, requested_at_millis, turn_deadline_millis)
        .expect("valid requested approval")
}

fn approver(id: &str) -> koduck_ai::domain::execution::ApproverId {
    let trust = koduck_ai::domain::TrustContext::new(
        TenantId::new("approver-tenant").expect("valid tenant"),
        id,
    )
    .expect("valid principal")
    .with_approval_scopes(koduck_ai::domain::ApprovalScopes::from_validated([
        koduck_ai::application::TOOL_APPROVAL_SCOPE,
    ]));
    koduck_ai::domain::execution::ApproverId::from_authenticated(&trust)
        .expect("scoped principal yields an approver identity")
}

struct FixturePolicyConfiguration {
    descriptor: CapabilityDescriptor,
    profile: PermissionProfile,
}

impl ToolPolicyConfiguration for FixturePolicyConfiguration {
    fn descriptor_for(&self, _action: &Action) -> Option<&CapabilityDescriptor> {
        Some(&self.descriptor)
    }

    fn profile_for(&self, profile_id: &str, profile_version: &str) -> Option<&PermissionProfile> {
        (self.profile.id() == profile_id && self.profile.version() == profile_version)
            .then_some(&self.profile)
    }
}

#[test]
fn migration_is_idempotent_and_decisions_are_single_winner() {
    let Some(mut harness) = harness() else {
        return;
    };
    for _ in 0..2 {
        harness
            .runtime
            .block_on(async {
                sqlx::raw_sql(include_str!(
                    "../../migrations/0002_cand_2_policy_execution.sql"
                ))
                .execute(&harness.pool)
                .await
            })
            .expect("idempotent migration applies repeatedly");
    }

    let approval = requested_approval(1_000, 60_000);
    let tenant = approval.tenant_id().clone();
    assert_eq!(
        harness.store.insert_requested(&approval),
        Ok(ApprovalInsertResolution::Inserted)
    );
    // Lost-acknowledgement replay: the identical immutable record
    // reconciles as already canonical.
    assert_eq!(
        harness.store.insert_requested(&approval),
        Ok(ApprovalInsertResolution::Existing {
            status: ApprovalStatus::Requested,
            decision: None,
            version: 1,
        }),
    );

    let won = harness
        .store
        .resolve_decision(
            approval.approval_id(),
            &tenant,
            ApprovalDecision::Accepted,
            &approver("approver-a"),
            2_000,
        )
        .expect("first decision resolves");
    assert_eq!(
        won,
        ApprovalDecisionResolution::Won {
            decision: ApprovalDecision::Accepted,
            version: 2,
        }
    );

    // An identical replay and a conflicting decision both observe the
    // committed canonical terminal and change no state.
    for decision in [ApprovalDecision::Accepted, ApprovalDecision::Declined] {
        let replay = harness
            .store
            .resolve_decision(
                approval.approval_id(),
                &tenant,
                decision,
                &approver("approver-b"),
                3_000,
            )
            .expect("replay resolves");
        assert_eq!(
            replay,
            ApprovalDecisionResolution::ExistingTerminal {
                decision: Some(ApprovalDecision::Accepted),
                status: ApprovalStatus::Accepted,
                version: 2,
            }
        );
    }

    // Cross-tenant and unknown identities expose no approval existence.
    let other_tenant = TenantId::new(format!("ci-{}", Uuid::new_v4())).expect("valid tenant");
    let cross = harness
        .store
        .resolve_decision(
            approval.approval_id(),
            &other_tenant,
            ApprovalDecision::Accepted,
            &approver("approver-a"),
            2_000,
        )
        .expect("cross-tenant resolve completes");
    assert_eq!(cross, ApprovalDecisionResolution::NotFound);
    let unknown = harness
        .store
        .resolve_decision(
            koduck_ai::domain::execution::ApprovalId::new(),
            &tenant,
            ApprovalDecision::Accepted,
            &approver("approver-a"),
            2_000,
        )
        .expect("unknown resolve completes");
    assert_eq!(unknown, ApprovalDecisionResolution::NotFound);
}

#[test]
fn thirty_two_competing_decisions_commit_exactly_one_terminal() {
    let Some(mut harness) = harness() else {
        return;
    };

    let approval = requested_approval(1_000, 60_000);
    let tenant = approval.tenant_id().clone();
    assert_eq!(
        harness.store.insert_requested(&approval),
        Ok(ApprovalInsertResolution::Inserted)
    );
    // Lost-acknowledgement replay: the identical immutable record
    // reconciles as already canonical.
    assert_eq!(
        harness.store.insert_requested(&approval),
        Ok(ApprovalInsertResolution::Existing {
            status: ApprovalStatus::Requested,
            decision: None,
            version: 1,
        }),
    );

    let contenders = 32;
    let barrier = Arc::new(Barrier::new(contenders));
    let mut handles = Vec::new();
    for index in 0..contenders {
        let mut store = harness.store.clone();
        let barrier = Arc::clone(&barrier);
        let tenant = tenant.clone();
        let approval_id = approval.approval_id();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            store
                .resolve_decision(
                    approval_id,
                    &tenant,
                    ApprovalDecision::Accepted,
                    &approver(&format!("approver-{index}")),
                    2_000,
                )
                .expect("contender decision completes")
        }));
    }
    let mut winners = 0;
    let mut existing = 0;
    for handle in handles {
        match handle.join().expect("contender thread completes") {
            ApprovalDecisionResolution::Won { decision, version } => {
                assert_eq!(decision, ApprovalDecision::Accepted);
                assert_eq!(version, 2);
                winners += 1;
            }
            ApprovalDecisionResolution::ExistingTerminal {
                decision,
                status,
                version,
            } => {
                assert_eq!(decision, Some(ApprovalDecision::Accepted));
                assert_eq!(status, ApprovalStatus::Accepted);
                assert_eq!(version, 2);
                existing += 1;
            }
            ApprovalDecisionResolution::NotFound => panic!("racing contender lost the record"),
        }
    }
    assert_eq!(winners, 1, "exactly one decision wins");
    assert_eq!(existing, contenders - 1);
}

#[test]
fn decision_at_or_after_expiry_commits_no_decision() {
    let Some(mut harness) = harness() else {
        return;
    };

    // requested_at 1_000 with a 2_000 Turn deadline yields a 2_000 expiry.
    let approval = requested_approval(1_000, 2_000);
    let tenant = approval.tenant_id().clone();
    assert_eq!(
        harness.store.insert_requested(&approval),
        Ok(ApprovalInsertResolution::Inserted)
    );
    // Lost-acknowledgement replay: the identical immutable record
    // reconciles as already canonical.
    assert_eq!(
        harness.store.insert_requested(&approval),
        Ok(ApprovalInsertResolution::Existing {
            status: ApprovalStatus::Requested,
            decision: None,
            version: 1,
        }),
    );

    let late = harness
        .store
        .resolve_decision(
            approval.approval_id(),
            &tenant,
            ApprovalDecision::Accepted,
            &approver("approver-a"),
            2_000,
        )
        .expect("late decision completes");
    assert_eq!(
        late,
        ApprovalDecisionResolution::ExistingTerminal {
            decision: None,
            status: ApprovalStatus::Expired,
            version: 2,
        }
    );

    // A still-timely decision before the window closes succeeds, proving the
    // expiry transition is not applied to in-window records.
    let timely_approval = requested_approval(1_000, 60_000);
    assert_eq!(
        harness.store.insert_requested(&timely_approval),
        Ok(ApprovalInsertResolution::Inserted)
    );
    let timely = harness
        .store
        .resolve_decision(
            timely_approval.approval_id(),
            &timely_approval.tenant_id().clone(),
            ApprovalDecision::Declined,
            &approver("approver-a"),
            1_999,
        )
        .expect("in-window decision completes");
    assert_eq!(
        timely,
        ApprovalDecisionResolution::Won {
            decision: ApprovalDecision::Declined,
            version: 2,
        }
    );
}

#[test]
fn conflicting_identity_replay_is_a_typed_conflict() {
    let Some(mut harness) = harness() else {
        return;
    };
    let approval = requested_approval(1_000, 60_000);
    // Seed the canonical identity with a different immutable action digest,
    // standing in for a committed record that no longer matches the replay.
    harness
        .runtime
        .block_on(async {
            sqlx::query(
                "INSERT INTO tool_approvals (
                    tenant_id, approval_id, thread_id, turn_id, attempt_id,
                    lease_generation, descriptor_id, descriptor_version, effect,
                    action_digest, profile_id, profile_version,
                    requested_at_millis, expires_at_millis, status, version
                ) VALUES (
                    $1, $2, '00000000-0000-0000-0000-000000000000',
                    '00000000-0000-0000-0000-000000000000',
                    '00000000-0000-0000-0000-000000000000',
                    1, 'other.tool', 'v9', 'read_data',
                    'decoy', 'other-profile', 'v9', 1, 2, 'requested', 1
                )",
            )
            .bind(approval.tenant_id().as_str())
            .bind(approval.approval_id().as_uuid())
            .execute(&harness.pool)
            .await
        })
        .expect("seed conflicting canonical row");
    assert_eq!(
        harness.store.insert_requested(&approval),
        Err(ApprovalStoreError::IdentityConflict)
    );
}

#[test]
fn validated_approver_identity_is_required_for_durable_terminals() {
    // The sealed capability is derivable only from an authenticated principal
    // carrying the gateway-validated approval scope; blank or unscoped
    // principals yield no approver identity at all.
    let unscoped = koduck_ai::domain::TrustContext::new(
        TenantId::new("approver-tenant").expect("valid tenant"),
        "approver-a",
    )
    .expect("valid principal");
    assert_eq!(
        koduck_ai::domain::execution::ApproverId::from_authenticated(&unscoped),
        None
    );
    // A blank subject cannot even construct an authenticated context, so the
    // capability's blank guard is unreachable defense in depth behind the
    // trust constructor's own validation.
    assert!(
        koduck_ai::domain::TrustContext::new(
            TenantId::new("approver-tenant").expect("valid tenant"),
            "  ",
        )
        .is_err()
    );

    // The schema-level defense in depth lives in its own focused test
    // (schema_rejects_illegal_terminal_tuples).
}

const ILLEGAL_TERMINAL_STATEMENTS: [(&str, &str); 7] = [
    (
        "blank approver",
        "INSERT INTO tool_approvals (
            tenant_id, approval_id, thread_id, turn_id, attempt_id,
            lease_generation, descriptor_id, descriptor_version, effect,
            action_digest, profile_id, profile_version,
            requested_at_millis, expires_at_millis,
            status, decision, approver, decided_at_millis, version
        ) VALUES (
            'schema-check-tenant', $1,
            '00000000-0000-0000-0000-000000000000',
            '00000000-0000-0000-0000-000000000000',
            '00000000-0000-0000-0000-000000000000',
            1, 'fixture.tool', 'v1', 'read_data',
            'decoy', 'profile-default', 'v1', 1, 2,
            'accepted', 'accepted', '', 1, 1
        )",
    ),
    (
        "whitespace-only approver",
        "INSERT INTO tool_approvals (
            tenant_id, approval_id, thread_id, turn_id, attempt_id,
            lease_generation, descriptor_id, descriptor_version, effect,
            action_digest, profile_id, profile_version,
            requested_at_millis, expires_at_millis,
            status, decision, approver, decided_at_millis, version
        ) VALUES (
            'schema-check-tenant', $1,
            '00000000-0000-0000-0000-000000000000',
            '00000000-0000-0000-0000-000000000000',
            '00000000-0000-0000-0000-000000000000',
            1, 'fixture.tool', 'v1', 'read_data',
            'decoy', 'profile-default', 'v1', 1, 2,
            'accepted', 'accepted', '   ', 1, 1
        )",
    ),
    (
        "decided terminal without a decision timestamp",
        "INSERT INTO tool_approvals (
            tenant_id, approval_id, thread_id, turn_id, attempt_id,
            lease_generation, descriptor_id, descriptor_version, effect,
            action_digest, profile_id, profile_version,
            requested_at_millis, expires_at_millis,
            status, decision, approver, decided_at_millis, version
        ) VALUES (
            'schema-check-tenant', $1,
            '00000000-0000-0000-0000-000000000000',
            '00000000-0000-0000-0000-000000000000',
            '00000000-0000-0000-0000-000000000000',
            1, 'fixture.tool', 'v1', 'read_data',
            'decoy', 'profile-default', 'v1', 1, 2,
            'accepted', 'accepted', 'approver-a', NULL, 1
        )",
    ),
    (
        "decided timestamp at expiry",
        "INSERT INTO tool_approvals (
            tenant_id, approval_id, thread_id, turn_id, attempt_id,
            lease_generation, descriptor_id, descriptor_version, effect,
            action_digest, profile_id, profile_version,
            requested_at_millis, expires_at_millis,
            status, decision, approver, decided_at_millis, version
        ) VALUES (
            'schema-check-tenant', $1,
            '00000000-0000-0000-0000-000000000000',
            '00000000-0000-0000-0000-000000000000',
            '00000000-0000-0000-0000-000000000000',
            1, 'fixture.tool', 'v1', 'read_data',
            'decoy', 'profile-default', 'v1', 1, 2,
            'accepted', 'accepted', 'approver-a', 2, 1
        )",
    ),
    (
        "decided timestamp after expiry",
        "INSERT INTO tool_approvals (
            tenant_id, approval_id, thread_id, turn_id, attempt_id,
            lease_generation, descriptor_id, descriptor_version, effect,
            action_digest, profile_id, profile_version,
            requested_at_millis, expires_at_millis,
            status, decision, approver, decided_at_millis, version
        ) VALUES (
            'schema-check-tenant', $1,
            '00000000-0000-0000-0000-000000000000',
            '00000000-0000-0000-0000-000000000000',
            '00000000-0000-0000-0000-000000000000',
            1, 'fixture.tool', 'v1', 'read_data',
            'decoy', 'profile-default', 'v1', 1, 2,
            'accepted', 'accepted', 'approver-a', 3, 1
        )",
    ),
    (
        "tab-only approver",
        "INSERT INTO tool_approvals (
            tenant_id, approval_id, thread_id, turn_id, attempt_id,
            lease_generation, descriptor_id, descriptor_version, effect,
            action_digest, profile_id, profile_version,
            requested_at_millis, expires_at_millis,
            status, decision, approver, decided_at_millis, version
        ) VALUES (
            'schema-check-tenant', $1,
            '00000000-0000-0000-0000-000000000000',
            '00000000-0000-0000-0000-000000000000',
            '00000000-0000-0000-0000-000000000000',
            1, 'fixture.tool', 'v1', 'read_data',
            'decoy', 'profile-default', 'v1', 1, 2,
            'accepted', 'accepted', E'\t', 1, 1
        )",
    ),
    (
        "newline-only approver",
        "INSERT INTO tool_approvals (
            tenant_id, approval_id, thread_id, turn_id, attempt_id,
            lease_generation, descriptor_id, descriptor_version, effect,
            action_digest, profile_id, profile_version,
            requested_at_millis, expires_at_millis,
            status, decision, approver, decided_at_millis, version
        ) VALUES (
            'schema-check-tenant', $1,
            '00000000-0000-0000-0000-000000000000',
            '00000000-0000-0000-0000-000000000000',
            '00000000-0000-0000-0000-000000000000',
            1, 'fixture.tool', 'v1', 'read_data',
            'decoy', 'profile-default', 'v1', 1, 2,
            'accepted', 'accepted', E'\n', 1, 1
        )",
    ),
];

#[test]
fn schema_rejects_illegal_terminal_tuples() {
    let Some(harness) = harness() else {
        return;
    };
    // Defense in depth: the schema itself rejects every illegal terminal
    // tuple — blank or whitespace-only approver, decided terminal without a
    // decision timestamp, decided timestamp at/after expiry — and a decision
    // timestamp on a requested record.
    for (description, statement) in ILLEGAL_TERMINAL_STATEMENTS {
        let rejected = harness.runtime.block_on(async {
            sqlx::query(statement)
                .bind(uuid::Uuid::new_v4())
                .execute(&harness.pool)
                .await
        });
        assert!(
            rejected.is_err(),
            "schema must reject the illegal terminal tuple: {description}"
        );
    }
    let requested_with_timestamp = harness.runtime.block_on(async {
        sqlx::query(
            "INSERT INTO tool_approvals (
                tenant_id, approval_id, thread_id, turn_id, attempt_id,
                lease_generation, descriptor_id, descriptor_version, effect,
                action_digest, profile_id, profile_version,
                requested_at_millis, expires_at_millis,
                status, approver, decided_at_millis, version
            ) VALUES (
                'schema-check-tenant', $1,
                '00000000-0000-0000-0000-000000000000',
                '00000000-0000-0000-0000-000000000000',
                '00000000-0000-0000-0000-000000000000',
                1, 'fixture.tool', 'v1', 'read_data',
                'decoy', 'profile-default', 'v1', 1, 2,
                'requested', NULL, 1, 1
            )",
        )
        .bind(uuid::Uuid::new_v4())
        .execute(&harness.pool)
        .await
    });
    assert!(
        requested_with_timestamp.is_err(),
        "schema must reject a decision timestamp on a requested record"
    );
}

#[test]
fn insert_replay_after_a_terminal_transition_returns_the_canonical_state() {
    let Some(mut harness) = harness() else {
        return;
    };
    let approval = requested_approval(1_000, 60_000);
    assert_eq!(
        harness.store.insert_requested(&approval),
        Ok(ApprovalInsertResolution::Inserted)
    );
    harness
        .store
        .resolve_decision(
            approval.approval_id(),
            approval.tenant_id(),
            ApprovalDecision::Declined,
            &approver("approver-a"),
            2_000,
        )
        .expect("decision resolves");
    // A lost-acknowledgement replay after another instance resolved the
    // record reports the terminal projection, not requested version 1.
    assert_eq!(
        harness.store.insert_requested(&approval),
        Ok(ApprovalInsertResolution::Existing {
            status: ApprovalStatus::Declined,
            decision: Some(ApprovalDecision::Declined),
            version: 2,
        })
    );

    // The same holds after the expiry transition closed another record.
    let expired = requested_approval(1_000, 2_000);
    assert_eq!(
        harness.store.insert_requested(&expired),
        Ok(ApprovalInsertResolution::Inserted)
    );
    harness
        .store
        .resolve_decision(
            expired.approval_id(),
            expired.tenant_id(),
            ApprovalDecision::Accepted,
            &approver("approver-a"),
            2_000,
        )
        .expect("late decision completes");
    assert_eq!(
        harness.store.insert_requested(&expired),
        Ok(ApprovalInsertResolution::Existing {
            status: ApprovalStatus::Expired,
            decision: None,
            version: 2,
        })
    );
}
