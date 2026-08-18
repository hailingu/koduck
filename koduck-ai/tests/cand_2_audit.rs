// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Named AC-13 black-box harness: audit metadata is complete, correlated,
//! bounded, and contains no credential or raw unbounded content
//! (ADR-0003 TC-14).

use koduck_ai::adapters::audit::serialize_audit_record;
use koduck_ai::adapters::tool::parse_action_parameters;
use koduck_ai::application::{
    DenialCode, EffectState, ExecutionFailure, MAX_AUDIT_RECORD_BYTES, PolicyDenialContext,
    ToolAuditRecord, ToolExecutionOutcome,
};
use koduck_ai::domain::execution::{
    ApprovalDecision, ApprovalId, ApprovalStatus, AttemptId, ExactActionBinding,
};
use koduck_ai::domain::tool::{Action, Effect};
use koduck_ai::domain::{LeaseGeneration, TenantId};
use uuid::Uuid;

/// Distinctive synthetic credential value that must never survive into an
/// audit record.
const SYNTHETIC_CREDENTIAL: &str = "sk-SYNTHETIC-CREDENTIAL-9f8e7d6c5b4a";

/// Distinctive raw executor-output marker that must never survive into an
/// audit record.
const RAW_OUTPUT_MARKER: &str = "RAWDATA-NOTFORAUDIT-";

/// Builds the exact 65,536-byte boundary parameters carrying the synthetic
/// credential reference.
fn boundary_parameters() -> koduck_ai::domain::tool::ActionParameters {
    let prefix = format!(r#"{{"credential":"{SYNTHETIC_CREDENTIAL}","padding":""#);
    let suffix = r#"","value":1}"#;
    let pad = "x".repeat(65_536 - prefix.len() - suffix.len());
    let raw = format!("{prefix}{pad}{suffix}");
    assert_eq!(
        raw.len(),
        65_536,
        "the boundary input is exactly 65,536 bytes"
    );
    parse_action_parameters(&raw).expect("the boundary parameters parse")
}

fn binding() -> ExactActionBinding {
    ExactActionBinding::new(
        TenantId::new(format!("ci-{}", Uuid::new_v4())).expect("valid tenant"),
        koduck_ai::domain::ThreadId::new(),
        koduck_ai::domain::TurnId::new(),
        LeaseGeneration::initial(),
        ("profile-default", "v1"),
        AttemptId::new(),
        Action::new(
            "fixture.tool",
            "v1",
            Effect::CredentialUse,
            "fixture-target",
            boundary_parameters(),
        )
        .expect("valid action"),
    )
    .expect("valid binding")
}

/// Rebuilds a binding with a new D-7 identity while retaining every action
/// input visible before policy allocates an attempt.
fn binding_with_attempt(binding: &ExactActionBinding, attempt_id: AttemptId) -> ExactActionBinding {
    ExactActionBinding::new(
        binding.tenant_id().clone(),
        binding.thread_id(),
        binding.turn_id(),
        binding.lease_generation(),
        (binding.profile_id(), binding.profile_version()),
        attempt_id,
        binding.action().clone(),
    )
    .expect("valid rebuilt binding")
}

/// Creates one policy-denial record from trusted metadata that was fully
/// resolved before the denial. The helper keeps the parameterized terminal
/// table focused on audit behavior rather than repeated setup.
fn policy_denial_record(
    binding: &ExactActionBinding,
    denial: DenialCode,
    at_millis: u64,
) -> ToolAuditRecord {
    ToolAuditRecord::policy_denial(
        &PolicyDenialContext::from_binding(binding),
        denial,
        at_millis,
    )
}

/// Asserts one serialized audit record against the TC-14 contract.
fn assert_correlated_and_minimized(
    record: &ToolAuditRecord,
    serialized: &str,
    expect_execution: bool,
) {
    assert!(
        serialized.len() <= MAX_AUDIT_RECORD_BYTES,
        "serialized audit record is at most 16,384 bytes ({} bytes)",
        serialized.len()
    );
    let value: serde_json::Value =
        serde_json::from_str(serialized).expect("audit record is valid JSON");
    for key in [
        "tenant_id",
        "thread_id",
        "turn_id",
        "descriptor_id",
        "descriptor_version",
        "profile_id",
        "profile_version",
        "action_digest",
        "lease_generation",
        "policy_decision",
        "at_millis",
    ] {
        assert!(
            value.get(key).is_some_and(|field| !field.is_null()),
            "correlated field {key} is present"
        );
    }
    assert_eq!(value["descriptor_id"], "fixture.tool");
    assert_eq!(value["descriptor_version"], "v1");
    assert_eq!(value["profile_id"], "profile-default");
    assert_eq!(value["profile_version"], "v1");
    assert_eq!(value["lease_generation"], 1);
    assert_eq!(
        value["action_digest"].as_str().map(str::len),
        Some(64),
        "the exact-action digest is the 64-character hex digest"
    );
    if expect_execution {
        assert!(
            value
                .get("execution_status")
                .is_some_and(|field| !field.is_null()),
            "an execution terminal carries its D-7 transition"
        );
        assert!(
            value
                .get("effect_state")
                .is_some_and(|field| !field.is_null()),
            "an execution terminal carries its executor effect state"
        );
    }
    assert_eq!(
        record.policy_decision(),
        value["policy_decision"]
            .as_str()
            .expect("policy decision code"),
        "the in-memory and serialized policy decisions agree"
    );
    assert!(
        !serialized.contains(SYNTHETIC_CREDENTIAL),
        "no credential value enters the audit record"
    );
    assert!(
        !serialized.contains("\"padding\"") && !serialized.contains("xxxxxxxx"),
        "no raw action-parameter content enters the audit record"
    );
    assert!(
        !serialized.contains(RAW_OUTPUT_MARKER),
        "no raw executor-result content enters the audit record"
    );
}

#[test]
fn audit_is_correlated_and_content_minimized() {
    let binding = binding();
    let at_millis = 1_755_000_000_000_u64;

    // Every policy-denial terminal class correlates the typed denial with
    // the exact action and carries no D-6/D-7 identity.
    for denial in [
        DenialCode::DescriptorMissing,
        DenialCode::DescriptorStale,
        DenialCode::DescriptorDisabled,
        DenialCode::DescriptorIncompatible,
        DenialCode::DescriptorConflicting,
        DenialCode::UnknownEffect,
        DenialCode::OutsidePermissionProfile,
    ] {
        let record = policy_denial_record(&binding, denial, at_millis);
        let serialized = serialize_audit_record(&record).expect("denial audit serializes");
        let value: serde_json::Value =
            serde_json::from_str(&serialized).expect("audit record is valid JSON");
        assert!(value["attempt_id"].is_null() && value["approval_id"].is_null());
        assert_correlated_and_minimized(&record, &serialized, false);
    }

    // Every canonical D-6 resolution terminal correlates the approval
    // identity, status, decision, and version.
    for (status, decision) in [
        (ApprovalStatus::Accepted, Some(ApprovalDecision::Accepted)),
        (ApprovalStatus::Declined, Some(ApprovalDecision::Declined)),
        (ApprovalStatus::Cancelled, Some(ApprovalDecision::Cancelled)),
        (ApprovalStatus::Expired, None),
    ] {
        let record = ToolAuditRecord::approval_resolution(
            &binding,
            ApprovalId::new(),
            status,
            decision,
            2,
            at_millis,
        );
        let serialized = serialize_audit_record(&record).expect("approval audit serializes");
        let value: serde_json::Value =
            serde_json::from_str(&serialized).expect("audit record is valid JSON");
        assert!(value["attempt_id"].is_string() && value["approval_id"].is_string());
        assert_eq!(value["approval_status"], status.as_str());
        assert_eq!(
            value["approval_decision"].as_str(),
            decision.map(ApprovalDecision::as_str)
        );
        assert_eq!(value["approval_version"], 2);
        assert_correlated_and_minimized(&record, &serialized, false);
    }

    // The success terminal at the 1,048,576-byte output boundary correlates
    // only the byte count and digest of the committed output.
    let raw_output = RAW_OUTPUT_MARKER
        .repeat(1_048_576 / RAW_OUTPUT_MARKER.len() + 1)
        .into_bytes();
    let raw_output = raw_output[..1_048_576].to_vec();
    assert_eq!(raw_output.len(), 1_048_576);
    let succeeded = ToolExecutionOutcome::Succeeded {
        output: raw_output,
        effect_state: EffectState::Started,
    };
    let record = ToolAuditRecord::execution_terminal(&binding, &succeeded, at_millis);
    let serialized = serialize_audit_record(&record).expect("success audit serializes");
    let value: serde_json::Value =
        serde_json::from_str(&serialized).expect("audit record is valid JSON");
    assert_eq!(value["output_bytes"], 1_048_576);
    assert_eq!(
        value["output_digest"].as_str().map(str::len),
        Some(64),
        "committed output is correlated by its hex digest"
    );
    assert_eq!(value["execution_status"], "succeeded");
    assert_eq!(value["effect_state"], "started");
    assert_correlated_and_minimized(&record, &serialized, true);

    // The failed, timed-out, and cancelled terminals carry their stable
    // codes and truthful effect states.
    let failed = ToolExecutionOutcome::Failed {
        code: ExecutionFailure::OutputLimitExceeded,
        effect_state: EffectState::Unknown,
    };
    let record = ToolAuditRecord::execution_terminal(&binding, &failed, at_millis);
    let serialized = serialize_audit_record(&record).expect("failure audit serializes");
    let value: serde_json::Value =
        serde_json::from_str(&serialized).expect("audit record is valid JSON");
    assert_eq!(value["failure_code"], "output_limit_exceeded");
    assert_eq!(value["effect_state"], "unknown");
    assert!(value["output_bytes"].is_null());
    assert_correlated_and_minimized(&record, &serialized, true);

    let timed_out = ToolExecutionOutcome::TimedOut {
        effect_state: EffectState::Unknown,
    };
    let record = ToolAuditRecord::execution_terminal(&binding, &timed_out, at_millis);
    let serialized = serialize_audit_record(&record).expect("timeout audit serializes");
    let value: serde_json::Value =
        serde_json::from_str(&serialized).expect("audit record is valid JSON");
    assert_eq!(value["execution_status"], "timed_out");
    assert_eq!(value["effect_state"], "unknown");
    assert_correlated_and_minimized(&record, &serialized, true);

    let cancelled = ToolExecutionOutcome::Cancelled {
        effect_state: EffectState::NotStarted,
    };
    let record = ToolAuditRecord::execution_terminal(&binding, &cancelled, at_millis);
    let serialized = serialize_audit_record(&record).expect("cancellation audit serializes");
    let value: serde_json::Value =
        serde_json::from_str(&serialized).expect("audit record is valid JSON");
    assert_eq!(value["execution_status"], "cancelled");
    assert_eq!(value["effect_state"], "not_started");
    assert_correlated_and_minimized(&record, &serialized, true);
}

#[test]
fn policy_denial_correlation_does_not_depend_on_an_unallocated_attempt_id() {
    let first = binding();
    let second = binding_with_attempt(&first, AttemptId::new());

    let first = serialize_audit_record(&policy_denial_record(
        &first,
        DenialCode::DescriptorMissing,
        1_755_000_000_000,
    ))
    .expect("first denial serializes");
    let second = serialize_audit_record(&policy_denial_record(
        &second,
        DenialCode::DescriptorMissing,
        1_755_000_000_000,
    ))
    .expect("second denial serializes");
    let first: serde_json::Value = serde_json::from_str(&first).expect("first JSON");
    let second: serde_json::Value = serde_json::from_str(&second).expect("second JSON");

    assert_eq!(
        first["action_digest"], second["action_digest"],
        "a pre-attempt denial cannot be correlated through a generated D-7 identity"
    );
}

#[test]
fn pre_attempt_policy_denial_keeps_unresolved_metadata_absent() {
    let binding = binding();
    let context = PolicyDenialContext::new(
        binding.tenant_id().clone(),
        binding.thread_id(),
        binding.turn_id(),
        binding.lease_generation(),
    );

    let serialized = serialize_audit_record(&ToolAuditRecord::policy_denial(
        &context,
        DenialCode::DescriptorMissing,
        1_755_000_000_000,
    ))
    .expect("denial audit serializes");
    let value: serde_json::Value = serde_json::from_str(&serialized).expect("valid JSON");

    for field in [
        "attempt_id",
        "approval_id",
        "descriptor_id",
        "descriptor_version",
        "profile_id",
        "profile_version",
        "action_digest",
    ] {
        assert!(
            value[field].is_null(),
            "{field} is unknown before policy resolution"
        );
    }
}
