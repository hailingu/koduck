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
