// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

#[test]
fn postgres_history_persists_and_checks_subject_ownership() {
    let migration = include_str!("../migrations/0001_cand_1_history.sql");
    let executor = include_str!("../src/adapters/history/postgres/sqlx_executor.rs");

    assert!(migration.contains("subject_id TEXT NOT NULL"));
    assert!(executor.contains(".bind(trust.subject_id.as_str())"));
    assert!(executor.contains("threads.subject_id"));
    assert!(executor.contains("command.trust.subject_id"));
}

#[test]
fn postgres_history_keeps_each_turn_contiguous_in_provider_context() {
    let executor = include_str!("../src/adapters/history/postgres/sqlx_executor.rs");

    assert!(
        executor.contains("turn_items.payload FROM turn_items JOIN turns"),
        "prior history must join its owning Turn so ordering retains Turn boundaries"
    );
    assert!(
        executor.contains("ORDER BY turns.created_at")
            && executor.contains("turn_items.turn_id, turn_items.sequence"),
        "prior history must group every Turn before ordering Items within that Turn"
    );
}

#[test]
fn durability_recovery_arbitrates_an_accepted_interrupt() {
    let executor = include_str!("../src/adapters/history/postgres/sqlx_executor.rs");
    let recovery = executor
        .split("async fn recover_failed_async")
        .nth(1)
        .and_then(|tail| tail.split("async fn renew_lease_async").next())
        .expect("recovery implementation remains inspectable");

    assert!(
        recovery.contains("t.status, t.next_sequence, t.interrupt_requested, l.fenced"),
        "recovery must read the accepted interrupt under its turn-row lock"
    );
    assert!(
        recovery.contains("TerminalOutcome::Interrupted")
            && recovery.contains("terminal_status = \"interrupted\""),
        "recovery must commit interrupted instead of failed when the request was accepted"
    );
}

#[test]
fn lease_reconciliation_preserves_the_persisted_terminal_priority() {
    let executor = include_str!("../src/adapters/history/postgres/sqlx_executor.rs");
    let reconciliation = executor
        .split("async fn reconcile_expired_async")
        .nth(1)
        .and_then(|tail| tail.split("async fn expired_lease_keys_async").next())
        .expect("reconciliation implementation remains inspectable");

    assert!(
        reconciliation.contains("t.interrupt_requested"),
        "reconciliation must read the accepted interrupt under its turn-row lock"
    );
    assert!(
        reconciliation.contains("status == \"recovery-pending\"")
            && reconciliation.contains("ReconcileOutcome::Failed"),
        "recovery-pending turns must retain the failed recovery terminal"
    );
    assert!(
        reconciliation.contains("ReconcileOutcome::Interrupted"),
        "an accepted interrupt must outrank orphan cancellation"
    );
}
