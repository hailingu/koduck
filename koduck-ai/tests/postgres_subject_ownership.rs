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
