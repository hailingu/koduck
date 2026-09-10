// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md

//! AC-1: the owned correction command validates its identity and content
//! exactly, before any database access (ADR-0004 CA-01).

use koduck_ai::application::{CorrectionCommand, CorrectionError, MAX_CORRECTION_CONTENT_BYTES};
use koduck_ai::domain::{ItemId, ThreadId, TrustContext, TurnId};

/// AC-1: command validation is exact — non-blank content of at most
/// 65,536 UTF-8 bytes is preserved exactly, every invalid input returns its
/// declared category before any store call exists on the path, and no
/// sequence or payload discriminator is caller-settable.
#[test]
fn command_validation() {
    let trust = TrustContext::new(trust_tenant(), "subject-a").expect("valid trust context");
    let thread = ThreadId::new();
    let turn = TurnId::new();
    let item = ItemId::new();
    let predecessor = ItemId::new();

    // One byte of non-whitespace content is admitted.
    let one_byte = valid_command(&trust, thread, turn, item, predecessor, "x");
    assert_eq!(one_byte.content(), "x");

    // The 65,535th and 65,536th bytes stay admitted and are preserved exactly.
    let near_cap = "a".repeat(MAX_CORRECTION_CONTENT_BYTES - 1);
    let near_cap = valid_command(&trust, thread, turn, item, predecessor, near_cap.as_str());
    assert_eq!(near_cap.content().len(), MAX_CORRECTION_CONTENT_BYTES - 1);
    let at_cap = "a".repeat(MAX_CORRECTION_CONTENT_BYTES);
    let at_cap = valid_command(&trust, thread, turn, item, predecessor, at_cap.as_str());
    assert_eq!(at_cap.content().len(), MAX_CORRECTION_CONTENT_BYTES);

    // Multibyte content is measured in UTF-8 bytes and preserved exactly:
    // 21,845 three-byte characters plus one ASCII byte equals 65,536 bytes.
    let multibyte = format!("{}x", "水".repeat(21_845));
    assert_eq!(multibyte.len(), MAX_CORRECTION_CONTENT_BYTES);
    let multibyte = valid_command(&trust, thread, turn, item, predecessor, multibyte.as_str());
    assert_eq!(multibyte.content().len(), MAX_CORRECTION_CONTENT_BYTES);
    assert!(multibyte.content().ends_with('x'));
    assert!(multibyte.content().starts_with("水"));

    // One byte over the cap is rejected as InvalidContent.
    let over_cap = "a".repeat(MAX_CORRECTION_CONTENT_BYTES + 1);
    assert_eq!(
        CorrectionCommand::new(
            trust.clone(),
            thread,
            turn,
            item,
            predecessor,
            over_cap.as_str(),
        ),
        Err(CorrectionError::InvalidContent)
    );

    // Empty and whitespace-only content are rejected as InvalidContent.
    assert_eq!(
        CorrectionCommand::new(trust.clone(), thread, turn, item, predecessor, ""),
        Err(CorrectionError::InvalidContent)
    );
    assert_eq!(
        CorrectionCommand::new(trust.clone(), thread, turn, item, predecessor, " \t\n"),
        Err(CorrectionError::InvalidContent)
    );

    // An identity equal to its predecessor is rejected as InvalidPredecessor
    // while the content itself is valid.
    assert_eq!(
        CorrectionCommand::new(trust.clone(), thread, turn, item, item, "corrected"),
        Err(CorrectionError::InvalidPredecessor)
    );

    // The command surface is read-only: the authenticated identity, scope,
    // caller-stable identity, and predecessor are exposed without setters,
    // and no sequence or discriminator accessor exists to call.
    let command = valid_command(&trust, thread, turn, item, predecessor, "kept");
    assert_eq!(command.trust().subject_id, "subject-a");
    assert_eq!(command.trust().tenant_id.as_str(), trust.tenant_id.as_str());
    assert_eq!(command.thread_id(), thread);
    assert_eq!(command.turn_id(), turn);
    assert_eq!(command.item_id(), item);
    assert_eq!(command.predecessor_item_id(), predecessor);
    assert_eq!(command.content(), "kept");
}

fn trust_tenant() -> koduck_ai::domain::TenantId {
    koduck_ai::domain::TenantId::new("cand11-command-validation").expect("valid tenant")
}

fn valid_command(
    trust: &TrustContext,
    thread: ThreadId,
    turn: TurnId,
    item: ItemId,
    predecessor: ItemId,
    content: &str,
) -> CorrectionCommand {
    CorrectionCommand::new(
        trust.clone(),
        thread,
        turn,
        item,
        predecessor,
        content.to_owned(),
    )
    .expect("valid correction command")
}
