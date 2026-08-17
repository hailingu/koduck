// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use koduck_ai::adapters::provider::{
    OpenAiCompatibleProvider, OpenAiProtocolTransport, ReqwestOpenAiTransport,
};
use koduck_ai::application::{AppendPolicy, ModelInput, ModelProvider, NewItem, ProviderEvent};
use koduck_ai::domain::{TenantId, ThreadId, TurnId};

struct DisconnectAwareUpstream {
    base_url: String,
    connected: Receiver<()>,
    disconnected: Receiver<bool>,
    thread: Option<thread::JoinHandle<()>>,
}

struct HeaderStallingUpstream {
    base_url: String,
    release: Option<Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl HeaderStallingUpstream {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test upstream");
        let base_url = format!(
            "http://{}",
            listener.local_addr().expect("upstream address")
        );
        let (release, released) = mpsc::channel();
        let thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept provider request");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("read provider request");
            released.recv().expect("release stalled response headers");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .expect("write provider headers");
        });
        Self {
            base_url,
            release: Some(release),
            thread: Some(thread),
        }
    }

    fn release(&mut self) {
        if let Some(release) = self.release.take() {
            release.send(()).expect("release provider response headers");
        }
    }
}

impl Drop for HeaderStallingUpstream {
    fn drop(&mut self) {
        self.release();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct FrameUpstream {
    base_url: String,
    thread: Option<thread::JoinHandle<()>>,
}

impl FrameUpstream {
    fn start(frame: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test upstream");
        let base_url = format!(
            "http://{}",
            listener.local_addr().expect("upstream address")
        );
        let thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept provider request");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("read provider request");
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .expect("write provider headers");
            write!(stream, "{:X}\r\n", frame.len()).expect("write provider chunk size");
            stream.write_all(&frame).expect("write provider frame");
            stream
                .write_all(b"\r\n0\r\n\r\n")
                .expect("finish provider body");
        });
        Self {
            base_url,
            thread: Some(thread),
        }
    }
}

impl Drop for FrameUpstream {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl DisconnectAwareUpstream {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test upstream");
        let base_url = format!(
            "http://{}",
            listener.local_addr().expect("upstream address")
        );
        let (connected_sender, connected) = mpsc::channel();
        let (disconnected_sender, disconnected) = mpsc::channel();
        let thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept provider request");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("read provider request");
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .expect("write provider headers");
            stream.flush().expect("flush provider headers");
            connected_sender
                .send(())
                .expect("report established provider response");
            stream
                .set_read_timeout(Some(Duration::from_millis(750)))
                .expect("set disconnect observation timeout");
            let disconnected = match stream.read(&mut [0_u8; 1]) {
                Ok(0) => true,
                Err(error)
                    if !matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    true
                }
                Ok(_) | Err(_) => false,
            };
            disconnected_sender
                .send(disconnected)
                .expect("report provider connection state");
        });
        Self {
            base_url,
            connected,
            disconnected,
            thread: Some(thread),
        }
    }

    fn wait_until_connected(&self) {
        self.connected
            .recv_timeout(Duration::from_secs(1))
            .expect("provider response becomes established");
    }

    fn wait_for_disconnect(&self) -> bool {
        self.disconnected
            .recv_timeout(Duration::from_secs(1))
            .expect("upstream reports connection state")
    }
}

impl Drop for DisconnectAwareUpstream {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_provider_stream_closes_an_idle_upstream() {
    let upstream = DisconnectAwareUpstream::start();
    let runtime = tokio::runtime::Handle::current();
    let base_url = upstream.base_url.clone();
    let frames = tokio::task::spawn_blocking(move || {
        let mut transport = ReqwestOpenAiTransport::new(
            reqwest::Client::new(),
            runtime,
            &base_url,
            "test-model",
            "test-key",
        );
        transport
            .chat_completion_frames(&model_input())
            .expect("provider response headers are available")
    })
    .await
    .expect("transport setup task joins");

    upstream.wait_until_connected();
    drop(frames);

    assert!(
        upstream.wait_for_disconnect(),
        "dropping the consumer stream must cancel the idle provider response pump"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_stream_is_pollable_before_response_headers_arrive() {
    let mut upstream = HeaderStallingUpstream::start();
    let runtime = tokio::runtime::Handle::current();
    let base_url = upstream.base_url.clone();
    let mut setup = tokio::task::spawn_blocking(move || {
        let mut transport = ReqwestOpenAiTransport::new(
            reqwest::Client::new(),
            runtime,
            &base_url,
            "test-model",
            "test-key",
        );
        transport.chat_completion_frames(&model_input())
    });

    let Ok(result) = tokio::time::timeout(Duration::from_millis(250), &mut setup).await else {
        upstream.release();
        let _ = setup.await;
        panic!("provider stream setup blocked while waiting for response headers");
    };
    let mut frames = result
        .expect("transport setup task joins")
        .expect("provider stream is created before response headers");

    assert_eq!(
        frames.next(),
        Some(Ok(koduck_ai::adapters::provider::OpenAiFrame::Pending))
    );
    drop(frames);
    upstream.release();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oversized_unterminated_provider_frame_is_rejected() {
    let mut frame = b"data: ".to_vec();
    frame.resize(1_114_113, b'x');
    let upstream = FrameUpstream::start(frame);
    let runtime = tokio::runtime::Handle::current();
    let base_url = upstream.base_url.clone();
    let mut frames = tokio::task::spawn_blocking(move || {
        let mut transport = ReqwestOpenAiTransport::new(
            reqwest::Client::new(),
            runtime,
            &base_url,
            "test-model",
            "test-key",
        );
        transport
            .chat_completion_frames(&model_input())
            .expect("provider stream setup succeeds")
    })
    .await
    .expect("transport setup task joins");

    let error = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::task::spawn_blocking(move || {
            frames
                .find_map(Result::err)
                .expect("oversized provider frame is rejected")
        }),
    )
    .await
    .expect("provider frame rejection is bounded")
    .expect("provider frame consumer joins");

    assert_eq!(error.code, "OPENAI_FRAME_TOO_LARGE");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exact_limit_item_payload_allows_the_provider_envelope() {
    const ITEM_PAYLOAD_LIMIT: usize = 1_048_576;
    const DELTA_PAYLOAD_OVERHEAD: usize = r#"{"content":""}"#.len();

    let content = "x".repeat(ITEM_PAYLOAD_LIMIT - DELTA_PAYLOAD_OVERHEAD);
    let frame = format!(
        "data: {}\n",
        serde_json::json!({"choices": [{"delta": {"content": content}}]})
    )
    .into_bytes();
    assert!(frame.len() > ITEM_PAYLOAD_LIMIT);
    let upstream = FrameUpstream::start(frame);
    let runtime = tokio::runtime::Handle::current();
    let base_url = upstream.base_url.clone();
    let event = tokio::task::spawn_blocking(move || {
        let transport = ReqwestOpenAiTransport::new(
            reqwest::Client::new(),
            runtime,
            &base_url,
            "test-model",
            "test-key",
        );
        let mut provider = OpenAiCompatibleProvider::new(transport);
        provider
            .stream(model_input())
            .expect("provider stream setup succeeds")
            .find(|event| !matches!(event, ProviderEvent::Pending))
            .expect("provider returns one owned event")
    })
    .await
    .expect("provider consumer joins");

    let ProviderEvent::Delta(content) = event else {
        panic!("exact-limit payload must remain a provider delta: {event:?}");
    };
    AppendPolicy::cand_1()
        .check_item(&NewItem::AgentMessageDelta { content })
        .expect("the decoded serialized Item payload is exactly within the contract limit");
}
