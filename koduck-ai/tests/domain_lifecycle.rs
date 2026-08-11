// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use koduck_ai::domain::{Turn, TurnStatus};

#[test]
fn completed_turn_is_terminal() {
    let started = Turn::start();
    let completed = started.complete().expect("started turn may complete");

    assert_eq!(completed.status(), TurnStatus::Completed);
    assert!(completed.interrupt().is_err());
}
