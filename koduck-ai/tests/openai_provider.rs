// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use koduck_ai::adapters::provider::{
    OpenAiCompatibleProvider, OpenAiProtocolTransport, OpenAiTransportError,
};
use koduck_ai::application::{ModelInput, ModelProvider, ProviderEvent};
use koduck_ai::domain::{TenantId, ThreadId, TurnId, Usage};

struct DeterministicProtocolServer {
    frames: Vec<String>,
}

impl OpenAiProtocolTransport for DeterministicProtocolServer {
    fn chat_completion_frames(
        &mut self,
        _input: &ModelInput,
    ) -> Result<Vec<String>, OpenAiTransportError> {
        Ok(self.frames.clone())
    }
}

fn model_input() -> ModelInput {
    ModelInput {
        tenant_id: TenantId::new("tenant-a").expect("valid tenant"),
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
        input: "hello".to_owned(),
        history: Vec::new(),
    }
}

#[test]
fn openai_compatible_protocol_maps_to_owned_events() {
    let server = DeterministicProtocolServer {
        frames: vec![
            r#"data: {"choices":[{"delta":{"content":"A"}}]}"#.to_owned(),
            r#"data: {"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}"#.to_owned(),
            "data: [DONE]".to_owned(),
        ],
    };
    let mut provider = OpenAiCompatibleProvider::new(server);

    let events = provider
        .stream(model_input())
        .expect("protocol stream opens")
        .collect::<Vec<_>>();

    assert_eq!(
        events,
        vec![
            ProviderEvent::Delta("A".to_owned()),
            ProviderEvent::Usage(Usage::new(3, 2).expect("valid usage")),
            ProviderEvent::Completed,
        ]
    );
}

#[test]
fn openai_compatible_error_maps_to_owned_code() {
    let server = DeterministicProtocolServer {
        frames: vec![r#"data: {"error":{"code":"UPSTREAM_RESET"}}"#.to_owned()],
    };
    let mut provider = OpenAiCompatibleProvider::new(server);

    let events = provider
        .stream(model_input())
        .expect("protocol stream opens")
        .collect::<Vec<_>>();

    assert_eq!(
        events,
        vec![ProviderEvent::Error {
            code: "UPSTREAM_RESET".to_owned(),
        }]
    );
}
