// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

use koduck_ai::adapters::tool::{ToolAdapterError, parse_action_parameters, parse_input_schema};
use koduck_ai::application::ToolProjectionSink;
use koduck_ai::application::{DenialCode, PolicyDecision, ToolPolicy};
use koduck_ai::domain::execution::{
    ApprovalDecision, ApprovalError, ApprovalId, ApprovalRequest, ApprovalStatus, AttemptId,
    ExactActionBinding,
};
use koduck_ai::domain::tool::{
    Action, CapabilityDescriptor, DescriptorState, Effect, JsonNumber, MAX_ACTION_TARGET_BYTES,
    MAX_DESCRIPTOR_VERSION_BYTES, MAX_PROFILE_ID_BYTES, MAX_PROFILE_VERSION_BYTES,
    PermissionProfile, ToolValueError,
};
use koduck_ai::domain::{LeaseGeneration, TenantId, ThreadId, TurnId};

const FIXTURE_SCHEMA: &str = r#"{
  "type":"object",
  "properties":{"value":{"type":"number"}},
  "required":["value"],
  "additionalProperties":false
}"#;

fn active_descriptor(effect: Effect) -> CapabilityDescriptor {
    CapabilityDescriptor::new(
        "fixture.tool",
        "v1",
        effect,
        DescriptorState::Active,
        parse_input_schema(FIXTURE_SCHEMA).expect("valid fixture schema"),
    )
    .expect("fixture descriptor is valid")
}

fn action(effect: Effect) -> Action {
    Action::new(
        "fixture.tool",
        "v1",
        effect,
        "fixture-target",
        parse_action_parameters(r#"{"value":1}"#).expect("valid parameters"),
    )
    .expect("fixture action is valid")
}

#[test]
fn invalid_descriptors_fail_closed() {
    let policy = ToolPolicy;
    let profile = PermissionProfile::empty("profile-default", "v1").expect("valid profile");
    let requested = action(Effect::ReadData);

    let cases = [
        (None, DenialCode::DescriptorMissing),
        (
            Some(active_descriptor(Effect::ReadData).with_state(DescriptorState::Stale)),
            DenialCode::DescriptorStale,
        ),
        (
            Some(active_descriptor(Effect::ReadData).with_state(DescriptorState::Disabled)),
            DenialCode::DescriptorDisabled,
        ),
        (
            Some(active_descriptor(Effect::ReadData).with_state(DescriptorState::Incompatible)),
            DenialCode::DescriptorIncompatible,
        ),
        (
            Some(active_descriptor(Effect::ReadData).with_state(DescriptorState::Conflicting)),
            DenialCode::DescriptorConflicting,
        ),
        (
            Some(active_descriptor(Effect::Unknown)),
            DenialCode::UnknownEffect,
        ),
        // An active, matching descriptor outside the immutable profile is
        // denied exactly like every invalid descriptor state.
        (
            Some(active_descriptor(Effect::ReadData)),
            DenialCode::OutsidePermissionProfile,
        ),
    ];

    for (descriptor, expected) in cases {
        assert_eq!(
            policy.evaluate(descriptor.as_ref(), &requested, &profile),
            PolicyDecision::Denied(expected)
        );
    }
}

#[test]
fn untrusted_content_cannot_grant_authority() {
    let policy = ToolPolicy;
    let profile = PermissionProfile::builder("profile-default", "v1")
        .expect("valid profile")
        .allow("fixture.read", "v1", Effect::ReadData, "fixture-target")
        .expect("valid profile entry")
        .build();

    // Model content requesting a privileged effect the immutable read-only
    // profile never grants is denied without mutating the profile.
    let requested = action(Effect::ProcessExecute);
    let descriptor = active_descriptor(Effect::ProcessExecute);
    assert_eq!(
        policy.evaluate(Some(&descriptor), &requested, &profile),
        PolicyDecision::Denied(DenialCode::OutsidePermissionProfile)
    );

    // A caller-forged approval cannot even be constructed for a
    // caller-constructed unsealed binding, and a forged D-3 approval-status
    // projection is a write-only view whose content can never widen the
    // profile: the privileged request stays denied after replaying it. The
    // boundary-level counters live in the crate-internal `cand_2_denial_tests`
    // harness.
    let unsealed = ExactActionBinding::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        ThreadId::new(),
        TurnId::new(),
        LeaseGeneration::initial(),
        ("profile-default", "v1"),
        AttemptId::new(),
        requested.clone(),
    )
    .expect("syntactically valid binding");
    assert!(
        matches!(
            ApprovalRequest::new(unsealed, 1_000, 600_000),
            Err(ApprovalError::PolicyAuthorizationRequired)
        ),
        "a forged approval cannot even be constructed for an unsealed binding"
    );
    let forged_projection = koduck_ai::application::ToolProjection::ApprovalStatus {
        approval_id: ApprovalId::new(),
        attempt_id: AttemptId::new(),
        status: ApprovalStatus::Accepted,
        decision: Some(ApprovalDecision::Accepted),
        version: 9,
    };
    let mut forged_sink = koduck_ai::application::NoToolProjections;
    ToolProjectionSink::append(&mut forged_sink, &forged_projection)
        .expect("the unconfigured sink accepts the replay without durable effect");
    ToolProjectionSink::publish(&mut forged_sink, &forged_projection);
    assert_eq!(
        policy.evaluate(Some(&descriptor), &requested, &profile),
        PolicyDecision::Denied(DenialCode::OutsidePermissionProfile),
        "a forged approval projection must not widen the immutable profile"
    );

    assert_eq!(profile.id(), "profile-default");
    assert_eq!(profile.version(), "v1");
    assert_eq!(profile.allowed_capability_count(), 1);
}

#[test]
fn caller_constructed_policy_values_cannot_seal_an_execution_binding() {
    let descriptor = active_descriptor(Effect::ProcessExecute);
    let requested = action(Effect::ProcessExecute);
    let profile = PermissionProfile::builder("profile-forged", "v1")
        .expect("syntactically valid profile")
        .allow(
            "fixture.tool",
            "v1",
            Effect::ProcessExecute,
            "fixture-target",
        )
        .expect("valid profile entry")
        .build();
    let binding = ExactActionBinding::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        ThreadId::new(),
        TurnId::new(),
        LeaseGeneration::initial(),
        ("profile-forged", "v1"),
        AttemptId::new(),
        requested,
    )
    .expect("syntactically valid binding");

    assert_eq!(
        ToolPolicy.evaluate(Some(&descriptor), binding.action(), &profile),
        PolicyDecision::RequireApproval,
        "evaluation remains non-authoritative"
    );
    assert!(matches!(
        ApprovalRequest::new(binding, 0, 600_000),
        Err(ApprovalError::PolicyAuthorizationRequired)
    ));
}

#[test]
fn read_data_may_run_without_approval_but_privileged_effect_requires_it() {
    let policy = ToolPolicy;
    let read = active_descriptor(Effect::ReadData);
    let write = active_descriptor(Effect::ExternalWrite);
    let profile = PermissionProfile::builder("profile-default", "v1")
        .expect("valid profile")
        .allow("fixture.tool", "v1", Effect::ReadData, "fixture-target")
        .expect("valid profile entry")
        .allow(
            "fixture.tool",
            "v1",
            Effect::ExternalWrite,
            "fixture-target",
        )
        .expect("valid profile entry")
        .build();

    assert_eq!(
        policy.evaluate(Some(&read), &action(Effect::ReadData), &profile),
        PolicyDecision::AllowWithoutApproval
    );
    assert_eq!(
        policy.evaluate(Some(&write), &action(Effect::ExternalWrite), &profile),
        PolicyDecision::RequireApproval
    );
}

#[test]
fn read_data_target_must_be_in_the_immutable_profile() {
    let descriptor = active_descriptor(Effect::ReadData);
    let profile = PermissionProfile::builder("profile-default", "v1")
        .expect("valid profile")
        .allow("fixture.tool", "v1", Effect::ReadData, "allowed-target")
        .expect("valid profile entry")
        .build();
    let outside_target = Action::new(
        "fixture.tool",
        "v1",
        Effect::ReadData,
        "outside-target",
        parse_action_parameters(r#"{"value":1}"#).expect("valid parameters"),
    )
    .expect("syntactically valid action");

    assert_eq!(
        ToolPolicy.evaluate(Some(&descriptor), &outside_target, &profile),
        PolicyDecision::Denied(DenialCode::OutsidePermissionProfile)
    );
}

#[test]
fn descriptor_schema_rejects_nonconforming_action_input() {
    let descriptor = active_descriptor(Effect::ExternalWrite);
    let profile = PermissionProfile::builder("profile-default", "v1")
        .expect("valid profile")
        .allow(
            "fixture.tool",
            "v1",
            Effect::ExternalWrite,
            "fixture-target",
        )
        .expect("valid profile entry")
        .build();
    let missing_required_value = Action::new(
        "fixture.tool",
        "v1",
        Effect::ExternalWrite,
        "fixture-target",
        parse_action_parameters(r#"{"other":1}"#).expect("valid parameters"),
    )
    .expect("syntactically valid action");

    assert_eq!(
        ToolPolicy.evaluate(Some(&descriptor), &missing_required_value, &profile),
        PolicyDecision::Denied(DenialCode::InvalidInput)
    );
}

#[test]
fn malformed_json_never_becomes_an_owned_action() {
    assert!(parse_action_parameters("{").is_err());
}

#[test]
fn serialized_action_input_limit_is_checked_before_json_parsing() {
    let at_limit = format!("{}{{}}", " ".repeat(65_534));
    let over_limit = format!("{}{{}}", " ".repeat(65_535));

    assert!(parse_action_parameters(&at_limit).is_ok());
    assert_eq!(
        parse_action_parameters(&over_limit),
        Err(ToolAdapterError::InputTooLarge)
    );
}

#[test]
fn serialized_descriptor_schema_limit_is_checked_before_json_parsing() {
    let valid_schema =
        r#"{"type":"object","properties":{},"required":[],"additionalProperties":false}"#;
    let at_limit = format!(
        "{}{}",
        " ".repeat(65_536 - valid_schema.len()),
        valid_schema
    );
    let oversized_invalid_json = "x".repeat(65_537);

    assert!(parse_input_schema(&at_limit).is_ok());
    assert_eq!(
        parse_input_schema(&oversized_invalid_json),
        Err(ToolAdapterError::SchemaTooLarge)
    );
}

#[test]
fn json_schema_number_accepts_a_finite_decimal() {
    let descriptor = active_descriptor(Effect::ExternalWrite);
    let profile = PermissionProfile::builder("profile-default", "v1")
        .expect("valid profile")
        .allow(
            "fixture.tool",
            "v1",
            Effect::ExternalWrite,
            "fixture-target",
        )
        .expect("valid profile entry")
        .build();
    let decimal = Action::new(
        "fixture.tool",
        "v1",
        Effect::ExternalWrite,
        "fixture-target",
        parse_action_parameters(r#"{"value":1.5}"#).expect("finite decimal is valid JSON"),
    )
    .expect("valid action");

    assert_eq!(
        ToolPolicy.evaluate(Some(&descriptor), &decimal, &profile),
        PolicyDecision::RequireApproval
    );
}

#[test]
fn action_parameters_preserve_high_precision_decimal_text() {
    let parameters = parse_action_parameters(r#"{"value":0.10000000000000000001}"#)
        .expect("high-precision decimal is valid JSON");

    assert_eq!(
        parameters.canonical(),
        r#"{"value":0.10000000000000000001}"#
    );
}

#[test]
fn owned_json_number_rejects_non_json_values() {
    assert!(JsonNumber::new("NaN").is_err());
    assert!(JsonNumber::new("1.").is_err());
    assert!(JsonNumber::new("01").is_err());
    assert!(JsonNumber::new("1.5e-2").is_ok());
}

#[test]
fn unsupported_json_schema_constraints_fail_closed() {
    let ignored_property_constraint = r#"{
      "type":"object",
      "properties":{"value":{"type":"number","minimum":1}},
      "required":["value"],
      "additionalProperties":false
    }"#;
    let ignored_root_constraint = r#"{
      "type":"object",
      "properties":{"value":{"type":"number"}},
      "required":["value"],
      "additionalProperties":false,
      "oneOf":[]
    }"#;

    assert!(parse_input_schema(ignored_property_constraint).is_err());
    assert!(parse_input_schema(ignored_root_constraint).is_err());
}

#[test]
fn duplicate_json_schema_members_fail_closed() {
    let duplicate_property = r#"{
      "type":"object",
      "properties":{
        "value":{"type":"number"},
        "value":{"type":"string"}
      },
      "required":["value"],
      "additionalProperties":false
    }"#;

    assert_eq!(
        parse_input_schema(duplicate_property),
        Err(ToolAdapterError::InvalidSchema)
    );
}

#[test]
fn duplicate_action_parameter_members_fail_closed_at_every_depth() {
    for serialized in [
        r#"{"path":"reviewed","path":"executed"}"#,
        r#"{"request":{"path":"reviewed","path":"executed"}}"#,
    ] {
        assert_eq!(
            parse_action_parameters(serialized),
            Err(ToolAdapterError::InvalidJson)
        );
    }
}

#[test]
fn owned_schema_constructor_rejects_duplicate_properties() {
    assert!(matches!(
        koduck_ai::domain::tool::InputSchema::object(
            [
                (
                    "value".to_owned(),
                    koduck_ai::domain::tool::JsonValueKind::String
                ),
                (
                    "value".to_owned(),
                    koduck_ai::domain::tool::JsonValueKind::Integer,
                ),
            ],
            ["value".to_owned()],
            false,
        ),
        Err(koduck_ai::domain::tool::ToolValueError::Invalid {
            field: "schema_property"
        })
    ));
}

#[test]
fn json_schema_integer_accepts_the_full_unsigned_json_range() {
    let schema = parse_input_schema(
        r#"{
          "type":"object",
          "properties":{"value":{"type":"integer"}},
          "required":["value"],
          "additionalProperties":false
        }"#,
    )
    .expect("supported integer schema");
    let descriptor = CapabilityDescriptor::new(
        "fixture.tool",
        "v1",
        Effect::ExternalWrite,
        DescriptorState::Active,
        schema,
    )
    .expect("valid descriptor");
    let profile = PermissionProfile::builder("profile-default", "v1")
        .expect("valid profile")
        .allow(
            "fixture.tool",
            "v1",
            Effect::ExternalWrite,
            "fixture-target",
        )
        .expect("valid profile entry")
        .build();
    let action = Action::new(
        "fixture.tool",
        "v1",
        Effect::ExternalWrite,
        "fixture-target",
        parse_action_parameters(r#"{"value":18446744073709551615}"#)
            .expect("u64 maximum is valid JSON"),
    )
    .expect("valid action");

    assert_eq!(
        ToolPolicy.evaluate(Some(&descriptor), &action, &profile),
        PolicyDecision::RequireApproval
    );
}

#[test]
fn action_envelope_rejects_an_oversized_descriptor_version() {
    let parameters = parse_action_parameters(r#"{"value":1}"#).expect("valid parameters");
    let at_limit = "a".repeat(MAX_DESCRIPTOR_VERSION_BYTES);

    assert!(
        Action::new(
            "fixture.tool",
            at_limit.clone(),
            Effect::ReadData,
            "fixture-target",
            parameters.clone(),
        )
        .is_ok()
    );
    assert_eq!(
        Action::new(
            "fixture.tool",
            format!("{at_limit}x"),
            Effect::ReadData,
            "fixture-target",
            parameters,
        )
        .unwrap_err(),
        ToolValueError::Invalid {
            field: "descriptor_version"
        }
    );
}

#[test]
fn action_envelope_rejects_an_oversized_target() {
    let parameters = parse_action_parameters(r#"{"value":1}"#).expect("valid parameters");
    let at_limit = "a".repeat(MAX_ACTION_TARGET_BYTES);

    assert!(
        Action::new(
            "fixture.tool",
            "v1",
            Effect::ReadData,
            at_limit.clone(),
            parameters.clone(),
        )
        .is_ok()
    );
    assert_eq!(
        Action::new(
            "fixture.tool",
            "v1",
            Effect::ReadData,
            format!("{at_limit}x"),
            parameters,
        )
        .unwrap_err(),
        ToolValueError::Invalid { field: "target" }
    );
}

#[test]
fn action_envelope_rejects_a_target_with_control_characters() {
    let parameters = parse_action_parameters(r#"{"value":1}"#).expect("valid parameters");

    for invalid_target in ["fixture\ttarget", "fixture\u{0}target", "fixture\ntarget"] {
        assert_eq!(
            Action::new(
                "fixture.tool",
                "v1",
                Effect::ReadData,
                invalid_target,
                parameters.clone(),
            )
            .unwrap_err(),
            ToolValueError::Invalid { field: "target" },
            "target {invalid_target:?} must be rejected before the D-7 envelope is allocated"
        );
    }
}

#[test]
fn action_envelope_rejects_a_non_ascii_target() {
    let parameters = parse_action_parameters(r#"{"value":1}"#).expect("valid parameters");

    assert_eq!(
        Action::new("fixture.tool", "v1", Effect::ReadData, "目标", parameters,).unwrap_err(),
        ToolValueError::Invalid { field: "target" },
        "a non-ASCII target must be rejected to keep the D-7 envelope identity ASCII"
    );
}

#[test]
fn profile_entries_share_the_action_envelope_bound() {
    let over_target = "a".repeat(MAX_ACTION_TARGET_BYTES + 1);
    let over_version = "a".repeat(MAX_DESCRIPTOR_VERSION_BYTES + 1);

    assert_eq!(
        PermissionProfile::builder("profile-default", "v1")
            .expect("valid profile")
            .allow("fixture.tool", "v1", Effect::ReadData, over_target)
            .unwrap_err(),
        ToolValueError::Invalid { field: "target" }
    );
    assert_eq!(
        PermissionProfile::builder("profile-default", "v1")
            .expect("valid profile")
            .allow(
                "fixture.tool",
                over_version,
                Effect::ReadData,
                "fixture-target"
            )
            .unwrap_err(),
        ToolValueError::Invalid {
            field: "descriptor_version"
        }
    );
}

#[test]
fn profile_identity_fields_share_one_envelope_bound() {
    // PermissionProfile::builder rejects an oversized or non-ASCII identity.
    assert_eq!(
        PermissionProfile::builder("a".repeat(MAX_PROFILE_ID_BYTES + 1), "v1").unwrap_err(),
        ToolValueError::Invalid {
            field: "profile_id"
        }
    );
    assert_eq!(
        PermissionProfile::builder("profile-default", "a".repeat(MAX_PROFILE_VERSION_BYTES + 1))
            .unwrap_err(),
        ToolValueError::Invalid {
            field: "profile_version"
        }
    );
    assert_eq!(
        PermissionProfile::builder("目标", "v1").unwrap_err(),
        ToolValueError::Invalid {
            field: "profile_id"
        }
    );

    // ExactActionBinding applies the same shared bound before it hashes or
    // retains the profile identity in D-6/D-7 state.
    let binding = |profile_id: String| {
        ExactActionBinding::new(
            TenantId::new("tenant-a").expect("valid tenant"),
            ThreadId::new(),
            TurnId::new(),
            LeaseGeneration::initial(),
            (profile_id, "v1"),
            AttemptId::new(),
            action(Effect::ReadData),
        )
    };
    assert!(
        binding("a".repeat(MAX_PROFILE_ID_BYTES)).is_ok(),
        "a profile ID at the byte bound is accepted"
    );
    assert!(
        binding("a".repeat(MAX_PROFILE_ID_BYTES + 1)).is_err(),
        "a profile ID over the byte bound is rejected before D-6/D-7 allocation"
    );
    assert!(
        binding(String::from("目标")).is_err(),
        "a non-ASCII profile ID is rejected before D-6/D-7 allocation"
    );
}
