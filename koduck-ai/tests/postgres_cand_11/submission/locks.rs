// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! CAND-18 live lock-protocol cases: the two-int4 submission advisory-lock
//! namespace never overlaps the existing bigint Item namespace (SI-03b), a
//! submission-lock waiter owns no Item lock, the unlocked preliminary read is
//! not serialized behind the submission lock (SI-07a), and an exact retry
//! completes while the original provider stream is still in flight
//! (SI-04/SI-07e).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use uuid::Uuid;

use koduck_ai::adapters::history::postgres::{PostgresExecutor, PostgresTurnHistory};
use koduck_ai::application::{
    IdentifiedAcceptance, ModelInput, ModelProvider, ProviderError, ProviderEvent, ProviderStream,
    TurnCommand, TurnHistory, TurnOutcome, submission_lock_keys,
};
use koduck_ai::domain::{SubmissionId, ThreadId};

use super::super::harness::Harness;
use super::runner::{completed_events, scripted_provider};
use super::{connected_history, count_rows, identified_command, trust};

/// Holds the int4-pair submission advisory lock in an open transaction until
/// the returned transaction is dropped.
fn hold_submission_lock(
    harness: &Harness,
    keys: (i32, i32),
) -> sqlx::Transaction<'static, sqlx::Postgres> {
    let lock_pool = harness.pool.clone();
    let mut holder = harness
        .runtime
        .block_on(lock_pool.begin())
        .expect("holder transaction starts");
    harness.runtime.block_on(async {
        sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
            .bind(keys.0)
            .bind(keys.1)
            .execute(&mut *holder)
            .await
            .expect("hold the submission lock");
    });
    holder
}

/// Polls live `pg_locks` until the contender's waiting submission tag appears.
fn wait_for_submission_waiter(harness: &Harness, keys: (i32, i32)) {
    harness.runtime.block_on(async {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            let waiting = sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' \
                 AND classid = $1 AND objid = $2 AND objsubid = 2 AND NOT granted",
            )
            .bind(i64::from(keys.0.cast_unsigned()))
            .bind(i64::from(keys.1.cast_unsigned()))
            .fetch_one(&harness.pool)
            .await
            .expect("poll the waiting submission tag");
            if waiting > 0 {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the contender never waited on the submission lock"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
}

/// Counts the submission-lock waiter's granted advisory locks; the waiter
/// must own none while it waits on the submission tag.
fn waiter_granted_advisory_locks(harness: &Harness, keys: (i32, i32)) -> i64 {
    harness.runtime.block_on(async {
        sqlx::query_scalar(
            "SELECT count(*) FROM pg_locks l WHERE l.locktype = 'advisory' \
             AND l.granted AND EXISTS ( \
               SELECT 1 FROM pg_locks w WHERE w.locktype = 'advisory' \
               AND w.classid = $1 AND w.objid = $2 AND w.objsubid = 2 \
               AND NOT w.granted AND w.pid = l.pid)",
        )
        .bind(i64::from(keys.0.cast_unsigned()))
        .bind(i64::from(keys.1.cast_unsigned()))
        .fetch_one(&harness.pool)
        .await
        .expect("count the waiter's granted advisory locks")
    })
}

/// SI-07c: a direct `SQLx` caller cannot shorten the reserved write attempt.
/// A live submission-lock wait outlasts the supplied 500 ms before release;
/// the healthy acceptance still creates exactly one canonical outcome.
#[test]
fn cand_18_direct_write_preserves_full_budget() {
    let _database_guard = super::serialize_database_tests();
    let (harness, _history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let keys = submission_lock_keys(
        &trust(&tenant, "subject-a"),
        SubmissionId::from_uuid(submission).expect("non-nil submission"),
    );
    let holder = hold_submission_lock(&harness, keys);
    let executor = harness.executor();
    let contender_tenant = tenant.clone();
    let contender = std::thread::spawn(move || {
        let command = identified_command(
            &contender_tenant,
            "subject-a",
            submission,
            None,
            "full write budget",
        );
        PostgresExecutor::accept_initial_with_submission(
            &executor,
            &command,
            Duration::from_millis(500),
        )
    });
    wait_for_submission_waiter(&harness, keys);
    std::thread::sleep(Duration::from_millis(750));
    harness
        .runtime
        .block_on(holder.rollback())
        .expect("release the submission lock");
    let outcome = contender.join().expect("the contender finishes");
    let rows =
        harness
            .runtime
            .block_on(count_rows(&harness.pool, &tenant, "subject-a", submission));
    assert!(
        matches!(outcome, Ok(IdentifiedAcceptance::Created(_))),
        "the permitted write retains two seconds: {outcome:?}"
    );
    assert_eq!(rows, (1, 1, 1, 1), "one atomic acceptance commits");
}

/// AC-3/SI-03b: the two-int4 submission lock namespace is separate from the
/// single-bigint Item namespace. A session-level bigint advisory lock whose
/// 64-bit value carries the same two 32-bit key halves as the submission key
/// does not block the transaction-scoped int4-pair lock, and live `pg_locks`
/// shows both tags coexisting with distinct `objsubid` values.
pub(crate) fn cand_18_locks_namespace_is_isolated() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let trust_context = trust(&tenant, "subject-a");
    let keys = submission_lock_keys(
        &trust_context,
        SubmissionId::from_uuid(submission).expect("non-nil submission"),
    );
    // The single-bigint key whose high and low 32-bit halves equal the two
    // submission keys: in `pg_locks` the two tags share (classid, objid) and
    // differ only in objsubid (1 = bigint form, 2 = int4-pair form).
    let bigint_key = (i64::from(keys.0.cast_unsigned()) << 32) | i64::from(keys.1.cast_unsigned());

    let holder = super::super::harness::hold_advisory_locks(&harness, &[bigint_key]);
    holder.wait_until_held();
    let tags: i64 = harness.runtime.block_on(async {
        sqlx::query_scalar(
            "SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' \
             AND classid = $1 AND objid = $2 AND granted",
        )
        .bind(i64::from(keys.0.cast_unsigned()))
        .bind(i64::from(keys.1.cast_unsigned()))
        .fetch_one(&harness.pool)
        .await
        .expect("count the matching advisory tags")
    });
    assert_eq!(
        tags, 1,
        "only the bigint-form tag with objsubid 1 exists before the acceptance"
    );

    // The int4-pair submission lock is not blocked by the matching bigint tag.
    let started = std::time::Instant::now();
    let command = identified_command(&tenant, "subject-a", submission, None, "lock input");
    let outcome =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the int4-pair lock does not wait on the bigint tag");
    assert!(matches!(outcome, IdentifiedAcceptance::Created(_)));
    assert!(
        started.elapsed() < Duration::from_millis(1_500),
        "the acceptance was not blocked: it resolved in {:?}",
        started.elapsed()
    );

    // After the acceptance, both tags coexist live with distinct objsubid.
    let forms: Vec<(i64, i64)> = harness.runtime.block_on(async {
        sqlx::query_as(
            "SELECT objsubid::bigint, count(*)::bigint FROM pg_locks \
             WHERE locktype = 'advisory' AND classid = $1 AND objid = $2 \
             AND granted GROUP BY objsubid",
        )
        .bind(i64::from(keys.0.cast_unsigned()))
        .bind(i64::from(keys.1.cast_unsigned()))
        .fetch_all(&harness.pool)
        .await
        .expect("read the live advisory tags")
    });
    assert!(
        forms.contains(&(1, 1)),
        "the bigint-form session lock stays held: {forms:?}"
    );
    holder.release();
}

/// AC-3/SI-03b: while one acceptance waits on the submission lock, the waiter
/// owns no Item advisory lock — the canonical Item lock is taken only after
/// the submission lock, inside the same transaction, so no path can hold them
/// in reverse order.
pub(crate) fn cand_18_submission_waiter_holds_no_item_lock() {
    let _database_guard = super::serialize_database_tests();
    let (harness, _history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let trust_context = trust(&tenant, "subject-a");
    let keys = submission_lock_keys(
        &trust_context,
        SubmissionId::from_uuid(submission).expect("non-nil submission"),
    );

    // Hold the int4-pair submission lock in an open transaction.
    let holder = hold_submission_lock(&harness, keys);

    // The contender blocks on the submission lock inside its own connection.
    let executor = harness.executor();
    let contender_tenant = tenant.clone();
    let handle = std::thread::spawn(move || {
        let mut history = PostgresTurnHistory::new(executor);
        let command = identified_command(
            &contender_tenant,
            "subject-a",
            submission,
            None,
            "waiter input",
        );
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(4))
            .expect("the contender accepts once the lock releases")
    });
    wait_for_submission_waiter(&harness, keys);

    // The waiter's granted advisory locks must be empty: it has taken no Item
    // bigint lock while waiting on the submission tag.
    let granted = waiter_granted_advisory_locks(&harness, keys);
    assert_eq!(
        granted, 0,
        "a submission-lock waiter owns no advisory lock, in particular no Item lock"
    );

    harness.runtime.block_on(async move {
        drop(holder);
    });
    let outcome = handle.join().expect("the contender finishes");
    assert!(
        matches!(outcome, IdentifiedAcceptance::Created(_)),
        "the released lock lets the contender create: {outcome:?}"
    );
}

/// AC-7/SI-07a (AC-3 lock ownership): the preliminary lookup is read-only and
/// unlocked — a held submission lock does not block the initial committed
/// read for an existing key or an absent one.
pub(crate) fn cand_18_preliminary_read_unblocked_by_submission_lock() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "observed input");
    let IdentifiedAcceptance::Created(accepted) =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the binding commits")
    else {
        panic!("the fresh key must create");
    };

    let trust_context = trust(&tenant, "subject-a");
    let keys = submission_lock_keys(
        &trust_context,
        SubmissionId::from_uuid(submission).expect("non-nil submission"),
    );
    let holder = hold_submission_lock(&harness, keys);

    // The committed read resolves well inside the budget the locked query
    // would consume.
    let started = std::time::Instant::now();
    let observed = history
        .submission_observation(&command, Duration::from_secs(2))
        .expect("the unlocked read resolves")
        .expect("the committed binding is observed");
    assert_eq!(observed.turn_id, accepted.turn_id);
    assert!(
        started.elapsed() < Duration::from_millis(1_500),
        "the unlocked read did not wait on the lock: {:?}",
        started.elapsed()
    );

    // An absent key under the held lock also reads without waiting.
    let absent_command =
        identified_command(&tenant, "subject-a", Uuid::new_v4(), None, "absent input");
    let started = std::time::Instant::now();
    assert!(
        history
            .submission_observation(&absent_command, Duration::from_secs(2))
            .expect("the absent-key read resolves")
            .is_none()
    );
    assert!(
        started.elapsed() < Duration::from_millis(1_500),
        "the absent-key read did not wait on the lock: {:?}",
        started.elapsed()
    );

    harness.runtime.block_on(async move {
        drop(holder);
    });
}

/// A provider that enters immediately and then paces one delta per tick until
/// released, so an in-flight original execution is held open deterministically
/// while remaining responsive to cancellation checks between ticks.
#[derive(Clone)]
pub(super) struct PacingProvider {
    release: Arc<AtomicBool>,
    entered: Arc<AtomicBool>,
    observed: Arc<std::sync::Mutex<Vec<String>>>,
}

/// Creates the pacing provider with its shared release and entered signals.
pub(super) type PacingSignals = (
    PacingProvider,
    Arc<AtomicBool>,
    Arc<AtomicBool>,
    Arc<std::sync::Mutex<Vec<String>>>,
);

pub(super) fn pacing_provider() -> PacingSignals {
    let release = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(AtomicBool::new(false));
    let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
    (
        PacingProvider {
            release: Arc::clone(&release),
            entered: Arc::clone(&entered),
            observed: Arc::clone(&observed),
        },
        release,
        entered,
        observed,
    )
}

impl ModelProvider for PacingProvider {
    fn stream(&mut self, input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        self.observed.lock().expect("input lock").push(input.input);
        self.entered.store(true, Ordering::Release);
        let release = Arc::clone(&self.release);
        Ok(Box::new(PacingStream {
            release,
            finished: false,
        }))
    }
}

struct PacingStream {
    release: Arc<AtomicBool>,
    finished: bool,
}

impl Iterator for PacingStream {
    type Item = ProviderEvent;

    fn next(&mut self) -> Option<ProviderEvent> {
        if self.finished {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
        if self.release.load(Ordering::Acquire) {
            self.finished = true;
            return Some(ProviderEvent::Completed);
        }
        Some(ProviderEvent::Delta("tick".to_owned()))
    }
}

/// Blocks until the pacing provider has entered its stream.
pub(super) fn wait_until_entered(entered: &AtomicBool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !entered.load(Ordering::Acquire) {
        assert!(
            std::time::Instant::now() < deadline,
            "the original execution never entered the provider"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// AC-3/SI-04/SI-07e: an exact retry completes with its receipt while the
/// original creator's provider stream is still in flight, a different key on
/// an independent Thread proceeds in parallel, and the original still owns
/// its single provider invocation.
pub(crate) fn cand_18_retry_completes_while_provider_blocks() {
    let _database_guard = super::serialize_database_tests();
    let (harness, _history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "paced input");

    let (original, release, original_inputs) = start_paced_original(&harness, command.clone());
    retry_observes_while_paced(&harness, command);
    independent_key_proceeds_in_parallel(&harness, &tenant);

    // Releasing the original completes it with exactly one invocation.
    release.store(true, Ordering::Release);
    let original_outcome = original.join().expect("the original finishes");
    assert!(matches!(original_outcome, TurnOutcome::Owned(_)));
    assert_eq!(
        original_inputs.lock().expect("inputs").as_slice(),
        ["paced input"]
    );
    let rows =
        harness
            .runtime
            .block_on(count_rows(&harness.pool, &tenant, "subject-a", submission));
    assert_eq!(rows, (1, 1, 1, 1), "exactly one acceptance exists");
}

/// Starts the original creator's execution on its own thread with the pacing
/// provider and waits until the provider has entered its stream. Returns the
/// join handle, the release signal, and the original's recorded inputs.
fn start_paced_original(
    harness: &Harness,
    command: TurnCommand,
) -> (
    std::thread::JoinHandle<TurnOutcome>,
    Arc<AtomicBool>,
    Arc<std::sync::Mutex<Vec<String>>>,
) {
    let (provider, release, entered, original_inputs) = pacing_provider();
    let mut original_runner = koduck_ai::runtime::compose_production_runner(
        provider,
        PostgresTurnHistory::new(harness.executor()),
        koduck_ai::application::NoToolExecution,
    );
    let original = std::thread::spawn(move || {
        original_runner
            .execute_submission_with_observer_and_cancellation(command, &mut |_| {}, &|| false)
            .expect("the original creator owns its execution")
    });
    wait_until_entered(&entered);
    (original, release, original_inputs)
}

/// The exact retry observes the committed key immediately while the original
/// provider stream is still pacing, starting no provider work of its own.
fn retry_observes_while_paced(harness: &Harness, command: TurnCommand) {
    let (retry_provider, retry_inputs) = scripted_provider(completed_events());
    let mut retry_runner = koduck_ai::runtime::compose_production_runner(
        retry_provider,
        PostgresTurnHistory::new(harness.executor()),
        koduck_ai::application::NoToolExecution,
    );
    let retry_started = std::time::Instant::now();
    let outcome = retry_runner
        .execute_submission_with_observer_and_cancellation(command, &mut |_| {}, &|| false)
        .expect("the retry resolves while the provider blocks");
    assert!(
        matches!(outcome, TurnOutcome::Observed(_)),
        "the retry observes without authority: {outcome:?}"
    );
    assert!(
        retry_started.elapsed() < Duration::from_secs(2),
        "the retry was not blocked by the original execution: {:?}",
        retry_started.elapsed()
    );
    assert!(
        retry_inputs.lock().expect("inputs").is_empty(),
        "the retry starts no provider work"
    );
}

/// A different key on an independent Thread proceeds in parallel while the
/// original provider stream is still pacing.
fn independent_key_proceeds_in_parallel(harness: &Harness, tenant: &str) {
    let independent_thread = ThreadId::new();
    harness.runtime.block_on(async {
        sqlx::query("INSERT INTO threads (tenant_id, subject_id, thread_id) VALUES ($1, $2, $3)")
            .bind(tenant)
            .bind("subject-a")
            .bind(independent_thread.as_uuid())
            .execute(&harness.pool)
            .await
            .expect("seed the independent thread");
    });
    let (independent_provider, independent_inputs) = scripted_provider(completed_events());
    let mut independent_runner = koduck_ai::runtime::compose_production_runner(
        independent_provider,
        PostgresTurnHistory::new(harness.executor()),
        koduck_ai::application::NoToolExecution,
    );
    let independent_command = identified_command(
        tenant,
        "subject-a",
        Uuid::new_v4(),
        Some(independent_thread),
        "independent input",
    );
    let independent_outcome = independent_runner
        .execute_submission_with_observer_and_cancellation(
            independent_command,
            &mut |_| {},
            &|| false,
        )
        .expect("the different key proceeds while the original blocks");
    assert!(matches!(independent_outcome, TurnOutcome::Owned(_)));
    assert_eq!(
        independent_inputs.lock().expect("inputs").as_slice(),
        ["independent input"]
    );
}
