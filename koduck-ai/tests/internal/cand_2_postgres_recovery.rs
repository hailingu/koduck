// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Recovery, lease, and approval-audit legs of the canonical `PostgreSQL`
//! persistence harness (ADR-0003 TC-10, TC-12, and TC-14).

use super::harness;
use koduck_ai::adapters::execution::SqlxApprovalRecordStore;
use koduck_ai::adapters::history::postgres::PostgresExecutor;
use koduck_ai::application::{AcceptedTurn, TurnHistory};
use koduck_ai::domain::execution::{ApprovalStatus, ExecutionStatus};
use koduck_ai::domain::{Item, ItemPayload, TenantId, ToolEffectState};

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one durable leg seeding every persisted correlation field before the reconciliation and audit assertions"
)]
fn foreground_recovery_closes_the_correlated_attempt_audit_records() {
    // Foreground recovery closing a prepared or running D-7 must also emit its
    // bounded correlated audit record in the same transaction — the crash
    // path needing operator evidence cannot be the one path without it
    // (ADR-0003 TC-14).
    let Some(harness) = harness() else {
        return;
    };
    let tenant = TenantId::new("recovery-audit").expect("valid tenant");
    let thread = koduck_ai::domain::ThreadId::new();
    let turn = koduck_ai::domain::TurnId::new();
    let generation = koduck_ai::domain::LeaseGeneration::initial();
    let parameters =
        koduck_ai::adapters::tool::parse_action_parameters("{}").expect("valid parameters");
    let action = koduck_ai::domain::tool::Action::new(
        "fixture.tool",
        "v1",
        koduck_ai::domain::tool::Effect::ExternalWrite,
        "fixture-target",
        parameters,
    )
    .expect("valid action");
    let binding = koduck_ai::domain::execution::ExactActionBinding::new(
        tenant.clone(),
        thread,
        turn,
        generation,
        ("profile-default", "v1"),
        koduck_ai::domain::execution::AttemptId::new(),
        action,
    )
    .expect("valid binding");
    let digest_hex = {
        let mut text = String::new();
        for byte in binding.action_digest().as_bytes() {
            use std::fmt::Write as _;
            let _ = write!(text, "{byte:02x}");
        }
        text
    };
    harness.runtime.block_on(async {
        sqlx::query(
            "INSERT INTO threads (tenant_id, subject_id, thread_id) \
             VALUES ($1, 'recovery-audit', $2) ON CONFLICT DO NOTHING",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture thread");
        sqlx::query(
            "INSERT INTO turns (tenant_id, thread_id, turn_id, status, next_sequence) \
             VALUES ($1, $2, $3, 'started', 1) ON CONFLICT DO NOTHING",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture turn");
        sqlx::query(
            "INSERT INTO turn_leases \
             (tenant_id, thread_id, turn_id, generation, renewed_at, expires_at, fenced) \
             VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP, \
                     CURRENT_TIMESTAMP + INTERVAL '1 hour', FALSE) \
             ON CONFLICT DO NOTHING",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .bind(1_i64)
        .execute(&harness.pool)
        .await
        .expect("fixture current lease");
        sqlx::query(
            "INSERT INTO tool_execution_attempts \
             (tenant_id, attempt_id, thread_id, turn_id, lease_generation, \
              descriptor_id, descriptor_version, effect, action_digest, \
              profile_id, profile_version, prepared_at_millis, started_at_millis, \
              status, version) \
             VALUES ($1, $5, $2, $3, $4, 'fixture.tool', 'v1', 'external_write', $6, \
                     'profile-default', 'v1', 1, 2, 'running', 2)",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .bind(1_i64)
        .bind(binding.attempt_id().as_uuid())
        .bind(&digest_hex)
        .execute(&harness.pool)
        .await
        .expect("fixture running attempt");
    });
    let executor = koduck_ai::adapters::history::postgres::SqlxPostgresExecutor::new(
        harness.pool.clone(),
        harness.runtime.handle().clone(),
    );
    let history =
        koduck_ai::adapters::history::postgres::PostgresTurnHistory::new(executor.clone());
    let accepted = AcceptedTurn::new(
        tenant.clone(),
        thread,
        turn,
        generation,
        Item::new(
            1,
            ItemPayload::UserMessage {
                content: "recovery fixture".to_owned(),
            },
        ),
    );
    assert_eq!(
        executor.recover_failed(
            &accepted,
            koduck_ai::adapters::history::postgres::LeaseTiming::cand_1(),
        ),
        Ok(koduck_ai::adapters::history::postgres::RecoveryOutcome::Pending),
        "the first foreground recovery attempt enters recovery-pending"
    );
    assert_eq!(
        executor.recover_failed(
            &accepted,
            koduck_ai::adapters::history::postgres::LeaseTiming::cand_1(),
        ),
        Ok(koduck_ai::adapters::history::postgres::RecoveryOutcome::Failed),
        "the foreground recovery closes its active C-5 state before terminalizing"
    );
    let replayed = history
        .replay(&tenant, turn)
        .expect("recovery projections are replayable");
    assert!(
        replayed.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::ToolResult {
                attempt_id: Some(attempt_id),
                status: ExecutionStatus::TimedOut,
                effect_state: Some(ToolEffectState::Unknown),
                code: None,
                output_bytes: 0,
                output_digest: None,
                version: Some(3),
            } if *attempt_id == binding.attempt_id()
        )),
        "expiry recovery appends the D-3 terminal projection for its closed D-7 attempt"
    );

    let audits: Vec<String> = harness
        .runtime
        .block_on(async {
            sqlx::query_scalar(
                "SELECT record FROM tool_audit_records \
                 WHERE tenant_id = $1 AND turn_id = $2",
            )
            .bind(tenant.as_str())
            .bind(turn.as_uuid())
            .fetch_all(&harness.pool)
            .await
        })
        .expect("audit rows are readable");
    assert_eq!(
        audits.len(),
        1,
        "one correlated audit row per closed attempt"
    );
    let record = &audits[0];
    assert!(
        record.contains(&binding.attempt_id().as_uuid().to_string()),
        "record correlates the closed attempt"
    );
    assert!(
        record.contains("timed_out"),
        "record carries the timed_out terminal"
    );
    assert!(
        record.contains(&digest_hex),
        "record carries the action digest"
    );
}

#[test]
fn renewal_stops_once_the_durable_interruption_barrier_commits() {
    // An authenticated interruption that reaches another replica commits the
    // durable barrier; the owning replica's renewal must stop at its next
    // beat so the foreground lease expires and the running effect stays
    // bounded by the fencing and expiry paths (ADR-0003 TC-10).
    let Some(harness) = harness() else {
        return;
    };
    let tenant = TenantId::new("renewal-barrier").expect("valid tenant");
    let thread = koduck_ai::domain::ThreadId::new();
    let turn = koduck_ai::domain::TurnId::new();
    let generation = koduck_ai::domain::LeaseGeneration::initial();
    let key = koduck_ai::adapters::history::postgres::LeaseKey::new(
        tenant.clone(),
        thread,
        turn,
        generation,
    );
    harness.runtime.block_on(async {
        sqlx::query(
            "INSERT INTO threads (tenant_id, subject_id, thread_id) \
             VALUES ($1, 'renewal-barrier', $2) ON CONFLICT DO NOTHING",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture thread");
        sqlx::query(
            "INSERT INTO turns (tenant_id, thread_id, turn_id, status, next_sequence) \
             VALUES ($1, $2, $3, 'started', 1) ON CONFLICT DO NOTHING",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture turn");
        sqlx::query(
            "INSERT INTO turn_leases \
             (tenant_id, thread_id, turn_id, generation, renewed_at, expires_at, fenced) \
             VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP, \
                     CURRENT_TIMESTAMP + INTERVAL '20 seconds', FALSE) \
             ON CONFLICT DO NOTHING",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .bind(1_i64)
        .execute(&harness.pool)
        .await
        .expect("fixture live lease");
    });
    let executor = koduck_ai::adapters::history::postgres::SqlxPostgresExecutor::new(
        harness.pool.clone(),
        harness.runtime.handle().clone(),
    );
    let mut history = koduck_ai::adapters::history::postgres::PostgresTurnHistory::new(executor);
    history
        .renew_lease(&key, koduck_ai::adapters::history::postgres::unix_time_ms())
        .expect("an unbarriered started Turn renews");

    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turns SET interrupting = TRUE \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture barrier committed");
    });
    assert_eq!(
        history.renew_lease(&key, koduck_ai::adapters::history::postgres::unix_time_ms()),
        Err(koduck_ai::application::HistoryError::Fenced),
        "renewal stops once the durable interruption barrier commits"
    );
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one durable leg seeding the full approval correlation row before the route decision and audit assertions"
)]
fn a_won_approval_decision_emits_its_correlated_audit_record() {
    // The production HTTP decision route writes the canonical D-6 terminal;
    // that transition must also append its bounded correlated audit record
    // atomically (ADR-0003 TC-14).
    let Some(harness) = harness() else {
        return;
    };
    let tenant = TenantId::new("approval-audit").expect("valid tenant");
    let thread = koduck_ai::domain::ThreadId::new();
    let turn = koduck_ai::domain::TurnId::new();
    let approval_id = koduck_ai::domain::execution::ApprovalId::new();
    let parameters = koduck_ai::adapters::tool::parse_action_parameters("{}").expect("valid");
    let action = koduck_ai::domain::tool::Action::new(
        "fixture.tool",
        "v1",
        koduck_ai::domain::tool::Effect::ExternalWrite,
        "fixture-target",
        parameters,
    )
    .expect("valid action");
    let binding = koduck_ai::domain::execution::ExactActionBinding::new(
        tenant.clone(),
        thread,
        turn,
        koduck_ai::domain::LeaseGeneration::initial(),
        ("profile-default", "v1"),
        koduck_ai::domain::execution::AttemptId::new(),
        action,
    )
    .expect("valid binding");
    let digest_hex = {
        let mut text = String::new();
        for byte in binding.action_digest().as_bytes() {
            use std::fmt::Write as _;
            let _ = write!(text, "{byte:02x}");
        }
        text
    };
    harness.runtime.block_on(async {
        sqlx::query(
            "INSERT INTO threads (tenant_id, subject_id, thread_id) \
             VALUES ($1, 'approver-a', $2) ON CONFLICT DO NOTHING",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture thread");
        sqlx::query(
            "INSERT INTO turns (tenant_id, thread_id, turn_id, status, next_sequence) \
             VALUES ($1, $2, $3, 'started', 1) ON CONFLICT DO NOTHING",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture turn");
        sqlx::query(
            "INSERT INTO tool_approvals \
             (tenant_id, approval_id, thread_id, turn_id, attempt_id, lease_generation, \
              descriptor_id, descriptor_version, effect, action_digest, profile_id, \
              profile_version, requested_at_millis, expires_at_millis, status, \
              requester_subject, version) \
             VALUES ($1, $2, $3, $4, $5, 1, 'fixture.tool', 'v1', 'external_write', $6, \
                     'profile-default', 'v1', 1, 600000, 'requested', 'approver-a', 1)",
        )
        .bind(tenant.as_str())
        .bind(approval_id.as_uuid())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .bind(binding.attempt_id().as_uuid())
        .bind(&digest_hex)
        .execute(&harness.pool)
        .await
        .expect("fixture requested approval");
    });

    let store =
        SqlxApprovalRecordStore::new(harness.pool.clone(), harness.runtime.handle().clone());
    let mut route = koduck_ai::application::ApprovalDecisionRoute::new(store);
    let trust = koduck_ai::domain::TrustContext::new(tenant.clone(), "approver-a")
        .expect("valid principal")
        .with_approval_scopes(koduck_ai::domain::ApprovalScopes::from_validated(vec![
            "ai.tool.approve".to_owned(),
        ]));
    let outcome = route.decide(
        &trust,
        thread,
        approval_id,
        koduck_ai::domain::execution::ApprovalDecision::Accepted,
        10_000,
    );
    assert!(
        matches!(
            outcome,
            koduck_ai::application::ApprovalDecisionOutcome::Resolved { version: 2, .. }
        ),
        "the decision wins, found {outcome:?}"
    );

    let audits: Vec<String> = harness
        .runtime
        .block_on(async {
            sqlx::query_scalar(
                "SELECT record FROM tool_audit_records \
                 WHERE tenant_id = $1 AND turn_id = $2",
            )
            .bind(tenant.as_str())
            .bind(turn.as_uuid())
            .fetch_all(&harness.pool)
            .await
        })
        .expect("audit rows are readable");
    assert_eq!(audits.len(), 1, "the won decision appends its audit record");
    let record = &audits[0];
    assert!(
        record.contains(&approval_id.as_uuid().to_string()),
        "record correlates the approval"
    );
    assert!(
        record.contains(&binding.attempt_id().as_uuid().to_string()),
        "record correlates the attempt"
    );
    assert!(
        record.contains(&digest_hex),
        "record carries the action digest"
    );
    assert!(record.contains("accepted"), "record carries the decision");
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one durable leg seeding the approval and expired-lease fixtures before the recovery and decision assertions"
)]
fn a_decision_on_a_requested_approval_is_rejected_after_the_turn_terminalizes() {
    // Lease recovery terminalizes a Turn whose foreground owner died and
    // closes its attempts, but a requested D-6 could still be decided inside
    // its five-minute window; the canonical D-6 transition must therefore
    // also condition on the owning Turn remaining non-terminal, so a decision
    // arriving after recovery cannot return success for a cancelled attempt
    // under a terminal Turn (ADR-0003 D-6 state machine, TC-12).
    let Some(harness) = harness() else {
        return;
    };
    let tenant = TenantId::new("terminal-turn-decision").expect("valid tenant");
    let thread = koduck_ai::domain::ThreadId::new();
    let turn = koduck_ai::domain::TurnId::new();
    let approval_id = koduck_ai::domain::execution::ApprovalId::new();
    let parameters = koduck_ai::adapters::tool::parse_action_parameters("{}").expect("valid");
    let action = koduck_ai::domain::tool::Action::new(
        "fixture.tool",
        "v1",
        koduck_ai::domain::tool::Effect::ExternalWrite,
        "fixture-target",
        parameters,
    )
    .expect("valid action");
    let binding = koduck_ai::domain::execution::ExactActionBinding::new(
        tenant.clone(),
        thread,
        turn,
        koduck_ai::domain::LeaseGeneration::initial(),
        ("profile-default", "v1"),
        koduck_ai::domain::execution::AttemptId::new(),
        action,
    )
    .expect("valid binding");
    let digest_hex = {
        let mut text = String::new();
        for byte in binding.action_digest().as_bytes() {
            use std::fmt::Write as _;
            let _ = write!(text, "{byte:02x}");
        }
        text
    };
    harness.runtime.block_on(async {
        sqlx::query(
            "INSERT INTO threads (tenant_id, subject_id, thread_id) \
             VALUES ($1, 'approver-a', $2) ON CONFLICT DO NOTHING",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture thread");
        sqlx::query(
            "INSERT INTO turns (tenant_id, thread_id, turn_id, status, next_sequence) \
             VALUES ($1, $2, $3, 'started', 1) ON CONFLICT DO NOTHING",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture turn");
        sqlx::query(
            "INSERT INTO turn_leases \
             (tenant_id, thread_id, turn_id, generation, renewed_at, expires_at, fenced) \
             VALUES ($1, $2, $3, 1, CURRENT_TIMESTAMP - INTERVAL '1 hour', \
                     CURRENT_TIMESTAMP - INTERVAL '55 minutes', FALSE) \
             ON CONFLICT DO NOTHING",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture expired lease");
        sqlx::query(
            "INSERT INTO tool_approvals \
             (tenant_id, approval_id, thread_id, turn_id, attempt_id, lease_generation, \
              descriptor_id, descriptor_version, effect, action_digest, profile_id, \
              profile_version, requested_at_millis, expires_at_millis, status, \
              requester_subject, version) \
             VALUES ($1, $2, $3, $4, $5, 1, 'fixture.tool', 'v1', 'external_write', $6, \
                     'profile-default', 'v1', 1, 600000, 'requested', 'approver-a', 1)",
        )
        .bind(tenant.as_str())
        .bind(approval_id.as_uuid())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .bind(binding.attempt_id().as_uuid())
        .bind(&digest_hex)
        .execute(&harness.pool)
        .await
        .expect("fixture requested approval");
    });

    // Expiry recovery terminalizes the Turn (cancelled) with its lease fenced.
    let executor = koduck_ai::adapters::history::postgres::SqlxPostgresExecutor::new(
        harness.pool.clone(),
        harness.runtime.handle().clone(),
    );
    let key = koduck_ai::adapters::history::postgres::LeaseKey::new(
        tenant.clone(),
        thread,
        turn,
        koduck_ai::domain::LeaseGeneration::initial(),
    );
    let mut history = koduck_ai::adapters::history::postgres::PostgresTurnHistory::new(executor);
    let outcome =
        history.reconcile_expired(&key, koduck_ai::adapters::history::postgres::unix_time_ms());
    assert!(
        matches!(
            outcome,
            Ok(koduck_ai::adapters::history::postgres::ReconcileOutcome::Cancelled)
        ),
        "the expired Turn cancels, found {outcome:?}"
    );

    // A decision arriving after recovery must not win the transition.
    let store =
        SqlxApprovalRecordStore::new(harness.pool.clone(), harness.runtime.handle().clone());
    let mut route = koduck_ai::application::ApprovalDecisionRoute::new(store);
    let trust = koduck_ai::domain::TrustContext::new(tenant.clone(), "approver-a")
        .expect("valid principal")
        .with_approval_scopes(koduck_ai::domain::ApprovalScopes::from_validated(vec![
            "ai.tool.approve".to_owned(),
        ]));
    let decided = route.decide(
        &trust,
        thread,
        approval_id,
        koduck_ai::domain::execution::ApprovalDecision::Accepted,
        10_000,
    );
    assert!(
        !matches!(
            decided,
            koduck_ai::application::ApprovalDecisionOutcome::Resolved { .. }
        ),
        "a decision under a terminal Turn must not resolve, found {decided:?}"
    );
    // Terminal recovery owns every unresolved D-6 beneath the recovered Turn.
    // It cancels the in-window approval rather than leaving an unreachable
    // `requested` record whose later decision can never win (ADR-0003 TC-10).
    let status: String = harness
        .runtime
        .block_on(async {
            sqlx::query_scalar(
                "SELECT status FROM tool_approvals \
                 WHERE tenant_id = $1 AND approval_id = $2",
            )
            .bind(tenant.as_str())
            .bind(approval_id.as_uuid())
            .fetch_one(&harness.pool)
            .await
        })
        .expect("approval status is readable");
    assert_eq!(
        status, "cancelled",
        "the recovered Turn closes the in-window approval"
    );
    // The same cancelled terminal replays after the original decision window;
    // recovery has already closed the record, so no expiry transition follows.
    let late = route.decide(
        &trust,
        thread,
        approval_id,
        koduck_ai::domain::execution::ApprovalDecision::Accepted,
        700_000,
    );
    assert!(
        matches!(
            late,
            koduck_ai::application::ApprovalDecisionOutcome::Conflict { status, .. }
                if status == koduck_ai::domain::execution::ApprovalStatus::Cancelled
        ),
        "a post-deadline decision reports the recovered cancellation, found {late:?}"
    );
    let final_status: String = harness
        .runtime
        .block_on(async {
            sqlx::query_scalar(
                "SELECT status FROM tool_approvals \
                 WHERE tenant_id = $1 AND approval_id = $2",
            )
            .bind(tenant.as_str())
            .bind(approval_id.as_uuid())
            .fetch_one(&harness.pool)
            .await
        })
        .expect("final approval status is readable");
    assert_eq!(
        final_status, "cancelled",
        "the recovered record retains its cancellation terminal"
    );
    // Recovery appends its correlated cancellation audit record atomically —
    // every approval terminal carries TC-14 evidence.
    let cancellation_audits: Vec<String> = harness
        .runtime
        .block_on(async {
            sqlx::query_scalar(
                "SELECT record FROM tool_audit_records \
                 WHERE tenant_id = $1 AND turn_id = $2",
            )
            .bind(tenant.as_str())
            .bind(turn.as_uuid())
            .fetch_all(&harness.pool)
            .await
        })
        .expect("audit rows are readable");
    assert_eq!(
        cancellation_audits.len(),
        1,
        "the recovered cancellation appends exactly one audit record"
    );
    assert!(
        cancellation_audits[0].contains("cancelled"),
        "the audit record carries the cancelled terminal, found {}",
        cancellation_audits[0]
    );
    assert!(
        cancellation_audits[0].contains("\"approval_decision\":null"),
        "the recovered terminal carries no fabricated decision, found {}",
        cancellation_audits[0]
    );

    // An already-terminal approval replays its terminal after the Turn
    // terminalizes: the Turn guard applies only while the record is still
    // `requested`, so the lost-response retry observes the same accepted
    // projection instead of a fabricated conflict (ADR-0003 TC-12).
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE tool_approvals SET status = 'accepted', decision = 'accepted', \
             approver = 'approver-a', decided_at_millis = 1, version = 2 \
             WHERE tenant_id = $1 AND approval_id = $2",
        )
        .bind(tenant.as_str())
        .bind(approval_id.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture accepted terminal");
    });
    let replayed = route.decide(
        &trust,
        thread,
        approval_id,
        koduck_ai::domain::execution::ApprovalDecision::Accepted,
        800_000,
    );
    assert_eq!(
        replayed,
        koduck_ai::application::ApprovalDecisionOutcome::Resolved {
            status: koduck_ai::domain::execution::ApprovalStatus::Accepted,
            decision: koduck_ai::domain::execution::ApprovalDecision::Accepted,
            version: 2,
        },
        "an identical replay under a terminal Turn returns the accepted terminal"
    );
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one durable regression seeds the interruption barrier, requested approval, recovery call, and atomic audit assertions"
)]
fn recovered_interruption_cancels_and_audits_requested_approvals() {
    // A process can die after committing the durable interruption barrier but
    // before it settles the requested D-6. Expiry recovery owns that same
    // interruption, so it must terminalize and audit the D-6 in the recovery
    // transaction rather than leave approval state pending forever. Recovery
    // records the barrier-owned cancellation without fabricating a C-7 approval
    // decision (TC-10, TC-14).
    let Some(harness) = harness() else {
        return;
    };
    let tenant = TenantId::new("interruption-approval-recovery").expect("valid tenant");
    let thread = koduck_ai::domain::ThreadId::new();
    let turn = koduck_ai::domain::TurnId::new();
    let approval_id = koduck_ai::domain::execution::ApprovalId::new();
    let attempt_id = uuid::Uuid::new_v4();
    let digest = "ab".repeat(32);
    harness.runtime.block_on(async {
        sqlx::query(
            "INSERT INTO threads (tenant_id, subject_id, thread_id) \
             VALUES ($1, 'approver-a', $2)",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture thread");
        sqlx::query(
            "INSERT INTO turns \
             (tenant_id, thread_id, turn_id, status, next_sequence, interrupting) \
             VALUES ($1, $2, $3, 'started', 1, TRUE)",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture interruption barrier");
        sqlx::query(
            "INSERT INTO turn_leases \
             (tenant_id, thread_id, turn_id, generation, renewed_at, expires_at, fenced) \
             VALUES ($1, $2, $3, 1, CURRENT_TIMESTAMP - INTERVAL '1 hour', \
                     CURRENT_TIMESTAMP - INTERVAL '55 minutes', FALSE)",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture expired lease");
        sqlx::query(
            "INSERT INTO tool_approvals \
             (tenant_id, approval_id, thread_id, turn_id, attempt_id, lease_generation, \
              descriptor_id, descriptor_version, effect, action_digest, profile_id, \
              profile_version, requested_at_millis, expires_at_millis, status, \
              requester_subject, version) \
             VALUES ($1, $2, $3, $4, $5, 1, 'fixture.tool', 'v1', 'external_write', $6, \
                     'profile-default', 'v1', 1, 600000, 'requested', 'approver-a', 1)",
        )
        .bind(tenant.as_str())
        .bind(approval_id.as_uuid())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .bind(attempt_id)
        .bind(&digest)
        .execute(&harness.pool)
        .await
        .expect("fixture requested approval");
    });

    let executor = koduck_ai::adapters::history::postgres::SqlxPostgresExecutor::new(
        harness.pool.clone(),
        harness.runtime.handle().clone(),
    );
    let key = koduck_ai::adapters::history::postgres::LeaseKey::new(
        tenant.clone(),
        thread,
        turn,
        koduck_ai::domain::LeaseGeneration::initial(),
    );
    let mut history = koduck_ai::adapters::history::postgres::PostgresTurnHistory::new(executor);
    assert!(matches!(
        history.reconcile_expired(&key, koduck_ai::adapters::history::postgres::unix_time_ms()),
        Ok(koduck_ai::adapters::history::postgres::ReconcileOutcome::Interrupted)
    ));

    let (status, decision, version): (String, Option<String>, i64) = harness
        .runtime
        .block_on(async {
            sqlx::query_as(
                "SELECT status, decision, version FROM tool_approvals \
                 WHERE tenant_id = $1 AND approval_id = $2",
            )
            .bind(tenant.as_str())
            .bind(approval_id.as_uuid())
            .fetch_one(&harness.pool)
            .await
        })
        .expect("approval terminal is readable");
    assert_eq!(
        (status.as_str(), decision.as_deref(), version),
        ("cancelled", None, 2)
    );
    let replayed = history
        .replay(&tenant, turn)
        .expect("recovered approval projection is replayable");
    assert!(
        replayed.iter().any(|item| matches!(
            &item.payload,
            ItemPayload::ApprovalStatus {
                approval_id: projected_approval_id,
                attempt_id: projected_attempt_id,
                status: ApprovalStatus::Cancelled,
                decision: None,
                version: 2,
            } if *projected_approval_id == approval_id
                && projected_attempt_id.as_uuid() == attempt_id
        )),
        "interruption recovery appends the D-3 cancellation projection for its closed D-6 approval"
    );
    let audits: Vec<String> = harness
        .runtime
        .block_on(async {
            sqlx::query_scalar(
                "SELECT record FROM tool_audit_records \
                 WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
            )
            .bind(tenant.as_str())
            .bind(thread.as_uuid())
            .bind(turn.as_uuid())
            .fetch_all(&harness.pool)
            .await
        })
        .expect("approval audit is readable");
    assert_eq!(
        audits.len(),
        1,
        "the recovered approval has one atomic audit"
    );
    assert!(audits[0].contains(&approval_id.as_uuid().to_string()));
    assert!(audits[0].contains(&attempt_id.to_string()));
    assert!(audits[0].contains("\"approval_status\":\"cancelled\""));
    assert!(audits[0].contains("\"approval_decision\":null"));
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one durable regression seeds the ordinary expiry recovery state and verifies its complete ordered D-6, D-7, and Turn closure"
)]
fn ordinary_expiry_cancels_requested_approvals_before_prepared_attempts() {
    // Any recovered Turn terminal closes the unresolved D-6 it owns.  The
    // append-only D-3 view preserves the lifecycle order: the approval
    // cancellation becomes visible before the bound prepared D-7 terminal
    // and before the recovered Turn terminal (ADR-0003 TC-06/TC-10/TC-14).
    let Some(harness) = harness() else {
        return;
    };
    let tenant = TenantId::new("ordinary-approval-recovery").expect("valid tenant");
    let thread = koduck_ai::domain::ThreadId::new();
    let turn = koduck_ai::domain::TurnId::new();
    let approval_id = koduck_ai::domain::execution::ApprovalId::new();
    let attempt_id = uuid::Uuid::new_v4();
    let digest = "ab".repeat(32);
    harness.runtime.block_on(async {
        sqlx::query(
            "INSERT INTO threads (tenant_id, subject_id, thread_id) \
             VALUES ($1, 'approver-a', $2)",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture thread");
        sqlx::query(
            "INSERT INTO turns (tenant_id, thread_id, turn_id, status, next_sequence) \
             VALUES ($1, $2, $3, 'started', 1)",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture turn");
        sqlx::query(
            "INSERT INTO turn_leases \
             (tenant_id, thread_id, turn_id, generation, renewed_at, expires_at, fenced) \
             VALUES ($1, $2, $3, 1, CURRENT_TIMESTAMP - INTERVAL '1 hour', \
                     CURRENT_TIMESTAMP - INTERVAL '55 minutes', FALSE)",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture expired lease");
        sqlx::query(
            "INSERT INTO tool_approvals \
             (tenant_id, approval_id, thread_id, turn_id, attempt_id, lease_generation, \
              descriptor_id, descriptor_version, effect, action_digest, profile_id, \
              profile_version, requested_at_millis, expires_at_millis, status, \
              requester_subject, version) \
             VALUES ($1, $2, $3, $4, $5, 1, 'fixture.tool', 'v1', 'external_write', $6, \
                     'profile-default', 'v1', 1, 600000, 'requested', 'approver-a', 1)",
        )
        .bind(tenant.as_str())
        .bind(approval_id.as_uuid())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .bind(attempt_id)
        .bind(&digest)
        .execute(&harness.pool)
        .await
        .expect("fixture requested approval");
        sqlx::query(
            "INSERT INTO tool_execution_attempts \
             (tenant_id, attempt_id, thread_id, turn_id, lease_generation, \
              descriptor_id, descriptor_version, effect, action_digest, profile_id, \
              profile_version, prepared_at_millis, status, version) \
             VALUES ($1, $2, $3, $4, 1, 'fixture.tool', 'v1', 'external_write', $5, \
                     'profile-default', 'v1', 1, 'prepared', 1)",
        )
        .bind(tenant.as_str())
        .bind(attempt_id)
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .bind(&digest)
        .execute(&harness.pool)
        .await
        .expect("fixture prepared attempt");
    });
    let executor = koduck_ai::adapters::history::postgres::SqlxPostgresExecutor::new(
        harness.pool.clone(),
        harness.runtime.handle().clone(),
    );
    let key = koduck_ai::adapters::history::postgres::LeaseKey::new(
        tenant.clone(),
        thread,
        turn,
        koduck_ai::domain::LeaseGeneration::initial(),
    );
    let mut history = koduck_ai::adapters::history::postgres::PostgresTurnHistory::new(executor);
    assert_eq!(
        history.reconcile_expired(&key, koduck_ai::adapters::history::postgres::unix_time_ms()),
        Ok(koduck_ai::adapters::history::postgres::ReconcileOutcome::Cancelled)
    );
    let (approval_status, attempt_status): (String, String) = harness
        .runtime
        .block_on(async {
            sqlx::query_as(
                "SELECT approval.status, attempt.status \
                 FROM tool_approvals approval \
                 JOIN tool_execution_attempts attempt USING (tenant_id, thread_id, turn_id) \
                 WHERE approval.tenant_id = $1 AND approval.approval_id = $2",
            )
            .bind(tenant.as_str())
            .bind(approval_id.as_uuid())
            .fetch_one(&harness.pool)
            .await
        })
        .expect("recovered terminals are readable");
    assert_eq!(
        (approval_status.as_str(), attempt_status.as_str()),
        ("cancelled", "cancelled")
    );
    let replayed = history
        .replay(&tenant, turn)
        .expect("recovered terminal projections are replayable");
    assert!(matches!(
        replayed.as_slice(),
        [
            koduck_ai::domain::Item {
                sequence: 1,
                payload: ItemPayload::ApprovalStatus {
                    approval_id: projected_approval_id,
                    attempt_id: approval_attempt_id,
                    status: ApprovalStatus::Cancelled,
                    decision: None,
                    version: 2,
                },
                ..
            },
            koduck_ai::domain::Item {
                sequence: 2,
                payload: ItemPayload::ToolResult {
                    attempt_id: Some(result_attempt_id),
                    status: ExecutionStatus::Cancelled,
                    effect_state: Some(ToolEffectState::NotStarted),
                    code: None,
                    output_bytes: 0,
                    output_digest: None,
                    version: Some(3),
                },
                ..
            },
            koduck_ai::domain::Item {
                sequence: 3,
                payload: ItemPayload::Terminal(koduck_ai::domain::TerminalOutcome::Cancelled),
                ..
            },
        ] if *projected_approval_id == approval_id
            && approval_attempt_id.as_uuid() == attempt_id
            && result_attempt_id.as_uuid() == attempt_id
    ));
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one durable regression seeds the expired lease and live D-7 before asserting recovery defers its terminal"
)]
fn lease_recovery_waits_for_a_running_action_deadline() {
    // An expired foreground lease is not cancellation evidence for a remote
    // effect that is still inside its 30-second D-7 deadline. Recovery must
    // retain the running attempt and retry after that deadline rather than
    // terminalizing the Turn early (TC-09/TC-10).
    let Some(harness) = harness() else {
        return;
    };
    let tenant = TenantId::new("recovery-awaits-running-deadline").expect("valid tenant");
    let thread = koduck_ai::domain::ThreadId::new();
    let turn = koduck_ai::domain::TurnId::new();
    let attempt_id = uuid::Uuid::new_v4();
    let started_at_millis = koduck_ai::adapters::history::postgres::unix_time_ms();
    harness.runtime.block_on(async {
        sqlx::query(
            "INSERT INTO threads (tenant_id, subject_id, thread_id)
             VALUES ($1, 'running-owner', $2)",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture thread");
        sqlx::query(
            "INSERT INTO turns (tenant_id, thread_id, turn_id, status, next_sequence)
             VALUES ($1, $2, $3, 'started', 1)",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture turn");
        sqlx::query(
            "INSERT INTO turn_leases
             (tenant_id, thread_id, turn_id, generation, renewed_at, expires_at, fenced)
             VALUES ($1, $2, $3, 1, CURRENT_TIMESTAMP - INTERVAL '1 hour',
                     CURRENT_TIMESTAMP - INTERVAL '55 minutes', FALSE)",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture expired lease");
        sqlx::query(
            "INSERT INTO tool_execution_attempts
             (tenant_id, attempt_id, thread_id, turn_id, lease_generation,
              descriptor_id, descriptor_version, effect, action_digest, profile_id,
              profile_version, prepared_at_millis, started_at_millis, status, version)
             VALUES ($1, $2, $3, $4, 1, 'fixture.tool', 'v1', 'external_write',
                     'ab', 'profile-default', 'v1', 1, $5, 'running', 2)",
        )
        .bind(tenant.as_str())
        .bind(attempt_id)
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .bind(i64::try_from(started_at_millis).expect("clock fits database"))
        .execute(&harness.pool)
        .await
        .expect("fixture running attempt");
    });
    let executor = koduck_ai::adapters::history::postgres::SqlxPostgresExecutor::new(
        harness.pool.clone(),
        harness.runtime.handle().clone(),
    );
    let key = koduck_ai::adapters::history::postgres::LeaseKey::new(
        tenant.clone(),
        thread,
        turn,
        koduck_ai::domain::LeaseGeneration::initial(),
    );
    let mut history = koduck_ai::adapters::history::postgres::PostgresTurnHistory::new(executor);
    assert_eq!(
        history.reconcile_expired(&key, started_at_millis),
        Ok(koduck_ai::adapters::history::postgres::ReconcileOutcome::TooEarly)
    );
    let (turn_status, attempt_status): (String, String) = harness
        .runtime
        .block_on(async {
            sqlx::query_as(
                "SELECT t.status, attempt.status
             FROM turns t JOIN tool_execution_attempts attempt
               USING (tenant_id, thread_id, turn_id)
             WHERE t.tenant_id = $1 AND t.thread_id = $2 AND t.turn_id = $3",
            )
            .bind(tenant.as_str())
            .bind(thread.as_uuid())
            .bind(turn.as_uuid())
            .fetch_one(&harness.pool)
            .await
        })
        .expect("recovery left the live attempt unchanged");
    assert_eq!(
        (turn_status.as_str(), attempt_status.as_str()),
        ("started", "running")
    );
}
