// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Integration harness for canonical D-6 `PostgreSQL` persistence.

use std::sync::Arc;
use std::sync::Barrier;

use koduck_ai::adapters::execution::{SqlxApprovalRecordStore, SqlxExecutionAttemptStore};
use koduck_ai::adapters::history::postgres::{PostgresExecutor, SqlxPostgresExecutor};
use koduck_ai::adapters::tool::{parse_action_parameters, parse_input_schema};
use koduck_ai::application::{
    AcceptedTurn, ApprovalDecisionResolution, ApprovalInsertResolution, ApprovalRecordStore,
    ApprovalStoreError, DurableAttemptTerminal, EffectState, ExecutionAttemptInterruptionGuard,
    ExecutionAttemptStore, HistoryError, InterruptionBarrierResolution, NewItem,
    PendingApprovalCancellation, PendingApprovalCanceller, ToolAuthorizationService,
    ToolExecutionOutcome, ToolPolicyConfiguration,
};
use koduck_ai::domain::execution::{
    ApprovalDecision, ApprovalRequest, ApprovalStatus, AttemptId, ExactActionBinding,
    ExecutionStatus,
};
use koduck_ai::domain::tool::{
    Action, CapabilityDescriptor, DescriptorState, Effect, PermissionProfile,
};
use koduck_ai::domain::{
    Item, ItemPayload, LeaseGeneration, TenantId, TerminalOutcome, ToolEffectState, TrustContext,
};
use sqlx::postgres::{PgPool, PgPoolOptions};
use uuid::Uuid;

#[path = "cand_2_postgres_approval_projection.rs"]
mod approval_projection;
#[path = "cand_2_postgres_attempt_limits.rs"]
mod attempt_limits;
#[path = "cand_2_postgres_attempts.rs"]
mod attempts;
#[path = "cand_2_postgres_audit.rs"]
mod audit;
#[path = "cand_2_postgres_prepared_close.rs"]
mod prepared_close;

#[path = "cand_2_postgres_interruption_claim.rs"]
mod interruption_claim;

#[path = "cand_2_postgres_production_path.rs"]
mod production_path;

#[path = "cand_2_postgres_recovery.rs"]
mod recovery;

struct Harness {
    runtime: tokio::runtime::Runtime,
    pool: PgPool,
    store: SqlxApprovalRecordStore,
    // Retain the exclusive fixture reservation until this harness drops.
    _database_permit: tokio::sync::OwnedSemaphorePermit,
}

const EXPIRY_GATE: i64 = 7_361_204_112;

/// Installs and holds the test-only advisory gate for an expiry transition.
fn install_expiry_gate(harness: &Harness) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    harness.runtime.block_on(async {
        sqlx::raw_sql(
            "CREATE OR REPLACE FUNCTION koduck_test_gate_expiry() RETURNS trigger AS $$
             BEGIN
               IF OLD.status = 'requested' AND NEW.status = 'expired' THEN
                 PERFORM pg_advisory_xact_lock(7361204112);
               END IF;
               RETURN NEW;
             END;
             $$ LANGUAGE plpgsql;
             DROP TRIGGER IF EXISTS koduck_test_gate_expiry ON tool_approvals;
             CREATE TRIGGER koduck_test_gate_expiry
               BEFORE UPDATE ON tool_approvals
               FOR EACH ROW EXECUTE FUNCTION koduck_test_gate_expiry();",
        )
        .execute(&harness.pool)
        .await
        .expect("expiry gate trigger is installed");
        let mut connection = harness
            .pool
            .acquire()
            .await
            .expect("expiry gate connection");
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(EXPIRY_GATE)
            .execute(&mut *connection)
            .await
            .expect("expiry gate is held");
        connection
    })
}

/// Waits until the expiry UPDATE is blocked inside the test-only trigger.
fn wait_for_expiry_gate(harness: &Harness) {
    harness.runtime.block_on(async {
        for _ in 0..50 {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS (
                   SELECT 1 FROM pg_stat_activity
                   WHERE wait_event = 'advisory'
                     AND query LIKE 'UPDATE tool_approvals%'
                 )",
            )
            .fetch_one(&harness.pool)
            .await
            .expect("expiry wait state is readable");
            if waiting {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("expiry transition did not reach the advisory gate");
    });
}

/// Releases and removes the test-only expiry gate under its Tokio runtime.
fn remove_expiry_gate(harness: &Harness, mut gate: sqlx::pool::PoolConnection<sqlx::Postgres>) {
    harness.runtime.block_on(async {
        sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(EXPIRY_GATE)
            .execute(&mut *gate)
            .await
            .expect("expiry gate is released");
        drop(gate);
        sqlx::raw_sql(
            "DROP TRIGGER IF EXISTS koduck_test_gate_expiry ON tool_approvals;
             DROP FUNCTION IF EXISTS koduck_test_gate_expiry();",
        )
        .execute(&harness.pool)
        .await
        .expect("expiry gate trigger is removed");
    });
}

fn harness() -> Option<Harness> {
    let database_url = std::env::var("KODUCK_AI_TEST_DATABASE_URL").ok()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("PostgreSQL test runtime");
    let database_permit = runtime.block_on(crate::test_migrations::reserve_database());
    let pool = runtime
        .block_on(
            PgPoolOptions::new()
                // 32 concurrent decision contenders each hold one pooled
                // connection across their transaction; the pool must admit
                // them all or the store's 2-second wait deadline fails on
                // pool queuing rather than transition contention.
                .max_connections(32)
                .connect(&database_url),
        )
        .expect("connect to disposable PostgreSQL");
    // The shared process-wide guard serializes this DDL against every other
    // env-gated harness in the same test binary (parallel CREATE TABLE IF NOT
    // EXISTS races in the PostgreSQL catalog).
    runtime.block_on(crate::test_migrations::ensure(&pool));
    let store = SqlxApprovalRecordStore::new(pool.clone(), runtime.handle().clone());
    Some(Harness {
        runtime,
        pool,
        store,
        _database_permit: database_permit,
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

/// Seeds the exact active Turn/lease authority required by requested D-6 insertion.
fn seed_approval_owner(harness: &Harness, approval: &ApprovalRequest) {
    attempts::seed_owner_rows(
        harness,
        approval.tenant_id(),
        approval.binding().thread_id(),
        approval.binding().turn_id(),
        approval.binding().lease_generation(),
    );
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
#[allow(
    clippy::too_many_lines,
    reason = "the migration idempotency and durable decision race must share one canonical fixture"
)]
fn migration_is_idempotent_and_decisions_are_single_winner() {
    let Some(mut harness) = harness() else {
        return;
    };
    for _ in 0..2 {
        for migration in [
            include_str!("../../migrations/0001_cand_1_history.sql"),
            include_str!("../../migrations/0002_cand_2_policy_execution.sql"),
            include_str!("../../migrations/0003_cand_2_requester_ownership.sql"),
            include_str!("../../migrations/0004_cand_2_tool_projections.sql"),
            include_str!("../../migrations/0005_cand_2_execution_attempts.sql"),
            include_str!("../../migrations/0006_cand_2_interrupt_barrier.sql"),
            include_str!("../../migrations/0007_cand_2_tool_audit.sql"),
            include_str!("../../migrations/0008_cand_2_interruption_approval_cancellation.sql"),
        ] {
            harness
                .runtime
                .block_on(async { sqlx::raw_sql(migration).execute(&harness.pool).await })
                .expect("idempotent migration applies repeatedly");
        }
    }

    let approval = requested_approval(1_000, 60_000);
    let tenant = approval.tenant_id().clone();
    seed_approval_owner(&harness, &approval);
    assert_eq!(
        harness.store.insert_requested(&approval, "requester"),
        Ok(ApprovalInsertResolution::Inserted)
    );
    // Lost-acknowledgement replay: the identical immutable record
    // reconciles as already canonical.
    assert_eq!(
        harness.store.insert_requested(&approval, "requester"),
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
            approval.binding().thread_id(),
            "requester",
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
                approval.binding().thread_id(),
                "requester",
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
            koduck_ai::domain::ThreadId::new(),
            "requester",
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
            koduck_ai::domain::ThreadId::new(),
            "requester",
            ApprovalDecision::Accepted,
            &approver("approver-a"),
            2_000,
        )
        .expect("unknown resolve completes");
    assert_eq!(unknown, ApprovalDecisionResolution::NotFound);
}

#[test]
fn terminal_decision_replay_does_not_require_a_second_pool_connection() {
    // A replay loses the guarded transition and must read the canonical
    // terminal. With a one-connection pool, holding the loser transaction
    // while that read starts would self-starve and return `Unavailable`.
    let Some(harness) = harness() else {
        return;
    };
    let database_url = std::env::var("KODUCK_AI_TEST_DATABASE_URL")
        .expect("the harness only exists with a PostgreSQL URL");
    let pool = harness
        .runtime
        .block_on(
            PgPoolOptions::new()
                .max_connections(1)
                .connect(&database_url),
        )
        .expect("one-connection PostgreSQL pool");
    let mut store = SqlxApprovalRecordStore::new(pool, harness.runtime.handle().clone());
    let approval = requested_approval(1_000, 60_000);
    let tenant = approval.tenant_id().clone();
    seed_approval_owner(&harness, &approval);
    assert_eq!(
        store.insert_requested(&approval, "requester"),
        Ok(ApprovalInsertResolution::Inserted)
    );
    assert_eq!(
        store.resolve_decision(
            approval.approval_id(),
            &tenant,
            approval.binding().thread_id(),
            "requester",
            ApprovalDecision::Accepted,
            &approver("approver-a"),
            2_000,
        ),
        Ok(ApprovalDecisionResolution::Won {
            decision: ApprovalDecision::Accepted,
            version: 2,
        })
    );
    assert_eq!(
        store.resolve_decision(
            approval.approval_id(),
            &tenant,
            approval.binding().thread_id(),
            "requester",
            ApprovalDecision::Accepted,
            &approver("approver-b"),
            3_000,
        ),
        Ok(ApprovalDecisionResolution::ExistingTerminal {
            decision: Some(ApprovalDecision::Accepted),
            status: ApprovalStatus::Accepted,
            version: 2,
        })
    );
}

#[test]
fn interruption_cancels_the_canonical_requested_approval() {
    let Some(mut harness) = harness() else {
        return;
    };
    let approval = requested_approval(1_000, 60_000);
    attempts::seed_owner_rows(
        &harness,
        approval.tenant_id(),
        approval.binding().thread_id(),
        approval.binding().turn_id(),
        approval.binding().lease_generation(),
    );
    assert_eq!(
        harness.store.insert_requested(&approval, "requester"),
        Ok(ApprovalInsertResolution::Inserted),
    );
    let executor =
        SqlxPostgresExecutor::new(harness.pool.clone(), harness.runtime.handle().clone());
    let accepted = AcceptedTurn::new(
        approval.tenant_id().clone(),
        approval.binding().thread_id(),
        approval.binding().turn_id(),
        approval.binding().lease_generation(),
        Item::new(
            1,
            ItemPayload::UserMessage {
                content: "approval interruption fixture".to_owned(),
            },
        ),
    );
    executor
        .append_tool_projection(
            &accepted,
            vec![NewItem::ApprovalStatus {
                approval_id: approval.approval_id(),
                attempt_id: approval.binding().attempt_id(),
                status: ApprovalStatus::Requested,
                decision: None,
                version: 1,
            }],
        )
        .expect("requested approval projection is durable");
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turns SET interrupting = TRUE
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(approval.tenant_id().as_str())
        .bind(approval.binding().thread_id().as_uuid())
        .bind(approval.binding().turn_id().as_uuid())
        .execute(&harness.pool)
        .await
        .expect("interruption barrier is established");
    });

    assert_eq!(
        PendingApprovalCanceller::cancel_requested(&mut harness.store, approval.binding(),),
        Ok(PendingApprovalCancellation::Cancelled),
    );
    let projection: (String, i64) = harness.runtime.block_on(async {
        sqlx::query_as(
            "SELECT status, version FROM tool_approvals
             WHERE tenant_id = $1 AND approval_id = $2",
        )
        .bind(approval.tenant_id().as_str())
        .bind(approval.approval_id().as_uuid())
        .fetch_one(&harness.pool)
        .await
        .expect("cancelled approval is readable")
    });
    assert_eq!(projection, ("cancelled".to_owned(), 2));

    let trust = TrustContext::new(approval.tenant_id().clone(), "d7-attempt-fixture")
        .expect("valid fixture owner");
    assert_eq!(
        executor.request_interrupt(&trust, approval.binding().turn_id(), Vec::new()),
        Ok(()),
    );
    let replay = executor
        .replay(approval.tenant_id(), approval.binding().turn_id())
        .expect("interrupted Turn history is readable");
    assert_cancelled_approval_precedes_terminal(&replay, &approval);
}

#[test]
fn interruption_requires_the_exact_missing_canonical_tool_terminal_set() {
    let Some(harness) = harness() else {
        return;
    };
    let executor =
        SqlxPostgresExecutor::new(harness.pool.clone(), harness.runtime.handle().clone());

    for case in ["empty", "incomplete", "already-projected"] {
        let first = attempts::prepared_binding(Effect::ReadData);
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
        let mut store = attempts::attempt_store(harness.pool.clone(), &harness.runtime);
        assert_eq!(
            attempts::insert_prepared(&harness, &mut store, &first, 1_000),
            Ok(koduck_ai::application::AttemptInsertResolution::Inserted),
        );
        let cancelled = DurableAttemptTerminal::from_outcome(&ToolExecutionOutcome::Cancelled {
            effect_state: EffectState::NotStarted,
        });
        assert!(matches!(
            store.commit_terminal(&first, &cancelled, 2_000),
            Ok(koduck_ai::application::AttemptTerminalResolution::Won { version: 3 })
        ));
        let first_terminal = cancelled_terminal(first.attempt_id());
        let supplied = match case {
            "empty" => Vec::new(),
            "incomplete" => {
                assert_eq!(
                    attempts::insert_prepared(&harness, &mut store, &second, 1_000),
                    Ok(koduck_ai::application::AttemptInsertResolution::Inserted),
                );
                assert!(matches!(
                    store.commit_terminal(&second, &cancelled, 2_000),
                    Ok(koduck_ai::application::AttemptTerminalResolution::Won { version: 3 })
                ));
                vec![first_terminal.clone()]
            }
            "already-projected" => {
                let accepted = AcceptedTurn::new(
                    first.tenant_id().clone(),
                    first.thread_id(),
                    first.turn_id(),
                    first.lease_generation(),
                    Item::new(
                        1,
                        ItemPayload::UserMessage {
                            content: "terminal-set fixture".to_owned(),
                        },
                    ),
                );
                executor
                    .append_tool_projection(&accepted, vec![first_terminal.clone()])
                    .expect("canonical terminal projection is durable");
                vec![first_terminal]
            }
            _ => unreachable!("enumerated fixture case"),
        };
        let trust = TrustContext::new(first.tenant_id().clone(), "d7-attempt-fixture")
            .expect("valid fixture owner");

        assert_eq!(
            executor.request_interrupt(&trust, first.turn_id(), supplied),
            Err(HistoryError::Unavailable),
            "{case} terminal batch must fail closed",
        );
        let status: String = harness.runtime.block_on(async {
            sqlx::query_scalar(
                "SELECT status FROM turns
                 WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
            )
            .bind(first.tenant_id().as_str())
            .bind(first.thread_id().as_uuid())
            .bind(first.turn_id().as_uuid())
            .fetch_one(&harness.pool)
            .await
            .expect("fixture Turn remains readable")
        });
        assert_eq!(status, "started", "{case} must not terminalize the Turn");
    }
}

/// Builds the exact public D-3 projection of a canonical cancelled D-7.
fn cancelled_terminal(attempt_id: AttemptId) -> NewItem {
    NewItem::ToolResult {
        attempt_id: Some(attempt_id),
        status: ExecutionStatus::Cancelled,
        code: None,
        effect_state: Some(ToolEffectState::NotStarted),
        output_bytes: 0,
        output_digest: None,
        version: Some(3),
    }
}

/// Proves replay keeps the requested view but resolves it with the canonical
/// interruption-owned cancellation before the Turn becomes terminal.
fn assert_cancelled_approval_precedes_terminal(replay: &[Item], approval: &ApprovalRequest) {
    assert!(
        matches!(
            replay,
            [
                requested,
                item,
                terminal
            ] if matches!(
                requested.payload,
                ItemPayload::ApprovalStatus {
                    approval_id,
                    attempt_id,
                    status: ApprovalStatus::Requested,
                    decision: None,
                    version: 1,
                } if approval_id == approval.approval_id()
                    && attempt_id == approval.binding().attempt_id()
            ) && matches!(
                item.payload,
                ItemPayload::ApprovalStatus {
                    approval_id,
                    attempt_id,
                    status: ApprovalStatus::Cancelled,
                    decision: None,
                    version: 2,
                } if approval_id == approval.approval_id()
                    && attempt_id == approval.binding().attempt_id()
            ) && terminal.payload == ItemPayload::Terminal(TerminalOutcome::Interrupted)
        ),
        "the canonical cancelled D-6 projection precedes the Turn terminal: {replay:?}",
    );
}

/// Reads the durable status tuple used by interruption race assertions.
fn approval_projection(
    harness: &Harness,
    approval: &ApprovalRequest,
) -> (String, Option<String>, i64) {
    harness
        .runtime
        .block_on(async {
            sqlx::query_as(
                "SELECT status, decision, version FROM tool_approvals
                 WHERE tenant_id = $1 AND approval_id = $2",
            )
            .bind(approval.tenant_id().as_str())
            .bind(approval.approval_id().as_uuid())
            .fetch_one(&harness.pool)
            .await
        })
        .expect("approval remains readable")
}

#[test]
fn approval_decision_waits_for_the_turn_lock_before_interruption_guarding() {
    let Some(mut harness) = harness() else {
        return;
    };
    let approval = requested_approval(1_000, 60_000);
    let tenant = approval.tenant_id().clone();
    let thread = approval.binding().thread_id();
    let turn = approval.binding().turn_id();
    attempts::seed_owner_rows(
        &harness,
        &tenant,
        thread,
        turn,
        approval.binding().lease_generation(),
    );
    assert_eq!(
        harness.store.insert_requested(&approval, "requester"),
        Ok(ApprovalInsertResolution::Inserted),
    );

    let mut owner_transaction = harness.runtime.block_on(async {
        let mut transaction = harness.pool.begin().await.expect("owner transaction");
        sqlx::query(
            "SELECT turn_id FROM turns
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3
             FOR UPDATE",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .fetch_one(&mut *transaction)
        .await
        .expect("interruption owns the Turn lock");
        transaction
    });

    let approval_id = approval.approval_id();
    let mut store = harness.store.clone();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (resolution_tx, resolution_rx) = std::sync::mpsc::channel();
    let contender = std::thread::spawn(move || {
        started_tx.send(()).expect("contender start is observable");
        let resolution = store.resolve_decision(
            approval_id,
            &tenant,
            thread,
            "requester",
            ApprovalDecision::Accepted,
            &approver("approver-a"),
            2_000,
        );
        resolution_tx
            .send(resolution)
            .expect("decision resolution is observable");
    });
    started_rx.recv().expect("decision contender starts");
    let early_resolution = resolution_rx.recv_timeout(std::time::Duration::from_millis(250));
    let waited_for_turn_lock = matches!(
        early_resolution,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    );

    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turns SET interrupting = TRUE
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(approval.tenant_id().as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .execute(&mut *owner_transaction)
        .await
        .expect("interruption barrier is established under the Turn lock");
        owner_transaction
            .commit()
            .await
            .expect("interruption barrier commits");
    });

    let resolution = match early_resolution {
        Ok(resolution) => resolution,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => resolution_rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .expect("decision completes after the Turn lock is released"),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            panic!("decision contender disconnected")
        }
    };
    contender.join().expect("decision contender completes");
    assert!(
        waited_for_turn_lock,
        "the decision must wait for the canonical Turn lock"
    );
    assert_eq!(
        resolution,
        Ok(ApprovalDecisionResolution::TurnGuardRejected),
    );
    assert_eq!(
        approval_projection(&harness, &approval),
        ("requested".to_owned(), None, 1),
    );
}

#[test]
fn expiry_fallback_holds_the_turn_lock_until_its_terminal_commits() {
    let Some(mut harness) = harness() else {
        return;
    };
    let approval = requested_approval(1_000, 2_000);
    let tenant = approval.tenant_id().clone();
    let thread = approval.binding().thread_id();
    let turn = approval.binding().turn_id();
    attempts::seed_owner_rows(
        &harness,
        &tenant,
        thread,
        turn,
        approval.binding().lease_generation(),
    );
    assert_eq!(
        harness.store.insert_requested(&approval, "requester"),
        Ok(ApprovalInsertResolution::Inserted),
    );

    let gate = install_expiry_gate(&harness);

    let approval_id = approval.approval_id();
    let mut expiry_store = harness.store.clone();
    let expiry_tenant = tenant.clone();
    let (expiry_tx, expiry_rx) = std::sync::mpsc::channel();
    let expiry = std::thread::spawn(move || {
        expiry_tx
            .send(expiry_store.resolve_decision(
                approval_id,
                &expiry_tenant,
                thread,
                "requester",
                ApprovalDecision::Accepted,
                &approver("approver-a"),
                2_000,
            ))
            .expect("expiry resolution is observable");
    });
    wait_for_expiry_gate(&harness);

    let mut interruption_store =
        SqlxExecutionAttemptStore::new(harness.pool.clone(), harness.runtime.handle().clone());
    let barrier_tenant = tenant.clone();
    let (barrier_tx, barrier_rx) = std::sync::mpsc::channel();
    let barrier = std::thread::spawn(move || {
        barrier_tx
            .send(interruption_store.begin_interruption(&barrier_tenant, thread, turn))
            .expect("interruption result is observable");
    });
    let early_barrier = barrier_rx.recv_timeout(std::time::Duration::from_millis(250));
    let barrier_waited = matches!(
        early_barrier,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    );

    remove_expiry_gate(&harness, gate);
    let expiry_resolution = expiry_rx
        .recv_timeout(std::time::Duration::from_secs(3))
        .expect("expiry completes after the gate is released");
    let barrier_resolution = match early_barrier {
        Ok(resolution) => resolution,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => barrier_rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .expect("interruption completes after expiry releases the Turn lock"),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            panic!("interruption contender disconnected")
        }
    };
    expiry.join().expect("expiry contender completes");
    barrier.join().expect("interruption contender completes");
    assert!(
        barrier_waited,
        "expiry must retain the canonical Turn lock through fallback and commit",
    );
    assert_eq!(
        expiry_resolution,
        Ok(ApprovalDecisionResolution::ExistingTerminal {
            decision: None,
            status: ApprovalStatus::Expired,
            version: 2,
        }),
    );
    assert_eq!(
        barrier_resolution,
        Ok(InterruptionBarrierResolution::Established),
    );
}

#[test]
fn thirty_two_competing_decisions_commit_exactly_one_terminal() {
    let Some(mut harness) = harness() else {
        return;
    };

    let approval = requested_approval(1_000, 60_000);
    let tenant = approval.tenant_id().clone();
    seed_approval_owner(&harness, &approval);
    assert_eq!(
        harness.store.insert_requested(&approval, "requester"),
        Ok(ApprovalInsertResolution::Inserted)
    );
    // Lost-acknowledgement replay: the identical immutable record
    // reconciles as already canonical.
    assert_eq!(
        harness.store.insert_requested(&approval, "requester"),
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
        let thread = approval.binding().thread_id();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            store
                .resolve_decision(
                    approval_id,
                    &tenant,
                    thread,
                    "requester",
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
            ApprovalDecisionResolution::TurnGuardRejected => {
                panic!("racing contender hit the Turn guard unexpectedly")
            }
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
    seed_approval_owner(&harness, &approval);
    assert_eq!(
        harness.store.insert_requested(&approval, "requester"),
        Ok(ApprovalInsertResolution::Inserted)
    );
    // Lost-acknowledgement replay: the identical immutable record
    // reconciles as already canonical.
    assert_eq!(
        harness.store.insert_requested(&approval, "requester"),
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
            approval.binding().thread_id(),
            "requester",
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
    seed_approval_owner(&harness, &timely_approval);
    assert_eq!(
        harness
            .store
            .insert_requested(&timely_approval, "requester"),
        Ok(ApprovalInsertResolution::Inserted)
    );
    let timely = harness
        .store
        .resolve_decision(
            timely_approval.approval_id(),
            &timely_approval.tenant_id().clone(),
            timely_approval.binding().thread_id(),
            "requester",
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
fn interruption_barrier_owns_an_expired_requested_approval() {
    let Some(mut harness) = harness() else {
        return;
    };
    let approval = requested_approval(1_000, 2_000);
    let tenant = approval.tenant_id().clone();
    let thread_id = approval.binding().thread_id();
    let turn_id = approval.binding().turn_id();
    seed_approval_owner(&harness, &approval);
    assert_eq!(
        harness.store.insert_requested(&approval, "requester"),
        Ok(ApprovalInsertResolution::Inserted)
    );
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turns SET interrupting = TRUE
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(tenant.as_str())
        .bind(thread_id.as_uuid())
        .bind(turn_id.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture interruption barrier");
    });

    let resolution = harness
        .store
        .resolve_decision(
            approval.approval_id(),
            &tenant,
            thread_id,
            "requester",
            ApprovalDecision::Accepted,
            &approver("approver-a"),
            2_000,
        )
        .expect("interruption guard answers deterministically");

    assert_eq!(resolution, ApprovalDecisionResolution::TurnGuardRejected);
    let (status, decision, version): (String, Option<String>, i64) = harness
        .runtime
        .block_on(async {
            sqlx::query_as(
                "SELECT status, decision, version FROM tool_approvals \
                 WHERE tenant_id = $1 AND approval_id = $2",
            )
            .bind(tenant.as_str())
            .bind(approval.approval_id().as_uuid())
            .fetch_one(&harness.pool)
            .await
        })
        .expect("approval remains readable");
    assert_eq!(
        (status.as_str(), decision.as_deref(), version),
        ("requested", None, 1),
        "recovery owns the interruption cancellation"
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
                    tenant_id, approval_id, requester_subject, thread_id, turn_id, attempt_id,
                    lease_generation, descriptor_id, descriptor_version, effect,
                    action_digest, profile_id, profile_version,
                    requested_at_millis, expires_at_millis, status, version
                ) VALUES (
                    $1, $2, 'requester', '00000000-0000-0000-0000-000000000000',
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
        harness.store.insert_requested(&approval, "requester"),
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
            tenant_id, approval_id, requester_subject, thread_id, turn_id, attempt_id,
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
            tenant_id, approval_id, requester_subject, thread_id, turn_id, attempt_id,
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
            tenant_id, approval_id, requester_subject, thread_id, turn_id, attempt_id,
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
            tenant_id, approval_id, requester_subject, thread_id, turn_id, attempt_id,
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
            tenant_id, approval_id, requester_subject, thread_id, turn_id, attempt_id,
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
            tenant_id, approval_id, requester_subject, thread_id, turn_id, attempt_id,
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
            tenant_id, approval_id, requester_subject, thread_id, turn_id, attempt_id,
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
                tenant_id, approval_id, requester_subject, thread_id, turn_id, attempt_id,
                lease_generation, descriptor_id, descriptor_version, effect,
                action_digest, profile_id, profile_version,
                requested_at_millis, expires_at_millis,
                status, approver, decided_at_millis, version
            ) VALUES (
                'schema-check-tenant', $1, 'requester',
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

    // The requester CHECK uses the same non-whitespace predicate as the
    // approver column: a whitespace-only requester is unresolvable by any
    // valid principal, so the schema rejects it even on a pending row.
    for (description, subject) in [
        ("space-only requester", " "),
        ("tab-only requester", "\t"),
        ("newline-only requester", "\n"),
    ] {
        let rejected = harness.runtime.block_on(async {
            sqlx::query(
                "INSERT INTO tool_approvals (
                    tenant_id, approval_id, requester_subject, thread_id, turn_id, attempt_id,
                    lease_generation, descriptor_id, descriptor_version, effect,
                    action_digest, profile_id, profile_version,
                    requested_at_millis, expires_at_millis,
                    status, version
                ) VALUES (
                    'schema-check-tenant', $1, $2,
                    '00000000-0000-0000-0000-000000000000',
                    '00000000-0000-0000-0000-000000000000',
                    '00000000-0000-0000-0000-000000000000',
                    1, 'fixture.tool', 'v1', 'read_data',
                    'decoy', 'profile-default', 'v1', 1, 2,
                    'requested', 1
                )",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(subject)
            .execute(&harness.pool)
            .await
        });
        assert!(
            rejected.is_err(),
            "schema must reject a whitespace-only requester: {description}"
        );
    }
}

#[test]
fn insert_replay_after_a_terminal_transition_returns_the_canonical_state() {
    let Some(mut harness) = harness() else {
        return;
    };
    let approval = requested_approval(1_000, 60_000);
    seed_approval_owner(&harness, &approval);
    assert_eq!(
        harness.store.insert_requested(&approval, "requester"),
        Ok(ApprovalInsertResolution::Inserted)
    );
    harness
        .store
        .resolve_decision(
            approval.approval_id(),
            approval.tenant_id(),
            approval.binding().thread_id(),
            "requester",
            ApprovalDecision::Declined,
            &approver("approver-a"),
            2_000,
        )
        .expect("decision resolves");
    // A lost-acknowledgement replay after another instance resolved the
    // record reports the terminal projection, not requested version 1.
    assert_eq!(
        harness.store.insert_requested(&approval, "requester"),
        Ok(ApprovalInsertResolution::Existing {
            status: ApprovalStatus::Declined,
            decision: Some(ApprovalDecision::Declined),
            version: 2,
        })
    );

    // The same holds after the expiry transition closed another record.
    let expired = requested_approval(1_000, 2_000);
    seed_approval_owner(&harness, &expired);
    assert_eq!(
        harness.store.insert_requested(&expired, "requester"),
        Ok(ApprovalInsertResolution::Inserted)
    );
    harness
        .store
        .resolve_decision(
            expired.approval_id(),
            expired.tenant_id(),
            expired.binding().thread_id(),
            "requester",
            ApprovalDecision::Accepted,
            &approver("approver-a"),
            2_000,
        )
        .expect("late decision completes");
    assert_eq!(
        harness.store.insert_requested(&expired, "requester"),
        Ok(ApprovalInsertResolution::Existing {
            status: ApprovalStatus::Expired,
            decision: None,
            version: 2,
        })
    );
}

/// Version-2 seed for the upgrade regression: one pending D-6 whose Thread
/// is owned by `subject-a`.
const VERSION_2_MATCHED_SEED: &str = "INSERT INTO threads (tenant_id, subject_id, thread_id)
     VALUES ('tenant-upgrade', 'subject-a', '11111111-1111-1111-1111-111111111111');
     INSERT INTO tool_approvals (tenant_id, approval_id, thread_id, turn_id, attempt_id,
         lease_generation, descriptor_id, descriptor_version, effect, action_digest,
         profile_id, profile_version, requested_at_millis, expires_at_millis, status, version)
     VALUES ('tenant-upgrade', '22222222-2222-2222-2222-222222222222',
         '11111111-1111-1111-1111-111111111111', '33333333-3333-3333-3333-333333333333',
         '44444444-4444-4444-4444-444444444444', 1, 'fixture.tool', 'v1', 'external_write',
         '00', 'profile-default', 'v1', 1000, 301000, 'requested', 1);";

/// Version-2 orphan: one pending D-6 whose Thread has no owner row.
const VERSION_2_ORPHAN_SEED: &str =
    "INSERT INTO tool_approvals (tenant_id, approval_id, thread_id, turn_id, attempt_id,
         lease_generation, descriptor_id, descriptor_version, effect, action_digest,
         profile_id, profile_version, requested_at_millis, expires_at_millis, status, version)
     VALUES ('tenant-upgrade', '55555555-5555-5555-5555-555555555555',
         '66666666-6666-6666-6666-666666666666', '77777777-7777-7777-7777-777777777777',
         '88888888-8888-8888-8888-888888888888', 1, 'fixture.tool', 'v1', 'external_write',
         '00', 'profile-default', 'v1', 1000, 301000, 'requested', 1);";

/// Version-2 seed with one pending D-6 whose Thread owner is a whitespace-only
/// subject that no valid principal can carry.
const VERSION_2_WHITESPACE_OWNER_SEED: &str =
    "INSERT INTO threads (tenant_id, subject_id, thread_id)
     VALUES ('tenant-upgrade', E'\t', '99999999-9999-9999-9999-999999999999');
     INSERT INTO tool_approvals (tenant_id, approval_id, thread_id, turn_id, attempt_id,
         lease_generation, descriptor_id, descriptor_version, effect, action_digest,
         profile_id, profile_version, requested_at_millis, expires_at_millis, status, version)
     VALUES ('tenant-upgrade', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
         '99999999-9999-9999-9999-999999999999', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb',
         'cccccccc-cccc-cccc-cccc-cccccccccccc', 1, 'fixture.tool', 'v1', 'external_write',
         '00', 'profile-default', 'v1', 1000, 301000, 'requested', 1);";

/// Applies the requester-ownership migration inside one upgrade-schema
/// connection.
async fn apply_requester_ownership_migration(
    conn: &mut sqlx::postgres::PgConnection,
) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(include_str!(
        "../../migrations/0003_cand_2_requester_ownership.sql"
    ))
    .execute(conn)
    .await?;
    Ok(())
}

#[test]
fn migration_0003_backfills_the_thread_owner_and_fails_on_orphans() {
    let Some(harness) = harness() else {
        return;
    };
    harness.runtime.block_on(async {
        // A dedicated schema replays the version-2 upgrade path without
        // touching the shared public schema the other harness tests migrated.
        let mut conn = harness.pool.acquire().await.expect("upgrade connection");
        let work: Result<(bool, bool, String), sqlx::Error> = async {
            sqlx::raw_sql(
                "DROP SCHEMA IF EXISTS cand_2_upgrade CASCADE; CREATE SCHEMA cand_2_upgrade;",
            )
            .execute(&mut *conn)
            .await?;
            sqlx::raw_sql("SET search_path TO cand_2_upgrade;")
                .execute(&mut *conn)
                .await?;
            for migration in [
                include_str!("../../migrations/0001_cand_1_history.sql"),
                include_str!("../../migrations/0002_cand_2_policy_execution.sql"),
            ] {
                sqlx::raw_sql(migration).execute(&mut *conn).await?;
            }
            sqlx::raw_sql(VERSION_2_MATCHED_SEED)
                .execute(&mut *conn)
                .await?;
            // Only the orphan row violates ownership in this phase, so its
            // failure is attributable to the orphan alone.
            sqlx::raw_sql(VERSION_2_ORPHAN_SEED)
                .execute(&mut *conn)
                .await?;
            let orphan_attempt = apply_requester_ownership_migration(&mut conn).await;
            sqlx::raw_sql(
                "DELETE FROM tool_approvals WHERE approval_id = '55555555-5555-5555-5555-555555555555';",
            )
            .execute(&mut *conn)
            .await?;
            // With the orphan resolved, only the whitespace-only Thread owner
            // violates ownership in this phase.
            sqlx::raw_sql(VERSION_2_WHITESPACE_OWNER_SEED)
                .execute(&mut *conn)
                .await?;
            let whitespace_attempt = apply_requester_ownership_migration(&mut conn).await;
            sqlx::raw_sql(
                "DELETE FROM tool_approvals WHERE approval_id = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa';",
            )
            .execute(&mut *conn)
            .await?;
            for _ in 0..2 {
                apply_requester_ownership_migration(&mut conn).await?;
            }
            let (subject,): (String,) =
                sqlx::query_as("SELECT requester_subject FROM tool_approvals WHERE tenant_id = 'tenant-upgrade'")
                    .fetch_one(&mut *conn)
                    .await?;
            Ok((orphan_attempt.is_err(), whitespace_attempt.is_err(), subject))
        }
        .await;

        // The pooled connection keeps its session search_path after the
        // schema is dropped, so restore it before the connection returns to
        // the pool — otherwise a later parallel test reusing it would run
        // unqualified queries against a dropped schema. The unqualified probe
        // proves the public schema is reachable again.
        sqlx::raw_sql("SET search_path TO public; DROP SCHEMA IF EXISTS cand_2_upgrade CASCADE;")
            .execute(&mut *conn)
            .await
            .expect("restore the search path and drop the upgrade schema");
        sqlx::query("SELECT 1 FROM tool_approvals LIMIT 1")
            .execute(&mut *conn)
            .await
            .expect("the restored connection resolves public.tool_approvals");

        let (orphan_failed, whitespace_failed, subject) = work.expect("upgrade regression phases");
        assert!(
            orphan_failed,
            "an orphan pending approval alone must fail the migration"
        );
        assert!(
            whitespace_failed,
            "a whitespace-only Thread owner alone must fail the migration"
        );
        assert_eq!(
            subject, "subject-a",
            "the backfill preserves the real Thread owner instead of a placeholder"
        );
    });
}

#[test]
fn startup_migrations_wait_for_the_cross_replica_advisory_lock() {
    // Concurrently starting replicas must serialize their startup migrations:
    // while another session holds the advisory lock, the bounded migration
    // sequence waits rather than racing the PostgreSQL catalog, and completes
    // once the lock is released (ADR-0001/ADR-0003 startup contract).
    let Some(harness) = harness() else {
        return;
    };
    let key = koduck_ai::runtime::STARTUP_MIGRATION_LOCK_KEY;
    let mut holder = harness
        .runtime
        .block_on(async { harness.pool.acquire().await })
        .expect("dedicated lock connection");
    harness
        .runtime
        .block_on(async {
            sqlx::query("SELECT pg_advisory_lock($1)")
                .bind(key)
                .execute(&mut *holder)
                .await
        })
        .expect("test holds the advisory lock");

    let contested = harness.runtime.block_on(async {
        tokio::time::timeout(
            std::time::Duration::from_millis(300),
            koduck_ai::runtime::apply_startup_migrations(
                &harness.pool,
                std::time::Duration::from_secs(2),
            ),
        )
        .await
    });
    let mut holder = Some(holder);
    if contested.is_ok() {
        // Release the lock and connection before any panic unwinds outside a
        // runtime context, so a failing assertion aborts cleanly instead of
        // panicking again in the connection's Drop.
        if let Some(mut connection) = holder.take() {
            harness
                .runtime
                .block_on(async {
                    sqlx::query("SELECT pg_advisory_unlock($1)")
                        .bind(key)
                        .execute(&mut *connection)
                        .await
                        .map(|_| ())
                })
                .ok();
            harness.runtime.block_on(async {
                drop(connection);
            });
        }
        panic!("the startup migration sequence must wait for the cross-replica advisory lock");
    }

    let mut connection = holder.expect("the holder survives the contested wait");
    let unlocked = harness.runtime.block_on(async {
        sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(key)
            .execute(&mut *connection)
            .await
            .map(|_| ())
    });
    // Return the dedicated connection to the pool inside a runtime context so
    // its pool-aware Drop cannot abort the test process outside Tokio.
    harness.runtime.block_on(async {
        drop(connection);
    });
    unlocked.expect("test releases the advisory lock");
    // The uncontended bound is generous relative to the production startup
    // deadline: this leg proves completion after the lock is released, not
    // the deadline itself, and CI runners vary widely on cold-cache DDL.
    let uncontended = harness.runtime.block_on(async {
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            koduck_ai::runtime::apply_startup_migrations(
                &harness.pool,
                std::time::Duration::from_secs(15),
            ),
        )
        .await
    });
    assert!(
        matches!(uncontended, Ok(Ok(()))),
        "the sequence completes after the lock is released"
    );
}
