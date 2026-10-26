//! The driver over in-memory pipes, where a test plays the agent line by line.

use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll};

use clauseo_protocol::{AgentSessionId, ContentBlock, ErrorCode, TextOrBlob, TurnId};
use futures::channel::mpsc;
use futures::{AsyncWrite, StreamExt, TryStreamExt};
use serde_json::{Value, json};

use super::*;

mod interface;

/// The agent's stdin: each write arrives as one message (the line sink writes a line at once).
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

/// The test's side of the agent: what the client wrote, and what to write back.
struct Agent {
    written: mpsc::UnboundedReceiver<Vec<u8>>,
    stdout: mpsc::UnboundedSender<io::Result<Vec<u8>>>,
}

impl Agent {
    async fn read(&mut self) -> Value {
        let line = self.written.next().await.expect("the client closed stdin");
        assert_eq!(line.last(), Some(&b'\n'));
        serde_json::from_slice(&line).unwrap()
    }

    fn send(&self, message: &Value) {
        let mut line = serde_json::to_vec(message).unwrap();
        line.push(b'\n');
        self.stdout.unbounded_send(Ok(line)).unwrap();
    }

    fn respond(&self, request: &Value, result: &Value) {
        self.send(&json!({"jsonrpc": "2.0", "id": request["id"], "result": result}));
    }

    /// Answers `initialize` with version 1 and no capabilities.
    async fn initialize(&mut self) {
        let init = self.read().await;
        assert_eq!(init["method"], "initialize");
        self.respond(&init, &json!({"protocolVersion": 1}));
    }

    /// Answers `initialize`, then `session/new` with session `s-1`.
    async fn start(&mut self) {
        self.initialize().await;
        let new = self.read().await;
        assert_eq!(new["method"], "session/new");
        self.respond(&new, &json!({"sessionId": "s-1"}));
    }

    fn update(&self, params: &Value) {
        self.send(&json!({"jsonrpc": "2.0", "method": "session/update", "params": params}));
    }
}

fn pipes() -> (AcpClient, async_channel::Receiver<AcpEvent>, Driver, Agent) {
    pipes_with(ConnectOptions::default())
}

fn pipes_with(
    options: ConnectOptions,
) -> (AcpClient, async_channel::Receiver<AcpEvent>, Driver, Agent) {
    let (written_tx, written) = mpsc::unbounded();
    let (stdout, stdout_rx) = mpsc::unbounded();
    let (client, events, driver) = connect(Stdin(written_tx), stdout_rx.into_async_read(), options);
    (client, events, driver, Agent { written, stdout })
}

fn cwd() -> PathBuf {
    PathBuf::from("/matters/lease")
}

#[test]
fn new_session_sends_empty_mcp_servers_and_meta() {
    let (client, _events, driver, mut agent) = pipes();
    let mut meta = serde_json::Map::new();
    meta.insert("clauseo".to_owned(), json!({"profile": {"tone": "plain"}}));
    let calls = async move {
        client.initialize().await.unwrap();
        let start = client
            .new_session(NewSession {
                cwd: cwd(),
                mcp_servers: Vec::new(),
                meta: Some(meta),
            })
            .await
            .unwrap();
        assert_eq!(start.session.as_str(), "s-1");
        assert_eq!(start.modes, None);
        assert_eq!(start.config_options, Vec::new());
    };
    let script = async {
        agent.initialize().await;
        let new = agent.read().await;
        assert_eq!(new["method"], "session/new");
        assert_eq!(new["params"]["mcpServers"], json!([]));
        assert_eq!(new["params"]["cwd"], "/matters/lease");
        assert_eq!(
            new["params"]["_meta"],
            json!({"clauseo": {"profile": {"tone": "plain"}}})
        );
        agent.respond(&new, &json!({"sessionId": "s-1"}));
        // The client is dropped after its calls: stdin closes.
        assert!(agent.written.next().await.is_none());
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}

#[test]
fn line_over_8_mib_closes_connection() {
    let (client, _events, driver, agent) = pipes();
    // Nine 1 MiB reads and no newline: past the cap before any line ends.
    for _ in 0..9 {
        agent
            .stdout
            .unbounded_send(Ok(vec![b' '; 1024 * 1024]))
            .unwrap();
    }
    let calls = async move { client.initialize().await };
    let (driver, init) = futures::executor::block_on(async { futures::join!(driver, calls) });
    assert!(
        matches!(driver, Err(Error::Protocol(ref why)) if why.contains("over")),
        "{driver:?}"
    );
    assert_eq!(init.unwrap_err(), Error::Closed);
}

#[test]
fn prompt_content_is_checked_before_anything_is_sent() {
    let (client, _events, driver, mut agent) = pipes();
    let session = AgentSessionId::new("s-1").unwrap();
    let turn = TurnId::new(uuid::Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0001,
    ))
    .unwrap();
    let calls = async move {
        client.initialize().await.unwrap();
        // A session this connection never started.
        let unknown = client.prompt(&session, turn, Vec::new()).await;
        assert_eq!(unknown, Err(Error::UnknownSession));
        let start = client.new_session(NewSession {
            cwd: cwd(),
            mcp_servers: Vec::new(),
            meta: None,
        });
        assert_eq!(start.await.unwrap().session, session);
        // The agent allows no images, and a blob cannot go from here.
        let image = ContentBlock::Image {
            data: TextOrBlob::Inline("iVBO".to_owned()),
            mime_type: "image/png".into(),
            uri: None,
        };
        let refused = client.prompt(&session, turn, vec![image]).await;
        assert_eq!(refused, Err(Error::UnsupportedContent));
        let steer = client.steer(&session, Vec::new()).await;
        assert_eq!(steer, Err(Error::SteerUnsupported));
    };
    let script = async {
        agent.initialize().await;
        let new = agent.read().await;
        agent.respond(&new, &json!({"sessionId": "s-1"}));
        // Nothing else reaches the agent before stdin closes.
        assert!(agent.written.next().await.is_none());
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}

#[test]
fn error_codes_map() {
    let cases = [
        (Error::Closed, ErrorCode::AgentExited),
        (
            Error::UnsupportedVersion { agent: 2 },
            ErrorCode::AgentUnsupportedVersion,
        ),
        (
            Error::AuthRequired {
                methods: Vec::new(),
            },
            ErrorCode::AgentAuthRequired,
        ),
        (
            Error::Rpc {
                code: -32603,
                message: String::new(),
            },
            ErrorCode::AgentProtocolError,
        ),
        (
            Error::Protocol(String::new()),
            ErrorCode::AgentProtocolError,
        ),
        (Error::TurnRunning, ErrorCode::ThreadTurnRunning),
        (Error::PermissionNotPending, ErrorCode::PermissionNotPending),
        (Error::UnknownOption, ErrorCode::PermissionUnknownOption),
        (
            Error::ElicitationNotPending,
            ErrorCode::ElicitationNotPending,
        ),
        (Error::UnsupportedContent, ErrorCode::CommandInvalid),
        (Error::UnknownSession, ErrorCode::Internal),
        (Error::TerminalAuthMethod, ErrorCode::Internal),
        (Error::SteerUnsupported, ErrorCode::Internal),
    ];
    for (error, code) in cases {
        assert_eq!(error.code(), code, "{error:?}");
    }
}
