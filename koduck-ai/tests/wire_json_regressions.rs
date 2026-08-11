// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use koduck_ai::adapters::http::{HttpAdapter, HttpMethod, HttpRequest, ServiceError, TurnService};
use koduck_ai::application::{TurnCommand, TurnResult};
use koduck_ai::domain::{
    Item, ItemPayload, TenantId, TerminalOutcome, TrustContext, TurnId, TurnStatus, Usage,
};

struct CapturingService {
    input: Option<String>,
    result: TurnResult,
}

impl TurnService for CapturingService {
    fn execute(&mut self, command: TurnCommand) -> Result<TurnResult, ServiceError> {
        self.input = Some(command.input);
        Ok(self.result.clone())
    }

    fn interrupt(&mut self, _trust: &TrustContext, _turn_id: TurnId) -> Result<(), ServiceError> {
        Err(ServiceError::NotFound)
    }
}

fn trust() -> TrustContext {
    TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context")
}

fn completed_result(content: &str) -> TurnResult {
    let usage = Usage::zero();
    let delta = Item::new(
        2,
        ItemPayload::AgentMessageDelta {
            content: content.to_owned(),
        },
    );
    let terminal = Item::new(
        3,
        ItemPayload::Terminal(TerminalOutcome::Completed { usage }),
    );
    TurnResult {
        thread_id: koduck_ai::domain::ThreadId::new(),
        turn_id: TurnId::new(),
        status: TurnStatus::Completed,
        published: vec![delta.clone(), terminal.clone()],
        replay: vec![delta, terminal],
    }
}

fn post(path: &str, body: &str) -> HttpRequest {
    HttpRequest {
        method: HttpMethod::Post,
        path: path.to_owned(),
        content_type: Some("application/json".to_owned()),
        body: body.to_owned(),
        trust: Some(trust()),
    }
}

#[test]
fn request_accepts_unicode_solidus_and_surrogate_pair_escapes() {
    let service = CapturingService {
        input: None,
        result: completed_result("ok"),
    };
    let mut adapter = HttpAdapter::new(service);

    let response = adapter.handle(post(
        "/api/v1/ai/chat",
        r#"{"input":"\u4F60\u597D\/\uD83D\uDE03"}"#,
    ));

    assert_eq!(response.status, 200);
}

#[test]
fn response_escapes_the_complete_json_control_range() {
    let content = (0_u8..=31).map(char::from).collect::<String>();
    let mut adapter = HttpAdapter::new(CapturingService {
        input: None,
        result: completed_result(&content),
    });

    let sync = adapter.handle(post("/api/v1/ai/chat", r#"{"input":"hello"}"#));
    let document: serde_json::Value = serde_json::from_str(&sync.body).expect("valid sync JSON");
    assert_eq!(document["items"][0]["content"], content);

    let sse = adapter.handle(post("/api/v1/ai/chat/stream", r#"{"input":"hello"}"#));
    for data in sse
        .body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
    {
        serde_json::from_str::<serde_json::Value>(data).expect("valid SSE data JSON");
    }
}

#[test]
fn request_still_rejects_duplicate_and_unknown_fields() {
    let mut adapter = HttpAdapter::new(CapturingService {
        input: None,
        result: completed_result("ok"),
    });
    for body in [
        r#"{"input":"one","input":"two"}"#,
        r#"{"input":"one","unknown":"two"}"#,
    ] {
        let response = adapter.handle(post("/api/v1/ai/chat", body));
        assert_eq!(response.status, 400);
    }
}
