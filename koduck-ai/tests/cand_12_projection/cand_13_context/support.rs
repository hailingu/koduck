// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md

//! Shared fixtures for the CAND-13 acceptance modules: the disposable
//! migrated database, the thread-safe in-memory history double with
//! call/append instrumentation, the recording provider, and the loopback
//! OpenAI-compatible upstream that captures real `Reqwest` request bodies.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use koduck_ai::adapters::history::postgres::{PostgresExecutor, SqlxPostgresExecutor};
use koduck_ai::application::{
    AcceptedTurn, CorrectionCommand, CorrectionStore, HistoryError, ModelInput, ModelProvider,
    ModelToolResult, NewItem, PriorTurnHistory, PriorTurnRow, ProjectionScope, ProviderError,
    ProviderEvent, ToolRound, TurnCommand, TurnHistory, TurnStreamEvent,
};
use koduck_ai::domain::item_correction::ItemCorrection;
use koduck_ai::domain::{
    Item, ItemId, ItemPayload, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId, Usage,
};

pub(crate) const TENANT: &str = "tenant-a";
pub(crate) const SUBJECT: &str = "subject-a";

/// A shared append-only ordering log used to prove event interleavings
/// between the loopback upstream thread and the runner thread.
pub(crate) type EventLog = Arc<Mutex<Vec<String>>>;

pub(crate) fn event_log() -> EventLog {
    Arc::new(Mutex::new(Vec::new()))
}

/// Appends one entry to the shared ordering log under its lock.
pub(crate) fn log_event(log: &EventLog, entry: &str) {
    log.lock().expect("event log lock").push(entry.to_owned());
}

/// Returns a snapshot of the recorded ordering log.
pub(crate) fn log_snapshot(log: &EventLog) -> Vec<String> {
    log.lock().expect("event log lock").clone()
}

pub(crate) fn command_trust() -> TrustContext {
    TrustContext::new(TenantId::new(TENANT).expect("valid tenant"), SUBJECT)
        .expect("valid command trust")
}

/// Returns a unique tenant/subject pair isolating one database fixture.
pub(crate) fn unique_owner() -> (TenantId, String) {
    (
        TenantId::new(format!("ci-{}", uuid::Uuid::new_v4())).expect("unique tenant"),
        "owner".to_owned(),
    )
}

/// Returns a trust context for the given tenant/subject pair.
pub(crate) fn owner_trust(tenant: &TenantId, subject: &str) -> TrustContext {
    TrustContext::new(tenant.clone(), subject).expect("valid owner trust")
}

pub(crate) fn owned_scope(thread: ThreadId, turn: TurnId) -> ProjectionScope {
    ProjectionScope::new(
        TenantId::new(TENANT).expect("valid tenant"),
        SUBJECT,
        thread,
        turn,
    )
    .expect("valid row scope")
}

/// Wraps one canonical Item as a row reporting the given owned scope.
pub(crate) fn owned_row(thread: ThreadId, turn: TurnId, item: Item) -> PriorTurnRow {
    PriorTurnRow::new(item, owned_scope(thread, turn))
}

pub(crate) fn completed_usage() -> Usage {
    Usage::new(1, 1).expect("valid usage")
}

/// Serializes disposable-database migration across this target's parallel
/// tests: the production migration list is not concurrency-safe when two
/// fixtures race to apply it on one fresh database.
static DATABASE_SETUP_LOCK: Mutex<()> = Mutex::new(());

/// Connects an isolated migrated `PostgreSQL`. A missing or malformed test
/// database configuration fails the check instead of silently passing it
/// (ADR-0006 Acceptance Checks).
pub(crate) fn migrated_pool() -> (sqlx::postgres::PgPool, tokio::runtime::Runtime) {
    let database_url = std::env::var("KODUCK_AI_TEST_DATABASE_URL")
        .expect("KODUCK_AI_TEST_DATABASE_URL must point at an isolated migrated PostgreSQL");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("PostgreSQL smoke runtime");
    let pool = runtime
        .block_on(
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(4)
                .connect(&database_url),
        )
        .expect("connect to disposable PostgreSQL");
    let setup_guard = DATABASE_SETUP_LOCK
        .lock()
        .expect("setup lock is unpoisoned");
    for migration in [
        include_str!("../../../migrations/0001_cand_1_history.sql"),
        include_str!("../../../migrations/0002_cand_2_policy_execution.sql"),
        include_str!("../../../migrations/0003_cand_2_requester_ownership.sql"),
        include_str!("../../../migrations/0004_cand_2_tool_projections.sql"),
        include_str!("../../../migrations/0005_cand_2_execution_attempts.sql"),
        include_str!("../../../migrations/0006_cand_2_interrupt_barrier.sql"),
        include_str!("../../../migrations/0007_cand_2_tool_audit.sql"),
        include_str!("../../../migrations/0008_cand_2_interruption_approval_cancellation.sql"),
        include_str!("../../../migrations/0009_cand_3_correction_items.sql"),
        include_str!("../../../migrations/0010_cand_18_chat_submissions.sql"),
    ] {
        runtime
            .block_on(async { sqlx::raw_sql(migration).execute(&pool).await })
            .expect("apply production migration");
    }
    drop(setup_guard);
    (pool, runtime)
}

/// Returns the durable column tuple of one canonical Item.
pub(crate) fn durable_columns(
    item: &Item,
) -> koduck_ai::adapters::history::postgres::DurableItemColumns {
    koduck_ai::adapters::history::postgres::DurableItemCodec::encode(&item.payload)
}

/// Inserts one Thread row owned by the given trust context.
pub(crate) fn seed_thread_row(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    trust: &TrustContext,
    thread_id: ThreadId,
) {
    runtime
        .block_on(
            sqlx::query(
                "INSERT INTO threads (tenant_id, subject_id, thread_id) VALUES ($1, $2, $3)",
            )
            .bind(trust.tenant_id.as_str())
            .bind(trust.subject_id.as_str())
            .bind(thread_id.as_uuid())
            .execute(pool),
        )
        .expect("seed thread row");
}

/// Inserts one Turn row with an explicit creation timestamp and status.
pub(crate) fn seed_turn_row(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    tenant: &TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
    created_at: &str,
    status: &str,
) {
    runtime
        .block_on(
            sqlx::query(
                "INSERT INTO turns (tenant_id, thread_id, turn_id, status, next_sequence, \
                 created_at) VALUES ($1, $2, $3, $4, 1, $5::timestamptz)",
            )
            .bind(tenant.as_str())
            .bind(thread_id.as_uuid())
            .bind(turn_id.as_uuid())
            .bind(status)
            .bind(created_at)
            .execute(pool),
        )
        .expect("seed turn row");
}

/// Inserts many canonical Item rows of one Turn in one batched statement.
pub(crate) fn seed_item_rows(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    tenant: &TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
    items: &[Item],
) {
    let mut sequences = Vec::with_capacity(items.len());
    let mut ids = Vec::with_capacity(items.len());
    let mut types = Vec::with_capacity(items.len());
    let mut payloads = Vec::with_capacity(items.len());
    let mut terminals = Vec::with_capacity(items.len());
    let mut targets = Vec::with_capacity(items.len());
    for item in items {
        let columns = durable_columns(item);
        sequences.push(i64::try_from(item.sequence).expect("sequence fits"));
        ids.push(item.item_id.as_uuid());
        types.push(columns.item_type);
        payloads.push(columns.payload);
        terminals.push(columns.is_terminal);
        targets.push(columns.corrects_item_id);
    }
    runtime
        .block_on(
            sqlx::query(
                "INSERT INTO turn_items (tenant_id, thread_id, turn_id, sequence, item_id, \
                 item_type, payload, is_terminal, corrects_item_id) \
                 SELECT $1, $2, $3, row_sequence, row_id, row_type, row_payload, \
                 row_terminal, row_target FROM UNNEST($4::bigint[], $5::uuid[], $6::text[], \
                 $7::text[], $8::bool[], $9::uuid[]) AS rows(row_sequence, row_id, row_type, \
                 row_payload, row_terminal, row_target)",
            )
            .bind(tenant.as_str())
            .bind(thread_id.as_uuid())
            .bind(turn_id.as_uuid())
            .bind(&sequences)
            .bind(&ids)
            .bind(&types)
            .bind(&payloads)
            .bind(&terminals)
            .bind(&targets)
            .execute(pool),
        )
        .expect("seed item rows");
}

/// Returns the canonical Item rows of one Turn as production replay reads them.
pub(crate) fn sql_items_of(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    tenant: &TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
) -> Vec<Item> {
    use koduck_ai::adapters::history::postgres::DurableItemCodec;
    use sqlx::Row;

    let rows = runtime
        .block_on(
            sqlx::query(
                "SELECT item_id, sequence, item_type, payload, corrects_item_id FROM turn_items \
                 WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 ORDER BY sequence",
            )
            .bind(tenant.as_str())
            .bind(thread_id.as_uuid())
            .bind(turn_id.as_uuid())
            .fetch_all(pool),
        )
        .expect("read fixture rows");
    rows.iter()
        .map(|row| {
            let item_type = row.try_get::<String, _>("item_type").expect("item type");
            let payload = row.try_get::<String, _>("payload").expect("payload");
            let payload = DurableItemCodec::decode(
                &item_type,
                &payload,
                row.try_get::<Option<uuid::Uuid>, _>("corrects_item_id")
                    .expect("correction target"),
            )
            .expect("fixture payload decodes");
            Item {
                item_id: koduck_ai::domain::ItemId::from_uuid(
                    row.try_get::<uuid::Uuid, _>("item_id").expect("item id"),
                ),
                sequence: row
                    .try_get::<i64, _>("sequence")
                    .expect("sequence")
                    .try_into()
                    .expect("sequence fits"),
                payload,
            }
        })
        .collect()
}

/// Counts the Turn rows of one Thread.
pub(crate) fn sql_turn_count(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    tenant: &TenantId,
    thread_id: ThreadId,
) -> i64 {
    runtime
        .block_on(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM turns WHERE tenant_id = $1 AND thread_id = $2",
            )
            .bind(tenant.as_str())
            .bind(thread_id.as_uuid())
            .fetch_one(pool),
        )
        .expect("count turns")
}

/// Thread-safe in-memory canonical history double with call and append
/// instrumentation for runner-level CAND-13 fixtures.
#[derive(Clone, Default)]
pub(crate) struct MemoryHistory {
    state: Arc<Mutex<MemoryState>>,
    log: EventLog,
}

#[derive(Default)]
struct MemoryState {
    turn_order: Vec<(ThreadId, TurnId)>,
    turn_items: BTreeMap<TurnId, Vec<Item>>,
    row_scope_turns: BTreeMap<TurnId, TurnId>,
    accepted: usize,
    prior_reads: usize,
    prior_read_error: Option<HistoryError>,
    persistent_read_error: Option<HistoryError>,
    interrupt_flag: Option<Arc<AtomicBool>>,
}

impl MemoryHistory {
    /// Builds the double around one caller-owned ordering log, so runner
    /// appends interleave with other recorded events.
    pub(crate) fn with_event_log(log: EventLog) -> Self {
        Self {
            state: Arc::new(Mutex::default()),
            log,
        }
    }

    /// Makes the persisted-interruption poll report `true` once the flag is set.
    pub(crate) fn with_interrupt_flag(self, flag: Arc<AtomicBool>) -> Self {
        self.state.lock().expect("history lock").interrupt_flag = Some(flag);
        self
    }

    /// Appends one correction Item directly, standing in for CAND-11
    /// admission after a terminal, and records it in the ordering log.
    pub(crate) fn inject_correction(
        &self,
        turn_id: TurnId,
        target: ItemId,
        content: &str,
        log_entry: Option<&str>,
    ) -> Item {
        let mut state = self.state.lock().expect("history lock");
        let items = state.turn_items.get_mut(&turn_id).expect("seeded Turn");
        let correction = Item::new(
            items.len() as u64 + 1,
            ItemPayload::Correction(
                ItemCorrection::new(content, target).expect("valid correction content"),
            ),
        );
        items.push(correction.clone());
        drop(state);
        if let Some(entry) = log_entry {
            log_event(&self.log, entry);
        }
        correction
    }

    /// Seeds a Turn whose stored rows are exactly the supplied Items.
    pub(crate) fn seed_turn(&self, thread_id: ThreadId, turn_id: TurnId, rows: Vec<Item>) {
        let mut state = self.state.lock().expect("history lock");
        state.turn_order.push((thread_id, turn_id));
        state.turn_items.insert(turn_id, rows);
    }

    /// Seeds a Turn whose rows report a different Turn in their scope, so a
    /// row/group Turn mismatch reaches preparation even at row index 0.
    pub(crate) fn seed_turn_with_row_scope(
        &self,
        thread_id: ThreadId,
        turn_id: TurnId,
        rows: Vec<Item>,
        row_scope_turn: TurnId,
    ) {
        let mut state = self.state.lock().expect("history lock");
        state.turn_order.push((thread_id, turn_id));
        state.turn_items.insert(turn_id, rows);
        state.row_scope_turns.insert(turn_id, row_scope_turn);
    }

    pub(crate) fn accepted_count(&self) -> usize {
        self.state.lock().expect("history lock").accepted
    }

    pub(crate) fn prior_reads(&self) -> usize {
        self.state.lock().expect("history lock").prior_reads
    }

    /// Returns the stored rows of one Turn.
    pub(crate) fn items_of(&self, turn_id: TurnId) -> Vec<Item> {
        self.state
            .lock()
            .expect("history lock")
            .turn_items
            .get(&turn_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Makes the next bounded prior-history read fail with the typed error.
    pub(crate) fn fail_next_prior_read(&self, error: HistoryError) {
        self.state.lock().expect("history lock").prior_read_error = Some(error);
    }

    /// Makes every later bounded prior-history read fail with the typed
    /// error, modelling an outage that persists across retries.
    pub(crate) fn fail_all_prior_reads(&self, error: HistoryError) {
        self.state
            .lock()
            .expect("history lock")
            .persistent_read_error = Some(error);
    }

    /// Clears the modelled read outage.
    pub(crate) fn clear_read_failures(&self) {
        let mut state = self.state.lock().expect("history lock");
        state.persistent_read_error = None;
        state.prior_read_error = None;
    }
}

impl TurnHistory for MemoryHistory {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
        _tool_terminals: Vec<NewItem>,
    ) -> Result<(), HistoryError> {
        Ok(())
    }

    fn interruption_requested(&self, _turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        let state = self.state.lock().expect("history lock");
        Ok(state
            .interrupt_flag
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::SeqCst)))
    }

    fn prior_thread_turns(
        &self,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> Result<Vec<PriorTurnHistory>, HistoryError> {
        let mut state = self.state.lock().expect("history lock");
        state.prior_reads += 1;
        if let Some(error) = state.prior_read_error.take() {
            return Err(error);
        }
        if let Some(error) = state.persistent_read_error.clone() {
            return Err(error);
        }
        let mut groups = Vec::new();
        for (owned_thread, turn_id) in &state.turn_order {
            if *owned_thread != thread_id {
                continue;
            }
            let row_scope_turn = state.row_scope_turns.get(turn_id).unwrap_or(turn_id);
            let scope = ProjectionScope::new(
                trust.tenant_id.clone(),
                &trust.subject_id,
                thread_id,
                *row_scope_turn,
            )
            .expect("valid double scope");
            let rows = state
                .turn_items
                .get(turn_id)
                .map(|items| {
                    items
                        .iter()
                        .cloned()
                        .map(|item| PriorTurnRow::new(item, scope.clone()))
                        .collect()
                })
                .unwrap_or_default();
            groups.push(PriorTurnHistory::new(*turn_id, rows));
        }
        Ok(groups)
    }

    fn submission_observation(
        &self,
        _command: &TurnCommand,
        _deadline: std::time::Duration,
    ) -> Result<Option<koduck_ai::application::SubmissionObservation>, HistoryError> {
        // The in-memory double hosts no submission bindings; the identified
        // flow is exercised against the production PostgreSQL adapter.
        Ok(None)
    }

    fn accept_initial_with_submission(
        &mut self,
        command: &TurnCommand,
        _deadline: std::time::Duration,
    ) -> Result<koduck_ai::application::IdentifiedAcceptance, HistoryError> {
        // Fail closed: an identified command must never be silently
        // processed as an unidentified fresh submission (SI-01d).
        let _ = command;
        Err(HistoryError::Unavailable)
    }

    fn accept_initial(&mut self, command: &TurnCommand) -> Result<AcceptedTurn, HistoryError> {
        let thread_id = command.thread_id.unwrap_or_default();
        let turn_id = TurnId::new();
        let input = Item::new(
            1,
            ItemPayload::UserMessage {
                content: command.input.clone(),
            },
        );
        let mut state = self.state.lock().expect("history lock");
        state.turn_order.push((thread_id, turn_id));
        state.turn_items.insert(turn_id, vec![input.clone()]);
        state.accepted += 1;
        drop(state);
        log_event(&self.log, "accepted");
        Ok(AcceptedTurn::new(
            command.trust.tenant_id.clone(),
            thread_id,
            turn_id,
            koduck_ai::domain::LeaseGeneration::initial(),
            input,
        ))
    }

    fn append(&mut self, turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        let mut state = self.state.lock().expect("history lock");
        let items = state
            .turn_items
            .get_mut(&turn.turn_id)
            .ok_or(HistoryError::NotFound)?;
        let durable = Item::new(items.len() as u64 + 1, item.into_payload());
        items.push(durable.clone());
        drop(state);
        log_event(&self.log, "appended");
        Ok(durable)
    }

    fn append_tool_projection(
        &mut self,
        turn: &AcceptedTurn,
        items: Vec<NewItem>,
    ) -> Result<Vec<Item>, HistoryError> {
        let mut state = self.state.lock().expect("history lock");
        let persisted = state
            .turn_items
            .get_mut(&turn.turn_id)
            .ok_or(HistoryError::NotFound)?;
        let first = persisted.len() as u64 + 1;
        let mut durable = Vec::with_capacity(items.len());
        for (offset, item) in items.into_iter().enumerate() {
            let item = Item::new(first + offset as u64, item.into_payload());
            persisted.push(item.clone());
            durable.push(item);
        }
        drop(state);
        log_event(&self.log, "projection_appended");
        Ok(durable)
    }

    fn replay(&self, _tenant_id: &TenantId, turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        self.state
            .lock()
            .expect("history lock")
            .turn_items
            .get(&turn_id)
            .cloned()
            .ok_or(HistoryError::NotFound)
    }
}

/// Provider double capturing every `ModelInput` and completing each stream.
#[derive(Clone)]
pub(crate) struct RecordingProvider {
    pub(crate) inputs: Rc<RefCell<Vec<ModelInput>>>,
}

impl RecordingProvider {
    pub(crate) fn new() -> Self {
        Self {
            inputs: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// Returns every provider request input seen so far, in order.
    pub(crate) fn recorded(&self) -> Vec<ModelInput> {
        self.inputs.borrow().clone()
    }
}

impl Default for RecordingProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelProvider for RecordingProvider {
    fn stream(
        &mut self,
        input: ModelInput,
    ) -> Result<koduck_ai::application::ProviderStream<'_>, ProviderError> {
        self.inputs.borrow_mut().push(input);
        Ok(Box::new(
            vec![
                ProviderEvent::Delta("A".to_owned()),
                ProviderEvent::Usage(Usage::new(1, 1).expect("valid usage")),
                ProviderEvent::Completed,
            ]
            .into_iter(),
        ))
    }
}

/// One seeded two-Turn Thread: a terminal first Turn with a post-terminal
/// correction of its user root, followed by a resumed second Turn.
pub(crate) struct SeededThread {
    pub(crate) first: AcceptedTurn,
    pub(crate) second: AcceptedTurn,
    pub(crate) corrected_item_id: ItemId,
}

/// Seeds the two-Turn Thread fixture through the production executor.
pub(crate) fn seed_two_turn_thread(executor: &SqlxPostgresExecutor) -> SeededThread {
    let owner = TrustContext::new(
        TenantId::new(format!("ci-{}", uuid::Uuid::new_v4())).expect("unique tenant"),
        "owner",
    )
    .expect("owner trust context");
    let first = executor
        .accept_initial(&TurnCommand::new(owner.clone(), None, "draft").expect("valid command"))
        .expect("accept first Turn");
    executor
        .append(
            &first,
            NewItem::AgentMessageDelta {
                content: "answer".to_owned(),
            },
        )
        .expect("append assistant delta");
    executor
        .append(
            &first,
            NewItem::Terminal(TerminalOutcome::Completed {
                usage: Usage::new(1, 1).expect("valid usage"),
            }),
        )
        .expect("append terminal");
    let correction = executor
        .correct(
            CorrectionCommand::new(
                owner.clone(),
                first.thread_id,
                first.turn_id,
                ItemId::new(),
                first.input.item_id,
                "revised",
            )
            .expect("valid correction command"),
        )
        .expect("admit post-terminal correction");
    let second = executor
        .accept_initial(
            &TurnCommand::new(owner.clone(), Some(first.thread_id), "second")
                .expect("valid resumed command"),
        )
        .expect("accept second Turn");
    SeededThread {
        first,
        second,
        corrected_item_id: correction.item_id,
    }
}

/// Asserts the complete provenance of every returned group against replay.
pub(crate) fn assert_grouped_provenance(
    executor: &SqlxPostgresExecutor,
    seeded: &SeededThread,
    groups: &[PriorTurnHistory],
    tenant: &TenantId,
) {
    assert_eq!(groups.len(), 2, "one group per source Turn");
    assert_eq!(groups[0].source_turn, seeded.first.turn_id);
    assert_eq!(groups[1].source_turn, seeded.second.turn_id);
    let replay = executor
        .replay(tenant, seeded.first.turn_id)
        .expect("first Turn replay");
    assert_eq!(
        groups[0]
            .rows
            .iter()
            .map(|row| row.item.item_id)
            .collect::<Vec<_>>(),
        replay.iter().map(|item| item.item_id).collect::<Vec<_>>(),
        "the complete first Turn — including the post-terminal correction — matches replay order"
    );
    for group in groups {
        assert!(!group.rows.is_empty());
        let expected = ProjectionScope::new(
            tenant.clone(),
            "owner",
            seeded.first.thread_id,
            group.source_turn,
        )
        .expect("valid expected scope");
        for row in &group.rows {
            assert_eq!(row.scope, expected);
        }
    }
}

/// Reads exactly one HTTP request (headers plus `Content-Length` body) on a
/// loopback connection and replies with the scripted raw response bytes.
pub(crate) fn sse_upstream(response: &'static [u8]) -> (String, std::sync::mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback upstream");
    let address = listener.local_addr().expect("loopback address");
    let (sender, receiver) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept provider request");
        let captured = read_one_request(&mut stream);
        sender.send(captured).expect("request receiver lives");
        stream.write_all(response).expect("write scripted response");
    });
    (format!("http://{address}"), receiver)
}

/// Serves the scripted responses to sequentially arriving requests, each on
/// its own `Connection: close` connection, and returns the captured bodies.
/// When `sync_after_first` is supplied, the upstream thread joins that
/// barrier right after writing the first response, pairing with the runner
/// thread for mid-Turn synchronization.
pub(crate) fn scripted_sse_upstream(
    responses: Vec<Vec<u8>>,
    log: Option<EventLog>,
    label: &str,
    sync_after_first: Option<Arc<std::sync::Barrier>>,
) -> (String, std::sync::mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback upstream");
    let address = listener.local_addr().expect("loopback address");
    let (sender, receiver) = std::sync::mpsc::channel();
    let label = label.to_owned();
    thread::spawn(move || {
        for (index, response) in responses.into_iter().enumerate() {
            let (mut stream, _) = listener.accept().expect("accept provider request");
            let captured = read_one_request(&mut stream);
            if let Some(log) = &log {
                log_event(log, &format!("{label}_request_{}", index + 1));
            }
            sender.send(captured).expect("request receiver lives");
            stream
                .write_all(&response)
                .expect("write scripted response");
            if index == 0
                && let Some(barrier) = &sync_after_first
            {
                barrier.wait();
            }
        }
    });
    (format!("http://{address}"), receiver)
}

/// Reads one complete HTTP/1.1 request and returns its body.
fn read_one_request(stream: &mut std::net::TcpStream) -> String {
    let mut captured: Vec<u8> = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = stream.read(&mut chunk).expect("read provider request");
        captured.extend_from_slice(&chunk[..read]);
        if request_complete(&captured) {
            break;
        }
    }
    let text = String::from_utf8(captured).expect("request is UTF-8");
    text.split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap_or_default()
}

/// Reports whether the captured bytes hold a complete HTTP/1.1 request.
pub(crate) fn request_complete(received: &[u8]) -> bool {
    let Some(headers_end) = received.windows(4).position(|w| w == b"\r\n\r\n") else {
        return false;
    };
    let headers = String::from_utf8_lossy(&received[..headers_end]);
    let length = headers
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    received.len() >= headers_end + 4 + length
}

/// One successful streamed completion ending in `stop` plus usage.
pub(crate) fn completion_frames(content: &str) -> Vec<u8> {
    sse_response(vec![
        format!(
            "data: {}",
            serde_json::json!({
                "choices": [{
                    "delta": { "content": content },
                    "finish_reason": "stop"
                }]
            })
        ),
        r#"data: {"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#.to_owned(),
        "data: [DONE]".to_owned(),
    ])
}

/// One streamed Tool-call round finishing in `tool_calls`.
pub(crate) fn tool_round_frames(content: &str, tool: &str) -> Vec<u8> {
    sse_response(vec![
        format!(
            "data: {}",
            serde_json::json!({
                "choices": [{
                    "delta": {
                        "content": content,
                        "tool_calls": [{
                            "index": 0,
                            "function": { "name": tool, "arguments": "{}" }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            })
        ),
        "data: [DONE]".to_owned(),
    ])
}

/// One immediate HTTP failure response with no body.
pub(crate) fn http_failure_response(status: u16) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status} Internal Server Error\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    )
    .into_bytes()
}

/// Wraps the frames of one scripted SSE response in a `Connection: close`
/// HTTP response whose body ends at EOF.
fn sse_response(frames: Vec<String>) -> Vec<u8> {
    let mut response =
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n".to_vec();
    for frame in frames {
        response.extend_from_slice(frame.as_bytes());
        response.extend_from_slice(b"\n\n");
    }
    response
}

/// Builds the prepared-view input whose wire translation is asserted.
pub(crate) fn corrected_transport_input() -> ModelInput {
    let user = ProviderHistoryItemFixture::user(1, "revised");
    let delta = ProviderHistoryItemFixture::delta(2, "answer");
    let usage = koduck_ai::application::ProviderHistoryItem {
        item_id: ItemId::new(),
        sequence: 3,
        kind: koduck_ai::application::ProviderHistoryKind::Usage,
        source_item_id: ItemId::new(),
        value: koduck_ai::application::ProviderHistoryValue::NonText(ItemPayload::Usage(
            Usage::new(1, 1).expect("valid usage"),
        )),
    };
    let terminal = koduck_ai::application::ProviderHistoryItem {
        item_id: ItemId::new(),
        sequence: 4,
        kind: koduck_ai::application::ProviderHistoryKind::Terminal,
        source_item_id: ItemId::new(),
        value: koduck_ai::application::ProviderHistoryValue::NonText(ItemPayload::Terminal(
            TerminalOutcome::Completed {
                usage: Usage::new(1, 1).expect("valid usage"),
            },
        )),
    };
    ModelInput {
        tenant_id: TenantId::new(TENANT).expect("valid tenant"),
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
        input: "next".to_owned(),
        history: vec![user, delta, usage, terminal],
        tool_rounds: vec![ToolRound {
            assistant_content: String::new(),
            calls: vec![koduck_ai::application::CommittedToolCall {
                call: koduck_ai::application::ModelToolCall {
                    name: "fixture.tool".to_owned(),
                    arguments: "{}".to_owned(),
                },
                result: ModelToolResult {
                    content: "ok".to_owned(),
                    is_error: false,
                },
            }],
        }],
    }
}

/// Textual provider-history view fixtures for transport assertions.
pub(crate) struct ProviderHistoryItemFixture;

impl ProviderHistoryItemFixture {
    /// One textual view at the given sequence with fresh identities.
    pub(crate) fn textual(
        sequence: u64,
        kind: koduck_ai::application::ProviderHistoryKind,
        content: &str,
    ) -> koduck_ai::application::ProviderHistoryItem {
        let item_id = ItemId::new();
        koduck_ai::application::ProviderHistoryItem {
            item_id,
            sequence,
            kind,
            source_item_id: item_id,
            value: koduck_ai::application::ProviderHistoryValue::Text(content.to_owned()),
        }
    }

    /// One user-message view.
    pub(crate) fn user(
        sequence: u64,
        content: &str,
    ) -> koduck_ai::application::ProviderHistoryItem {
        Self::textual(
            sequence,
            koduck_ai::application::ProviderHistoryKind::UserMessage,
            content,
        )
    }

    /// One agent-delta view.
    pub(crate) fn delta(
        sequence: u64,
        content: &str,
    ) -> koduck_ai::application::ProviderHistoryItem {
        Self::textual(
            sequence,
            koduck_ai::application::ProviderHistoryKind::AgentMessageDelta,
            content,
        )
    }
}

/// Drains the observer events of one executed turn for publication asserts.
pub(crate) fn observed_items(events: &[TurnStreamEvent]) -> Vec<Item> {
    events
        .iter()
        .filter_map(|event| match event {
            TurnStreamEvent::Item { item, .. } => Some(item.clone()),
            TurnStreamEvent::Started { .. } => None,
        })
        .collect()
}

/// Returns the terminal outcomes of one replayed Turn.
pub(crate) fn terminals_of(items: &[Item]) -> Vec<TerminalOutcome> {
    items
        .iter()
        .filter_map(|item| match &item.payload {
            ItemPayload::Terminal(outcome) => Some(outcome.clone()),
            _ => None,
        })
        .collect()
}

/// Waits for the channel to yield every expected captured request body.
pub(crate) fn captured_bodies(
    receiver: &std::sync::mpsc::Receiver<String>,
    count: usize,
) -> Vec<String> {
    let mut bodies = Vec::with_capacity(count);
    while bodies.len() < count {
        let body = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("upstream captured the request");
        bodies.push(body);
    }
    bodies
}

/// Parses one captured request body as a JSON document.
pub(crate) fn parse_request(body: &str) -> serde_json::Value {
    serde_json::from_str(body).expect("request body is JSON")
}

/// Returns the `messages` array of one captured request body.
pub(crate) fn messages_of(body: &str) -> Vec<serde_json::Value> {
    parse_request(body)["messages"]
        .as_array()
        .expect("messages array")
        .clone()
}
