// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Named AC-12 black-box harness: canonical D-6 and D-7 transitions permit
//! exactly one winner under multi-instance races against a disposable
//! production `PostgreSQL` (ADR-0003 TC-12).

use std::sync::Arc;
use std::sync::Barrier;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use koduck_ai::adapters::execution::{SqlxApprovalRecordStore, SqlxExecutionAttemptStore};
use koduck_ai::adapters::http::{ServiceError, TurnService};
use koduck_ai::application::{
    ApprovalDecisionRoute, AttemptInsertResolution, AttemptTerminalResolution,
    CanonicalAttemptTerminal, DispatchClaimResolution, DurableAttemptTerminal, EffectState,
    ExecutionAttemptStore, ToolExecutionOutcome, TurnResult,
};
use koduck_ai::domain::execution::{ApprovalId, AttemptId, ExactActionBinding, ExecutionStatus};
use koduck_ai::domain::tool::{Action, Effect};
use koduck_ai::domain::{LeaseGeneration, TenantId, ThreadId, TrustContext, TurnId, TurnStatus};
use koduck_ai::runtime::build_router;
use sqlx::postgres::{PgPool, PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

const MIGRATIONS: [&str; 7] = [
    include_str!("../migrations/0001_cand_1_history.sql"),
    include_str!("../migrations/0002_cand_2_policy_execution.sql"),
    include_str!("../migrations/0003_cand_2_requester_ownership.sql"),
    include_str!("../migrations/0004_cand_2_tool_projections.sql"),
    include_str!("../migrations/0005_cand_2_execution_attempts.sql"),
    include_str!("../migrations/0006_cand_2_interrupt_barrier.sql"),
    include_str!("../migrations/0007_cand_2_tool_audit.sql"),
];

struct Harness {
    runtime: tokio::runtime::Runtime,
    pool: PgPool,
}

// Applied once per process; each application runs every migration twice so
// the idempotency leg of this check is exercised on every run.
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
                // The decision-race leg runs 32 concurrent route contenders,
                // each holding one pooled connection across its transaction;
                // the pool must admit them all so the store's 2-second
                // deadline measures transition contention, not pool queuing.
                .max_connections(32)
                .connect(&database_url),
        )
        .expect("connect to disposable PostgreSQL");
    MIGRATION.get_or_init(|| {
        for _ in 0..2 {
            for migration in MIGRATIONS {
                runtime
                    .block_on(async { sqlx::raw_sql(migration).execute(&pool).await })
                    .expect("apply production migration twice idempotently");
            }
        }
    });
    Some(Harness { runtime, pool })
}

#[derive(Clone)]
struct StubTurns;

impl TurnService for StubTurns {
    fn execute(
        &mut self,
        _command: koduck_ai::application::TurnCommand,
    ) -> Result<TurnResult, ServiceError> {
        Ok(TurnResult {
            thread_id: ThreadId::new(),
            turn_id: TurnId::new(),
            status: TurnStatus::Completed,
            published: Vec::new(),
            replay: Vec::new(),
        })
    }

    fn interrupt(&mut self, _trust: &TrustContext, _turn_id: TurnId) -> Result<(), ServiceError> {
        Ok(())
    }
}

async fn response_body(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), 1_048_576)
        .await
        .expect("bounded response body");
    String::from_utf8(bytes.to_vec()).expect("response body is UTF-8")
}

/// Seeds the authenticated C-6 owner required by the D-7 race fixture.
fn seed_current_lease(harness: &Harness, binding: &ExactActionBinding) {
    harness.runtime.block_on(async {
        sqlx::query(
            "INSERT INTO threads (tenant_id, subject_id, thread_id) \
             VALUES ($1, 'd7-race-fixture', $2) ON CONFLICT DO NOTHING",
        )
        .bind(binding.tenant_id().as_str())
        .bind(binding.thread_id().as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture thread exists");
        sqlx::query(
            "INSERT INTO turns (tenant_id, thread_id, turn_id, status, next_sequence) \
             VALUES ($1, $2, $3, 'started', 1) ON CONFLICT DO NOTHING",
        )
        .bind(binding.tenant_id().as_str())
        .bind(binding.thread_id().as_uuid())
        .bind(binding.turn_id().as_uuid())
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
        .bind(binding.tenant_id().as_str())
        .bind(binding.thread_id().as_uuid())
        .bind(binding.turn_id().as_uuid())
        .bind(i64::try_from(binding.lease_generation().get()).expect("lease fits i64"))
        .execute(&harness.pool)
        .await
        .expect("fixture lease exists");
    });
}

/// Seeds one requested D-6 row with the exact canonical shape
/// `SqlxApprovalRecordStore::insert_requested` writes.
///
/// Constructing the domain `ApprovalRequest` itself requires the
/// crate-internal C-5 sealing service, which is deliberately not a public
/// extension point (TC-05), so the fixture mirrors the committed row instead
/// of widening the crate surface. The transition under race is the
/// conditional `requested -> accepted` UPDATE, not the seed.
fn seed_requested_approval(
    harness: &Harness,
    tenant: &TenantId,
    thread: ThreadId,
    approval_id: ApprovalId,
) {
    harness
        .runtime
        .block_on(
            sqlx::query(
                "
                INSERT INTO tool_approvals (
                    tenant_id, approval_id, requester_subject, thread_id, turn_id,
                    attempt_id, lease_generation, descriptor_id, descriptor_version,
                    effect, action_digest, profile_id, profile_version,
                    requested_at_millis, expires_at_millis, status, version
                ) VALUES (
                    $1, $2, 'requester', $3,
                    '00000000-0000-0000-0000-000000000001',
                    '00000000-0000-0000-0000-000000000000', 1,
                    'fixture.tool', 'v1', 'external_write',
                    '0000000000000000000000000000000000000000000000000000000000000000',
                    'profile-default', 'v1', 1000, 4102444800000, 'requested', 1
                )
                ",
            )
            .bind(tenant.as_str())
            .bind(approval_id.as_uuid())
            .bind(thread.as_uuid())
            .execute(&harness.pool),
        )
        .expect("seed requested approval");
}

#[test]
fn postgres_cand_2_transitions_are_single_winner() {
    let Some(harness) = harness() else {
        return;
    };
    let contenders = 32;
    let tenant = TenantId::new(format!("ci-{}", Uuid::new_v4())).expect("valid tenant");
    let thread = ThreadId::new();

    // D-6 decision leg through the production HTTP route and SQLx store.
    race_d6_decision_single_winner(&harness, &tenant, thread, contenders);

    // D-7 dispatch-claim and terminal-commit legs through the production
    // D-7 store port.
    let mut attempts =
        SqlxExecutionAttemptStore::new(harness.pool.clone(), harness.runtime.handle().clone());
    let binding = ExactActionBinding::new(
        tenant.clone(),
        thread,
        TurnId::new(),
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
    seed_current_lease(&harness, &binding);
    assert_eq!(
        attempts.insert_prepared(&binding, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );
    race_d7_dispatch_claim_single_winner(&attempts, &binding, contenders);
    race_d7_terminal_commit_single_winner(&mut attempts, &binding, contenders);
}

/// D-6 leg: 32 racing decisions through the production HTTP route over the
/// production `SQLx` store. Every contender observes the identical canonical
/// terminal projection; the durable record proves exactly one transition won
/// (version 2, one approver, one decision timestamp).
fn race_d6_decision_single_winner(
    harness: &Harness,
    tenant: &TenantId,
    thread: ThreadId,
    contenders: usize,
) {
    let approval_id = ApprovalId::new();
    seed_requested_approval(harness, tenant, thread, approval_id);

    let approvals =
        SqlxApprovalRecordStore::new(harness.pool.clone(), harness.runtime.handle().clone());
    let router = build_router(StubTurns, ApprovalDecisionRoute::new(approvals));
    let barrier = Arc::new(Barrier::new(contenders));
    let responses: Vec<(StatusCode, String)> = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 0..contenders {
            let router = router.clone();
            let barrier = Arc::clone(&barrier);
            let runtime = &harness.runtime;
            let tenant = tenant.clone();
            handles.push(scope.spawn(move || {
                let request = Request::post(format!(
                    "/api/v1/ai/approvals/{}/decisions",
                    approval_id.as_uuid()
                ))
                .header("content-type", "application/json")
                .header("x-koduck-approval-scopes", "ai.tool.approve")
                .header("x-koduck-tenant-id", tenant.as_str())
                .header("x-koduck-subject-id", "requester")
                .header("x-koduck-thread-id", thread.as_uuid().to_string())
                .body(Body::from(r#"{"decision":"accepted"}"#))
                .expect("valid decision request");
                barrier.wait();
                runtime.block_on(async {
                    let response = router
                        .oneshot(request)
                        .await
                        .expect("decision route answers");
                    (response.status(), response_body(response).await)
                })
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().expect("decision contender completes"))
            .collect()
    });
    for (status, body) in &responses {
        assert_eq!(*status, StatusCode::OK, "decision response body: {body}");
        assert_eq!(
            body.as_str(),
            format!(
                "{{\"approval_id\":\"{}\",\"status\":\"accepted\",\"decision\":\"accepted\",\"version\":2}}",
                approval_id.as_uuid()
            ),
            "every contender observes the identical canonical terminal projection"
        );
    }
    let canonical: (String, Option<String>, i64) = harness
        .runtime
        .block_on(
            sqlx::query_as(
                "SELECT status, approver, version FROM tool_approvals
                 WHERE tenant_id = $1 AND approval_id = $2",
            )
            .bind(tenant.as_str())
            .bind(approval_id.as_uuid())
            .fetch_one(&harness.pool),
        )
        .expect("canonical approval row");
    assert_eq!(
        canonical,
        ("accepted".to_owned(), Some("requester".to_owned()), 2)
    );
}

/// D-7 dispatch-claim leg: 32 racing claims on one prepared attempt; only
/// the single winner is permitted an executor dispatch (dispatch count 1).
fn race_d7_dispatch_claim_single_winner(
    attempts: &SqlxExecutionAttemptStore,
    binding: &ExactActionBinding,
    contenders: usize,
) {
    let dispatches = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(contenders));
    let claims: Vec<DispatchClaimResolution> = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 0..contenders {
            let mut attempts = attempts.clone();
            let barrier = Arc::clone(&barrier);
            let dispatches = Arc::clone(&dispatches);
            let binding = binding.clone();
            handles.push(scope.spawn(move || {
                barrier.wait();
                let resolution = attempts
                    .claim_running(&binding, 2_000)
                    .expect("claim contender completes");
                if matches!(resolution, DispatchClaimResolution::Claimed { .. }) {
                    dispatches.fetch_add(1, Ordering::SeqCst);
                }
                resolution
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().expect("claim contender completes"))
            .collect()
    });
    let mut claimed = 0;
    let mut existing = 0;
    for resolution in claims {
        match resolution {
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
    assert_eq!(
        dispatches.load(Ordering::SeqCst),
        1,
        "executor dispatch count is exactly 1"
    );
}

/// D-7 terminal-commit leg: 32 racing terminal commits converge on one
/// canonical terminal; every loser and the idempotent replay observe that
/// same terminal D-7 projection.
fn race_d7_terminal_commit_single_winner(
    attempts: &mut SqlxExecutionAttemptStore,
    binding: &ExactActionBinding,
    contenders: usize,
) {
    let terminal = DurableAttemptTerminal::from_outcome(&ToolExecutionOutcome::Succeeded {
        output: b"committed".to_vec(),
        effect_state: EffectState::Started,
    });
    let barrier = Arc::new(Barrier::new(contenders));
    let terminals: Vec<AttemptTerminalResolution> = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 0..contenders {
            let mut attempts = attempts.clone();
            let barrier = Arc::clone(&barrier);
            let terminal = terminal.clone();
            let binding = binding.clone();
            handles.push(scope.spawn(move || {
                barrier.wait();
                attempts
                    .commit_terminal(&binding, &terminal, 3_000)
                    .expect("terminal contender completes")
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().expect("terminal contender completes"))
            .collect()
    });
    let canonical_outcome = ToolExecutionOutcome::Succeeded {
        output: b"committed".to_vec(),
        effect_state: EffectState::Started,
    };
    let mut won = 0;
    let mut replayed = 0;
    for resolution in terminals {
        match resolution {
            AttemptTerminalResolution::Won { version } => {
                assert_eq!(version, 3);
                won += 1;
            }
            AttemptTerminalResolution::ExistingTerminal(canonical) => {
                assert_eq!(canonical.binding(), binding);
                assert_eq!(canonical.version(), 3);
                assert_eq!(canonical.outcome(), &canonical_outcome);
                replayed += 1;
            }
            other => panic!("unexpected terminal resolution: {other:?}"),
        }
    }
    assert_eq!(won, 1, "exactly one terminal commit wins");
    assert_eq!(replayed, contenders - 1);

    // The idempotent replay contains exactly the one terminal D-7 projection.
    assert_eq!(
        attempts.commit_terminal(binding, &terminal, 4_000),
        Ok(AttemptTerminalResolution::ExistingTerminal(Box::new(
            CanonicalAttemptTerminal::from_persistence(binding.clone(), 3, canonical_outcome)
                .expect("canonical terminal validates"),
        ))),
    );
}
