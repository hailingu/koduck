// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md

//! AC-5: raw admission boundaries remain exact (4,096 raw Items and 1 MiB of
//! canonical escaped payload bytes, overflow sentinel row included), derived
//! history can never expand beyond the admitted raw totals, the correction
//! encoding premise holds byte-for-byte through the public durable codec, and
//! a connection that cannot be acquired inside the unchanged two-second
//! attempt deadline rejects as `Unavailable` before any acceptance.

use std::time::{Duration, Instant};

use koduck_ai::adapters::history::postgres::{
    DurableItemCodec, PostgresExecutor, PostgresTurnHistory, SqlxPostgresExecutor,
};
use koduck_ai::adapters::http::{HttpAdapter, HttpMethod, HttpRequest};
use koduck_ai::application::{
    HistoryError, PriorTurnHistory, PriorTurnRow, ProviderHistoryKind, ProviderHistoryValue,
    TurnCommand, TurnRunner, prepare_provider_history,
};
use koduck_ai::domain::item_correction::ItemCorrection;
use koduck_ai::domain::{
    Item, ItemId, ItemPayload, TenantId, ThreadId as DomainThread, TrustContext,
    TurnId as DomainTurn,
};

use super::support::*;
use crate::fixtures::*;

/// The exact inclusive raw-Item cap (ADR-0006 PC-03).
const MAX_RAW_ITEMS: usize = 4_096;
/// The exact inclusive canonical payload byte cap.
const MAX_RAW_BYTES: usize = 1_048_576;
/// The `{"content":"…"}` envelope size of one textual payload.
const ENVELOPE: usize = 14;

/// Runs the complete AC-5 check against isolated migrated `PostgreSQL`.
pub(crate) fn run() {
    let (pool, runtime) = migrated_pool();
    let executor = SqlxPostgresExecutor::new(pool.clone(), runtime.handle().clone());
    raw_count_boundaries(&runtime, &pool, &executor);
    byte_boundaries(&runtime, &pool, &executor);
    escaping_and_unicode_count_in_bytes(&runtime, &pool, &executor);
    codec_equality_premise_holds();
    derived_history_never_expands();
    raw_overflow_with_small_projection(&runtime, &pool, &executor);
    read_deadline_rejects_before_acceptance(&runtime);
    runtime.block_on(pool.close());
}

/// Seeds one owned Thread whose single Turn holds the supplied Items.
fn seed_items_thread(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    tenant: &TenantId,
    subject: &str,
    items: &[Item],
) -> DomainThread {
    let thread_id = DomainThread::new();
    let turn_id = DomainTurn::new();
    seed_thread_row(runtime, pool, &owner_trust(tenant, subject), thread_id);
    seed_turn_row(
        runtime,
        pool,
        tenant,
        thread_id,
        turn_id,
        "2026-01-01T00:00:00+00:00",
        "started",
    );
    seed_item_rows(runtime, pool, tenant, thread_id, turn_id, items);
    thread_id
}

/// Builds one valid user-plus-deltas Turn of the given raw Item count.
fn filler_items(count: usize) -> Vec<Item> {
    let mut items = Vec::with_capacity(count);
    items.push(user_item(1, "root"));
    for sequence in 2..=count as u64 {
        items.push(delta_item(sequence, "d"));
    }
    items
}

/// Raw Item counts below and at the cap prepare complete views; one Item over
/// rejects as `ContextLimit` with no partial group on both chat routes.
fn raw_count_boundaries(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    executor: &SqlxPostgresExecutor,
) {
    let (tenant, subject) = unique_owner();
    let owner = owner_trust(&tenant, &subject);
    for count in [MAX_RAW_ITEMS - 1, MAX_RAW_ITEMS] {
        let thread = seed_items_thread(runtime, pool, &tenant, &subject, &filler_items(count));
        let groups = executor
            .prior_thread_turns(&owner, thread)
            .expect("the below-cap and at-cap counts prepare");
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0].rows.len(),
            count,
            "the complete group is returned for {count}"
        );
        let prepared = prepare_provider_history(&owner, thread, &groups)
            .expect("the at-limit raw history prepares");
        assert_eq!(prepared.len(), count);
    }
    let overflow = seed_items_thread(
        runtime,
        pool,
        &tenant,
        &subject,
        &filler_items(MAX_RAW_ITEMS + 1),
    );
    assert_eq!(
        executor.prior_thread_turns(&owner, overflow),
        Err(HistoryError::ContextLimit),
        "the 4,097th raw Item rejects through the overflow sentinel"
    );
    assert_rejects_with_400_and_zero_effects(runtime, pool, executor, &owner, overflow);
}

/// Canonical payload totals at 1,048,575 / 1,048,576 bytes prepare; one byte
/// over rejects as `ContextLimit`.
fn byte_boundaries(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    executor: &SqlxPostgresExecutor,
) {
    let (tenant, subject) = unique_owner();
    let owner = owner_trust(&tenant, &subject);
    for content_bytes in [MAX_RAW_BYTES - ENVELOPE - 1, MAX_RAW_BYTES - ENVELOPE] {
        let items = vec![user_item(1, &"a".repeat(content_bytes))];
        let thread = seed_items_thread(runtime, pool, &tenant, &subject, &items);
        let groups = executor
            .prior_thread_turns(&owner, thread)
            .expect("the below-cap and at-cap byte totals prepare");
        let prepared = prepare_provider_history(&owner, thread, &groups)
            .expect("the at-limit byte total prepares");
        assert_eq!(prepared.len(), 1);
    }
    let items = vec![user_item(1, &"a".repeat(MAX_RAW_BYTES - ENVELOPE + 1))];
    let overflow = seed_items_thread(runtime, pool, &tenant, &subject, &items);
    assert_eq!(
        executor.prior_thread_turns(&owner, overflow),
        Err(HistoryError::ContextLimit),
        "one canonical payload byte over the cap rejects"
    );
    assert_rejects_with_400_and_zero_effects(runtime, pool, executor, &owner, overflow);
}

/// Escaped content counts by its escaped encoding: a raw-UTF-8-short payload
/// whose escaped form crosses the cap still rejects.
fn escaping_and_unicode_count_in_bytes(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    executor: &SqlxPostgresExecutor,
) {
    let (tenant, subject) = unique_owner();
    let owner = owner_trust(&tenant, &subject);
    // Each backslash escapes to two JSON bytes: 524,281 raw bytes encode to
    // exactly 1,048,576 canonical bytes and stay admitted; one more rejects.
    let at_limit = vec![user_item(1, &"\\".repeat(524_281))];
    let thread = seed_items_thread(runtime, pool, &tenant, &subject, &at_limit);
    assert!(
        executor.prior_thread_turns(&owner, thread).is_ok(),
        "the escaped form at the exact cap stays admitted"
    );
    let over = vec![user_item(1, &"\\".repeat(524_282))];
    let overflow = seed_items_thread(runtime, pool, &tenant, &subject, &over);
    assert_eq!(
        executor.prior_thread_turns(&owner, overflow),
        Err(HistoryError::ContextLimit),
        "escaped bytes, not raw UTF-8 length, drive the canonical cap"
    );
    // Multi-byte characters stay unescaped but still count as UTF-8 bytes.
    let unicode_at_limit = vec![user_item(1, &"é".repeat(524_281))];
    let thread = seed_items_thread(runtime, pool, &tenant, &subject, &unicode_at_limit);
    assert!(
        executor.prior_thread_turns(&owner, thread).is_ok(),
        "unescaped multi-byte content at the exact cap stays admitted"
    );
    assert_rejects_with_400_and_zero_effects(runtime, pool, executor, &owner, overflow);
}

/// Asserts the 4,097-row sentinel and the public durable codec's LIMIT bound.
fn assert_rejects_with_400_and_zero_effects(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    executor: &SqlxPostgresExecutor,
    owner: &TrustContext,
    thread: DomainThread,
) {
    let provider = RecordingProvider::new();
    let mut adapter = HttpAdapter::new(TurnRunner::new(
        provider.clone(),
        PostgresTurnHistory::new(executor.clone()),
    ));
    let turns_before = sql_turn_count(runtime, pool, &owner.tenant_id, thread);
    for path in ["/api/v1/ai/chat", "/api/v1/ai/chat/stream"] {
        let response = adapter.handle(HttpRequest {
            method: HttpMethod::Post,
            path: path.to_owned(),
            content_type: Some("application/json".to_owned()),
            body: format!(r#"{{"input":"hello","thread_id":"{}"}}"#, thread.as_uuid()),
            trust: Some(owner.clone()),
        });
        assert_eq!(response.status, 400, "path {path}");
        assert!(response.body.contains("\"code\":\"invalid-request\""));
    }
    assert_eq!(
        sql_turn_count(runtime, pool, &owner.tenant_id, thread),
        turns_before,
        "no new Turn is accepted"
    );
    assert!(
        provider.recorded().is_empty(),
        "no provider request is made"
    );
}

/// The named encoding premise (PC-03): correction, user, and assistant
/// payloads with identical content encode byte-for-byte identically through
/// the public durable codec, including escaping.
fn codec_equality_premise_holds() {
    for content in [
        "plain".to_owned(),
        "with \"quotes\" and \\backslashes\\".to_owned(),
        "control \u{1} and\tnewline\n".to_owned(),
        "unicode é 你好".to_owned(),
    ] {
        let correction = ItemCorrection::new(&content, ItemId::new()).expect("valid correction");
        let user = DurableItemCodec::encode(&ItemPayload::UserMessage {
            content: content.clone(),
        });
        let delta = DurableItemCodec::encode(&ItemPayload::AgentMessageDelta {
            content: content.clone(),
        });
        let corrected = DurableItemCodec::encode(&ItemPayload::Correction(correction));
        assert_eq!(
            user.payload, delta.payload,
            "user and delta envelopes agree for {content:?}"
        );
        assert_eq!(
            user.payload, corrected.payload,
            "the correction replacement encoding equals the root encoding for {content:?}"
        );
    }
}

/// Derived history never expands: each scenario's effective Item count equals
/// raw minus corrections and its effective canonical bytes never exceed raw.
fn derived_history_never_expands() {
    let scenarios = derived_scenarios();
    for (label, groups, thread) in scenarios {
        let trust = command_trust();
        let prepared = prepare_provider_history(&trust, thread, &groups)
            .unwrap_or_else(|error| panic!("scenario {label} prepares: {error}"));
        let raw_items: usize = groups.iter().map(|group| group.rows.len()).sum();
        let corrections: usize = groups
            .iter()
            .flat_map(|group| group.rows.iter())
            .filter(|row| matches!(row.item.payload, ItemPayload::Correction(_)))
            .count();
        assert_eq!(
            prepared.len(),
            raw_items - corrections,
            "scenario {label}: effective count equals raw minus corrections"
        );
        let raw_bytes: usize = groups
            .iter()
            .flat_map(|group| group.rows.iter())
            .map(|row| DurableItemCodec::encode(&row.item.payload).payload.len())
            .sum();
        let effective_bytes: usize = prepared
            .iter()
            .map(|view| match &view.value {
                ProviderHistoryValue::Text(content) => {
                    DurableItemCodec::encode(&textual_payload(view.kind, content))
                        .payload
                        .len()
                }
                ProviderHistoryValue::NonText(payload) => {
                    DurableItemCodec::encode(payload).payload.len()
                }
            })
            .sum();
        assert!(
            effective_bytes <= raw_bytes,
            "scenario {label}: effective bytes {effective_bytes} exceed raw {raw_bytes}"
        );
    }
}

/// Rebuilds the original-kind payload carrying one view's selected content.
fn textual_payload(kind: ProviderHistoryKind, content: &str) -> ItemPayload {
    match kind {
        ProviderHistoryKind::UserMessage => ItemPayload::UserMessage {
            content: content.to_owned(),
        },
        _ => ItemPayload::AgentMessageDelta {
            content: content.to_owned(),
        },
    }
}

/// The derived non-expansion scenario corpus.
fn derived_scenarios() -> Vec<(&'static str, Vec<PriorTurnHistory>, DomainThread)> {
    let thread = DomainThread::new();
    let group = |items: Vec<Item>| {
        let turn = DomainTurn::new();
        PriorTurnHistory::new(
            turn,
            items
                .into_iter()
                .map(|item| PriorTurnRow::new(item, owned_scope(thread, turn)))
                .collect(),
        )
    };
    let root = user_item(1, "origin");
    let first = correction_item(2, root.item_id, "second");
    let second = correction_item(3, first.item_id, "third");
    let repeated = vec![root, first, second];
    let delta_one = delta_item(1, "one");
    let delta_two = delta_item(2, "two");
    let fix_two = correction_item(3, delta_two.item_id, "two-fixed");
    let fix_one = correction_item(4, delta_one.item_id, "one-fixed");
    let independent = vec![delta_one, delta_two, fix_two, fix_one];
    let untouched: Vec<Item> = crate::fixtures::non_correction_payload_fixtures()
        .into_iter()
        .enumerate()
        .map(|(index, payload)| Item::new(index as u64 + 1, payload))
        .collect();
    vec![
        (
            "repeated corrections on one root",
            vec![group(repeated)],
            thread,
        ),
        (
            "independent corrections on two roots",
            vec![group(independent)],
            thread,
        ),
        (
            "every non-text kind unchanged",
            vec![group(untouched)],
            thread,
        ),
        (
            "uncorrected source",
            vec![group(vec![user_item(1, "kept"), delta_item(2, "kept")])],
            thread,
        ),
    ]
}

/// A raw overflow whose projection would be tiny still rejects: one root plus
/// a 4,096-link correction chain is 4,097 raw Items with one effective view.
fn raw_overflow_with_small_projection(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    executor: &SqlxPostgresExecutor,
) {
    let (tenant, subject) = unique_owner();
    let owner = owner_trust(&tenant, &subject);
    let mut items = Vec::with_capacity(MAX_RAW_ITEMS + 1);
    items.push(user_item(1, "origin"));
    let mut previous = items[0].item_id;
    for sequence in 2..=(MAX_RAW_ITEMS + 1) as u64 {
        let link = correction_item(sequence, previous, "tip");
        previous = link.item_id;
        items.push(link);
    }
    let thread = seed_items_thread(runtime, pool, &tenant, &subject, &items);
    assert_eq!(
        executor.prior_thread_turns(&owner, thread),
        Err(HistoryError::ContextLimit),
        "raw overflow still rejects when the derived view would be a single Item"
    );
}

/// A read that cannot acquire the only pooled connection inside the unchanged
/// two-second attempt deadline rejects as `Unavailable` under a four-second
/// guard, with zero acceptance or dispatch.
fn read_deadline_rejects_before_acceptance(runtime: &tokio::runtime::Runtime) {
    let database_url = std::env::var("KODUCK_AI_TEST_DATABASE_URL")
        .expect("KODUCK_AI_TEST_DATABASE_URL must point at an isolated migrated PostgreSQL");
    let single = runtime
        .block_on(
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect(&database_url),
        )
        .expect("connect the single-connection pool");
    let holder_pool = single.clone();
    let holder = runtime.spawn(async move {
        let mut connection = holder_pool
            .acquire()
            .await
            .expect("holder acquires the connection");
        sqlx::query("SELECT pg_sleep(6)")
            .execute(&mut *connection)
            .await
            .expect("the holder keeps the connection busy");
    });
    std::thread::sleep(Duration::from_millis(200));
    let executor = SqlxPostgresExecutor::new(single.clone(), runtime.handle().clone());
    let (tenant, subject) = unique_owner();
    let owner = owner_trust(&tenant, &subject);
    let provider = RecordingProvider::new();
    let mut runner = TurnRunner::new(provider.clone(), PostgresTurnHistory::new(executor.clone()));

    let started = Instant::now();
    let result = runner.execute(
        TurnCommand::new(owner, Some(DomainThread::new()), "resume")
            .expect("valid resumed command"),
    );
    let elapsed = started.elapsed();

    assert!(matches!(
        result,
        Err(koduck_ai::application::TurnRunError::History(
            HistoryError::Unavailable
        ))
    ));
    assert!(
        elapsed >= Duration::from_secs(2) && elapsed < Duration::from_secs(4),
        "the unchanged two-second attempt deadline expires inside the four-second guard (took {elapsed:?})"
    );
    assert!(
        provider.recorded().is_empty(),
        "no provider request is made"
    );
    holder.abort();
    runtime.block_on(single.close());
}
