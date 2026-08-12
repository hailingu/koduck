// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

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
    assert!(
        workflow.contains("pull_request:")
            && workflow.contains("- dev")
            && workflow.contains("- docs/adr/ADR-0002-required-ai-ci-postgres-verification.md")
            && workflow.contains("- docs/adr/INDEX.md")
            && workflow.contains("postgres:")
            && workflow.contains("KODUCK_AI_TEST_DATABASE_URL")
            && workflow.contains("timeout-minutes:")
            && !workflow.contains("upload-artifact"),
        "workflow must bind dev pull requests to bounded disposable PostgreSQL verification"
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
