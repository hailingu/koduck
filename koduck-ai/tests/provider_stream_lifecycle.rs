// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use koduck_ai::adapters::provider::{OpenAiProtocolTransport, ReqwestOpenAiTransport};
use koduck_ai::application::ModelInput;
use koduck_ai::domain::{TenantId, ThreadId, TurnId};

struct DisconnectAwareUpstream {
    base_url: String,
    disconnected: Receiver<bool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl DisconnectAwareUpstream {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test upstream");
        let base_url = format!(
            "http://{}",
            listener.local_addr().expect("upstream address")
        );
        let (sender, disconnected) = mpsc::channel();
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
            sender
                .send(disconnected)
                .expect("report provider connection state");
        });
        Self {
            base_url,
            disconnected,
            thread: Some(thread),
        }
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

    drop(frames);

    assert!(
        upstream.wait_for_disconnect(),
        "dropping the consumer stream must cancel the idle provider response pump"
    );
}
