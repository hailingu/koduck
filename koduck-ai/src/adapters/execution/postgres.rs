// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! `SQLx`-backed canonical D-6 approval-record persistence.

use std::future::Future;
use std::time::Duration;

use sqlx::{PgPool, Row};
use tokio::runtime::Handle;

use crate::application::{
    AppendPolicy, ApprovalDecisionResolution, ApprovalInsertResolution, ApprovalRecordStore,
    ApprovalStoreError,
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
    #[allow(
        clippy::too_many_lines,
        reason = "the winning branch additionally appends the atomic correlated audit record"
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
            // Lock the owning Turn row unconditionally before deciding, so an
            // interruption transaction cannot commit its barrier between this
            // transaction's snapshot and the decision write: the lock
            // serializes the two orderings canonically (ADR-0003 TC-12).
            let owner = sqlx::query(
                "SELECT owner.status, owner.interrupting FROM turns owner \
                 WHERE owner.tenant_id = $1 AND owner.thread_id = $2 \
                   AND owner.turn_id = (SELECT turn_id FROM tool_approvals \
                                        WHERE tenant_id = $1 AND approval_id = $3 \
                                          AND requester_subject = $4 AND thread_id = $2) \
                 FOR UPDATE",
            )
            .bind(tenant_id.as_str())
            .bind(thread_id.as_uuid())
            .bind(approval_id.as_uuid())
            .bind(requester_subject)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|_| ApprovalStoreError::Unavailable)?;
            if let Some(owner_row) = &owner {
                let owner_status: String = owner_row
                    .try_get("status")
                    .map_err(|_| ApprovalStoreError::Unavailable)?;
                let interrupting: bool = owner_row
                    .try_get("interrupting")
                    .map_err(|_| ApprovalStoreError::Unavailable)?;
                if owner_status != "started" || interrupting {
                    // The owning Turn is terminal or interrupted: commit no
                    // decision. An in-window record stays `requested` for the
                    // Turn's own reconciliation; a record whose deadline
                    // already passed commits its audited expiry terminal here
                    // so no pending approval can stay permanently
                    // `requested` (ADR-0003 D-6 state machine, TC-14).
                    let deadline_row = sqlx::query(
                        "SELECT requested_at_millis, expires_at_millis FROM tool_approvals \
                         WHERE tenant_id = $1 AND approval_id = $2 \
                           AND requester_subject = $3 AND thread_id = $4 \
                           AND status = 'requested'",
                    )
                    .bind(tenant_id.as_str())
                    .bind(approval_id.as_uuid())
                    .bind(requester_subject)
                    .bind(thread_id.as_uuid())
                    .fetch_optional(&mut *transaction)
                    .await
                    .map_err(|_| ApprovalStoreError::Unavailable)?;
                    if let Some(record) = deadline_row {
                        let expires_at: i64 = record
                            .try_get("expires_at_millis")
                            .map_err(|_| ApprovalStoreError::Unavailable)?;
                        if i64::try_from(decided_at_millis)
                            .map_or(true, |decided| decided >= expires_at)
                        {
                            let expired = sqlx::query(
                                "UPDATE tool_approvals \
                                 SET status = 'expired', version = version + 1 \
                                 WHERE tenant_id = $1 AND approval_id = $2 \
                                   AND requester_subject = $3 AND thread_id = $4 \
                                   AND status = 'requested' AND expires_at_millis <= $5 \
                                 RETURNING version",
                            )
                            .bind(tenant_id.as_str())
                            .bind(approval_id.as_uuid())
                            .bind(requester_subject)
                            .bind(thread_id.as_uuid())
                            .bind(millis(decided_at_millis)?)
                            .fetch_optional(&mut *transaction)
                            .await
                            .map_err(|_| ApprovalStoreError::Unavailable)?;
                            transaction
                                .commit()
                                .await
                                .map_err(|_| ApprovalStoreError::Unavailable)?;
                            return match expired {
                                Some(row) => Ok(ApprovalDecisionResolution::ExistingTerminal {
                                    decision: None,
                                    status: ApprovalStatus::Expired,
                                    version: row_version(&row)?,
                                }),
                                None => Ok(ApprovalDecisionResolution::TurnGuardRejected),
                            };
                        }
                    }
                    return Ok(ApprovalDecisionResolution::TurnGuardRejected);
                }
            }
            let winner = sqlx::query(
                "UPDATE tool_approvals
                 SET status = $3, decision = $3, approver = $4,
                     decided_at_millis = $5, version = version + 1
                 WHERE tenant_id = $1 AND approval_id = $2
                   AND requester_subject = $6 AND thread_id = $7
                   AND status = 'requested' AND expires_at_millis > $5
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
                    decision,
                    version,
                    decided_at_millis,
                    tenant_id,
                    &row,
                )
                .await?;
                transaction
                    .commit()
                    .await
                    .map_err(|_| ApprovalStoreError::Unavailable)?;
                return Ok(ApprovalDecisionResolution::Won { decision, version });
            }
            drop(transaction);
            let existing = sqlx::query(
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
            let Some(row) = existing else {
                return Ok(ApprovalDecisionResolution::NotFound);
            };
            let status_text: String = row
                .try_get("status")
                .map_err(|_| ApprovalStoreError::Unavailable)?;
            if status_text == "requested" {
                // Only a genuinely expired record transitions here: the winner
                // update can also lose to the Turn guard while the record is
                // still inside its decision window, and such a rejection must
                // leave the canonical status untouched (ADR-0003 D-6 state
                // machine).
                let expired = sqlx::query(
                    "UPDATE tool_approvals
                     SET status = 'expired', version = version + 1
                     WHERE tenant_id = $1 AND approval_id = $2
                       AND requester_subject = $3 AND thread_id = $4
                       AND status = 'requested' AND expires_at_millis <= $5
                     RETURNING version",
                )
                .bind(tenant_id.as_str())
                .bind(approval_id.as_uuid())
                .bind(requester_subject)
                .bind(thread_id.as_uuid())
                .bind(millis(decided_at_millis)?)
                .fetch_optional(&self.pool)
                .await
                .map_err(|_| ApprovalStoreError::Unavailable)?;
                return match expired {
                    Some(expired_row) => Ok(ApprovalDecisionResolution::ExistingTerminal {
                        decision: None,
                        status: ApprovalStatus::Expired,
                        version: row_version(&expired_row)?,
                    }),
                    // Another contender won a transition between the read and
                    // this write; the canonical row is already terminal.
                    None => {
                        self.reread_terminal(approval_id, tenant_id, requester_subject, thread_id)
                            .await
                    }
                };
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
        })
    }
}

impl SqlxApprovalRecordStore {
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

/// Appends the bounded correlated audit record for one won D-6 decision
/// inside its resolving transaction (ADR-0003 TC-14).
async fn emit_decision_audit(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    approval_id: ApprovalId,
    decision: ApprovalDecision,
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
    let status = match decision {
        ApprovalDecision::Accepted => crate::domain::execution::ApprovalStatus::Accepted,
        ApprovalDecision::Declined => crate::domain::execution::ApprovalStatus::Declined,
        ApprovalDecision::Cancelled => crate::domain::execution::ApprovalStatus::Cancelled,
    };
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
