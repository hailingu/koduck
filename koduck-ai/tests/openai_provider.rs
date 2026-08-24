// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0004-provider-stream-completion-normalization.md

use koduck_ai::adapters::provider::{
    OpenAiCompatibleProvider, OpenAiFrame, OpenAiFrameStream, OpenAiProtocolTransport,
    OpenAiTransportError,
};
use koduck_ai::application::{ModelInput, ModelProvider, ProviderEvent};
use koduck_ai::domain::{TenantId, ThreadId, TurnId, Usage};

/// Deterministic fixture sentinel standing in for one explicit transport
/// clean end (ADR-0004 PSC-1): appended after the final `data:` frame, it
/// yields the production transport's ordered `OpenAiFrame::CleanEnd`. The
/// NUL byte cannot collide with a `data: ` frame.
const CLEAN_END: &str = "\u{0}clean-end";

struct DeterministicProtocolServer {
    frames: Vec<String>,
}

impl OpenAiProtocolTransport for DeterministicProtocolServer {
    fn chat_completion_frames(
        &mut self,
        _input: &ModelInput,
    ) -> Result<OpenAiFrameStream, OpenAiTransportError> {
        Ok(Box::new(self.frames.clone().into_iter().map(|frame| {
            if frame == CLEAN_END {
                Ok(OpenAiFrame::CleanEnd)
            } else {
                Ok(OpenAiFrame::Data(frame))
            }
        })))
    }
}

fn model_input() -> ModelInput {
    ModelInput {
        tenant_id: TenantId::new("tenant-a").expect("valid tenant"),
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
        input: "hello".to_owned(),
        history: Vec::new(),
        tool_rounds: Vec::new(),
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

#[test]
fn null_usage_on_delta_chunk_is_not_a_usage_frame() {
    let server = DeterministicProtocolServer {
        frames: vec![
            r#"data: {"choices":[{"delta":{"content":"A"}}],"usage":null}"#.to_owned(),
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
            ProviderEvent::Completed
        ]
    );
}

#[test]
fn duplicate_usage_frames_fail_closed() {
    let server = DeterministicProtocolServer {
        frames: vec![
            r#"data: {"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}"#.to_owned(),
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
            ProviderEvent::Usage(Usage::new(3, 2).expect("valid usage")),
            ProviderEvent::Error {
                code: "DUPLICATE_USAGE_FRAME".to_owned(),
            },
        ]
    );
}

#[test]
fn usage_frame_cannot_contain_a_tool_call() {
    let server = DeterministicProtocolServer {
        frames: vec![
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"fixture.tool","arguments":"{}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}"#.to_owned(),
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
        vec![ProviderEvent::Error {
            code: "INVALID_USAGE_FRAME".to_owned(),
        }],
        "a usage frame must not silently discard a Tool call"
    );
}

#[test]
fn provider_output_after_usage_fails_closed() {
    let server = DeterministicProtocolServer {
        frames: vec![
            r#"data: {"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}"#.to_owned(),
            r#"data: {"choices":[{"delta":{"content":"late"}}]}"#.to_owned(),
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
            ProviderEvent::Usage(Usage::new(3, 2).expect("valid usage")),
            ProviderEvent::Error {
                code: "INVALID_USAGE_FRAME".to_owned(),
            },
        ],
        "final usage cannot be followed by additional provider output"
    );
}

#[test]
fn openai_tool_call_fragments_assemble_into_one_owned_tool_call() {
    let server = DeterministicProtocolServer {
        frames: vec![
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"fixture.tool","arguments":""}}]}}]}"#.to_owned(),
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"value\":1}"}}]}}]}"#.to_owned(),
            r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#.to_owned(),
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
        vec![ProviderEvent::ToolCall {
            name: "fixture.tool".to_owned(),
            arguments: r#"{"value":1}"#.to_owned(),
        },],
        "a Tool-call round's [DONE] ends the stream without a Turn completion"
    );
}

#[test]
fn tool_call_finish_frame_includes_its_final_argument_fragment() {
    let server = DeterministicProtocolServer {
        frames: vec![
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"fixture.tool","arguments":"{"}}]}}]}"#.to_owned(),
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"}"}}]},"finish_reason":"tool_calls"}]}"#.to_owned(),
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
        vec![ProviderEvent::ToolCall {
            name: "fixture.tool".to_owned(),
            arguments: "{}".to_owned(),
        }],
        "the finish frame's delta is part of the tool-call assembly"
    );
}

#[test]
fn mixed_content_and_tool_call_frame_preserves_the_assistant_text() {
    let server = DeterministicProtocolServer {
        frames: vec![
            r#"data: {"choices":[{"delta":{"content":"I will check it. ","tool_calls":[{"index":0,"function":{"name":"fixture.tool","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}"#.to_owned(),
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
            ProviderEvent::Delta("I will check it. ".to_owned()),
            ProviderEvent::ToolCall {
                name: "fixture.tool".to_owned(),
                arguments: "{}".to_owned(),
            },
        ],
        "a valid mixed delta retains assistant text before the Tool-call round"
    );
}

#[test]
fn openai_parallel_tool_calls_assemble_in_index_order() {
    let server = DeterministicProtocolServer {
        frames: vec![
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"name":"second.tool"}}]}}]}"#.to_owned(),
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"first.tool","arguments":"{}"}}]}}]}"#.to_owned(),
            r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#.to_owned(),
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
            ProviderEvent::ToolCall {
                name: "first.tool".to_owned(),
                arguments: "{}".to_owned(),
            },
            ProviderEvent::ToolCall {
                name: "second.tool".to_owned(),
                arguments: String::new(),
            },
        ],
        "a Tool-call round's [DONE] ends the stream without a Turn completion"
    );
}

#[test]
fn malformed_tool_call_fragments_fail_closed() {
    for frames in [
        // A fragment without the required index is rejected.
        vec![
            r#"data: {"choices":[{"delta":{"tool_calls":[{"function":{"name":"fixture.tool"}}]}}]}"#,
        ],
        // A finish without any assembled call is rejected.
        vec![r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#],
        // A function fragment that is not an object is rejected.
        vec![r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":"nope"}]}}]}"#],
        // A present but wrong-typed tool_calls member is rejected instead of
        // being silently dropped behind the later [DONE] frame.
        vec![
            r#"data: {"choices":[{"delta":{"tool_calls":{}}}]}"#,
            "data: [DONE]",
        ],
        // A present non-string function name is rejected at its fragment.
        vec![r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":7}}]}}]}"#],
    ] {
        let server = DeterministicProtocolServer {
            frames: frames.into_iter().map(str::to_owned).collect(),
        };
        let mut provider = OpenAiCompatibleProvider::new(server);
        let events = provider
            .stream(model_input())
            .expect("protocol stream opens")
            .collect::<Vec<_>>();
        assert_eq!(
            events,
            vec![ProviderEvent::Error {
                code: "INVALID_TOOL_CALL_FRAME".to_owned(),
            }],
            "malformed tool-call frames must fail closed"
        );
    }
}

#[test]
fn streamed_tool_call_arguments_are_bounded_cumulatively() {
    // Many sub-limit fragments still fail closed once the cumulative
    // assembled arguments cross the canonical 65,536-byte action-input bound
    // (ADR-0003): the later per-action validation is not the first guard.
    let fragment = |text: &str| {
        format!(
            r#"data: {{"choices":[{{"delta":{{"tool_calls":[{{"index":0,"function":{{"arguments":"{text}"}}}}]}}}}]}}"#
        )
    };
    let chunk = "a".repeat(32_768);
    let server = DeterministicProtocolServer {
        frames: vec![
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"fixture.tool","arguments":""}}]}}]}"#.to_owned(),
            fragment(&chunk),
            fragment(&chunk.clone()),
            fragment(&chunk),
        ],
    };
    let mut provider = OpenAiCompatibleProvider::new(server);

    let events = provider
        .stream(model_input())
        .expect("protocol stream opens")
        .collect::<Vec<_>>();

    assert_eq!(
        events,
        vec![ProviderEvent::Error {
            code: "TOOL_CALL_ARGUMENTS_TOO_LARGE".to_owned(),
        }],
        "cumulative streamed arguments fail closed at the canonical input bound"
    );
}

#[test]
fn a_thirty_third_assembled_tool_call_fails_closed() {
    // Every serviced call records at least a ToolCall and a ToolResult D-3
    // item, so the 64-item per-Turn provider buffer (ADR-0001) could never
    // record a 33rd call; the assembly fails closed instead of allocating it.
    let frames = (0..33)
        .map(|index| {
            format!(
                r#"data: {{"choices":[{{"delta":{{"tool_calls":[{{"index":{index},"function":{{"name":"fixture.tool","arguments":"{{}}"}}}}]}}}}]}}"#
            )
        })
        .collect();
    let server = DeterministicProtocolServer { frames };
    let mut provider = OpenAiCompatibleProvider::new(server);

    let events = provider
        .stream(model_input())
        .expect("protocol stream opens")
        .collect::<Vec<_>>();

    assert_eq!(
        events,
        vec![ProviderEvent::Error {
            code: "TOO_MANY_TOOL_CALLS".to_owned(),
        }],
        "the 33rd assembled call fails closed before allocation"
    );
}

#[test]
fn done_with_unfinished_tool_call_fragments_fails_closed() {
    // Fragments were accumulated but the provider ended the stream without
    // `finish_reason: "tool_calls"`: accepting completion would silently drop
    // the requested action, so the malformed sequence fails closed.
    let server = DeterministicProtocolServer {
        frames: vec![
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"fixture.tool","arguments":"{\"va"}}]}}]}"#.to_owned(),
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
        vec![ProviderEvent::Error {
            code: "INVALID_TOOL_CALL_FRAME".to_owned(),
        }],
        "[DONE] with unfinished Tool-call fragments must fail closed"
    );
}

#[test]
fn completion_variants_map_to_the_same_owned_events() {
    // ADR-0004 PSC-2/PSC-3: the `[DONE]` sentinel and one validated
    // `finish_reason: "stop"` followed by optional usage and an explicit
    // clean end are equivalent terminal evidence.
    let server = DeterministicProtocolServer {
        frames: vec![
            r#"data: {"choices":[{"delta":{"content":"A"}}]}"#.to_owned(),
            r#"data: {"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}"#.to_owned(),
            "data: [DONE]".to_owned(),
        ],
    };
    let mut sentinel_provider = OpenAiCompatibleProvider::new(server);
    let sentinel_events = sentinel_provider
        .stream(model_input())
        .expect("protocol stream opens")
        .collect::<Vec<_>>();
    assert_eq!(
        sentinel_events,
        vec![
            ProviderEvent::Delta("A".to_owned()),
            ProviderEvent::Usage(Usage::new(3, 2).expect("valid usage")),
            ProviderEvent::Completed,
        ]
    );

    let server = DeterministicProtocolServer {
        frames: vec![
            r#"data: {"choices":[{"delta":{"content":"A"},"finish_reason":"stop"}]}"#.to_owned(),
            r#"data: {"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}"#.to_owned(),
            CLEAN_END.to_owned(),
        ],
    };
    let mut clean_end_provider = OpenAiCompatibleProvider::new(server);
    let clean_end_events = clean_end_provider
        .stream(model_input())
        .expect("protocol stream opens")
        .collect::<Vec<_>>();

    assert_eq!(
        clean_end_events, sentinel_events,
        "the clean-end stop variant owns the same ordered Delta, Usage, and exactly one Completed event"
    );
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive fail-closed table is the contract's cohesive unit (ADR-0004 PSC-5)"
)]
fn invalid_clean_end_sequences_fail_closed() {
    // ADR-0004 PSC-5: every ambiguous, unsupported, repeated, or late-output
    // clean-end sequence emits its declared typed error and never completes.
    let stop = r#"data: {"choices":[{"delta":{"content":"A"},"finish_reason":"stop"}]}"#;
    let usage = r#"data: {"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}"#;
    let tool_fragment = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"fixture.tool","arguments":"{\"va"}}]}}]}"#;
    let cases: Vec<(&str, Vec<&str>, &str)> = vec![
        (
            "clean end without any finish reason",
            vec![
                r#"data: {"choices":[{"delta":{"content":"A"}}]}"#,
                CLEAN_END,
            ],
            "OPENAI_UNEXPECTED_EOF",
        ),
        (
            "clean end after an unsupported finish reason",
            vec![
                r#"data: {"choices":[{"delta":{"content":"A"},"finish_reason":"length"}]}"#,
                usage,
                CLEAN_END,
            ],
            "OPENAI_UNEXPECTED_EOF",
        ),
        (
            "repeated stop finish",
            vec![
                stop,
                r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
                CLEAN_END,
            ],
            "INVALID_FINISH_FRAME",
        ),
        (
            "conflicting finish reasons",
            vec![
                stop,
                r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
                CLEAN_END,
            ],
            "INVALID_FINISH_FRAME",
        ),
        (
            "content output after a finish frame",
            vec![
                stop,
                r#"data: {"choices":[{"delta":{"content":"late"}}]}"#,
                CLEAN_END,
            ],
            "INVALID_FINISH_FRAME",
        ),
        (
            "tool output after a finish frame",
            vec![stop, tool_fragment, CLEAN_END],
            "INVALID_FINISH_FRAME",
        ),
        (
            "error output after a finish frame",
            vec![
                stop,
                r#"data: {"error":{"code":"UPSTREAM_RESET"}}"#,
                CLEAN_END,
            ],
            "INVALID_FINISH_FRAME",
        ),
        (
            "duplicate usage after a finish frame",
            vec![stop, usage, usage, CLEAN_END],
            "DUPLICATE_USAGE_FRAME",
        ),
        (
            "invalid usage after a finish frame",
            vec![
                stop,
                r#"data: {"choices":[],"usage":{"completion_tokens":2,"total_tokens":5}}"#,
                CLEAN_END,
            ],
            "INVALID_USAGE_FRAME",
        ),
        (
            "unfinished tool fragments at clean end",
            vec![tool_fragment, CLEAN_END],
            "INVALID_TOOL_CALL_FRAME",
        ),
    ];
    for (case, frames, expected_code) in cases {
        let server = DeterministicProtocolServer {
            frames: frames.into_iter().map(str::to_owned).collect(),
        };
        let mut provider = OpenAiCompatibleProvider::new(server);
        let events = provider
            .stream(model_input())
            .expect("protocol stream opens")
            .collect::<Vec<_>>();
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ProviderEvent::Completed)),
            "case {case}: zero Completed events"
        );
        assert_eq!(
            events.last(),
            Some(&ProviderEvent::Error {
                code: expected_code.to_owned(),
            }),
            "case {case}: the declared typed error terminates the stream"
        );
    }
}
