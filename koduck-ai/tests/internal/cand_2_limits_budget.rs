use super::*;

/// An unscoped approval-required call fails C-7 pre-validation before any D-7
/// allocation, so repeated unauthorized requests leave the full 16-slot
/// budget intact (TC-05).
pub(super) fn unauthorized_requests_preserve_the_attempt_budget() {
    let script = std::iter::repeat_n(Script::Ok(b"ok", EffectState::Started), 16).collect();
    let (mut tool_boundary, dispatches) = boundary(config(Effect::ProcessExecute), script);
    let call = inputs(action(Effect::ProcessExecute, "{}"), T0 + 600_000);

    let mut decisions = 0;
    for _ in 0..16 {
        let error = tool_boundary
            .execute(
                &call,
                &trust(),
                &mut |_| {
                    decisions += 1;
                    (ApprovalDecision::Accepted, T0)
                },
                &mut fixed_clock(T0),
            )
            .expect_err("an unscoped principal is rejected before any D-7 allocation");
        assert!(
            matches!(error, ToolCallError::Approval(ApprovalError::NotAuthorized)),
            "the unscoped call must fail C-7 pre-validation: {error:?}"
        );
    }
    assert_eq!(decisions, 0, "the decision provider never observes the D-6");
    assert_eq!(
        dispatches.lock().expect("log is healthy").len(),
        0,
        "an unauthorized request must dispatch zero times"
    );

    // The full budget remains: sixteen scoped approvals execute, and the 17th
    // allocation is rejected with the exact attempt_limit code.
    for slot in 1..=16 {
        let outcome = tool_boundary
            .execute(
                &call,
                &approver(),
                &mut |_| (ApprovalDecision::Accepted, T0),
                &mut fixed_clock(T0),
            )
            .unwrap_or_else(|_| panic!("authorized attempt {slot} must execute"));
        assert!(matches!(outcome, ToolExecutionOutcome::Succeeded { .. }));
    }
    let error = tool_boundary
        .execute(
            &call,
            &approver(),
            &mut |_| (ApprovalDecision::Accepted, T0),
            &mut fixed_clock(T0),
        )
        .expect_err("the 17th allocation must be rejected");
    assert!(
        matches!(
            error,
            ToolCallError::Preparation(ExecutionPreparationError::Rejected(
                ExecutionError::AttemptLimit
            ))
        ),
        "the 17th allocation must carry the exact attempt_limit code: {error:?}"
    );
    assert_eq!(
        dispatches.lock().expect("log is healthy").len(),
        16,
        "exactly the sixteen authorized attempts dispatch"
    );
}

/// An unscoped approval-required call whose D-6 window is already expired also
/// fails before any D-7 allocation, so expired-window request loops cannot
/// drain the budget either (TC-05/TC-09).
pub(super) fn expired_unscoped_requests_preserve_the_attempt_budget() {
    let expired_call = inputs(action(Effect::ProcessExecute, "{}"), T0);
    let valid_call = inputs(action(Effect::ProcessExecute, "{}"), T0 + 600_000);
    let script = std::iter::repeat_n(Script::Ok(b"ok", EffectState::Started), 16).collect();
    let (mut tool_boundary, dispatches) = boundary(config(Effect::ProcessExecute), script);

    // The window is expired at creation (the Turn deadline equals now), so an
    // allocate-then-cancel path would consume one slot per call.
    for _ in 0..16 {
        let error = tool_boundary
            .execute(
                &expired_call,
                &trust(),
                &mut |_| (ApprovalDecision::Accepted, T0),
                &mut fixed_clock(T0),
            )
            .expect_err("an expired-window unscoped call is rejected before allocation");
        assert!(
            matches!(error, ToolCallError::Approval(ApprovalError::NotAuthorized)),
            "the expired unscoped call must fail C-7 pre-validation: {error:?}"
        );
    }
    assert_eq!(
        dispatches.lock().expect("log is healthy").len(),
        0,
        "an expired unscoped request must dispatch zero times"
    );

    // The full budget remains: sixteen scoped approvals execute, and the 17th
    // allocation is rejected with the exact attempt_limit code.
    for slot in 1..=16 {
        let outcome = tool_boundary
            .execute(
                &valid_call,
                &approver(),
                &mut |_| (ApprovalDecision::Accepted, T0),
                &mut fixed_clock(T0),
            )
            .unwrap_or_else(|_| panic!("authorized attempt {slot} must execute"));
        assert!(matches!(outcome, ToolExecutionOutcome::Succeeded { .. }));
    }
    let error = tool_boundary
        .execute(
            &valid_call,
            &approver(),
            &mut |_| (ApprovalDecision::Accepted, T0),
            &mut fixed_clock(T0),
        )
        .expect_err("the 17th allocation must be rejected");
    assert!(
        matches!(
            error,
            ToolCallError::Preparation(ExecutionPreparationError::Rejected(
                ExecutionError::AttemptLimit
            ))
        ),
        "the 17th allocation must carry the exact attempt_limit code: {error:?}"
    );
    assert_eq!(
        dispatches.lock().expect("log is healthy").len(),
        16,
        "exactly the sixteen authorized attempts dispatch"
    );
}
