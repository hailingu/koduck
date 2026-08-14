// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn domain_and_application_dependencies_are_inward() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let application = crate_root.join("src/application");
    assert!(application.is_dir(), "src/application must exist");

    let mut violations = Vec::new();
    inspect_rust_files(&crate_root.join("src/domain"), &mut violations);
    inspect_rust_files(&application, &mut violations);

    assert!(
        violations.is_empty(),
        "forbidden outward dependencies: {}",
        violations.join(", ")
    );
}

#[test]
fn cand_2_policy_dependencies_are_inward_and_unbypassable() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();
    inspect_rust_files(&crate_root.join("src/domain"), &mut violations);
    inspect_rust_files(&crate_root.join("src/application"), &mut violations);
    assert!(
        violations.is_empty(),
        "CAND-2 policy has outward dependencies: {}",
        violations.join(", ")
    );

    let mut production = String::new();
    collect_text(&crate_root.join("src"), &mut production);
    assert_eq!(
        production.matches("impl IsolatedExecutor for").count(),
        1,
        "exactly one production executor implementation must exist"
    );
    assert!(production.contains("impl IsolatedExecutor for DisabledExecutor"));
    for forbidden in ["std::process::Command", "tokio::process::Command"] {
        assert!(
            !production.contains(forbidden),
            "CAND-2 contains forbidden direct execution API {forbidden}"
        );
    }
}

#[test]
fn cand_2_has_no_direct_or_legacy_execution_fallback() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut production = String::new();
    collect_text(&crate_root.join("src"), &mut production);
    production.push_str(
        &fs::read_to_string(crate_root.join("Cargo.toml")).expect("crate manifest is readable"),
    );
    assert!(production.contains("DisabledExecutor"));
    assert!(production.contains("ExecutorUnavailable"));
    assert!(production.contains("DispatchPermit"));
    assert_eq!(
        production.matches("self.executor.execute(&permit").count(),
        1,
        "only the coordinator may present the opaque dispatch permit"
    );
    for forbidden in [
        "koduck-quant",
        "native_tool_loop",
        "std::process::Command",
        "tokio::process::Command",
    ] {
        assert!(
            !production.contains(forbidden),
            "production graph contains forbidden direct or legacy path {forbidden}"
        );
    }
}

#[test]
fn cand_2_digest_and_turn_budget_are_stable_authorities() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let execution = fs::read_to_string(crate_root.join("src/domain/execution.rs"))
        .expect("CAND-2 domain execution source is readable");
    let application_execution = fs::read_to_string(crate_root.join("src/application/execution.rs"))
        .expect("CAND-2 application execution source is readable");

    assert!(!execution.contains("DefaultHasher"));
    assert!(!execution.contains("OnceLock"));
    assert!(execution.contains("Arc<Mutex<TurnAuthorityState>>"));
    assert!(!execution.contains("Weak<Mutex<TurnAuthorityState>>"));
    assert!(!execution.contains("pub struct TurnAuthorityCatalog"));
    assert!(!execution.contains("pub struct AttemptBudget"));
    assert!(!execution.contains("pub struct TurnExecutionRegistry"));
    assert!(execution.contains("pub struct TurnExecutionAuthority"));
    let mut production = String::new();
    collect_text(&crate_root.join("src"), &mut production);
    // Count the bare identifier regardless of call syntax — method `.x(`, UFCS
    // `Type::x(`, or whitespace `x (` all register — so a second dispatch claim
    // cannot hide behind phrasing, and a longer identifier such as
    // `claim_dispatched` does not match. The expected count includes each
    // method's single definition plus its call sites.
    assert_eq!(
        count_identifier_tokens(&production, "claim_dispatch"),
        2,
        "claim_dispatch must have exactly one definition and one coordinator call site"
    );
    assert_eq!(
        count_identifier_tokens(&production, "mirror_terminal"),
        3,
        "mirror_terminal must have exactly one definition and two conditional-commit call sites"
    );
    assert!(!execution.contains("pub fn authority_for_binding"));
    assert!(
        !execution.contains("    pub fn resolve(\n"),
        "D-6 resolution must be reachable only through authenticated application policy"
    );
    assert!(
        application_execution.contains("AttemptCommitResult"),
        "the durable commit port must represent a won or existing canonical terminal"
    );
    assert!(
        production.contains("AttemptCommitError::Conflict"),
        "the durable commit port must represent a conflicting terminal race"
    );
    assert_eq!(
        count_identifier_tokens(&production, "allocate_attempt"),
        2,
        "allocate_attempt must have exactly one definition and one lease-validating preparer call site"
    );
    assert!(
        !application_execution.contains("pub const fn new(lease: L)"),
        "preparers must be created by the injected Tool execution runtime"
    );
    assert!(
        application_execution.contains("approval: Option<&ApprovalRequest>"),
        "the execution boundary must preserve the policy-authorized no-D-6 path"
    );
    assert!(
        !application_execution.contains("pub trait ApprovalAuthorizer"),
        "untrusted callers must not be able to supply their own C-7 authority"
    );
    assert!(!application_execution.contains("can_approve_tools: bool"));
    assert!(
        !application_execution.contains("pub struct ToolExecutionAuthorityStore"),
        "runtime assembly must own the sole Turn authority store"
    );
    assert!(application_execution.contains("ToolExecutionAuthorityRoot"));
    assert!(application_execution.contains("pub(crate) fn new(root: &ToolExecutionAuthorityRoot)"));
    assert!(!execution.contains("#[derive(Clone, Debug)]\npub struct TurnExecutionAuthority"));
    assert!(!execution.contains("#[derive(Clone, Debug)]\npub struct ApprovalRequest"));
    assert!(
        !execution.contains("#[derive(Clone, Debug, Eq, PartialEq)]\npub struct ExecutionAttempt")
    );
    assert!(
        !execution
            .contains("pub fn start(\n        &mut self,\n        approval: &ApprovalRequest")
    );
    assert!(
        !execution.contains("pub fn finish(") && !execution.contains("pub(crate) fn finish("),
        "finish must remain private so only mirror_terminal can mutate the D-7 terminal"
    );
}

#[test]
fn cand_2_authority_issuers_are_not_public_extension_points() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let policy = fs::read_to_string(crate_root.join("src/application/policy.rs"))
        .expect("CAND-2 policy source is readable");
    let execution = fs::read_to_string(crate_root.join("src/application/execution.rs"))
        .expect("CAND-2 execution source is readable");
    let authority = fs::read_to_string(crate_root.join("src/domain/execution/authority.rs"))
        .expect("CAND-2 authority catalog source is readable");

    assert!(
        !policy.contains("pub trait ToolPolicyConfiguration"),
        "untrusted callers must not be able to provide descriptor/profile authority"
    );
    assert!(
        !policy.contains("pub struct ToolAuthorizationService"),
        "only trusted runtime assembly may construct the C-5 sealing service"
    );
    assert!(
        !execution.contains("pub const fn new(authorizer: A)"),
        "untrusted callers must not be able to inject a self-asserted C-7 authorizer"
    );
    assert!(
        !execution.contains("pub fn new() -> Self"),
        "no public zero-argument constructor may create a second Turn authority root"
    );
    assert!(
        !execution.contains("OnceLock"),
        "Turn authority must be explicitly owned by runtime assembly, not a global singleton"
    );
    assert!(
        execution.contains("ToolExecutionAuthorityRoot"),
        "runtime assembly must own one explicit Turn authority root"
    );
    assert!(
        !execution.contains("reclaim_terminal_turn") && !authority.contains("reclaim_terminal("),
        "Turn authority must remain retained until T-3 can prove canonical terminal state and prevent resurrection"
    );

    // The crate-visible policy and approval setters carry unique names so the
    // sole-call-site count is syntax-independent: each must be reachable only
    // from its trusted service, never from a Tool/MCP or approval-transport
    // adapter. The expected count is one definition plus one service call site.
    let mut production = String::new();
    collect_text(&crate_root.join("src"), &mut production);
    assert_eq!(
        count_identifier_tokens(&production, "authorize_policy"),
        2,
        "authorize_policy must have exactly one definition and one ToolAuthorizationService caller"
    );
    assert_eq!(
        count_identifier_tokens(&production, "apply_validated_decision"),
        2,
        "apply_validated_decision must have exactly one definition and one ApprovalDecisionService caller"
    );
}

fn inspect_rust_files(path: &Path, violations: &mut Vec<String>) {
    for entry in fs::read_dir(path).expect("architecture source directory exists") {
        let entry = entry.expect("source directory entry is readable");
        let path = entry.path();
        if path.is_dir() {
            inspect_rust_files(&path, violations);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            let source = fs::read_to_string(&path).expect("Rust source is readable");
            for forbidden in ["axum", "sqlx", "serde_json", "adapters::"] {
                if source.contains(forbidden) {
                    violations.push(format!("{} contains {forbidden}", path.display()));
                }
            }
        }
    }
}

#[test]
fn cand_1_has_no_legacy_or_external_history_fallback() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut production = String::new();
    collect_text(&crate_root.join("src"), &mut production);
    production.push_str(
        &fs::read_to_string(crate_root.join("Cargo.toml")).expect("manifest is readable"),
    );

    for forbidden in [
        "LlmProvider",
        "/api/ai/chat",
        "APISIX",
        "koduck-memory",
        "koduck-multitask",
        "memory_client",
        "multitask_client",
    ] {
        assert!(
            !production.contains(forbidden),
            "CAND-1 production graph contains forbidden fallback identifier {forbidden}"
        );
    }
    assert_eq!(
        production
            .matches("TurnHistory for PostgresTurnHistory")
            .count(),
        1,
        "exactly one production TurnHistory implementation must be PostgreSQL"
    );
    assert_eq!(
        production.matches("impl TurnHistory for").count()
            + production
                .matches("TurnHistory for PostgresTurnHistory")
                .count(),
        1,
        "no alternate production history implementation is permitted"
    );

    let migration = fs::read_to_string(crate_root.join("migrations/0001_cand_1_history.sql"))
        .expect("canonical PostgreSQL migration exists");
    for relation in ["threads", "turns", "turn_items", "turn_leases"] {
        assert!(
            migration.contains(&format!("CREATE TABLE {relation}"))
                || migration.contains(&format!("CREATE TABLE IF NOT EXISTS {relation}")),
            "migration defines {relation}"
        );
    }
}

#[test]
fn production_runtime_wires_reviewed_failure_and_streaming_guards() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let sqlx =
        fs::read_to_string(crate_root.join("src/adapters/history/postgres/sqlx_executor.rs"))
            .expect("SQLx executor source is readable");
    assert!(
        sqlx.contains("tokio::time::timeout") && sqlx.contains("AppendPolicy::cand_1().deadline()"),
        "production append must enforce the approved two-second deadline"
    );

    let provider = fs::read_to_string(crate_root.join("src/adapters/provider/mod.rs"))
        .expect("provider adapter source is readable");
    assert!(
        !provider.contains("block_on(response.text())") && provider.contains("response.chunk()"),
        "provider transport must consume upstream frames incrementally"
    );

    let runtime = fs::read_to_string(crate_root.join("src/runtime/mod.rs"))
        .expect("runtime source is readable");
    let http = fs::read_to_string(crate_root.join("src/adapters/http/mod.rs"))
        .expect("HTTP adapter source is readable");
    let history = fs::read_to_string(crate_root.join("src/adapters/history/postgres.rs"))
        .expect("PostgreSQL history source is readable");
    assert!(
        !runtime.contains("Arc<Mutex<HttpAdapter") && !runtime.contains("adapter.lock()"),
        "request execution must not hold a turn-wide router mutex"
    );
    assert!(
        runtime.contains("start_reconciliation_worker")
            && runtime.contains("handle_stream")
            && http.contains("execute_stream")
            && history.contains("start_turn_liveness"),
        "runtime must start liveness reconciliation and incremental SSE execution"
    );
}

#[test]
fn production_io_and_background_work_are_bounded() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let sqlx =
        fs::read_to_string(crate_root.join("src/adapters/history/postgres/sqlx_executor.rs"))
            .expect("SQLx executor source is readable");
    assert!(
        !sqlx.contains("self.runtime.block_on(operation)")
            && sqlx.contains("AppendPolicy::cand_1().deadline()"),
        "every synchronous PostgreSQL operation must use the approved deadline"
    );

    let history = fs::read_to_string(crate_root.join("src/adapters/history/postgres.rs"))
        .expect("PostgreSQL history source is readable");
    let recovery = fs::read_to_string(crate_root.join("src/adapters/history/postgres/recovery.rs"))
        .expect("PostgreSQL recovery source is readable");
    assert!(
        history.contains("MAX_BACKGROUND_WORKERS")
            && history.contains("BackgroundAdmission")
            && recovery.contains("admission.try_acquire"),
        "lease renewal and recovery must share bounded background admission"
    );

    let provider = fs::read_to_string(crate_root.join("src/adapters/provider/mod.rs"))
        .expect("provider adapter source is readable");
    let runtime = fs::read_to_string(crate_root.join("src/runtime/mod.rs"))
        .expect("runtime source is readable");
    for timeout in [
        "PROVIDER_RESPONSE_HEADER_TIMEOUT",
        "PROVIDER_STREAM_IDLE_TIMEOUT",
        "PROVIDER_TOTAL_TIMEOUT",
    ] {
        assert!(provider.contains(timeout), "provider must define {timeout}");
    }
    assert!(
        provider.contains("tokio::time::timeout") && runtime.contains(".connect_timeout("),
        "provider requests must have connect, header, idle, and total deadlines"
    );
    assert!(runtime.contains("async fn database_setup_attempt"));
    assert_eq!(
        runtime.matches("database_deadline,").count(),
        2,
        "both PostgreSQL startup operations must use the shared bounded helper"
    );
}

#[test]
fn application_api_does_not_expose_an_unwired_unpublished_buffer() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let application_module = fs::read_to_string(crate_root.join("src/application/mod.rs"))
        .expect("application module is readable");
    let durability = fs::read_to_string(crate_root.join("src/application/durability.rs"))
        .expect("durability source is readable");

    assert!(
        !application_module.contains("UnpublishedBuffer")
            && !durability.contains("struct UnpublishedBuffer"),
        "the application API must describe the production append policy, not expose an unused buffer"
    );
}

#[test]
fn postgres_executor_stays_within_the_approved_file_size_limit() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let executor =
        fs::read_to_string(crate_root.join("src/adapters/history/postgres/sqlx_executor.rs"))
            .expect("SQLx executor source is readable");

    assert!(
        executor.lines().count() <= 800,
        "sqlx_executor.rs must be split or governed by an approved engineering exception"
    );
}

// ADR: docs/adr/ADR-0002-required-ai-ci-postgres-verification.md
#[test]
fn required_ci_maps_every_routed_command_and_postgres_boundary() {
    let repository_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("koduck-ai belongs to the repository workspace")
        .to_path_buf();
    let workflow_path = repository_root.join(".github/workflows/koduck-ai.yml");
    let workflow = fs::read_to_string(&workflow_path).unwrap_or_else(|error| {
        panic!(
            "required Koduck AI workflow must exist at {}: {error}",
            workflow_path.display()
        )
    });

    for check_name in [
        "name: koduck-ai-format",
        "name: koduck-ai-clippy",
        "name: koduck-ai-test-postgres",
    ] {
        assert!(
            workflow.contains(check_name),
            "workflow must expose required check {check_name}"
        );
    }
    for command in [
        "cargo fmt --all --check",
        "cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings",
        "cargo test -p koduck-ai --all-targets --all-features",
    ] {
        assert!(
            workflow.contains(command),
            "workflow must execute routed command {command}"
        );
    }
    let format_job = workflow
        .split_once("  format:")
        .and_then(|(_, jobs)| jobs.split_once("\n  clippy:"))
        .map(|(job, _)| job)
        .expect("the required format job has a bounded workflow section");
    for command in [
        "npm test --prefix tools/governance-validator",
        "npm run validate --prefix tools/governance-validator",
    ] {
        assert!(
            format_job.contains(command),
            "required koduck-ai-format check must execute routed governance command {command}"
        );
    }
    assert!(
        workflow.contains("pull_request:")
            && workflow.contains("- dev")
            && !workflow.contains("\n    paths:")
            && !workflow.contains("\n    paths-ignore:")
            && workflow.contains("postgres:")
            && workflow.contains("KODUCK_AI_TEST_DATABASE_URL")
            && workflow.contains("timeout-minutes:")
            && !workflow.contains("upload-artifact"),
        "every dev pull request must emit the required bounded PostgreSQL verification checks"
    );
    assert!(
        !workflow.contains("\n  push:"),
        "a task-branch push must not duplicate the pull-request checks for the same revision"
    );
}

// ADR: docs/adr/ADR-0002-required-ai-ci-postgres-verification.md
#[test]
fn ci_pins_the_selected_rust_toolchain() {
    let repository_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("koduck-ai belongs to the repository workspace")
        .to_path_buf();
    let workspace_manifest = fs::read_to_string(repository_root.join("Cargo.toml"))
        .expect("workspace manifest is readable");
    let workflow = fs::read_to_string(repository_root.join(".github/workflows/koduck-ai.yml"))
        .expect("Koduck AI workflow is readable");

    assert!(
        workspace_manifest.contains("rust-version = \"1.95\""),
        "workspace metadata must select Rust 1.95"
    );
    assert_eq!(
        workflow.matches("toolchain: \"1.95\"").count(),
        3,
        "every Koduck AI CI job must pin Rust 1.95"
    );
}

#[test]
fn cargo_metadata_matches_the_repository_license() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repository_root = crate_root
        .parent()
        .expect("koduck-ai belongs to the repository workspace");
    let license = fs::read_to_string(repository_root.join("LICENSE")).expect("root license");

    assert!(license.starts_with("MIT License"));
    assert!(
        env!("CARGO_PKG_LICENSE") == "MIT",
        "Cargo package metadata must identify the repository's MIT license"
    );
}

// ADR: docs/adr/ADR-0002-required-ai-ci-postgres-verification.md
#[test]
fn postgres_claims_use_the_production_executor_instead_of_source_inspection() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let integration = fs::read_to_string(crate_root.join("tests/postgres_subject_ownership.rs"))
        .expect("PostgreSQL integration test source is readable");

    assert!(
        integration.contains("KODUCK_AI_TEST_DATABASE_URL")
            && integration.contains("SqlxPostgresExecutor")
            && integration.contains("0001_cand_1_history.sql")
            && integration.contains("production_postgres_contract"),
        "PostgreSQL claims must run the production migration and SQLx executor"
    );
    assert!(
        !integration
            .contains("include_str!(\"../src/adapters/history/postgres/sqlx_executor.rs\")")
            && !integration.contains("executor.contains("),
        "source-string assertions cannot establish PostgreSQL behavior"
    );
}

fn collect_text(path: &Path, output: &mut String) {
    for entry in fs::read_dir(path).expect("production source directory exists") {
        let entry = entry.expect("source directory entry is readable");
        let path = entry.path();
        if path.is_dir() {
            collect_text(&path, output);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            output.push_str(&fs::read_to_string(path).expect("production source is readable"));
        }
    }
}

/// Counts `identifier` as a complete Rust token in `source`, independent of the
/// surrounding call syntax, so the single-dispatch boundary cannot be bypassed
/// by phrasing a call as a method `.x(`, UFCS `Type::x(`, or whitespace `x (`,
/// and a longer identifier such as `claim_dispatched` does not match.
fn count_identifier_tokens(source: &str, identifier: &str) -> usize {
    fn is_ident_continue(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || byte == b'_'
    }
    let bytes = source.as_bytes();
    let mut count = 0;
    let mut search_from = 0;
    while let Some(relative) = source[search_from..].find(identifier) {
        let start = search_from + relative;
        let end = start + identifier.len();
        let left_is_boundary = start == 0 || !is_ident_continue(bytes[start - 1]);
        let right_is_boundary = end == bytes.len() || !is_ident_continue(bytes[end]);
        if left_is_boundary && right_is_boundary {
            count += 1;
        }
        search_from = end;
    }
    count
}
