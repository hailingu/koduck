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
            migration.contains(&format!("CREATE TABLE {relation}")),
            "migration defines {relation}"
        );
    }
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
