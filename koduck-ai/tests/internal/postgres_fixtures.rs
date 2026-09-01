// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Shared durable-state seeding for the crate-internal Postgres harnesses.
//!
//! Every internal Postgres regression starts from the same canonical fixture
//! shape — one thread, one started Turn, one lease window, and optionally one
//! requested D-6 approval or D-7 attempt row — so the seeds live here once
//! instead of being restated per test with drifting column lists. All
//! varying values stay bound parameters, never interpolated SQL.

use sqlx::PgPool;

use crate::domain::{TenantId, ThreadId, TurnId};

/// Fixture identity shared by the durable seeds in this module.
///
/// The tenant label doubles as the durable `subject_id`/`requester_subject`
/// wherever a seed needs a principal name, keeping one label per regression
/// identifiable in query failures.
pub(crate) struct TurnRowIds<'a> {
    /// Tenant that owns every seeded row.
    pub tenant: &'a TenantId,
    /// Thread that owns the seeded Turn.
    pub thread: &'a ThreadId,
    /// Turn whose owner state the seeded rows hang from.
    pub turn: &'a TurnId,
}

/// Lease validity window a [`seed_lease`] fixture should plant.
pub(crate) enum LeaseWindow {
    /// Renewed now and expiring after the given interval, such as
    /// `"1 hour"`; the Turn stays dispatchable.
    Live(&'static str),
    /// Renewed an hour ago and expired five minutes ago, so expiry
    /// recovery owns the Turn.
    Expired,
}

/// Seeds the thread row every other durable fixture references.
pub(crate) async fn seed_thread(pool: &PgPool, ids: &TurnRowIds<'_>, subject: &str) {
    sqlx::query(
        "INSERT INTO threads (tenant_id, subject_id, thread_id) \
         VALUES ($1, $3, $2) ON CONFLICT DO NOTHING",
    )
    .bind(ids.tenant.as_str())
    .bind(ids.thread.as_uuid())
    .bind(subject)
    .execute(pool)
    .await
    .expect("fixture thread");
}

/// Seeds one started Turn, optionally already carrying the durable
/// interruption barrier.
pub(crate) async fn seed_turn(pool: &PgPool, ids: &TurnRowIds<'_>, interrupting: bool) {
    sqlx::query(
        "INSERT INTO turns \
         (tenant_id, thread_id, turn_id, status, next_sequence, interrupting) \
         VALUES ($1, $2, $3, 'started', 1, $4) ON CONFLICT DO NOTHING",
    )
    .bind(ids.tenant.as_str())
    .bind(ids.thread.as_uuid())
    .bind(ids.turn.as_uuid())
    .bind(interrupting)
    .execute(pool)
    .await
    .expect("fixture turn");
}

/// Seeds the generation-1 unfenced lease for the seeded Turn.
pub(crate) async fn seed_lease(pool: &PgPool, ids: &TurnRowIds<'_>, window: LeaseWindow) {
    let (renewed_offset, expires_offset) = match window {
        LeaseWindow::Live(ttl) => ("0 seconds", ttl),
        LeaseWindow::Expired => ("-1 hour", "-55 minutes"),
    };
    sqlx::query(
        "INSERT INTO turn_leases \
         (tenant_id, thread_id, turn_id, generation, renewed_at, expires_at, fenced) \
         VALUES ($1, $2, $3, 1, CURRENT_TIMESTAMP + $4::INTERVAL, \
                 CURRENT_TIMESTAMP + $5::INTERVAL, FALSE) \
         ON CONFLICT DO NOTHING",
    )
    .bind(ids.tenant.as_str())
    .bind(ids.thread.as_uuid())
    .bind(ids.turn.as_uuid())
    .bind(renewed_offset)
    .bind(expires_offset)
    .execute(pool)
    .await
    .expect("fixture lease");
}

/// Seeds one prepared D-7 attempt row for the seeded Turn.
pub(crate) async fn seed_prepared_attempt(
    pool: &PgPool,
    ids: &TurnRowIds<'_>,
    attempt_id: uuid::Uuid,
    digest_hex: &str,
) {
    sqlx::query(
        "INSERT INTO tool_execution_attempts \
         (tenant_id, attempt_id, thread_id, turn_id, lease_generation, \
          descriptor_id, descriptor_version, effect, action_digest, profile_id, \
          profile_version, prepared_at_millis, status, version) \
         VALUES ($1, $4, $2, $3, 1, 'fixture.tool', 'v1', 'external_write', $5, \
                 'profile-default', 'v1', 1, 'prepared', 1)",
    )
    .bind(ids.tenant.as_str())
    .bind(ids.thread.as_uuid())
    .bind(ids.turn.as_uuid())
    .bind(attempt_id)
    .bind(digest_hex)
    .execute(pool)
    .await
    .expect("fixture prepared attempt");
}

/// Seeds one running D-7 attempt row (prepared at 1, started at
/// `started_at_millis`, version 2) for the seeded Turn.
pub(crate) async fn seed_running_attempt(
    pool: &PgPool,
    ids: &TurnRowIds<'_>,
    attempt_id: uuid::Uuid,
    digest_hex: &str,
    started_at_millis: i64,
) {
    sqlx::query(
        "INSERT INTO tool_execution_attempts \
         (tenant_id, attempt_id, thread_id, turn_id, lease_generation, \
          descriptor_id, descriptor_version, effect, action_digest, profile_id, \
          profile_version, prepared_at_millis, started_at_millis, status, version) \
         VALUES ($1, $4, $2, $3, 1, 'fixture.tool', 'v1', 'external_write', $5, \
                 'profile-default', 'v1', 1, $6, 'running', 2)",
    )
    .bind(ids.tenant.as_str())
    .bind(ids.thread.as_uuid())
    .bind(ids.turn.as_uuid())
    .bind(attempt_id)
    .bind(digest_hex)
    .bind(started_at_millis)
    .execute(pool)
    .await
    .expect("fixture running attempt");
}

/// Seeds one in-window requested D-6 approval row for the seeded Turn.
///
/// The requester is the fixture `subject` so a seeded principal can decide
/// the record; the immutable action columns mirror [`fixture_binding`].
pub(crate) async fn seed_requested_approval(
    pool: &PgPool,
    ids: &TurnRowIds<'_>,
    subject: &str,
    approval_id: uuid::Uuid,
    attempt_id: uuid::Uuid,
    digest_hex: &str,
) {
    sqlx::query(
        "INSERT INTO tool_approvals \
         (tenant_id, approval_id, thread_id, turn_id, attempt_id, lease_generation, \
          descriptor_id, descriptor_version, effect, action_digest, profile_id, \
          profile_version, requested_at_millis, expires_at_millis, status, \
          requester_subject, version) \
         VALUES ($1, $2, $3, $4, $5, 1, 'fixture.tool', 'v1', 'external_write', $6, \
                 'profile-default', 'v1', 1, 600000, 'requested', $7, 1)",
    )
    .bind(ids.tenant.as_str())
    .bind(approval_id)
    .bind(ids.thread.as_uuid())
    .bind(ids.turn.as_uuid())
    .bind(attempt_id)
    .bind(digest_hex)
    .bind(subject)
    .execute(pool)
    .await
    .expect("fixture requested approval");
}

/// Builds the canonical fixture binding whose immutable columns match every
/// seeded `tool_execution_attempts` and `tool_approvals` row: descriptor
/// `fixture.tool` v1, an external-write effect on `fixture-target`, empty
/// parameters, and the default profile.
pub(crate) fn fixture_binding(
    tenant: &TenantId,
    thread: ThreadId,
    turn: TurnId,
) -> crate::domain::execution::ExactActionBinding {
    let action = crate::domain::tool::Action::new(
        "fixture.tool",
        "v1",
        crate::domain::tool::Effect::ExternalWrite,
        "fixture-target",
        crate::adapters::tool::parse_action_parameters("{}").expect("valid parameters"),
    )
    .expect("valid action");
    crate::domain::execution::ExactActionBinding::new(
        tenant.clone(),
        thread,
        turn,
        crate::domain::LeaseGeneration::initial(),
        ("profile-default", "v1"),
        crate::domain::execution::AttemptId::new(),
        action,
    )
    .expect("valid binding")
}

/// Hex-encodes an action digest exactly as the durable stores bind it.
pub(crate) fn hex_digest(bytes: &[u8; 32]) -> String {
    let mut text = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
}
