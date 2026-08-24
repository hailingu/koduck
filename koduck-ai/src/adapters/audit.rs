// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! `JSON` serialization and durable persistence for bounded C-5 audit
//! records (ADR-0003 TC-14).

use std::future::Future;
use std::time::Duration;

use sqlx::PgPool;
use tokio::runtime::Handle;

use crate::application::{
    AppendPolicy, ToolAuditEmitError, ToolAuditError, ToolAuditRecord, ToolAuditRecordTooLarge,
    ToolAuditSink, ToolAuditTrail,
};

/// Production audit trail: `JSON` wire serialization within the TC-14 bound,
/// delivered to the configured durable-trail sink.
///
/// The byte bound is enforced through the application-owned
/// `serialized_within_bound` check, so the trail cannot widen audit content
/// (ADR-0003 TC-14).
#[derive(Clone)]
pub struct SerializingToolAuditTrail<S> {
    sink: S,
}

impl<S> SerializingToolAuditTrail<S> {
    /// Creates the trail over one durable-trail sink.
    #[must_use]
    pub const fn new(sink: S) -> Self {
        Self { sink }
    }
}

impl<S> ToolAuditTrail for SerializingToolAuditTrail<S>
where
    S: ToolAuditSink,
{
    fn emit(&mut self, record: &ToolAuditRecord) -> Result<(), ToolAuditEmitError> {
        let serialized = serialize_audit_record(record).map_err(ToolAuditEmitError::TooLarge)?;
        self.sink
            .record(record, &serialized)
            .map_err(ToolAuditEmitError::Sink)
    }
}

/// Production durable audit-trail sink over the canonical
/// `tool_audit_records` table: every emission appends one bounded row whose
/// Turn-correlation columns come from the owned application record, never
/// from re-parsed record content (ADR-0003 TC-14).
///
/// The table is append-only evidence; a failed append is reported to the
/// caller as a typed error without retry storms, so the committed terminal
/// stands while the missing audit evidence stays observable.
#[derive(Clone)]
pub struct SqlxToolAuditSink {
    pool: PgPool,
    runtime: Handle,
}

impl SqlxToolAuditSink {
    /// Creates a sink whose synchronous port calls drive `SQLx` on `runtime`.
    #[must_use]
    pub const fn new(pool: PgPool, runtime: Handle) -> Self {
        Self { pool, runtime }
    }

    fn wait(
        &self,
        operation: impl Future<Output = Result<(), ToolAuditError>>,
    ) -> Result<(), ToolAuditError> {
        let deadline: Duration = AppendPolicy::cand_1().deadline();
        self.runtime.block_on(async {
            tokio::time::timeout(deadline, operation)
                .await
                .map_err(|_| ToolAuditError)?
        })
    }
}

impl ToolAuditSink for SqlxToolAuditSink {
    fn record(&mut self, record: &ToolAuditRecord, serialized: &str) -> Result<(), ToolAuditError> {
        let at_millis = i64::try_from(record.at_millis()).map_err(|_| ToolAuditError)?;
        let thread_id = uuid::Uuid::parse_str(record.thread_id()).map_err(|_| ToolAuditError)?;
        let turn_id = uuid::Uuid::parse_str(record.turn_id()).map_err(|_| ToolAuditError)?;
        self.wait(async {
            sqlx::query(
                "INSERT INTO tool_audit_records \
                 (tenant_id, thread_id, turn_id, at_millis, record) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(record.tenant_id())
            .bind(thread_id)
            .bind(turn_id)
            .bind(at_millis)
            .bind(serialized)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|_| ToolAuditError)
        })
    }
}

/// Serializes one audit record within the TC-14 byte bound.
///
/// # Errors
///
/// Returns [`ToolAuditRecordTooLarge`] when the serialized record would
/// exceed [`MAX_AUDIT_RECORD_BYTES`]; the caller then emits no record rather
/// than truncating correlated evidence.
pub fn serialize_audit_record(record: &ToolAuditRecord) -> Result<String, ToolAuditRecordTooLarge> {
    let serialized = serde_json::to_string(record).map_err(|_| ToolAuditRecordTooLarge)?;
    record.serialized_within_bound(serialized)
}
