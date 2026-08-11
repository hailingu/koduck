// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use std::collections::BTreeMap;

use koduck_ai::runtime::RuntimeConfig;

fn complete_environment() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "KODUCK_AI_BIND_ADDR".to_owned(),
            "127.0.0.1:8080".to_owned(),
        ),
        (
            "KODUCK_AI_DATABASE_URL".to_owned(),
            "postgres://koduck@database/koduck".to_owned(),
        ),
        (
            "KODUCK_AI_OPENAI_BASE_URL".to_owned(),
            "https://provider.example/v1".to_owned(),
        ),
        (
            "KODUCK_AI_OPENAI_MODEL".to_owned(),
            "provider-model".to_owned(),
        ),
        (
            "KODUCK_AI_OPENAI_API_KEY".to_owned(),
            "not-a-real-secret".to_owned(),
        ),
    ])
}

#[test]
fn runtime_config_requires_postgres_and_provider_inputs() {
    let config = RuntimeConfig::from_environment(&complete_environment())
        .expect("complete runtime configuration is valid");

    assert_eq!(config.bind_addr().to_string(), "127.0.0.1:8080");
    assert_eq!(
        config.database_url(),
        "postgres://koduck@database/koduck"
    );
    assert_eq!(config.provider_base_url(), "https://provider.example/v1");
    assert_eq!(config.provider_model(), "provider-model");
    assert_eq!(config.provider_api_key(), "not-a-real-secret");
    assert!(!format!("{config:?}").contains("not-a-real-secret"));

    let mut missing_database = complete_environment();
    missing_database.remove("KODUCK_AI_DATABASE_URL");
    assert_eq!(
        RuntimeConfig::from_environment(&missing_database)
            .expect_err("database URL is required")
            .to_string(),
        "missing required environment variable KODUCK_AI_DATABASE_URL"
    );
}
