//! The talk over in-memory pipes, against an app-server the test plays line by line with the
//! fixtures: the requests sent, the answers mapped, and stdin closed at the end.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use clauseo_codex_appserver::Error as AppServerError;
use clauseo_protocol::BillingProvider;
use futures::channel::mpsc;
use futures::executor::block_on;
use futures::future::join;
use futures::{AsyncRead, AsyncWrite, StreamExt as _};
use serde_json::{Value, json};

use super::{answers, fixture, pay_as_you_go, plus_outcome};
use crate::ReadOutcome;
use crate::readers::codex::{Talked, outcome, talk};

/// The app-server's stdin: each write is one message (the client writes a line at once).
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

/// The app-server's stdout: the chunks the test sends, in order.
struct Stdout {
    chunks: mpsc::UnboundedReceiver<Vec<u8>>,
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
                Some(chunk) => this.rest = chunk,
                None => return Poll::Ready(Ok(0)),
            }
        }
        let count = this.rest.len().min(buf.len());
        buf[..count].copy_from_slice(&this.rest[..count]);
        this.rest.drain(..count);
        Poll::Ready(Ok(count))
    }
}

/// The test's side: what the reader wrote, and what to write back.
struct Server {
    written: mpsc::UnboundedReceiver<Vec<u8>>,
    stdout: mpsc::UnboundedSender<Vec<u8>>,
}

impl Server {
    async fn read(&mut self) -> Value {
        let line = self.written.next().await.expect("the reader closed stdin");
        serde_json::from_slice(&line).unwrap()
    }

    async fn request(&mut self, method: &str) -> Value {
        let request = self.read().await;
        assert_eq!(request["method"], method, "{request}");
        request
    }

    /// Asserts the reader writes nothing more and closes stdin.
    async fn read_end(&mut self) {
        if let Some(line) = self.written.next().await {
            panic!(
                "expected stdin to close, got {}",
                String::from_utf8_lossy(&line)
            );
        }
    }

    fn send(&self, message: &Value) {
        let mut line = serde_json::to_vec(message).unwrap();
        line.push(b'\n');
        self.stdout.unbounded_send(line).unwrap();
    }

    fn respond(&self, request: &Value, result: &Value) {
        self.send(&json!({"id": request["id"], "result": result}));
    }

    /// Plays `initialize` and expects `initialized` after it.
    async fn handshake(&mut self) {
        let init = self.request("initialize").await;
        assert_eq!(init["params"]["clientInfo"]["name"], "clauseo");
        assert_eq!(init["params"]["capabilities"], json!({}));
        self.respond(
            &init,
            &json!({"codexHome": "/data/codex-home", "platformFamily": "unix",
                    "platformOs": "macos", "userAgent": "codex/0.162.1"}),
        );
        assert_eq!(self.read().await, json!({"method": "initialized"}));
    }
}

fn pipes() -> (Stdin, Stdout, Server) {
    let (written_tx, written) = mpsc::unbounded();
    let (stdout, chunks) = mpsc::unbounded();
    (
        Stdin(written_tx),
        Stdout {
            chunks,
            rest: Vec::new(),
        },
        Server { written, stdout },
    )
}

fn answered(talked: Talked) -> ReadOutcome {
    match talked {
        Talked::Answered(answers) => outcome(*answers),
        other => panic!("no answers: {other:?}"),
    }
}

#[test]
fn a_chatgpt_account_reads_its_limits_without_reset_credit_details() {
    let fixture = fixture(include_str!("../../../../fixtures/codex/plus.json"));
    let (stdin, stdout, mut server) = pipes();
    let script = async {
        server.handshake().await;
        let account = server.request("account/read").await;
        assert_eq!(account["params"], json!({}));
        server.respond(&account, &fixture["accountRead"]);
        let limits = server.request("account/rateLimits/read").await;
        assert_eq!(limits["params"], json!({"excludeResetCreditDetails": true}));
        server.respond(&limits, &fixture["rateLimitsRead"]);
        server.read_end().await;
    };
    let (talked, ()) = block_on(join(talk(stdin, stdout), script));
    let Talked::Answered(got) = &talked else {
        panic!("no answers: {talked:?}");
    };
    assert_eq!(**got, answers(&fixture));
    assert_eq!(answered(talked), plus_outcome());
}

#[test]
fn api_key_and_signed_out_accounts_ask_no_rate_limits() {
    for (text, expected) in [
        (
            include_str!("../../../../fixtures/codex/api-key.json"),
            pay_as_you_go("api-key", BillingProvider::OpenAi),
        ),
        (
            include_str!("../../../../fixtures/codex/signed-out.json"),
            ReadOutcome::SignedOut,
        ),
    ] {
        let fixture = fixture(text);
        let (stdin, stdout, mut server) = pipes();
        let script = async {
            server.handshake().await;
            let account = server.request("account/read").await;
            server.respond(&account, &fixture["accountRead"]);
            server.read_end().await;
        };
        let (talked, ()) = block_on(join(talk(stdin, stdout), script));
        assert_eq!(answered(talked), expected);
    }
}

#[test]
fn an_error_answer_fails_the_talk_and_still_closes_stdin() {
    let (stdin, stdout, mut server) = pipes();
    let script = async {
        server.handshake().await;
        let account = server.request("account/read").await;
        let message = format!("token sk{}", "-x");
        server.send(&json!({"id": account["id"], "error": {"code": -32000, "message": message}}));
        server.read_end().await;
    };
    let (talked, ()) = block_on(join(talk(stdin, stdout), script));
    assert!(
        matches!(
            talked,
            Talked::Failed(AppServerError::Rpc { code: -32000, .. })
        ),
        "{talked:?}"
    );
}

#[test]
fn stdout_ending_before_an_answer_is_closed() {
    let (stdin, stdout, mut server) = pipes();
    let script = async {
        let _init = server.request("initialize").await;
        drop(server.stdout);
    };
    let (talked, ()) = block_on(join(talk(stdin, stdout), script));
    assert!(
        matches!(talked, Talked::Failed(AppServerError::Closed)),
        "{talked:?}"
    );
}
