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
use super::executor_envelope::{EffectState, ExecutionFailure};
use super::policy::DenialCode;
use super::tool_projection::output_digest;

/// Maximum serialized size of one audit record (ADR-0003 TC-14).
pub const MAX_AUDIT_RECORD_BYTES: usize = 16_384;

/// Maximum raw tenant bytes retained verbatim in an audit record.
///
/// Every other record field is domain-bounded (descriptor and profile
/// identities at 128 bytes, fixed UUIDs, fixed hex digests, stable codes), so
/// the tenant pseudonym — which accepts any non-blank length — is the only
/// field that could push a serialized record past [`MAX_AUDIT_RECORD_BYTES`]
/// and drop a required audit terminal. An over-bound tenant is retained as a
/// deterministic bounded form instead: an at-most-128-byte prefix, `~`, and
/// the 64-hex SHA-256 digest of the full identity, so every valid identity
/// still emits exactly one correlated record and the same identity always
/// retains the same form (ADR-0003 TC-14).
const MAX_AUDIT_TENANT_BYTES: usize = 256;

/// Retains the tenant pseudonym in a form that cannot exceed the audit
/// record's share of [`MAX_AUDIT_RECORD_BYTES`].
///
/// Identities within [`MAX_AUDIT_TENANT_BYTES`] bytes are retained verbatim;
/// longer identities keep their at-most-128-byte prefix (cut at a UTF-8
/// character boundary) plus the digest of the full identity, so correlation
/// stays deterministic for over-bound tenants.
fn bounded_tenant_id(tenant: &TenantId) -> String {
    let raw = tenant.as_str();
    if raw.len() <= MAX_AUDIT_TENANT_BYTES {
        return raw.to_owned();
    }
    let mut prefix_end = 128.min(raw.len());
    while prefix_end > 0 && !raw.is_char_boundary(prefix_end) {
        prefix_end -= 1;
    }
    format!("{}~{}", &raw[..prefix_end], output_digest(raw.as_bytes()))
}

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
/// credential values, so none can enter an audit record; the tenant
/// pseudonym is the single unbounded identity and is retained through
/// [`bounded_tenant_id`]'s bounded deterministic form, so no valid identity
/// can push a record past [`MAX_AUDIT_RECORD_BYTES`] (ADR-0003 TC-14).
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

    /// Creates the audit record for the retry-budget terminal delivered
    /// without a new D-7.
    ///
    /// The Turn's 16-slot budget is exhausted, so no new D-6/D-7 identity
    /// exists; the record correlates the exact action (descriptor, Permission
    /// Profile, pre-attempt digest) with the delivered `failed/attempt_limit`
    /// terminal through its stable code, without any attempt or approval
    /// identity (ADR-0003 TC-08/TC-14).
    #[must_use]
    pub fn budget_exhausted(context: &PolicyDenialContext, at_millis: u64) -> Self {
        let stable = ExecutionFailure::AttemptLimit.stable_code();
        Self {
            policy_decision: stable.to_owned(),
            failure_code: Some(stable.to_owned()),
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

    /// Creates the canonical D-6 resolution audit record from the persisted
    /// approval correlation columns.
    ///
    /// The store's winning decision transition returns these columns, so the
    /// route-level resolution and its audit append share one atomic
    /// transaction without reconstructing the exact-action parameters
    /// (ADR-0003 TC-14).
    #[allow(
        clippy::too_many_arguments,
        reason = "each parameter is one persisted correlation field of the resolved approval"
    )]
    #[must_use]
    pub fn approval_resolution_from_persisted(
        tenant_id: &crate::domain::TenantId,
        thread_id: crate::domain::ThreadId,
        turn_id: crate::domain::TurnId,
        attempt_id: &crate::domain::execution::AttemptId,
        approval_id: ApprovalId,
        descriptor_id: &str,
        descriptor_version: &str,
        profile_id: &str,
        profile_version: &str,
        action_digest_hex: &str,
        lease_generation: u64,
        status: ApprovalStatus,
        decision: Option<ApprovalDecision>,
        version: u64,
        at_millis: u64,
    ) -> Self {
        Self {
            approval_id: Some(approval_id.as_uuid().to_string()),
            approval_status: Some(status.as_str().to_owned()),
            approval_decision: decision.map(|decision| decision.as_str().to_owned()),
            approval_version: Some(version),
            attempt_id: Some(attempt_id.as_uuid().to_string()),
            policy_decision: "approval_resolved".to_owned(),
            descriptor_id: Some(descriptor_id.to_owned()),
            descriptor_version: Some(descriptor_version.to_owned()),
            profile_id: Some(profile_id.to_owned()),
            profile_version: Some(profile_version.to_owned()),
            action_digest: Some(action_digest_hex.to_owned()),
            lease_generation,
            tenant_id: bounded_tenant_id(tenant_id),
            thread_id: thread_id.as_uuid().to_string(),
            turn_id: turn_id.as_uuid().to_string(),
            at_millis,
            ..Self::correlated_defaults()
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

    /// Builds the correlated execution-terminal record for one D-7 closed by
    /// lease-expiry recovery, from the closed attempt's persisted
    /// correlation fields.
    ///
    /// The exact-action parameters are not persisted on the attempt row, so
    /// the record carries the stored digest directly instead of recomputing
    /// it; the bounded serialization and byte bound are unchanged
    /// (ADR-0003 TC-14).
    #[allow(
        clippy::too_many_arguments,
        reason = "each parameter is one persisted correlation field of the closed attempt"
    )]
    #[must_use]
    pub fn lease_recovery_terminal(
        tenant_id: &crate::domain::TenantId,
        thread_id: crate::domain::ThreadId,
        turn_id: crate::domain::TurnId,
        attempt_id: &crate::domain::execution::AttemptId,
        descriptor_id: &str,
        descriptor_version: &str,
        profile_id: &str,
        profile_version: &str,
        action_digest_hex: &str,
        lease_generation: u64,
        outcome: &ToolExecutionOutcome,
        at_millis: u64,
    ) -> Self {
        let mut record = Self {
            tenant_id: bounded_tenant_id(tenant_id),
            thread_id: thread_id.as_uuid().to_string(),
            turn_id: turn_id.as_uuid().to_string(),
            attempt_id: Some(attempt_id.as_uuid().to_string()),
            policy_decision: "executed".to_owned(),
            execution_status: Some(outcome_status(outcome).as_str().to_owned()),
            effect_state: Some(outcome_effect_state(outcome).as_str().to_owned()),
            descriptor_id: Some(descriptor_id.to_owned()),
            descriptor_version: Some(descriptor_version.to_owned()),
            profile_id: Some(profile_id.to_owned()),
            profile_version: Some(profile_version.to_owned()),
            action_digest: Some(action_digest_hex.to_owned()),
            lease_generation,
            at_millis,
            ..Self::correlated_defaults()
        };
        if let ToolExecutionOutcome::Failed { code, .. } = outcome {
            record.failure_code = Some(code.stable_code().to_owned());
        }
        record
    }

    /// Returns the neutral field defaults shared by manual constructors.
    fn correlated_defaults() -> Self {
        Self {
            tenant_id: String::new(),
            thread_id: String::new(),
            turn_id: String::new(),
            attempt_id: None,
            approval_id: None,
            descriptor_id: None,
            descriptor_version: None,
            profile_id: None,
            profile_version: None,
            action_digest: None,
            lease_generation: 0,
            policy_decision: String::new(),
            approval_status: None,
            approval_decision: None,
            approval_version: None,
            execution_status: None,
            effect_state: None,
            failure_code: None,
            output_bytes: None,
            output_digest: None,
            at_millis: 0,
        }
    }

    /// Returns the shared exact-action correlation fields.
    fn correlated_base(binding: &ExactActionBinding, at_millis: u64) -> Self {
        let action = binding.action();
        Self {
            tenant_id: bounded_tenant_id(binding.tenant_id()),
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
            tenant_id: bounded_tenant_id(&context.tenant_id),
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

    /// Returns the owning tenant pseudonym, for durable Turn correlation.
    #[must_use]
    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    /// Returns the owning durable Thread identity, for Turn correlation.
    #[must_use]
    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    /// Returns the owning durable Turn identity, for Turn correlation.
    #[must_use]
    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    /// Returns the audit observation time of this terminal.
    #[must_use]
    pub const fn at_millis(&self) -> u64 {
        self.at_millis
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
/// The sink receives the owned application record together with its
/// already-serialized form that passed the TC-14 bound, so no adapter can
/// widen audit content and a durable trail can persist its own Turn
/// correlation columns without re-parsing the serialized record.
pub trait ToolAuditSink {
    /// Records one serialized audit terminal.
    ///
    /// # Errors
    ///
    /// Returns an implementation error when the audit trail cannot receive
    /// the record; the caller reports the failure without retry storms.
    fn record(&mut self, record: &ToolAuditRecord, serialized: &str) -> Result<(), ToolAuditError>;
}

/// A sink failure reported to the caller without concealing the terminal.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("tool audit sink unavailable")]
pub struct ToolAuditError;

/// Fail-closed audit sink used when no trail is configured.
///
/// Recording fails rather than silently dropping correlated evidence
/// (ADR-0003 TC-14).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoToolAudits;

impl ToolAuditSink for NoToolAudits {
    fn record(
        &mut self,
        _record: &ToolAuditRecord,
        _serialized: &str,
    ) -> Result<(), ToolAuditError> {
        Err(ToolAuditError)
    }
}

impl ToolAuditTrail for NoToolAudits {
    fn emit(&mut self, _record: &ToolAuditRecord) -> Result<(), ToolAuditEmitError> {
        // No trail is configured: the emission fails closed and surfaces as a
        // structured diagnostic rather than silently dropping evidence
        // (ADR-0003 TC-14).
        Err(ToolAuditEmitError::Sink(ToolAuditError))
    }
}

/// Adapter-owned emission boundary for audit terminals.
///
/// The adapter serializes this owned application type into its wire format
/// and enforces [`MAX_AUDIT_RECORD_BYTES`] through
/// [`ToolAuditRecord::serialized_within_bound`] before delivering the record
/// to its trail, so no adapter can widen audit content (ADR-0003 TC-14).
pub trait ToolAuditTrail {
    /// Emits one bounded audit terminal to the configured trail.
    ///
    /// # Errors
    ///
    /// Returns [`ToolAuditEmitError::TooLarge`] when the record cannot
    /// serialize within [`MAX_AUDIT_RECORD_BYTES`] — no record is emitted
    /// rather than truncating correlated evidence — or
    /// [`ToolAuditEmitError::Sink`] when the trail cannot receive it.
    fn emit(&mut self, record: &ToolAuditRecord) -> Result<(), ToolAuditEmitError>;
}

/// Why one audit terminal could not be emitted to the trail.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ToolAuditEmitError {
    /// The serialized record exceeded the TC-14 byte bound.
    #[error("serialized audit record exceeds the 16,384-byte bound")]
    TooLarge(ToolAuditRecordTooLarge),
    /// The audit trail could not receive the serialized record.
    #[error("tool audit sink unavailable")]
    Sink(ToolAuditError),
}

/// Emits one bounded audit terminal without changing any canonical state.
///
/// A record that cannot serialize within the TC-14 bound, or a trail that
/// cannot receive it, is never concealed and never retried: the failure is
/// reported as a structured, content-free diagnostic so operators and
/// reconciliation tooling can observe the missing audit evidence, while the
/// already-committed terminal stands unchanged (ADR-0003 TC-14).
pub(crate) fn record_audit(audits: &mut dyn ToolAuditTrail, record: &ToolAuditRecord) {
    if let Err(error) = audits.emit(record) {
        let (event, cause) = match error {
            ToolAuditEmitError::TooLarge(cause) => {
                ("tool_audit_record_too_large", cause.to_string())
            }
            ToolAuditEmitError::Sink(cause) => ("tool_audit_sink_failed", cause.to_string()),
        };
        eprintln!(
            "event={event} error={cause} policy_decision={}",
            record.policy_decision()
        );
    }
}
