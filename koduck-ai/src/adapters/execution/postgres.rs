// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! `SQLx`-backed canonical D-6 approval-record persistence.

use std::future::Future;
use std::time::Duration;

use sqlx::{PgPool, Row};
use tokio::runtime::Handle;

use crate::application::{
    AppendPolicy, ApprovalDecisionResolution, ApprovalInsertResolution, ApprovalRecordStore,
    ApprovalStoreError, ExecutionFailure, ExecutionPending, PendingApprovalCancellation,
    PendingApprovalCanceller,
};
use crate::domain::TenantId;
use crate::domain::execution::{
    ApprovalDecision, ApprovalId, ApprovalRequest, ApprovalStatus, ApproverId,
};

/// Production D-6 store using one `SQLx` pool and its owning Tokio runtime.
///
/// Every operation is one conditional durable write or read on the canonical
/// `tool_approvals` table, so competing decisions converge on a single
/// committed terminal (ADR-0003 TC-12): exactly one conditional
/// `requested -> terminal` update wins per approval identity and every loser
/// reads the already-committed canonical row.
#[derive(Clone)]
pub struct SqlxApprovalRecordStore {
    pool: PgPool,
    runtime: Handle,
}

impl SqlxApprovalRecordStore {
    /// Creates a store whose synchronous port calls drive `SQLx` on `runtime`.
    #[must_use]
    pub const fn new(pool: PgPool, runtime: Handle) -> Self {
        Self { pool, runtime }
    }

    fn wait<T>(
        &self,
        operation: impl Future<Output = Result<T, ApprovalStoreError>>,
    ) -> Result<T, ApprovalStoreError> {
        let deadline: Duration = AppendPolicy::cand_1().deadline();
        self.runtime.block_on(async {
            tokio::time::timeout(deadline, operation)
                .await
                .map_err(|_| ApprovalStoreError::Unavailable)?
        })
    }
}

impl ApprovalRecordStore for SqlxApprovalRecordStore {
    fn insert_requested(
        &mut self,
        request: &ApprovalRequest,
        requester_subject: &str,
    ) -> Result<ApprovalInsertResolution, ApprovalStoreError> {
        let binding = request.binding();
        let action = binding.action();
        self.wait(async {
            // ON CONFLICT DO NOTHING keeps a lost-acknowledgement replay from
            // becoming an error: the conflict branch below then verifies the
            // immutable fields against the committed canonical row, so the
            // caller reconciles the record instead of retrying blind.
            let outcome = sqlx::query(
                "INSERT INTO tool_approvals (
                    tenant_id, approval_id, requester_subject, thread_id, turn_id,
                    attempt_id, lease_generation, descriptor_id, descriptor_version,
                    effect, action_digest, profile_id, profile_version,
                    requested_at_millis, expires_at_millis,
                    status, version
                ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, 'requested', 1
                )
                ON CONFLICT (tenant_id, approval_id) DO NOTHING",
            )
            .bind(binding.tenant_id().as_str())
            .bind(request.approval_id().as_uuid())
            .bind(requester_subject)
            .bind(binding.thread_id().as_uuid())
            .bind(binding.turn_id().as_uuid())
            .bind(binding.attempt_id().as_uuid())
            .bind(millis(binding.lease_generation().get())?)
            .bind(action.descriptor_id())
            .bind(action.descriptor_version())
            .bind(effect_code(action.effect()))
            .bind(hex_digest(binding.action_digest().as_bytes()))
            .bind(binding.profile_id())
            .bind(binding.profile_version())
            .bind(millis(request.requested_at_millis())?)
            .bind(millis(request.expires_at_millis())?)
            .execute(&self.pool)
            .await
            .map_err(|_| ApprovalStoreError::Unavailable)?;
            if outcome.rows_affected() == 1 {
                return Ok(ApprovalInsertResolution::Inserted);
            }
            let existing = sqlx::query(
                "SELECT requester_subject, thread_id, turn_id, attempt_id,
                        lease_generation, descriptor_id, descriptor_version, effect,
                        action_digest, profile_id, profile_version,
                        requested_at_millis, expires_at_millis,
                        status, decision, version
                 FROM tool_approvals
                 WHERE tenant_id = $1 AND approval_id = $2",
            )
            .bind(binding.tenant_id().as_str())
            .bind(request.approval_id().as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| ApprovalStoreError::Unavailable)?;
            let Some(row) = existing else {
                // DO NOTHING reported a conflict the same transaction cannot
                // observe again; treat undecidable durable state as unavailable.
                return Err(ApprovalStoreError::Unavailable);
            };
            Self::conflict_resolution(&row, request, requester_subject)
        })
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "ownership dimensions are individually conditional lookup keys"
    )]
    fn resolve_decision(
        &mut self,
        approval_id: ApprovalId,
        tenant_id: &TenantId,
        thread_id: crate::domain::ThreadId,
        requester_subject: &str,
        decision: ApprovalDecision,
        approver: &ApproverId,
        decided_at_millis: u64,
    ) -> Result<ApprovalDecisionResolution, ApprovalStoreError> {
        self.wait(async {
            // The winning decision transition and its correlated audit append
            // commit atomically: RETURNING carries the persisted approval
            // correlation columns the bounded audit record needs, so the
            // production decision route cannot resolve a D-6 without its
            // TC-14 evidence.
            let mut transaction = self
                .pool
                .begin()
                .await
                .map_err(|_| ApprovalStoreError::Unavailable)?;
            // Decision and interruption take the same canonical Turn lock
            // before either transition. The probe is unconditional on the
            // owner state so a currently dispatchable Turn is still locked;
            // after any wait, the guarded update observes the winning barrier.
            let _ = sqlx::query(
                "SELECT owner.turn_id
                 FROM turns owner
                 JOIN tool_approvals approval
                   ON approval.tenant_id = owner.tenant_id
                  AND approval.thread_id = owner.thread_id
                  AND approval.turn_id = owner.turn_id
                 WHERE approval.tenant_id = $1 AND approval.approval_id = $2
                   AND approval.requester_subject = $3 AND approval.thread_id = $4
                 FOR UPDATE OF owner",
            )
            .bind(tenant_id.as_str())
            .bind(approval_id.as_uuid())
            .bind(requester_subject)
            .bind(thread_id.as_uuid())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|_| ApprovalStoreError::Unavailable)?;
            let winner = sqlx::query(
                "UPDATE tool_approvals
                 SET status = $3, decision = $3, approver = $4,
                     decided_at_millis = $5, version = version + 1
                 WHERE tenant_id = $1 AND approval_id = $2
                   AND requester_subject = $6 AND thread_id = $7
                   AND status = 'requested' AND expires_at_millis > $5
                   AND NOT EXISTS (
                       SELECT 1 FROM turns owner
                       WHERE owner.tenant_id = $1 AND owner.thread_id = $7
                         AND owner.turn_id = tool_approvals.turn_id
                         AND (owner.status <> 'started' OR owner.interrupting)
                   )
                 RETURNING version, thread_id, turn_id, attempt_id, \
                            lease_generation, descriptor_id, descriptor_version, \
                            action_digest, profile_id, profile_version",
            )
            .bind(tenant_id.as_str())
            .bind(approval_id.as_uuid())
            .bind(decision_code(decision))
            .bind(approver.as_str())
            .bind(millis(decided_at_millis)?)
            .bind(requester_subject)
            .bind(thread_id.as_uuid())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|_| ApprovalStoreError::Unavailable)?;
            if let Some(row) = winner {
                let version = row_version(&row)?;
                emit_decision_audit(
                    &mut transaction,
                    approval_id,
                    crate::domain::execution::ApprovalStatus::from_decision(decision),
                    Some(decision),
                    version,
                    decided_at_millis,
                    tenant_id,
                    &row,
                )
                .await?;
                // A failed COMMIT acknowledgement is ambiguous: PostgreSQL
                // may have durably committed the decision and audit before
                // the client lost the acknowledgement. Reconcile that
                // canonical terminal rather than reporting a spurious 503.
                if transaction.commit().await.is_err() {
                    return self
                        .reread_terminal(approval_id, tenant_id, requester_subject, thread_id)
                        .await;
                }
                return Ok(ApprovalDecisionResolution::Won { decision, version });
            }
            // `classify_decision_loss` uses the pool for its canonical read
            // or expiry transition. Releasing the losing transaction first
            // prevents a saturated pool from waiting on its own connection.
            drop(transaction);
            self.classify_decision_loss(
                &self.pool,
                tenant_id,
                requester_subject,
                thread_id,
                approval_id,
                decided_at_millis,
            )
            .await
        })
    }
}

impl PendingApprovalCanceller for SqlxApprovalRecordStore {
    fn cancel_requested(
        &mut self,
        binding: &crate::domain::execution::ExactActionBinding,
    ) -> Result<PendingApprovalCancellation, ExecutionPending> {
        self.wait(async {
            let mut transaction = self
                .pool
                .begin()
                .await
                .map_err(|_| ApprovalStoreError::Unavailable)?;
            let row = sqlx::query(
                "UPDATE tool_approvals approval
                 SET status = 'cancelled', version = approval.version + 1
                 FROM turns owner
                 WHERE approval.tenant_id = $1 AND approval.thread_id = $2
                   AND approval.turn_id = $3 AND approval.attempt_id = $4
                   AND approval.lease_generation = $5
                   AND approval.descriptor_id = $6 AND approval.descriptor_version = $7
                   AND approval.effect = $8 AND approval.action_digest = $9
                   AND approval.profile_id = $10 AND approval.profile_version = $11
                   AND approval.status = 'requested'
                   AND owner.tenant_id = approval.tenant_id
                   AND owner.thread_id = approval.thread_id
                   AND owner.turn_id = approval.turn_id AND owner.interrupting
                 RETURNING approval.approval_id, approval.thread_id, approval.turn_id,
                           approval.attempt_id, approval.lease_generation,
                           approval.descriptor_id, approval.descriptor_version,
                           approval.action_digest, approval.profile_id,
                           approval.profile_version, approval.version",
            )
            .bind(binding.tenant_id().as_str())
            .bind(binding.thread_id().as_uuid())
            .bind(binding.turn_id().as_uuid())
            .bind(binding.attempt_id().as_uuid())
            .bind(millis(binding.lease_generation().get())?)
            .bind(binding.action().descriptor_id())
            .bind(binding.action().descriptor_version())
            .bind(effect_code(binding.action().effect()))
            .bind(hex_digest(binding.action_digest().as_bytes()))
            .bind(binding.profile_id())
            .bind(binding.profile_version())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|_| ApprovalStoreError::Unavailable)?;
            let Some(row) = row else {
                drop(transaction);
                let status: Option<String> = sqlx::query_scalar(
                    "SELECT status FROM tool_approvals
                     WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3
                       AND attempt_id = $4 AND lease_generation = $5
                       AND descriptor_id = $6 AND descriptor_version = $7
                       AND effect = $8 AND action_digest = $9
                       AND profile_id = $10 AND profile_version = $11",
                )
                .bind(binding.tenant_id().as_str())
                .bind(binding.thread_id().as_uuid())
                .bind(binding.turn_id().as_uuid())
                .bind(binding.attempt_id().as_uuid())
                .bind(millis(binding.lease_generation().get())?)
                .bind(binding.action().descriptor_id())
                .bind(binding.action().descriptor_version())
                .bind(effect_code(binding.action().effect()))
                .bind(hex_digest(binding.action_digest().as_bytes()))
                .bind(binding.profile_id())
                .bind(binding.profile_version())
                .fetch_optional(&self.pool)
                .await
                .map_err(|_| ApprovalStoreError::Unavailable)?;
                return status
                    .filter(|status| status != "requested")
                    .map(|_| PendingApprovalCancellation::AlreadyResolved)
                    .ok_or(ApprovalStoreError::Unavailable);
            };
            let approval_id = ApprovalId::from_uuid(
                row.try_get("approval_id")
                    .map_err(|_| ApprovalStoreError::Unavailable)?,
            );
            let version = row_version(&row)?;
            emit_decision_audit(
                &mut transaction,
                approval_id,
                ApprovalStatus::Cancelled,
                None,
                version,
                crate::adapters::history::postgres::unix_time_ms(),
                binding.tenant_id(),
                &row,
            )
            .await?;
            transaction
                .commit()
                .await
                .map_err(|_| ApprovalStoreError::Unavailable)?;
            Ok(PendingApprovalCancellation::Cancelled)
        })
        .map_err(|_| ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::DurabilityUnavailable,
            effect_state: crate::application::EffectState::Unknown,
        })
    }
}

impl SqlxApprovalRecordStore {
    /// Classifies a lost decision transition: re-reads the canonical record,
    /// transitions a genuinely expired `requested` row to its terminal, and
    /// returns the existing terminal otherwise (ADR-0003 TC-12).
    async fn classify_decision_loss(
        &self,
        pool: &sqlx::PgPool,
        tenant_id: &TenantId,
        requester_subject: &str,
        thread_id: crate::domain::ThreadId,
        approval_id: ApprovalId,
        decided_at_millis: u64,
    ) -> Result<ApprovalDecisionResolution, ApprovalStoreError> {
        let existing = sqlx::query(
            "SELECT status, decision, version FROM tool_approvals
         WHERE tenant_id = $1 AND approval_id = $2
           AND requester_subject = $3 AND thread_id = $4",
        )
        .bind(tenant_id.as_str())
        .bind(approval_id.as_uuid())
        .bind(requester_subject)
        .bind(thread_id.as_uuid())
        .fetch_optional(pool)
        .await
        .map_err(|_| ApprovalStoreError::Unavailable)?;
        let Some(row) = existing else {
            return Ok(ApprovalDecisionResolution::NotFound);
        };
        let status_text: String = row
            .try_get("status")
            .map_err(|_| ApprovalStoreError::Unavailable)?;
        if status_text == "requested" {
            // Only a genuinely expired record transitions here. An active
            // authenticated interruption owns the requested D-6 terminal, so
            // recovery must cancel it rather than allowing expiry to win;
            // ordinary terminal Turns retain their existing expiry behavior
            // (ADR-0003 TC-10/TC-12).
            let mut expiry_transaction = pool
                .begin()
                .await
                .map_err(|_| ApprovalStoreError::Unavailable)?;
            let expired = sqlx::query(
                "UPDATE tool_approvals
             SET status = 'expired', version = version + 1
             WHERE tenant_id = $1 AND approval_id = $2
               AND requester_subject = $3 AND thread_id = $4
               AND status = 'requested' AND expires_at_millis <= $5
               AND NOT EXISTS (
                   SELECT 1 FROM turns owner
                   WHERE owner.tenant_id = $1 AND owner.thread_id = $4
                     AND owner.turn_id = tool_approvals.turn_id
                     AND owner.interrupting
               )
             RETURNING version, thread_id, turn_id, attempt_id, \
                        lease_generation, descriptor_id, descriptor_version, \
                        action_digest, profile_id, profile_version",
            )
            .bind(tenant_id.as_str())
            .bind(approval_id.as_uuid())
            .bind(requester_subject)
            .bind(thread_id.as_uuid())
            .bind(millis(decided_at_millis)?)
            .fetch_optional(&mut *expiry_transaction)
            .await
            .map_err(|_| ApprovalStoreError::Unavailable)?;
            if let Some(expired_row) = &expired {
                // Every expiry terminal — including this loser-side
                // transition — appends its correlated audit record
                // atomically with D-6 (ADR-0003 TC-14).
                let version = row_version(expired_row)?;
                emit_decision_audit(
                    &mut expiry_transaction,
                    approval_id,
                    ApprovalStatus::Expired,
                    None,
                    version,
                    decided_at_millis,
                    tenant_id,
                    expired_row,
                )
                .await?;
                if expiry_transaction.commit().await.is_err() {
                    return self
                        .reread_terminal(approval_id, tenant_id, requester_subject, thread_id)
                        .await;
                }
                return Ok(ApprovalDecisionResolution::ExistingTerminal {
                    decision: None,
                    status: ApprovalStatus::Expired,
                    version,
                });
            }
            drop(expiry_transaction);
            // Another contender won a transition between the read and this
            // write, or the owning Turn guard rejected expiry.
            if self
                .interruption_owns_approval(
                    pool,
                    tenant_id,
                    requester_subject,
                    thread_id,
                    approval_id,
                )
                .await?
            {
                return Ok(ApprovalDecisionResolution::TurnGuardRejected);
            }
            return self
                .reread_terminal(approval_id, tenant_id, requester_subject, thread_id)
                .await;
        }
        Ok(ApprovalDecisionResolution::ExistingTerminal {
            decision: row
                .try_get::<Option<String>, _>("decision")
                .map_err(|_| ApprovalStoreError::Unavailable)?
                .as_deref()
                .and_then(decision_from_code),
            status: status_from_code(&status_text).ok_or(ApprovalStoreError::Unavailable)?,
            version: row_version(&row)?,
        })
    }

    /// Reports whether an authenticated interruption owns the requested D-6
    /// terminal without changing the still-requested approval.
    async fn interruption_owns_approval(
        &self,
        pool: &sqlx::PgPool,
        tenant_id: &TenantId,
        requester_subject: &str,
        thread_id: crate::domain::ThreadId,
        approval_id: ApprovalId,
    ) -> Result<bool, ApprovalStoreError> {
        let owner = sqlx::query(
            "SELECT approval.status, owner.interrupting
             FROM tool_approvals approval
             JOIN turns owner
               ON owner.tenant_id = approval.tenant_id
              AND owner.thread_id = approval.thread_id
              AND owner.turn_id = approval.turn_id
             WHERE approval.tenant_id = $1 AND approval.approval_id = $2
               AND approval.requester_subject = $3 AND approval.thread_id = $4",
        )
        .bind(tenant_id.as_str())
        .bind(approval_id.as_uuid())
        .bind(requester_subject)
        .bind(thread_id.as_uuid())
        .fetch_optional(pool)
        .await
        .map_err(|_| ApprovalStoreError::Unavailable)?;
        let Some(owner) = owner else {
            return Ok(false);
        };
        let status: String = owner
            .try_get("status")
            .map_err(|_| ApprovalStoreError::Unavailable)?;
        let interrupting: bool = owner
            .try_get("interrupting")
            .map_err(|_| ApprovalStoreError::Unavailable)?;
        Ok(interruption_owns_requested_approval(&status, interrupting))
    }

    /// Classifies one conflicting canonical row against the replayed record.
    ///
    /// Matching immutable fields yield the row's current canonical projection
    /// so the replaying caller can reconcile unambiguously; any drift is the
    /// typed identity conflict.
    fn conflict_resolution(
        row: &sqlx::postgres::PgRow,
        request: &ApprovalRequest,
        requester_subject: &str,
    ) -> Result<ApprovalInsertResolution, ApprovalStoreError> {
        let binding = request.binding();
        let action = binding.action();
        // Decode numeric canonical values fail-closed before comparing, so a
        // drifted negative durable value surfaces as `Unavailable` instead of
        // comparing equal to its expected positive counterpart.
        let lease_generation = canonical_non_negative(row, "lease_generation")?;
        let requested_at_millis = canonical_non_negative(row, "requested_at_millis")?;
        let expires_at_millis = canonical_non_negative(row, "expires_at_millis")?;
        let matches = row
            .try_get::<String, _>("requester_subject")
            .is_ok_and(|value| value == requester_subject)
            && row
                .try_get::<uuid::Uuid, _>("thread_id")
                .is_ok_and(|value| value == binding.thread_id().as_uuid())
            && row
                .try_get::<uuid::Uuid, _>("turn_id")
                .is_ok_and(|value| value == binding.turn_id().as_uuid())
            && row
                .try_get::<uuid::Uuid, _>("attempt_id")
                .is_ok_and(|value| value == binding.attempt_id().as_uuid())
            && lease_generation == binding.lease_generation().get()
            && row
                .try_get::<String, _>("descriptor_id")
                .is_ok_and(|value| value == action.descriptor_id())
            && row
                .try_get::<String, _>("descriptor_version")
                .is_ok_and(|value| value == action.descriptor_version())
            && row
                .try_get::<String, _>("effect")
                .is_ok_and(|value| value == effect_code(action.effect()))
            && row
                .try_get::<String, _>("action_digest")
                .is_ok_and(|value| value == hex_digest(binding.action_digest().as_bytes()))
            && row
                .try_get::<String, _>("profile_id")
                .is_ok_and(|value| value == binding.profile_id())
            && row
                .try_get::<String, _>("profile_version")
                .is_ok_and(|value| value == binding.profile_version())
            && requested_at_millis == request.requested_at_millis()
            && expires_at_millis == request.expires_at_millis();
        if !matches {
            return Err(ApprovalStoreError::IdentityConflict);
        }
        let status_text: String = row
            .try_get("status")
            .map_err(|_| ApprovalStoreError::Unavailable)?;
        Ok(ApprovalInsertResolution::Existing {
            status: status_from_code(&status_text).ok_or(ApprovalStoreError::Unavailable)?,
            decision: row
                .try_get::<Option<String>, _>("decision")
                .map_err(|_| ApprovalStoreError::Unavailable)?
                .as_deref()
                .and_then(decision_from_code),
            version: row_version(row)?,
        })
    }

    async fn reread_terminal(
        &self,
        approval_id: ApprovalId,
        tenant_id: &TenantId,
        requester_subject: &str,
        thread_id: crate::domain::ThreadId,
    ) -> Result<ApprovalDecisionResolution, ApprovalStoreError> {
        let row = sqlx::query(
            "SELECT status, decision, version FROM tool_approvals
             WHERE tenant_id = $1 AND approval_id = $2
               AND requester_subject = $3 AND thread_id = $4",
        )
        .bind(tenant_id.as_str())
        .bind(approval_id.as_uuid())
        .bind(requester_subject)
        .bind(thread_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| ApprovalStoreError::Unavailable)?;
        let Some(row) = row else {
            return Ok(ApprovalDecisionResolution::NotFound);
        };
        let status_text: String = row
            .try_get("status")
            .map_err(|_| ApprovalStoreError::Unavailable)?;
        if status_text == "requested" {
            // The record became requested again, which no legal transition
            // permits; treat undecidable durable state as unavailable.
            return Err(ApprovalStoreError::Unavailable);
        }
        Ok(ApprovalDecisionResolution::ExistingTerminal {
            decision: row
                .try_get::<Option<String>, _>("decision")
                .map_err(|_| ApprovalStoreError::Unavailable)?
                .as_deref()
                .and_then(decision_from_code),
            status: status_from_code(&status_text).ok_or(ApprovalStoreError::Unavailable)?,
            version: row_version(&row)?,
        })
    }
}

fn canonical_non_negative(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Result<u64, ApprovalStoreError> {
    let value = row
        .try_get::<i64, _>(column)
        .map_err(|_| ApprovalStoreError::Unavailable)?;
    u64::try_from(value).map_err(|_| ApprovalStoreError::Unavailable)
}

fn row_version(row: &sqlx::postgres::PgRow) -> Result<u64, ApprovalStoreError> {
    let version = row
        .try_get::<i64, _>("version")
        .map_err(|_| ApprovalStoreError::Unavailable)?;
    u64::try_from(version)
        .ok()
        .filter(|version| *version >= 1)
        .ok_or(ApprovalStoreError::Unavailable)
}

/// Converts one non-negative millisecond timestamp to its durable binding.
///
/// # Errors
///
/// Returns [`ApprovalStoreError::Unavailable`] when the timestamp exceeds the
/// durable column domain.
fn millis(value: u64) -> Result<i64, ApprovalStoreError> {
    i64::try_from(value).map_err(|_| ApprovalStoreError::Unavailable)
}

fn decision_code(decision: ApprovalDecision) -> &'static str {
    match decision {
        ApprovalDecision::Accepted => "accepted",
        ApprovalDecision::Declined => "declined",
        ApprovalDecision::Cancelled => "cancelled",
    }
}

fn decision_from_code(code: &str) -> Option<ApprovalDecision> {
    match code {
        "accepted" => Some(ApprovalDecision::Accepted),
        "declined" => Some(ApprovalDecision::Declined),
        "cancelled" => Some(ApprovalDecision::Cancelled),
        _ => None,
    }
}

fn status_from_code(code: &str) -> Option<ApprovalStatus> {
    match code {
        "requested" => Some(ApprovalStatus::Requested),
        "accepted" => Some(ApprovalStatus::Accepted),
        "declined" => Some(ApprovalStatus::Declined),
        "cancelled" => Some(ApprovalStatus::Cancelled),
        "expired" => Some(ApprovalStatus::Expired),
        _ => None,
    }
}

fn effect_code(effect: crate::domain::tool::Effect) -> &'static str {
    match effect {
        crate::domain::tool::Effect::ReadData => "read_data",
        crate::domain::tool::Effect::ExternalWrite => "external_write",
        crate::domain::tool::Effect::FilesystemWrite => "filesystem_write",
        crate::domain::tool::Effect::ProcessExecute => "process_execute",
        crate::domain::tool::Effect::NetworkEgress => "network_egress",
        crate::domain::tool::Effect::CredentialUse => "credential_use",
        crate::domain::tool::Effect::Unknown => "unknown",
    }
}

fn hex_digest(bytes: &[u8; 32]) -> String {
    let mut text = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Restricts interruption ownership to the only mutable D-6 state. A resolved
/// terminal must be re-read so an identical decision replay remains idempotent.
fn interruption_owns_requested_approval(status: &str, interrupting: bool) -> bool {
    status == "requested" && interrupting
}

/// Appends the bounded correlated audit record for one won D-6 decision
/// inside its resolving transaction (ADR-0003 TC-14).
#[allow(
    clippy::too_many_arguments,
    reason = "each parameter is one persisted correlation field or committed terminal dimension"
)]
async fn emit_decision_audit(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    approval_id: ApprovalId,
    status: ApprovalStatus,
    decision: Option<ApprovalDecision>,
    version: u64,
    decided_at_millis: u64,
    tenant_id: &TenantId,
    row: &sqlx::postgres::PgRow,
) -> Result<(), ApprovalStoreError> {
    use sqlx::Row as _;
    let thread: uuid::Uuid = row
        .try_get("thread_id")
        .map_err(|_| ApprovalStoreError::Unavailable)?;
    let turn: uuid::Uuid = row
        .try_get("turn_id")
        .map_err(|_| ApprovalStoreError::Unavailable)?;
    let attempt: uuid::Uuid = row
        .try_get("attempt_id")
        .map_err(|_| ApprovalStoreError::Unavailable)?;
    let descriptor_id: String = row
        .try_get("descriptor_id")
        .map_err(|_| ApprovalStoreError::Unavailable)?;
    let descriptor_version: String = row
        .try_get("descriptor_version")
        .map_err(|_| ApprovalStoreError::Unavailable)?;
    let action_digest: String = row
        .try_get("action_digest")
        .map_err(|_| ApprovalStoreError::Unavailable)?;
    let profile_id: String = row
        .try_get("profile_id")
        .map_err(|_| ApprovalStoreError::Unavailable)?;
    let profile_version: String = row
        .try_get("profile_version")
        .map_err(|_| ApprovalStoreError::Unavailable)?;
    let lease_generation: i64 = row
        .try_get("lease_generation")
        .map_err(|_| ApprovalStoreError::Unavailable)?;
    let record = crate::application::ToolAuditRecord::approval_resolution_from_persisted(
        tenant_id,
        crate::domain::ThreadId::from_uuid(thread),
        crate::domain::TurnId::from_uuid(turn),
        &crate::domain::execution::AttemptId::from_uuid(attempt),
        approval_id,
        &descriptor_id,
        &descriptor_version,
        &profile_id,
        &profile_version,
        &action_digest,
        u64::try_from(lease_generation).map_err(|_| ApprovalStoreError::Unavailable)?,
        status,
        decision,
        version,
        decided_at_millis,
    );
    let serialized = crate::adapters::audit::serialize_audit_record(&record)
        .map_err(|_| ApprovalStoreError::Unavailable)?;
    sqlx::query(
        "INSERT INTO tool_audit_records \
         (tenant_id, thread_id, turn_id, at_millis, record) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(record.tenant_id())
    .bind(thread)
    .bind(turn)
    .bind(millis(decided_at_millis)?)
    .bind(serialized)
    .execute(&mut **transaction)
    .await
    .map_err(|_| ApprovalStoreError::Unavailable)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::interruption_owns_requested_approval;

    #[test]
    fn interruption_guard_only_owns_a_still_requested_approval() {
        assert!(interruption_owns_requested_approval("requested", true));
        assert!(!interruption_owns_requested_approval("accepted", true));
        assert!(!interruption_owns_requested_approval("expired", true));
        assert!(!interruption_owns_requested_approval("requested", false));
    }
}
