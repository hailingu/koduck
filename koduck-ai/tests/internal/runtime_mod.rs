// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Focused runtime-trust tests for the startup and identity-sealing edge.

use std::time::Duration;

use axum::http::{HeaderMap, HeaderName, HeaderValue};

use super::{RuntimeError, database_setup_attempt, trust_context};

#[tokio::test]
async fn database_setup_attempt_maps_deadline_expiration() {
    let result = database_setup_attempt(Duration::from_millis(1), async {
        tokio::time::sleep(Duration::from_secs(1)).await;
        Ok::<_, sqlx::Error>(())
    })
    .await;

    assert!(matches!(result, Err(RuntimeError::DatabaseTimeout)));
}

fn identity_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("x-koduck-tenant-id"),
        HeaderValue::from_static("tenant-a"),
    );
    headers.insert(
        HeaderName::from_static("x-koduck-subject-id"),
        HeaderValue::from_static("subject-a"),
    );
    headers
}

fn with_scope_header(mut headers: HeaderMap, scopes: &str) -> HeaderMap {
    headers.insert(
        HeaderName::from_static("x-koduck-approval-scopes"),
        HeaderValue::from_str(scopes).expect("test scope header is valid header text"),
    );
    headers
}

fn with_scope_header_bytes(mut headers: HeaderMap, scopes: &[u8]) -> HeaderMap {
    headers.insert(
        HeaderName::from_static("x-koduck-approval-scopes"),
        HeaderValue::from_bytes(scopes).expect("test scope header is valid header bytes"),
    );
    headers
}

#[test]
fn trust_context_seals_gateway_validated_approval_scopes() {
    let headers = with_scope_header(identity_headers(), "ai.tool.approve,audit.read");
    let trust = trust_context(&headers).expect("gateway-validated identity is accepted");
    assert!(trust.has_approval_scope("ai.tool.approve"));
    assert!(trust.has_approval_scope("audit.read"));
    assert!(!trust.has_approval_scope("ai.tool.execute"));
}

#[test]
fn trust_context_without_scope_header_carries_no_approval_scope() {
    let headers = identity_headers();
    let trust = trust_context(&headers).expect("identity without scopes is accepted");
    assert!(!trust.has_approval_scope("ai.tool.approve"));
}

#[test]
fn trust_context_rejects_malformed_gateway_scope_header() {
    let oversize_token = format!("{}.{}", "a".repeat(128), "b");
    let too_many_scopes = vec!["scope.n"; 17].join(",");
    for malformed in [
        String::new(),
        ",ai.tool.approve".to_owned(),
        "ai.tool.approve,".to_owned(),
        "ai.tool approve".to_owned(),
        " ai.tool.approve".to_owned(),
        "ai.tool.approve ".to_owned(),
        "ai.tool.approve ,audit.read".to_owned(),
        "\tai.tool.approve".to_owned(),
        oversize_token,
        too_many_scopes,
    ] {
        let headers = with_scope_header(identity_headers(), &malformed);
        assert!(
            trust_context(&headers).is_none(),
            "malformed gateway scope header must invalidate identity: {malformed:?}"
        );
    }
    // Obs-text bytes survive header parsing as valid UTF-8 but are not
    // valid scope tokens, so the validator must still reject them.
    let headers = with_scope_header_bytes(identity_headers(), "范围.工具".as_bytes());
    assert!(trust_context(&headers).is_none());
}

#[test]
fn trust_context_scope_header_is_tenant_independent() {
    // The sealed scopes attach only to the gateway-validated identity; a
    // different tenant header still produces that tenant's context.
    let mut headers = with_scope_header(identity_headers(), "ai.tool.approve");
    headers.insert(
        HeaderName::from_static("x-koduck-tenant-id"),
        HeaderValue::from_static("tenant-b"),
    );
    let trust = trust_context(&headers).expect("identity is accepted");
    assert_eq!(trust.tenant_id.as_str(), "tenant-b");
    assert!(trust.has_approval_scope("ai.tool.approve"));
}

#[test]
fn trust_context_rejects_invalid_tenant_even_with_valid_scopes() {
    let mut headers = with_scope_header(identity_headers(), "ai.tool.approve");
    headers.insert(
        HeaderName::from_static("x-koduck-tenant-id"),
        HeaderValue::from_static("  "),
    );
    assert!(trust_context(&headers).is_none());
}
