//! The client over in-memory pipes: [`pipes`] connects it, and [`Agent`] is the test's side.

use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll};

use clauseo_acp::{AcpClient, AcpEvent, ConnectOptions, Driver, NewSession};
use clauseo_protocol::{AgentSessionId, TurnId};
use futures::channel::mpsc;
use futures::{AsyncRead, AsyncWrite, StreamExt};
use serde_json::{Value, json};

/// The agent's stdin: each write arrives as one message (the client writes a line at once).
struct Stdin(mpsc::UnboundedSender<Vec<u8>>);

impl AsyncWrite for Stdin {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        drop(self.0.unbounded_send(buf.to_vec()));
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.0.close_channel();
        Poll::Ready(Ok(()))
    }
}

/// The agent's stdout: the chunks the test sends, read in order. A chunk that is an error fails
/// that one read only, as a pipe read interrupted by a signal does, and reading goes on.
struct Stdout {
    chunks: mpsc::UnboundedReceiver<io::Result<Vec<u8>>>,
    /// What is left of the chunk being read.
    rest: Vec<u8>,
}

impl AsyncRead for Stdout {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        while this.rest.is_empty() {
            match futures::ready!(this.chunks.poll_next_unpin(cx)) {
                Some(Ok(chunk)) => this.rest = chunk,
                Some(Err(error)) => return Poll::Ready(Err(error)),
                None => return Poll::Ready(Ok(0)),
            }
        }
        let count = this.rest.len().min(buf.len());
        buf[..count].copy_from_slice(&this.rest[..count]);
        this.rest.drain(..count);
        Poll::Ready(Ok(count))
    }
}

/// The test's side of the agent: what the client wrote, and what to write back.
pub(crate) struct Agent {
    pub(crate) written: mpsc::UnboundedReceiver<Vec<u8>>,
    pub(crate) stdout: mpsc::UnboundedSender<io::Result<Vec<u8>>>,
}

impl Agent {
    /// The next line the client wrote.
    pub(crate) async fn read(&mut self) -> Value {
        let line = self.written.next().await.expect("the client closed stdin");
        assert_eq!(line.last(), Some(&b'\n'));
        serde_json::from_slice(&line).unwrap()
    }

    /// Asserts the client writes nothing more and closes stdin.
    pub(crate) async fn read_end(&mut self) {
        if let Some(line) = self.written.next().await {
            panic!(
                "expected stdin to close, got {}",
                String::from_utf8_lossy(&line)
            );
        }
    }

    pub(crate) fn send(&self, message: &Value) {
        let mut line = serde_json::to_vec(message).unwrap();
        line.push(b'\n');
        self.stdout.unbounded_send(Ok(line)).unwrap();
    }

    pub(crate) fn respond(&self, request: &Value, result: &Value) {
        self.send(&json!({"jsonrpc": "2.0", "id": request["id"], "result": result}));
    }

    /// Answers `initialize` with version 1 and `capabilities` as its `agentCapabilities`.
    pub(crate) async fn initialize_with(&mut self, capabilities: Value) {
        let init = self.read().await;
        assert_eq!(init["method"], "initialize");
        self.respond(
            &init,
            &json!({"protocolVersion": 1, "agentCapabilities": capabilities}),
        );
    }

    /// Answers `initialize` with no capabilities, then `session/new` with session `s-1`.
    pub(crate) async fn start(&mut self) {
        self.start_with(json!({})).await;
    }

    /// Answers `initialize` with `capabilities`, then `session/new` with session `s-1`.
    pub(crate) async fn start_with(&mut self, capabilities: Value) {
        self.initialize_with(capabilities).await;
        let new = self.read().await;
        assert_eq!(new["method"], "session/new");
        self.respond(&new, &json!({"sessionId": "s-1"}));
    }

    pub(crate) fn update(&self, update: &Value) {
        self.send(&json!({"jsonrpc": "2.0", "method": "session/update",
            "params": {"sessionId": "s-1", "update": update}}));
    }
}

pub(crate) fn pipes() -> (AcpClient, async_channel::Receiver<AcpEvent>, Driver, Agent) {
    let (written_tx, written) = mpsc::unbounded();
    let (stdout, stdout_rx) = mpsc::unbounded();
    let (client, events, driver) = clauseo_acp::connect(
        Stdin(written_tx),
        Stdout {
            chunks: stdout_rx,
            rest: Vec::new(),
        },
        ConnectOptions::default(),
    );
    (client, events, driver, Agent { written, stdout })
}

/// `initialize` then `session/new`, which [`Agent::start`] answers.
pub(crate) async fn handshake(client: &AcpClient) {
    client.initialize().await.unwrap();
    let start = client.new_session(NewSession {
        cwd: PathBuf::from("/matters/lease"),
        mcp_servers: Vec::new(),
        meta: None,
    });
    assert_eq!(start.await.unwrap().session, session());
}

pub(crate) fn session() -> AgentSessionId {
    AgentSessionId::new("s-1").unwrap()
}

/// A fresh turn id: a fixed v7 uuid with `n` in its last bits.
pub(crate) fn turn(n: u128) -> TurnId {
    TurnId::new(uuid::Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0000 | n,
    ))
    .unwrap()
}

/// Every event the receiver holds now.
pub(crate) fn drain(events: &async_channel::Receiver<AcpEvent>) -> Vec<AcpEvent> {
    std::iter::from_fn(|| events.try_recv().ok()).collect()
}
