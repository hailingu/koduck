// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0004-provider-stream-completion-normalization.md

//! OpenAI-compatible protocol translation into provider-neutral application events.

use std::thread;
use std::time::Duration;

use thiserror::Error;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;

use crate::application::{ModelInput, ModelProvider, ProviderError, ProviderStream};

mod messages;
mod stream_state;

use messages::provider_messages;
use stream_state::StreamState;

const MAX_SERIALIZED_ITEM_PAYLOAD_BYTES: usize = 1_048_576;
// Keep transport buffering bounded without applying the Item payload contract
// to the provider's `data:` prefix and JSON envelope.
const MAX_OPENAI_FRAME_OVERHEAD_BYTES: usize = 65_536;
const MAX_OPENAI_FRAME_BYTES: usize =
    MAX_SERIALIZED_ITEM_PAYLOAD_BYTES + MAX_OPENAI_FRAME_OVERHEAD_BYTES;
const PROVIDER_RESPONSE_HEADER_TIMEOUT: Duration = Duration::from_secs(30);
const PROVIDER_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const PROVIDER_TOTAL_TIMEOUT: Duration = Duration::from_mins(2);

#[derive(Clone, Copy)]
struct ProviderTiming {
    response_header: Duration,
    stream_idle: Duration,
    total: Duration,
}

impl ProviderTiming {
    const fn cand_1() -> Self {
        Self {
            response_header: PROVIDER_RESPONSE_HEADER_TIMEOUT,
            stream_idle: PROVIDER_STREAM_IDLE_TIMEOUT,
            total: PROVIDER_TOTAL_TIMEOUT,
        }
    }
}

/// A transport-level failure before OpenAI-compatible frames are available.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("OpenAI-compatible transport failed: {code}")]
pub struct OpenAiTransportError {
    /// Stable adapter-owned transport code.
    pub code: String,
}

/// Boundary implemented by the configured HTTP transport or deterministic protocol server.
pub trait OpenAiProtocolTransport {
    /// Returns the ordered `data:` frames for one chat-completions request.
    ///
    /// # Errors
    ///
    /// Returns [`OpenAiTransportError`] when the configured transport cannot
    /// open or consume the provider stream.
    fn chat_completion_frames(
        &mut self,
        input: &ModelInput,
    ) -> Result<OpenAiFrameStream, OpenAiTransportError>;
}

/// One data frame or bounded idle poll from an OpenAI-compatible stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenAiFrame {
    /// One complete `data:` line.
    Data(String),
    /// No frame arrived during the bounded control-poll interval.
    Pending,
    /// Explicit clean end: the HTTP response body reached a successful EOF
    /// after every buffered byte was decoded (ADR-0004 PSC-1). Emitted at
    /// most once per stream, ordered after every decoded data frame.
    CleanEnd,
}

/// A lazy sequence of OpenAI-compatible SSE frames and idle polls.
pub type OpenAiFrameStream =
    Box<dyn Iterator<Item = Result<OpenAiFrame, OpenAiTransportError>> + Send>;

/// Reqwest transport for one explicitly configured OpenAI-compatible endpoint.
#[derive(Clone)]
pub struct ReqwestOpenAiTransport {
    client: reqwest::Client,
    runtime: Handle,
    endpoint: String,
    model: String,
    api_key: String,
    timing: ProviderTiming,
}

impl ReqwestOpenAiTransport {
    /// Creates the production provider transport without issuing a request.
    #[must_use]
    pub fn new(
        client: reqwest::Client,
        runtime: Handle,
        base_url: &str,
        model: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            client,
            runtime,
            endpoint: format!("{}/chat/completions", base_url.trim_end_matches('/')),
            model: model.into(),
            api_key: api_key.into(),
            timing: ProviderTiming::cand_1(),
        }
    }
}

impl OpenAiProtocolTransport for ReqwestOpenAiTransport {
    fn chat_completion_frames(
        &mut self,
        input: &ModelInput,
    ) -> Result<OpenAiFrameStream, OpenAiTransportError> {
        let messages = provider_messages(input);
        let request = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({
                "model": self.model,
                "messages": messages,
                "stream": true,
                "stream_options": { "include_usage": true },
            }));
        let (sender, mut receiver) = mpsc::channel(64);
        let timing = self.timing;
        self.runtime.spawn(async move {
            let timeout_sender = sender.clone();
            if tokio::time::timeout(timing.total, pump_request(request, sender, timing))
                .await
                .is_err()
            {
                let _ = timeout_sender
                    .send(Err(transport_error("OPENAI_TOTAL_TIMEOUT")))
                    .await;
            }
        });
        Ok(Box::new(std::iter::from_fn(move || {
            match receiver.try_recv() {
                Ok(frame) => Some(frame),
                Err(TryRecvError::Empty) => {
                    thread::sleep(Duration::from_millis(50));
                    Some(Ok(OpenAiFrame::Pending))
                }
                Err(TryRecvError::Disconnected) => None,
            }
        })))
    }
}

async fn pump_request(
    request: reqwest::RequestBuilder,
    sender: mpsc::Sender<Result<OpenAiFrame, OpenAiTransportError>>,
    timing: ProviderTiming,
) {
    let response = tokio::select! {
        () = sender.closed() => return,
        response = tokio::time::timeout(timing.response_header, request.send()) => {
            if let Ok(response) = response {
                response
            } else {
                let _ = sender
                    .send(Err(transport_error("OPENAI_RESPONSE_HEADER_TIMEOUT")))
                    .await;
                return;
            }
        },
    };
    let Ok(response) = response else {
        let _ = sender
            .send(Err(transport_error("OPENAI_REQUEST_FAILED")))
            .await;
        return;
    };
    let status = response.status();
    if !status.is_success() {
        let _ = sender
            .send(Err(transport_error(&format!(
                "OPENAI_HTTP_{}",
                status.as_u16()
            ))))
            .await;
        return;
    }
    pump_response(response, sender, timing).await;
}

async fn pump_response(
    mut response: reqwest::Response,
    sender: mpsc::Sender<Result<OpenAiFrame, OpenAiTransportError>>,
    timing: ProviderTiming,
) {
    let mut pending = Vec::new();
    let mut saw_frame = false;
    loop {
        let chunk = tokio::select! {
            () = sender.closed() => return,
            chunk = tokio::time::timeout(timing.stream_idle, response.chunk()) => {
                if let Ok(chunk) = chunk {
                    chunk
                } else {
                    let _ = sender
                        .send(Err(transport_error("OPENAI_STREAM_IDLE_TIMEOUT")))
                        .await;
                    return;
                }
            },
        };
        match chunk {
            Ok(Some(chunk)) => {
                if let Err(error) =
                    consume_chunk(&sender, &mut pending, &chunk, &mut saw_frame).await
                {
                    let _ = sender.send(Err(error)).await;
                    return;
                }
            }
            Ok(None) => break,
            Err(_) => {
                let _ = sender
                    .send(Err(transport_error("OPENAI_BODY_FAILED")))
                    .await;
                return;
            }
        }
    }
    if !pending.is_empty() && send_frame(&sender, pending, &mut saw_frame).await.is_err() {
        let _ = sender
            .send(Err(transport_error("OPENAI_BODY_FAILED")))
            .await;
        return;
    }
    if !saw_frame {
        let _ = sender
            .send(Err(transport_error("OPENAI_EMPTY_STREAM")))
            .await;
        return;
    }
    // Successful decoded body EOF emits the one explicit clean-end frame,
    // ordered after every decoded data frame (ADR-0004 PSC-1). Response-header
    // and stream-idle timeouts, total timeout, body-read failures, oversized
    // frames, and consumer cancellation all return before this point, so they
    // never emit clean end; a closed channel drops the send without an error.
    let _ = sender.send(Ok(OpenAiFrame::CleanEnd)).await;
}

async fn consume_chunk(
    sender: &mpsc::Sender<Result<OpenAiFrame, OpenAiTransportError>>,
    pending: &mut Vec<u8>,
    chunk: &[u8],
    saw_frame: &mut bool,
) -> Result<(), OpenAiTransportError> {
    let mut remaining = chunk;
    while let Some(newline) = remaining.iter().position(|byte| *byte == b'\n') {
        append_frame_bytes(pending, &remaining[..=newline])?;
        let mut line = std::mem::take(pending);
        while line
            .last()
            .is_some_and(|byte| matches!(*byte, b'\n' | b'\r'))
        {
            line.pop();
        }
        send_frame(sender, line, saw_frame)
            .await
            .map_err(|()| transport_error("OPENAI_BODY_FAILED"))?;
        remaining = &remaining[newline + 1..];
    }
    append_frame_bytes(pending, remaining)
}

fn append_frame_bytes(pending: &mut Vec<u8>, bytes: &[u8]) -> Result<(), OpenAiTransportError> {
    if pending
        .len()
        .checked_add(bytes.len())
        .is_none_or(|length| length > MAX_OPENAI_FRAME_BYTES)
    {
        return Err(transport_error("OPENAI_FRAME_TOO_LARGE"));
    }
    pending.extend_from_slice(bytes);
    Ok(())
}

async fn send_frame(
    sender: &mpsc::Sender<Result<OpenAiFrame, OpenAiTransportError>>,
    line: Vec<u8>,
    saw_frame: &mut bool,
) -> Result<(), ()> {
    let line = String::from_utf8(line).map_err(|_| ())?;
    if !line.starts_with("data: ") {
        return Ok(());
    }
    *saw_frame = true;
    sender
        .send(Ok(OpenAiFrame::Data(line)))
        .await
        .map_err(|_| ())
}

fn transport_error(code: &str) -> OpenAiTransportError {
    OpenAiTransportError {
        code: code.to_owned(),
    }
}

/// Translates OpenAI-compatible streaming frames into owned provider events.
#[derive(Clone)]
pub struct OpenAiCompatibleProvider<T> {
    transport: T,
}

impl<T> OpenAiCompatibleProvider<T> {
    /// Creates the adapter around one explicitly configured transport.
    #[must_use]
    pub const fn new(transport: T) -> Self {
        Self { transport }
    }
}

impl<T> ModelProvider for OpenAiCompatibleProvider<T>
where
    T: OpenAiProtocolTransport,
{
    fn stream(&mut self, input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        let mut frames = self
            .transport
            .chat_completion_frames(&input)
            .map_err(|error| ProviderError { code: error.code })?;
        let mut state = StreamState::default();
        let events = std::iter::from_fn(move || state.next_event(&mut frames));
        Ok(Box::new(events))
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    use crate::application::ModelInput;
    use crate::domain::{Item, ItemPayload, TenantId, ThreadId, TurnId};

    use super::{
        OpenAiFrame, OpenAiProtocolTransport, ProviderTiming, ReqwestOpenAiTransport,
        provider_messages, pump_request,
    };

    fn local_client() -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("local test client")
    }

    fn test_input() -> ModelInput {
        ModelInput {
            tenant_id: TenantId::new("tenant-a").expect("valid tenant"),
            thread_id: ThreadId::new(),
            turn_id: TurnId::new(),
            input: "hello".to_owned(),
            history: Vec::new(),
            tool_rounds: Vec::new(),
        }
    }

    fn stalled_server(response_headers: Option<&'static [u8]>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("local listener");
        let address = listener.local_addr().expect("local address");
        thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("local connection");
            let _ = socket.set_read_timeout(Some(Duration::from_millis(100)));
            let mut request = [0_u8; 4096];
            let _ = socket.read(&mut request);
            if let Some(headers) = response_headers {
                socket.write_all(headers).expect("response headers");
                socket.flush().expect("response flush");
            }
            thread::sleep(Duration::from_millis(100));
        });
        format!("http://{address}")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn provider_response_header_and_stream_idle_timeouts_are_typed() {
        let timing = ProviderTiming {
            response_header: Duration::from_millis(10),
            stream_idle: Duration::from_millis(10),
            total: Duration::from_secs(1),
        };

        let header_url = stalled_server(None);
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        pump_request(local_client().post(header_url), sender, timing).await;
        assert_eq!(
            receiver.recv().await.expect("header timeout result"),
            Err(super::transport_error("OPENAI_RESPONSE_HEADER_TIMEOUT"))
        );

        let idle_url = stalled_server(Some(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
        ));
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        pump_request(local_client().post(idle_url), sender, timing).await;
        assert_eq!(
            receiver.recv().await.expect("idle timeout result"),
            Err(super::transport_error("OPENAI_STREAM_IDLE_TIMEOUT"))
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn provider_total_timeout_terminates_pending_establishment() {
        let url = stalled_server(None);
        let mut transport = ReqwestOpenAiTransport::new(
            local_client(),
            tokio::runtime::Handle::current(),
            &url,
            "model",
            "secret",
        );
        transport.timing = ProviderTiming {
            response_header: Duration::from_secs(1),
            stream_idle: Duration::from_secs(1),
            total: Duration::from_millis(10),
        };

        let mut frames = transport
            .chat_completion_frames(&test_input())
            .expect("stream is created");
        let terminal = (0..4).find_map(|_| match frames.next() {
            Some(Err(error)) => Some(error),
            Some(Ok(OpenAiFrame::Pending)) => None,
            other => panic!("unexpected provider frame: {other:?}"),
        });

        assert_eq!(
            terminal,
            Some(super::transport_error("OPENAI_TOTAL_TIMEOUT"))
        );
    }

    #[test]
    fn provider_history_coalesces_deltas_within_each_prior_turn() {
        let input = ModelInput {
            tenant_id: TenantId::new("tenant-a").expect("valid tenant"),
            thread_id: ThreadId::new(),
            turn_id: TurnId::new(),
            input: "second".to_owned(),
            history: vec![
                Item::new(
                    1,
                    ItemPayload::UserMessage {
                        content: "first".to_owned(),
                    },
                ),
                Item::new(
                    2,
                    ItemPayload::AgentMessageDelta {
                        content: "A".to_owned(),
                    },
                ),
                Item::new(
                    3,
                    ItemPayload::AgentMessageDelta {
                        content: "B".to_owned(),
                    },
                ),
            ],
            tool_rounds: Vec::new(),
        };

        assert_eq!(
            provider_messages(&input),
            vec![
                serde_json::json!({ "role": "user", "content": "first" }),
                serde_json::json!({ "role": "assistant", "content": "AB" }),
                serde_json::json!({ "role": "user", "content": "second" }),
            ]
        );
    }

    #[test]
    fn continuation_request_preserves_causal_round_order() {
        // Round 2 was raised on round 1's committed result; the continuation
        // serializes alternating assistant-call/result groups rather than
        // rewriting both calls as concurrent (ADR-0003 TC-11).
        let mut input = test_input();
        input.tool_rounds = vec![
            crate::application::ToolRound {
                assistant_content: "I will inspect it.".to_owned(),
                calls: vec![crate::application::CommittedToolCall {
                    call: crate::application::ModelToolCall {
                        name: "fixture.tool".to_owned(),
                        arguments: r#"{"value":1}"#.to_owned(),
                    },
                    result: crate::application::ModelToolResult {
                        content: "ok-output".to_owned(),
                        is_error: false,
                    },
                }],
            },
            crate::application::ToolRound {
                assistant_content: String::new(),
                calls: vec![crate::application::CommittedToolCall {
                    call: crate::application::ModelToolCall {
                        name: "other.tool".to_owned(),
                        arguments: "{}".to_owned(),
                    },
                    result: crate::application::ModelToolResult {
                        content: "TOOL_EXECUTION_UNAVAILABLE".to_owned(),
                        is_error: true,
                    },
                }],
            },
        ];

        assert_eq!(
            provider_messages(&input),
            vec![
                serde_json::json!({ "role": "user", "content": "hello" }),
                serde_json::json!({
                    "role": "assistant",
                    "content": "I will inspect it.",
                    "tool_calls": [
                        {
                            "id": "call_0",
                            "type": "function",
                            "function": {
                                "name": "fixture.tool",
                                "arguments": r#"{"value":1}"#,
                            },
                        },
                    ],
                }),
                serde_json::json!({
                    "role": "tool",
                    "tool_call_id": "call_0",
                    "content": "ok-output",
                }),
                serde_json::json!({
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": { "name": "other.tool", "arguments": "{}" },
                        },
                    ],
                }),
                serde_json::json!({
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "content": "TOOL_EXECUTION_UNAVAILABLE",
                }),
            ],
            "each round serializes as its own assistant-call/result group in causal order"
        );
    }
}
