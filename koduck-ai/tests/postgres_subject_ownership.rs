// ADR: docs/adr/ADR-0002-required-ai-ci-postgres-verification.md

use std::thread;

use koduck_ai::adapters::history::postgres::{
    LeaseTiming, PostgresExecutor, RecoveryOutcome, SqlxPostgresExecutor,
};
use koduck_ai::application::{AcceptedTurn, HistoryError, NewItem, TurnCommand};
use koduck_ai::domain::{ItemPayload, TenantId, TerminalOutcome, TrustContext, Usage};
use sqlx::postgres::{PgPool, PgPoolOptions};
use tokio::runtime::Runtime;
use uuid::Uuid;

#[test]
fn production_postgres_contract() {
    let Ok(database_url) = std::env::var("KODUCK_AI_TEST_DATABASE_URL") else {
        return;
    };
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
    runtime
        .block_on(
            sqlx::raw_sql(include_str!("../migrations/0001_cand_1_history.sql")).execute(&pool),
        )
        .expect("apply production migration");
    let executor = SqlxPostgresExecutor::new(pool.clone(), runtime.handle().clone());

    let tenant = TenantId::new(format!("ci-{}", Uuid::new_v4())).expect("unique tenant");
    let owner = TrustContext::new(tenant.clone(), "owner").expect("owner trust context");
    let intruder = TrustContext::new(tenant.clone(), "intruder").expect("intruder trust context");
    let accepted = verify_payload_and_subject_ownership(&executor, &tenant, &owner, &intruder);
    verify_interrupt_terminal_arbitration(&executor, &tenant, &owner, &accepted);
    verify_recovery_pending_interrupt_is_rejected(&runtime, &pool, &executor, &tenant, &owner);
    verify_expired_started_interrupt_is_rejected(&runtime, &pool, &executor, &tenant, &owner);
    verify_stale_generation_fencing(&runtime, &pool, &executor, &tenant, owner);

    runtime.block_on(pool.close());
}

fn verify_expired_started_interrupt_is_rejected(
    runtime: &Runtime,
    pool: &PgPool,
    executor: &SqlxPostgresExecutor,
    tenant: &TenantId,
    owner: &TrustContext,
) {
    let command =
        TurnCommand::new(owner.clone(), None, "expired interrupt").expect("valid expired command");
    let accepted = executor
        .accept_initial(&command)
        .expect("accept expired fixture");
    runtime
        .block_on(
            sqlx::query(
                "UPDATE turn_leases SET renewed_at = CURRENT_TIMESTAMP - INTERVAL '23 seconds', \
                 expires_at = CURRENT_TIMESTAMP - INTERVAL '3 seconds' \
                 WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
            )
            .bind(tenant.as_str())
            .bind(accepted.thread_id.as_uuid())
            .bind(accepted.turn_id.as_uuid())
            .execute(pool),
        )
        .expect("expire active fixture");

    assert_eq!(
        executor.request_interrupt(owner, accepted.turn_id),
        Err(HistoryError::Fenced),
        "an expired owner must not accept an interrupt after its live stream can end"
    );
}

fn verify_recovery_pending_interrupt_is_rejected(
    runtime: &Runtime,
    pool: &PgPool,
    executor: &SqlxPostgresExecutor,
    tenant: &TenantId,
    owner: &TrustContext,
) {
    let command = TurnCommand::new(owner.clone(), None, "recovery interrupt")
        .expect("valid recovery command");
    let accepted = executor
        .accept_initial(&command)
        .expect("accept recovery fixture");
    runtime
        .block_on(
            sqlx::query(
                "UPDATE turns SET status = 'recovery-pending' \
                 WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
            )
            .bind(tenant.as_str())
            .bind(accepted.thread_id.as_uuid())
            .bind(accepted.turn_id.as_uuid())
            .execute(pool),
        )
        .expect("enter durable recovery-pending state");

    assert_eq!(
        executor.request_interrupt(owner, accepted.turn_id),
        Err(HistoryError::Fenced),
        "a detached recovery-pending turn must not accept an interrupt it cannot stream"
    );
    assert_eq!(
        executor.recover_failed(&accepted, LeaseTiming::cand_1()),
        Ok(RecoveryOutcome::Failed)
    );
    let replay = executor
        .replay(tenant, accepted.turn_id)
        .expect("recovery interrupt replay");
    assert!(matches!(
        replay.last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Failed { .. }))
    ));
}

fn verify_payload_and_subject_ownership(
    executor: &SqlxPostgresExecutor,
    tenant: &TenantId,
    owner: &TrustContext,
    intruder: &TrustContext,
) -> AcceptedTurn {
    let input = "contract-valid\0payload";
    let command = TurnCommand::new(owner.clone(), None, input).expect("valid command");
    let accepted = executor
        .accept_initial(&command)
        .expect("production acceptance transaction");

    let replay = executor
        .replay(tenant, accepted.turn_id)
        .expect("production replay");
    assert!(matches!(
        &replay[0].payload,
        ItemPayload::UserMessage { content } if content == input
    ));
    assert_eq!(
        executor.prior_thread_items(intruder, accepted.thread_id),
        Err(HistoryError::NotFound),
        "a different subject must not observe the thread"
    );
    accepted
}

fn verify_interrupt_terminal_arbitration(
    executor: &SqlxPostgresExecutor,
    tenant: &TenantId,
    owner: &TrustContext,
    accepted: &AcceptedTurn,
) {
    executor
        .request_interrupt(owner, accepted.turn_id)
        .expect("persist accepted interrupt");
    let completion_executor = executor.clone();
    let completion_turn = accepted.clone();
    let completion = thread::spawn(move || {
        completion_executor.append(
            &completion_turn,
            NewItem::Terminal(TerminalOutcome::Completed {
                usage: Usage::zero(),
            }),
        )
    });
    let failure_executor = executor.clone();
    let failure_turn = accepted.clone();
    let failure = thread::spawn(move || {
        failure_executor.append(
            &failure_turn,
            NewItem::Terminal(TerminalOutcome::Failed {
                code: "RACED_PROVIDER_FAILURE".to_owned(),
            }),
        )
    });
    let terminal_results = [
        completion.join().expect("completion contender joins"),
        failure.join().expect("failure contender joins"),
    ];
    assert_eq!(
        terminal_results
            .iter()
            .filter(|result| result.is_ok())
            .count(),
        0,
        "an accepted interrupt is already the durable terminal winner"
    );
    assert_eq!(
        terminal_results
            .iter()
            .filter(|result| **result == Err(HistoryError::AlreadyTerminal))
            .count(),
        2
    );
    let terminal_replay = executor
        .replay(tenant, accepted.turn_id)
        .expect("terminal replay");
    assert_eq!(
        terminal_replay
            .iter()
            .filter(|item| matches!(item.payload, ItemPayload::Terminal(_)))
            .count(),
        1
    );
    assert!(matches!(
        terminal_replay.last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Interrupted))
    ));
}

fn verify_stale_generation_fencing(
    runtime: &Runtime,
    pool: &PgPool,
    executor: &SqlxPostgresExecutor,
    tenant: &TenantId,
    owner: TrustContext,
) {
    let stale_command = TurnCommand::new(owner, None, "stale owner").expect("valid command");
    let stale = executor
        .accept_initial(&stale_command)
        .expect("accept stale-owner fixture");
    let durable_before = executor
        .replay(tenant, stale.turn_id)
        .expect("stale fixture replay");
    runtime
        .block_on(
            sqlx::query(
                "UPDATE turn_leases SET fenced = TRUE, generation = generation + 1 \
                 WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
            )
            .bind(tenant.as_str())
            .bind(stale.thread_id.as_uuid())
            .bind(stale.turn_id.as_uuid())
            .execute(pool),
        )
        .expect("fence accepted generation");
    assert_eq!(
        executor.append(
            &stale,
            NewItem::AgentMessageDelta {
                content: "must not persist".to_owned(),
            },
        ),
        Err(HistoryError::Fenced)
    );
    assert_eq!(
        executor
            .replay(tenant, stale.turn_id)
            .expect("replay remains readable"),
        durable_before,
        "a stale generation must not add an Item"
    );
}
