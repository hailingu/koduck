// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Bounded, correlated audit metadata for C-5 policy, approval, and
//! execution terminals (ADR-0003 TC-14).

use serde::Serialize;
use thiserror::Error;

use crate::domain::execution::{
    ActionDigest, ApprovalDecision, ApprovalId, ApprovalStatus, ExactActionBinding, ExecutionStatus,
};
use crate::domain::{LeaseGeneration, TenantId, ThreadId, TurnId};

use super::ToolExecutionOutcome;
use super::executor_envelope::EffectState;
use super::policy::DenialCode;
use super::tool_projection::output_digest;

/// Maximum serialized size of one audit record (ADR-0003 TC-14).
pub const MAX_AUDIT_RECORD_BYTES: usize = 16_384;

/// A serialized audit record exceeded the TC-14 byte bound.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("serialized audit record exceeds the 16,384-byte bound")]
pub struct ToolAuditRecordTooLarge;

/// Trusted metadata available when policy denies before D-6 or D-7 allocation.
///
/// Descriptor, Permission Profile, and exact-action digest fields remain
/// optional because malformed calls and missing configuration can deny before
/// those values exist. The context intentionally contains no raw parameters,
/// executor output, or credential values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyDenialContext {
    tenant_id: TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
    lease_generation: LeaseGeneration,
    descriptor_id: Option<String>,
    descriptor_version: Option<String>,
    profile_id: Option<String>,
    profile_version: Option<String>,
    action_digest: Option<ActionDigest>,
}

impl PolicyDenialContext {
    /// Creates the authenticated Turn metadata for one pre-attempt denial.
    #[must_use]
    pub fn new(
        tenant_id: TenantId,
        thread_id: ThreadId,
        turn_id: TurnId,
        lease_generation: LeaseGeneration,
    ) -> Self {
        Self {
            tenant_id,
            thread_id,
            turn_id,
            lease_generation,
            descriptor_id: None,
            descriptor_version: None,
            profile_id: None,
            profile_version: None,
            action_digest: None,
        }
    }

    /// Adds descriptor metadata when configuration resolved it before denial.
    #[must_use]
    pub fn with_descriptor(
        mut self,
        descriptor_id: impl Into<String>,
        descriptor_version: impl Into<String>,
    ) -> Self {
        self.descriptor_id = Some(descriptor_id.into());
        self.descriptor_version = Some(descriptor_version.into());
        self
    }

    /// Adds Permission Profile metadata when policy selected it before denial.
    #[must_use]
    pub fn with_profile(
        mut self,
        profile_id: impl Into<String>,
        profile_version: impl Into<String>,
    ) -> Self {
        self.profile_id = Some(profile_id.into());
        self.profile_version = Some(profile_version.into());
        self
    }

    /// Adds a bounded correlation digest when a complete action was available
    /// before policy denied it.
    #[must_use]
    pub const fn with_action_digest(mut self, action_digest: ActionDigest) -> Self {
        self.action_digest = Some(action_digest);
        self
    }

    /// Reconstructs pre-attempt metadata from a sealed binding for callers
    /// that already completed trusted action translation.
    #[must_use]
    pub fn from_binding(binding: &ExactActionBinding) -> Self {
        let action = binding.action();
        Self::new(
            binding.tenant_id().clone(),
            binding.thread_id(),
            binding.turn_id(),
            binding.lease_generation(),
        )
        .with_descriptor(action.descriptor_id(), action.descriptor_version())
        .with_profile(binding.profile_id(), binding.profile_version())
        .with_action_digest(binding.pre_attempt_audit_digest())
    }
}

/// One correlated, content-minimized audit terminal record.
///
/// The record correlates tenant, Thread, Turn, D-6/D-7 identity, descriptor
/// and Permission Profile versions, the action digest, lease generation,
/// policy decision, executor effect state, timing, byte counts, and the
/// stable terminal code. It is constructed only from owned canonical values:
/// the type has no field for raw action parameters, executor results, or
/// credential values, so none can enter an audit record (ADR-0003 TC-14).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolAuditRecord {
    /// Tenant pseudonym that owns the audited action.
    tenant_id: String,
    /// Durable Thread identity.
    thread_id: String,
    /// Durable Turn identity.
    turn_id: String,
    /// Canonical D-7 identity, or `None` for a pre-attempt policy denial.
    attempt_id: Option<String>,
    /// Canonical D-6 identity, or `None` when no approval exists.
    approval_id: Option<String>,
    /// Bound descriptor identity.
    descriptor_id: Option<String>,
    /// Bound descriptor version.
    descriptor_version: Option<String>,
    /// Bound Permission Profile identity.
    profile_id: Option<String>,
    /// Bound Permission Profile version.
    profile_version: Option<String>,
    /// Hex-encoded exact-action correlation digest.
    action_digest: Option<String>,
    /// Bound foreground lease generation.
    lease_generation: u64,
    /// Stable policy-decision code for this terminal.
    policy_decision: String,
    /// Canonical D-6 lifecycle status at the audited terminal, if any.
    approval_status: Option<String>,
    /// Canonical D-6 decision at the audited terminal, if any.
    approval_decision: Option<String>,
    /// Canonical D-6 record version at the audited terminal, if any.
    approval_version: Option<u64>,
    /// Canonical D-7 transition at the audited terminal, if any.
    execution_status: Option<String>,
    /// Executor-observed effect state at the terminal, if any.
    effect_state: Option<String>,
    /// Stable failure code of a failed terminal, if any.
    failure_code: Option<String>,
    /// Committed output byte count of a success terminal, if any.
    output_bytes: Option<u64>,
    /// Hex-encoded digest of committed output, if any.
    output_digest: Option<String>,
    /// Audit observation time in Unix epoch milliseconds.
    at_millis: u64,
}

impl ToolAuditRecord {
    /// Creates the audit record for one default-deny policy terminal.
    ///
    /// A denial allocates no D-6/D-7, so the record carries the typed denial
    /// code and the exact-action correlation without attempt or approval
    /// identity (ADR-0003 TC-02/TC-14).
    #[must_use]
    pub fn policy_denial(
        context: &PolicyDenialContext,
        denial: DenialCode,
        at_millis: u64,
    ) -> Self {
        Self {
            policy_decision: denial.stable_code().to_owned(),
            ..Self::policy_denial_base(context, at_millis)
        }
    }

    /// Creates the audit record for one canonical D-6 resolution terminal.
    ///
    /// The canonical identity, status, decision, and version come from the
    /// already-resolved D-6; the record correlates them with the exact-action
    /// binding without any approver- or caller-supplied content
    /// (ADR-0003 TC-05/TC-14).
    #[must_use]
    pub fn approval_resolution(
        binding: &ExactActionBinding,
        approval_id: ApprovalId,
        status: ApprovalStatus,
        decision: Option<ApprovalDecision>,
        version: u64,
        at_millis: u64,
    ) -> Self {
        Self {
            attempt_id: Some(binding.attempt_id().as_uuid().to_string()),
            approval_id: Some(approval_id.as_uuid().to_string()),
            approval_status: Some(status.as_str().to_owned()),
            approval_decision: decision.map(|decision| decision.as_str().to_owned()),
            approval_version: Some(version),
            policy_decision: "approval_resolved".to_owned(),
            ..Self::correlated_base(binding, at_millis)
        }
    }

    /// Creates the audit record for one D-7 execution terminal.
    ///
    /// The bounded outcome supplies the transition, effect state, stable
    /// failure code, and committed byte count; the committed output is
    /// correlated by its digest, never by content (ADR-0003 TC-11/TC-14).
    #[must_use]
    pub fn execution_terminal(
        binding: &ExactActionBinding,
        outcome: &ToolExecutionOutcome,
        at_millis: u64,
    ) -> Self {
        let mut record = Self {
            attempt_id: Some(binding.attempt_id().as_uuid().to_string()),
            policy_decision: "executed".to_owned(),
            execution_status: Some(outcome_status(outcome).as_str().to_owned()),
            effect_state: Some(outcome_effect_state(outcome).as_str().to_owned()),
            ..Self::correlated_base(binding, at_millis)
        };
        match outcome {
            ToolExecutionOutcome::Succeeded { output, .. } => {
                record.output_bytes = Some(output.len() as u64);
                record.output_digest = Some(output_digest(output));
            }
            ToolExecutionOutcome::Failed { code, .. } => {
                record.failure_code = Some(code.stable_code().to_owned());
            }
            ToolExecutionOutcome::TimedOut { .. } | ToolExecutionOutcome::Cancelled { .. } => {}
        }
        record
    }

    /// Returns the shared exact-action correlation fields.
    fn correlated_base(binding: &ExactActionBinding, at_millis: u64) -> Self {
        let action = binding.action();
        Self {
            tenant_id: binding.tenant_id().as_str().to_owned(),
            thread_id: binding.thread_id().as_uuid().to_string(),
            turn_id: binding.turn_id().as_uuid().to_string(),
            attempt_id: None,
            approval_id: None,
            descriptor_id: Some(action.descriptor_id().to_owned()),
            descriptor_version: Some(action.descriptor_version().to_owned()),
            profile_id: Some(binding.profile_id().to_owned()),
            profile_version: Some(binding.profile_version().to_owned()),
            action_digest: Some(format!("{:x}", binding.action_digest())),
            lease_generation: binding.lease_generation().get(),
            policy_decision: String::new(),
            approval_status: None,
            approval_decision: None,
            approval_version: None,
            execution_status: None,
            effect_state: None,
            failure_code: None,
            output_bytes: None,
            output_digest: None,
            at_millis,
        }
    }

    /// Returns the correlation fields available before D-7 allocation.
    fn policy_denial_base(context: &PolicyDenialContext, at_millis: u64) -> Self {
        Self {
            tenant_id: context.tenant_id.as_str().to_owned(),
            thread_id: context.thread_id.as_uuid().to_string(),
            turn_id: context.turn_id.as_uuid().to_string(),
            attempt_id: None,
            approval_id: None,
            descriptor_id: context.descriptor_id.clone(),
            descriptor_version: context.descriptor_version.clone(),
            profile_id: context.profile_id.clone(),
            profile_version: context.profile_version.clone(),
            action_digest: context.action_digest.map(|digest| format!("{digest:x}")),
            lease_generation: context.lease_generation.get(),
            policy_decision: String::new(),
            approval_status: None,
            approval_decision: None,
            approval_version: None,
            execution_status: None,
            effect_state: None,
            failure_code: None,
            output_bytes: None,
            output_digest: None,
            at_millis,
        }
    }

    /// Returns the stable policy-decision code carried by this record.
    #[must_use]
    pub fn policy_decision(&self) -> &str {
        &self.policy_decision
    }

    /// Serializes the record within the TC-14 byte bound.
    ///
    /// # Errors
    ///
    /// Returns [`ToolAuditRecordTooLarge`] when the serialized record would
    /// exceed [`MAX_AUDIT_RECORD_BYTES`]; the caller then emits no record
    /// rather than truncating correlated evidence. The adapter layer owns
    /// the wire serialization.
    pub fn serialized_within_bound(
        &self,
        serialized: String,
    ) -> Result<String, ToolAuditRecordTooLarge> {
        if serialized.len() > MAX_AUDIT_RECORD_BYTES {
            return Err(ToolAuditRecordTooLarge);
        }
        Ok(serialized)
    }
}

fn outcome_status(outcome: &ToolExecutionOutcome) -> ExecutionStatus {
    match outcome {
        ToolExecutionOutcome::Succeeded { .. } => ExecutionStatus::Succeeded,
        ToolExecutionOutcome::Cancelled { .. } => ExecutionStatus::Cancelled,
        ToolExecutionOutcome::TimedOut { .. } => ExecutionStatus::TimedOut,
        ToolExecutionOutcome::Failed { .. } => ExecutionStatus::Failed,
    }
}

fn outcome_effect_state(outcome: &ToolExecutionOutcome) -> EffectState {
    match outcome {
        ToolExecutionOutcome::Succeeded { effect_state, .. }
        | ToolExecutionOutcome::Cancelled { effect_state }
        | ToolExecutionOutcome::TimedOut { effect_state }
        | ToolExecutionOutcome::Failed { effect_state, .. } => *effect_state,
    }
}

/// Consumer-owned sink receiving one bounded audit record per terminal.
///
/// The sink receives only already-serialized records that passed the TC-14
/// bound, so no adapter can widen audit content.
pub trait ToolAuditSink {
    /// Records one serialized audit terminal.
    ///
    /// # Errors
    ///
    /// Returns an implementation error when the audit trail cannot receive
    /// the record; the caller reports the failure without retry storms.
    fn record(&mut self, serialized: &str) -> Result<(), ToolAuditError>;
}

/// A sink failure reported to the caller without concealing the terminal.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("tool audit sink unavailable")]
pub struct ToolAuditError;

/// Fail-closed audit sink used until an audit trail adapter is configured.
///
/// Recording fails rather than silently dropping correlated evidence
/// (ADR-0003 TC-14).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoToolAudits;

impl ToolAuditSink for NoToolAudits {
    fn record(&mut self, _serialized: &str) -> Result<(), ToolAuditError> {
        Err(ToolAuditError)
    }
}
