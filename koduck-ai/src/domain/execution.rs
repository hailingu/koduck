// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Canonical exact-attempt approval and execution lifecycle rules.

use std::collections::BTreeMap;
use std::fmt::{self, LowerHex};
use std::sync::{Arc, Mutex, MutexGuard};

use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use super::tool::{Action, validate_profile_identity};
use super::{LeaseGeneration, TenantId, ThreadId, TurnId};

mod authority;

pub(crate) use authority::TurnAuthorityCatalog;
use authority::TurnAuthorityKey;

const APPROVAL_MAX_AGE_MILLIS: u64 = 300_000;
const MAX_ATTEMPTS_PER_TURN: u8 = 16;
const ACTION_DIGEST_DOMAIN: &[u8] = b"koduck-action-v1\0";

/// Stable identity for one canonical D-6 Approval Request.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ApprovalId(Uuid);

impl ApprovalId {
    /// Allocates a random version-4 Approval Request identity.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Wraps a UUID received from a validated adapter.
    #[must_use]
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    /// Returns the UUID for persistence and adapter serialization.
    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for ApprovalId {
    fn default() -> Self {
        Self::new()
    }
}

/// The C-7-validated approval scope a principal must carry to resolve a D-6.
pub const TOOL_APPROVAL_SCOPE: &str = "ai.tool.approve";

/// The C-7-validated subject identity that resolved one D-6 decision.
///
/// Construction is crate-internal and derivable only from an authenticated
/// [`TrustContext`](super::TrustContext) whose sealed scopes carry
/// [`TOOL_APPROVAL_SCOPE`], so no external caller can mint this capability
/// and mutate canonical approval state around the C-5 decision service
/// (ADR-0003 TC-05); the durable store commits a terminal only from this
/// validated identity, never from request or Tool/MCP content.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ApproverId(String);

impl ApproverId {
    /// Derives the approver identity from one authenticated, scoped principal.
    ///
    /// Returns `None` when the subject is blank or the validated identity
    /// does not carry [`TOOL_APPROVAL_SCOPE`], because such a principal may
    /// never resolve a D-6.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "T-2 approval transport wiring is not complete")
    )]
    pub(crate) fn from_authenticated(trust: &super::TrustContext) -> Option<Self> {
        if trust.subject_id.trim().is_empty() || !trust.has_approval_scope(TOOL_APPROVAL_SCOPE) {
            return None;
        }
        Some(Self(trust.subject_id.clone()))
    }

    /// Returns the validated approver identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable identity for one canonical D-7 Execution Attempt.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AttemptId(Uuid);

impl AttemptId {
    /// Allocates a random version-4 Execution Attempt identity.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Wraps an existing UUID received from canonical persistence.
    #[must_use]
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    /// Returns the UUID for persistence and adapter serialization.
    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for AttemptId {
    fn default() -> Self {
        Self::new()
    }
}

/// Stable SHA-256 correlation digest for one canonical exact action.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ActionDigest([u8; 32]);

impl ActionDigest {
    /// Returns the stable digest bytes for adapter serialization.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl LowerHex for ActionDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// An invalid exact-action binding.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("{field} is invalid")]
pub struct BindingError {
    field: &'static str,
}

/// Every immutable field that one D-6 binds to exactly one D-7.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ExactActionBinding {
    tenant_id: TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
    lease_generation: LeaseGeneration,
    profile_id: String,
    profile_version: String,
    attempt_id: AttemptId,
    action: Action,
    action_digest: ActionDigest,
    approval_requirement: Option<ApprovalRequirement>,
}

impl ExactActionBinding {
    /// Creates an exact binding and its non-authoritative audit digest.
    ///
    /// # Errors
    ///
    /// Returns [`BindingError`] when the profile ID or version is blank,
    /// non-ASCII, contains control characters, or exceeds its shared byte bound.
    pub fn new(
        tenant_id: TenantId,
        thread_id: ThreadId,
        turn_id: TurnId,
        lease_generation: LeaseGeneration,
        profile: (impl Into<String>, impl Into<String>),
        attempt_id: AttemptId,
        action: Action,
    ) -> Result<Self, BindingError> {
        let (profile_id, profile_version) = profile;
        let profile_id = profile_id.into();
        let profile_version = profile_version.into();
        if let Err(field) = validate_profile_identity(&profile_id, &profile_version) {
            return Err(BindingError { field });
        }
        let mut binding = Self {
            tenant_id,
            thread_id,
            turn_id,
            lease_generation,
            profile_id,
            profile_version,
            attempt_id,
            action,
            action_digest: ActionDigest([0; 32]),
            approval_requirement: None,
        };
        binding.action_digest = binding.calculate_digest();
        Ok(binding)
    }

    /// Returns the bound D-7 identity.
    #[must_use]
    pub const fn attempt_id(&self) -> AttemptId {
        self.attempt_id
    }

    /// Returns the tenant bound to this exact action.
    #[must_use]
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the Thread bound to this exact action.
    #[must_use]
    pub const fn thread_id(&self) -> ThreadId {
        self.thread_id
    }

    /// Returns the Turn bound to this exact action.
    #[must_use]
    pub const fn turn_id(&self) -> TurnId {
        self.turn_id
    }

    /// Returns the foreground lease generation bound to this exact action.
    #[must_use]
    pub const fn lease_generation(&self) -> LeaseGeneration {
        self.lease_generation
    }

    /// Returns a bounded audit-correlation digest.
    ///
    /// Authorization always compares this complete structure, never the digest alone.
    #[must_use]
    pub const fn action_digest(&self) -> ActionDigest {
        self.action_digest
    }

    /// Returns the owned action for policy evaluation and adapter serialization.
    #[must_use]
    pub const fn action(&self) -> &Action {
        &self.action
    }

    /// Returns the bound Permission Profile ID.
    #[must_use]
    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }

    /// Returns the bound Permission Profile version.
    #[must_use]
    pub fn profile_version(&self) -> &str {
        &self.profile_version
    }

    #[allow(dead_code)] // T-2 runtime policy wiring is incomplete.
    pub(crate) fn authorize_policy(&mut self, requirement: ApprovalRequirement) {
        self.approval_requirement = Some(requirement);
    }

    pub(crate) const fn approval_requirement(&self) -> Option<ApprovalRequirement> {
        self.approval_requirement
    }

    fn calculate_digest(&self) -> ActionDigest {
        let mut hasher = Sha256::new();
        hasher.update(ACTION_DIGEST_DOMAIN);
        update_digest_field(&mut hasher, "descriptor_id", self.action.descriptor_id());
        update_digest_field(
            &mut hasher,
            "descriptor_version",
            self.action.descriptor_version(),
        );
        update_digest_field(&mut hasher, "target", self.action.target());
        update_digest_field(&mut hasher, "parameters", self.action.parameters());
        update_digest_field(&mut hasher, "effect", effect_name(self.action.effect()));
        update_digest_field(&mut hasher, "profile_id", &self.profile_id);
        update_digest_field(&mut hasher, "profile_version", &self.profile_version);
        update_digest_field(&mut hasher, "turn_id", &self.turn_id.as_uuid().to_string());
        update_digest_field(
            &mut hasher,
            "lease_generation",
            &self.lease_generation.get().to_string(),
        );
        update_digest_field(
            &mut hasher,
            "attempt_id",
            &self.attempt_id.as_uuid().to_string(),
        );
        ActionDigest(hasher.finalize().into())
    }
}

/// Whether C-5 requires a canonical D-6 before dispatch.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[allow(dead_code)] // T-2 runtime policy wiring is incomplete.
pub(crate) enum ApprovalRequirement {
    NotRequired,
    Required,
}

fn update_digest_field(hasher: &mut Sha256, name: &str, value: &str) {
    hasher.update(name.as_bytes());
    hasher.update(b"=");
    hasher.update(value.len().to_string().as_bytes());
    hasher.update(b":");
    hasher.update(value.as_bytes());
    hasher.update(b"\0");
}

const fn effect_name(effect: super::tool::Effect) -> &'static str {
    match effect {
        super::tool::Effect::ReadData => "read_data",
        super::tool::Effect::ExternalWrite => "external_write",
        super::tool::Effect::FilesystemWrite => "filesystem_write",
        super::tool::Effect::ProcessExecute => "process_execute",
        super::tool::Effect::NetworkEgress => "network_egress",
        super::tool::Effect::CredentialUse => "credential_use",
        super::tool::Effect::Unknown => "unknown",
    }
}

/// A client decision for one requested D-6.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalDecision {
    /// Authorizes the exact bound D-7.
    Accepted,
    /// Declines the proposed action.
    Declined,
    /// Cancels the pending request without execution.
    Cancelled,
}

/// Canonical D-6 lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalStatus {
    /// No terminal decision has won.
    Requested,
    /// The exact attempt is authorized.
    Accepted,
    /// The approver declined the action.
    Declined,
    /// The caller or platform cancelled the request.
    Cancelled,
    /// The earlier of the Turn or five-minute deadline elapsed.
    Expired,
}

/// A rejected D-6 transition or authorization check.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ApprovalError {
    /// Another terminal decision already won.
    #[error("approval already resolved")]
    AlreadyResolved,
    /// The approval expired before this transition.
    #[error("approval expired")]
    Expired,
    /// The exact action does not match this approval.
    #[error("approval does not match execution attempt")]
    BindingMismatch,
    /// The request has not been accepted.
    #[error("approval is not accepted")]
    NotAccepted,
    /// The approver identity is empty.
    #[error("approver identity is invalid")]
    InvalidApprover,
    /// C-7 did not validate the principal for this approval identity and scope.
    #[error("approver is not authorized")]
    NotAuthorized,
    /// C-5 did not authorize this exact action for approval creation.
    #[error("policy did not authorize an approval request")]
    PolicyAuthorizationRequired,
    /// The policy-authorized action is not permitted to create a D-6.
    #[error("approval is not required for this action")]
    ApprovalNotRequired,
}

/// Canonical D-6 state with one conditional terminal transition.
#[derive(Debug)]
#[allow(dead_code)] // T-2 approval transport wiring is incomplete.
pub struct ApprovalRequest {
    approval_id: ApprovalId,
    binding: ExactActionBinding,
    requested_at_millis: u64,
    expires_at_millis: u64,
    status: ApprovalStatus,
    decision: Option<ApprovalDecision>,
    approver: Option<String>,
    version: u64,
}

impl ApprovalRequest {
    /// Creates a requested approval with the earlier applicable deadline.
    /// # Errors
    /// Returns [`ApprovalError::Expired`] if no positive approval window exists.
    pub fn new(
        binding: ExactActionBinding,
        requested_at_millis: u64,
        turn_deadline_millis: u64,
    ) -> Result<Self, ApprovalError> {
        match binding.approval_requirement() {
            Some(ApprovalRequirement::Required) => {}
            Some(ApprovalRequirement::NotRequired) => {
                return Err(ApprovalError::ApprovalNotRequired);
            }
            None => return Err(ApprovalError::PolicyAuthorizationRequired),
        }
        let five_minute_deadline = requested_at_millis.saturating_add(APPROVAL_MAX_AGE_MILLIS);
        let expires_at_millis = turn_deadline_millis.min(five_minute_deadline);
        if expires_at_millis <= requested_at_millis {
            return Err(ApprovalError::Expired);
        }
        Ok(Self {
            approval_id: ApprovalId::new(),
            binding,
            requested_at_millis,
            expires_at_millis,
            status: ApprovalStatus::Requested,
            decision: None,
            approver: None,
            version: 1,
        })
    }

    /// Returns the stable D-6 identity.
    #[must_use]
    pub const fn approval_id(&self) -> ApprovalId {
        self.approval_id
    }

    /// Returns the current canonical status.
    #[must_use]
    pub const fn status(&self) -> ApprovalStatus {
        self.status
    }

    /// Returns the current canonical record version.
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// Returns the canonical decision, or `None` while requested or expired.
    #[must_use]
    pub const fn decision(&self) -> Option<ApprovalDecision> {
        self.decision
    }

    /// Returns the earlier of the Turn deadline and five-minute age limit.
    #[must_use]
    pub const fn expires_at_millis(&self) -> u64 {
        self.expires_at_millis
    }

    /// Returns the durable D-6 creation time the expiry window was computed from.
    #[must_use]
    pub const fn requested_at_millis(&self) -> u64 {
        self.requested_at_millis
    }

    /// Applies one authenticated decision with idempotent identical replay.
    /// # Errors
    /// Returns a typed error for expiry, invalid identity, or conflicting decision.
    #[allow(dead_code)] // T-2 approval transport wiring is incomplete.
    pub(crate) fn apply_validated_decision(
        &mut self,
        decision: ApprovalDecision,
        approver: impl Into<String>,
        decided_at_millis: u64,
    ) -> Result<u64, ApprovalError> {
        let approver = approver.into();
        if approver.trim().is_empty() {
            return Err(ApprovalError::InvalidApprover);
        }
        if self.status != ApprovalStatus::Requested {
            return if self.decision == Some(decision) && self.approver.as_deref() == Some(&approver)
            {
                Ok(self.version)
            } else {
                Err(ApprovalError::AlreadyResolved)
            };
        }
        if decided_at_millis >= self.expires_at_millis {
            self.status = ApprovalStatus::Expired;
            self.version += 1;
            return Err(ApprovalError::Expired);
        }
        self.status = match decision {
            ApprovalDecision::Accepted => ApprovalStatus::Accepted,
            ApprovalDecision::Declined => ApprovalStatus::Declined,
            ApprovalDecision::Cancelled => ApprovalStatus::Cancelled,
        };
        self.decision = Some(decision);
        self.approver = Some(approver);
        self.version += 1;
        Ok(self.version)
    }

    /// Verifies that this accepted D-6 matches every exact binding field.
    ///
    /// # Errors
    ///
    /// Returns [`ApprovalError`] when the request is not accepted or differs.
    pub fn authorize(&self, binding: &ExactActionBinding) -> Result<(), ApprovalError> {
        if self.status != ApprovalStatus::Accepted {
            return Err(ApprovalError::NotAccepted);
        }
        if &self.binding != binding {
            return Err(ApprovalError::BindingMismatch);
        }
        Ok(())
    }

    /// Returns the tenant bound to this canonical approval.
    #[must_use]
    pub fn tenant_id(&self) -> &TenantId {
        &self.binding.tenant_id
    }

    /// Returns the exact D-7 binding this approval authorizes.
    #[must_use]
    pub const fn binding(&self) -> &ExactActionBinding {
        &self.binding
    }

    /// Returns the Thread bound to this canonical approval.
    #[must_use]
    pub const fn thread_id(&self) -> ThreadId {
        self.binding.thread_id
    }
}

/// Canonical D-7 lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionStatus {
    /// Policy has prepared but not dispatched the attempt.
    Prepared,
    /// Exactly one executor dispatch claim has won.
    Running,
    /// Executor result committed successfully.
    Succeeded,
    /// Execution ended with a typed failure.
    Failed,
    /// The action deadline elapsed.
    TimedOut,
    /// Policy, approval, owner, or caller stopped the attempt.
    Cancelled,
}

/// A rejected D-7 transition or Turn attempt allocation.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ExecutionError {
    /// This D-7 already left `prepared`, so it cannot dispatch again.
    #[error("execution attempt already dispatched")]
    AlreadyDispatched,
    /// The supplied D-6 does not authorize this exact D-7.
    #[error("approval does not match execution attempt")]
    ApprovalMismatch,
    /// The Turn already allocated all 16 attempt slots.
    #[error("attempt limit reached")]
    AttemptLimit,
    /// The binding belongs to a different Turn authority.
    #[error("execution attempt belongs to another turn")]
    TurnMismatch,
    /// The requested lifecycle transition is not legal from the current state.
    #[error("execution attempt transition is invalid")]
    InvalidTransition,
    /// The Turn authority already allocated this D-7 identity.
    #[error("execution attempt identity already allocated")]
    AttemptAlreadyAllocated,
    /// Another D-7 already owns this Turn's single running slot.
    #[error("another execution attempt is already running for this turn")]
    ConcurrentAttempt,
    /// No unforgeable C-5 policy result accompanies the requested binding.
    #[error("policy authorization is required before execution preparation")]
    PolicyAuthorizationRequired,
    /// The Turn was interrupted and cannot allocate more execution attempts.
    #[error("turn interruption prevents execution attempt allocation")]
    InterruptionRequested,
}

/// One canonical D-7 whose dispatch claim is single-winner.
#[derive(Debug)]
pub struct ExecutionAttempt {
    binding: ExactActionBinding,
    status: ExecutionStatus,
    started_at_millis: Option<u64>,
    authority_state: Arc<Mutex<TurnAuthorityState>>,
}

impl PartialEq for ExecutionAttempt {
    fn eq(&self, other: &Self) -> bool {
        self.binding == other.binding
            && self.status == other.status
            && self.started_at_millis == other.started_at_millis
            && Arc::ptr_eq(&self.authority_state, &other.authority_state)
    }
}

impl Eq for ExecutionAttempt {}

impl ExecutionAttempt {
    /// Allocates one Turn slot and prepares one exact execution attempt.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionError::AttemptLimit`] after the Turn's sixteenth slot.
    fn prepare(
        binding: ExactActionBinding,
        authority_state: Arc<Mutex<TurnAuthorityState>>,
    ) -> Self {
        Self {
            binding,
            status: ExecutionStatus::Prepared,
            started_at_millis: None,
            authority_state,
        }
    }

    /// Returns the current D-7 lifecycle state.
    #[must_use]
    pub const fn status(&self) -> ExecutionStatus {
        self.status
    }

    /// Returns the immutable exact-action binding owned by this D-7.
    #[must_use]
    pub const fn binding(&self) -> &ExactActionBinding {
        &self.binding
    }

    /// Returns the canonical dispatch time after this D-7 is running.
    #[must_use]
    pub(crate) const fn started_at_millis(&self) -> Option<u64> {
        self.started_at_millis
    }

    /// Claims the only permitted dispatch for this D-7.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionError`] when already dispatched or not exactly authorized.
    fn start(
        &mut self,
        approval: Option<&ApprovalRequest>,
        started_at_millis: u64,
    ) -> Result<(), ExecutionError> {
        if self.status != ExecutionStatus::Prepared {
            return Err(ExecutionError::AlreadyDispatched);
        }
        match (self.binding.approval_requirement(), approval) {
            (Some(ApprovalRequirement::NotRequired), None) => {}
            (Some(ApprovalRequirement::Required), Some(approval))
                if approval.authorize(&self.binding).is_ok() => {}
            _ => return Err(ExecutionError::ApprovalMismatch),
        }
        self.status = ExecutionStatus::Running;
        self.started_at_millis = Some(started_at_millis);
        Ok(())
    }

    /// Records a terminal selected by the guarded coordinator mirror only.
    fn finish(&mut self, status: ExecutionStatus) -> Result<(), ExecutionError> {
        let valid = matches!(
            (self.status, status),
            (ExecutionStatus::Prepared, ExecutionStatus::Cancelled)
                | (
                    ExecutionStatus::Running,
                    ExecutionStatus::Succeeded
                        | ExecutionStatus::Failed
                        | ExecutionStatus::TimedOut
                        | ExecutionStatus::Cancelled
                )
        );
        if !valid {
            return Err(ExecutionError::InvalidTransition);
        }
        self.status = status;
        Ok(())
    }
}

/// Turn-scoped allocation counter for initial and retry D-7 identities.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct AttemptBudget {
    used: u8,
}

impl AttemptBudget {
    /// Creates an unused 16-slot Turn budget.
    #[must_use]
    const fn new() -> Self {
        Self { used: 0 }
    }

    /// Allocates one slot for an initial execution or retry.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionError::AttemptLimit`] after slot 16.
    fn allocate(&mut self) -> Result<u8, ExecutionError> {
        if self.used == MAX_ATTEMPTS_PER_TURN {
            return Err(ExecutionError::AttemptLimit);
        }
        self.used += 1;
        Ok(self.used)
    }

    /// Returns the number of allocated initial and retry attempts.
    #[must_use]
    const fn used(self) -> u8 {
        self.used
    }
}

#[derive(Debug)]
struct TurnAuthorityState {
    key: TurnAuthorityKey,
    profile_id: String,
    profile_version: String,
    budget: AttemptBudget,
    attempts: BTreeMap<AttemptId, (ExactActionBinding, ExecutionStatus, Option<u64>, bool)>,
    interruption_requested: bool,
}

fn recover_lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Process-local handle to one Turn's allocation and dispatch authority.
///
/// Only the lease-validating application preparer may create or duplicate handles.
#[derive(Debug)]
pub struct TurnExecutionAuthority {
    state: Arc<Mutex<TurnAuthorityState>>,
}

impl TurnExecutionAuthority {
    /// Creates another non-public handle sharing this exact Turn authority state.
    pub(crate) fn new_handle(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
        }
    }

    /// Allocates and prepares one D-7 owned by this exact Turn/profile authority.
    ///
    /// This is the sole D-7 allocation entry point. It carries a unique name so
    /// architecture tests can enforce that only the lease-validating application
    /// preparer calls it, keeping every allocation behind the TC-07
    /// current-generation lease check.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionError::TurnMismatch`] for another Turn/profile and
    /// [`ExecutionError::AttemptLimit`] after the sixteenth allocation.
    pub(crate) fn allocate_attempt(
        &mut self,
        binding: ExactActionBinding,
    ) -> Result<ExecutionAttempt, ExecutionError> {
        if binding.approval_requirement().is_none() {
            return Err(ExecutionError::PolicyAuthorizationRequired);
        }
        let mut state = recover_lock(&self.state);
        if binding.tenant_id != state.key.tenant
            || binding.thread_id != state.key.thread
            || binding.turn_id != state.key.turn
            || binding.profile_id != state.profile_id
            || binding.profile_version != state.profile_version
        {
            return Err(ExecutionError::TurnMismatch);
        }
        if state.interruption_requested {
            return Err(ExecutionError::InterruptionRequested);
        }
        if state.attempts.contains_key(&binding.attempt_id) {
            return Err(ExecutionError::AttemptAlreadyAllocated);
        }
        state.budget.allocate()?;
        let attempt_id = binding.attempt_id;
        state.attempts.insert(
            attempt_id,
            (binding.clone(), ExecutionStatus::Prepared, None, false),
        );
        drop(state);
        Ok(ExecutionAttempt::prepare(binding, Arc::clone(&self.state)))
    }

    /// Claims this Turn's only running slot for one allocated exact D-7.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionError`] for a stale/foreign attempt, mismatched approval,
    /// duplicate dispatch claim, or a second concurrent attempt.
    pub(crate) fn claim_dispatch(
        &mut self,
        attempt: &mut ExecutionAttempt,
        approval: Option<&ApprovalRequest>,
        started_at_millis: u64,
    ) -> Result<(), ExecutionError> {
        if !Arc::ptr_eq(&self.state, &attempt.authority_state) {
            return Err(ExecutionError::TurnMismatch);
        }
        let mut state = recover_lock(&self.state);
        let attempt_id = attempt.binding().attempt_id;
        if state.interruption_requested {
            return Err(ExecutionError::InterruptionRequested);
        }
        let Some((allocated_binding, allocated_status, _, terminal_commit_in_flight)) =
            state.attempts.get(&attempt_id)
        else {
            return Err(ExecutionError::TurnMismatch);
        };
        if allocated_binding != attempt.binding() {
            return Err(ExecutionError::TurnMismatch);
        }
        if *allocated_status != ExecutionStatus::Prepared
            || attempt.status() != ExecutionStatus::Prepared
            || *terminal_commit_in_flight
        {
            return Err(ExecutionError::AlreadyDispatched);
        }
        if state
            .attempts
            .values()
            .any(|(_, status, _, _)| *status == ExecutionStatus::Running)
        {
            return Err(ExecutionError::ConcurrentAttempt);
        }
        attempt.start(approval, started_at_millis)?;
        let Some((_, allocated_status, allocated_started_at, _)) =
            state.attempts.get_mut(&attempt_id)
        else {
            return Err(ExecutionError::InvalidTransition);
        };
        *allocated_status = ExecutionStatus::Running;
        *allocated_started_at = attempt.started_at_millis;
        Ok(())
    }

    /// Returns the number of allocated initial and retry D-7 identities.
    #[must_use]
    pub fn used(&self) -> u8 {
        recover_lock(&self.state).budget.used()
    }
}
