//! The talk over in-memory pipes, against an app-server the test plays line by line with the
//! fixtures: the requests sent, the answers mapped, and stdin closed at the end. The grace
//! `account/read` gets runs on a `ManualTimer` the test fires, so nothing sleeps.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use clauseo_codex_appserver::Error as AppServerError;
use clauseo_process::ManualTimer;
use clauseo_protocol::{AccountRef, Billing, BillingProvider, PlanLabel, WindowKind};
use futures::channel::mpsc;
use futures::executor::block_on;
use futures::future::join;
use futures::{AsyncRead, AsyncWrite, StreamExt as _};
use serde_json::{Value, json};

use super::{answers, fixture, pay_as_you_go, plus_outcome, plus_unread_outcome};
use crate::readers::codex::{ACCOUNT_GRACE, Talked, outcome, talk};
use crate::{AccountRead, ReadOutcome};

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

    fn fail(&self, request: &Value, code: i64, message: &str) {
        self.send(&json!({"id": request["id"], "error": {"code": code, "message": message}}));
    }

    /// Reads `account/read` then `account/rateLimits/read`, both written before any answer, and
    /// checks their params.
    async fn both_requests(&mut self) -> (Value, Value) {
        let account = self.request("account/read").await;
        assert_eq!(account["params"], json!({}));
        let limits = self.request("account/rateLimits/read").await;
        assert_eq!(limits["params"], json!({"excludeResetCreditDetails": true}));
        (account, limits)
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

/// The error codex 0.162.1's `account/read` gives when workspace routing discovery times out.
const DISCOVERY_TIMED_OUT: &str = "workspace routing discovery timed out";

fn plus() -> Value {
    fixture(include_str!("../../../../fixtures/codex/plus.json"))
}

#[test]
fn both_requests_are_on_the_wire_before_either_answer() {
    let fixture = plus();
    let (stdin, stdout, mut server) = pipes();
    let timer = ManualTimer::new();
    let script = async {
        server.handshake().await;
        // Nothing has been answered yet: both requests came regardless.
        let (account, limits) = server.both_requests().await;
        assert_ne!(account["id"], limits["id"]);
        server.respond(&account, &fixture["accountRead"]);
        server.respond(&limits, &fixture["rateLimitsRead"]);
        server.read_end().await;
    };
    let (talked, ()) = block_on(join(talk(stdin, stdout, &timer), script));
    assert!(matches!(talked, Talked::Answered(_)), "{talked:?}");
}

#[test]
fn a_chatgpt_account_reads_its_limits_without_reset_credit_details() {
    let fixture = plus();
    let (stdin, stdout, mut server) = pipes();
    let timer = ManualTimer::new();
    let script = async {
        server.handshake().await;
        let (account, limits) = server.both_requests().await;
        server.respond(&account, &fixture["accountRead"]);
        server.respond(&limits, &fixture["rateLimitsRead"]);
        server.read_end().await;
    };
    let (talked, ()) = block_on(join(talk(stdin, stdout, &timer), script));
    let Talked::Answered(got) = &talked else {
        panic!("no answers: {talked:?}");
    };
    assert_eq!(**got, answers(&fixture));
    assert_eq!(answered(talked), plus_outcome());
    assert_eq!(timer.fired(), 0);
}

#[test]
fn limits_first_then_account_read_within_the_grace_is_the_same_outcome() {
    let fixture = plus();
    let (stdin, stdout, mut server) = pipes();
    let timer = ManualTimer::new();
    let script = async {
        server.handshake().await;
        let (account, limits) = server.both_requests().await;
        server.respond(&limits, &fixture["rateLimitsRead"]);
        server.respond(&account, &fixture["accountRead"]);
        server.read_end().await;
    };
    let (talked, ()) = block_on(join(talk(stdin, stdout, &timer), script));
    assert_eq!(answered(talked), plus_outcome());
    assert_eq!(timer.fired(), 0);
}

#[test]
fn a_late_account_read_gives_the_limits_alone_after_the_grace() {
    let fixture = plus();
    let (stdin, stdout, mut server) = pipes();
    let timer = ManualTimer::new();
    let script = async {
        server.handshake().await;
        let (account, limits) = server.both_requests().await;
        server.respond(&limits, &fixture["rateLimitsRead"]);
        // The limits are in: the only wait the talk arms is the grace, and once it passes the
        // talk ends without `account/read`'s answer, so the read takes the limits' time plus the
        // grace, not discovery's.
        assert_eq!(timer.next_armed().await, ACCOUNT_GRACE);
        assert!(timer.fire());
        server.read_end().await;
        // `account` is never answered: discovery is still running when the talk ends.
        assert_eq!(account["method"], "account/read");
    };
    let (talked, ()) = block_on(join(talk(stdin, stdout, &timer), script));
    assert_eq!(timer.armed(), 1);
    assert_eq!(ACCOUNT_GRACE, std::time::Duration::from_millis(500));
    let outcome = answered(talked);
    assert_eq!(outcome, plus_unread_outcome());
    let ReadOutcome::Usage { limits, .. } = outcome else {
        unreachable!()
    };
    assert_eq!(limits.account.0, "plan");
    assert_eq!(
        limits.plan.as_ref().map(|plan| plan.0.as_str()),
        Some("ChatGPT Plus")
    );
    assert_eq!(limits.windows.len(), 2);
}

#[test]
fn account_read_failing_on_discovery_gives_the_limits_alone() {
    // Either order: the error before the limits, and after them inside the grace.
    for error_first in [true, false] {
        let fixture = plus();
        let (stdin, stdout, mut server) = pipes();
        let timer = ManualTimer::new();
        let script = async {
            server.handshake().await;
            let (account, limits) = server.both_requests().await;
            if error_first {
                server.fail(&account, -32603, DISCOVERY_TIMED_OUT);
                server.respond(&limits, &fixture["rateLimitsRead"]);
            } else {
                server.respond(&limits, &fixture["rateLimitsRead"]);
                server.fail(&account, -32603, DISCOVERY_TIMED_OUT);
            }
            server.read_end().await;
        };
        let (talked, ()) = block_on(join(talk(stdin, stdout, &timer), script));
        assert_eq!(
            answered(talked),
            plus_unread_outcome(),
            "error first: {error_first}"
        );
        assert_eq!(timer.fired(), 0);
    }
}

#[test]
fn api_key_and_signed_out_accounts_are_classified_by_account_read() {
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
        // The limits' refusal before `account/read`, and after it.
        for refusal_first in [true, false] {
            let fixture = fixture(text);
            let (stdin, stdout, mut server) = pipes();
            let timer = ManualTimer::new();
            let script = async {
                server.handshake().await;
                let (account, limits) = server.both_requests().await;
                if refusal_first {
                    server.fail(&limits, -32600, "codex account authentication required");
                    server.respond(&account, &fixture["accountRead"]);
                } else {
                    server.respond(&account, &fixture["accountRead"]);
                }
                server.read_end().await;
            };
            let (talked, ()) = block_on(join(talk(stdin, stdout, &timer), script));
            assert_eq!(answered(talked), expected);
            assert_eq!(timer.armed(), 0);
        }
    }
}

#[test]
fn a_chatgpt_account_whose_limits_fail_fails_the_talk() {
    let fixture = plus();
    let (stdin, stdout, mut server) = pipes();
    let script = async {
        server.handshake().await;
        let (account, limits) = server.both_requests().await;
        server.fail(&limits, -32000, "usage endpoint failed");
        server.respond(&account, &fixture["accountRead"]);
        server.read_end().await;
    };
    let (talked, ()) = block_on(join(talk(stdin, stdout, &ManualTimer::new()), script));
    assert!(
        matches!(
            talked,
            Talked::Failed(AppServerError::Rpc { code: -32000, .. })
        ),
        "{talked:?}"
    );
}

#[test]
fn an_error_answer_fails_the_talk_and_still_closes_stdin() {
    let (stdin, stdout, mut server) = pipes();
    let script = async {
        server.handshake().await;
        let (account, limits) = server.both_requests().await;
        let message = format!("token sk{}", "-x");
        server.fail(&account, -32000, &message);
        server.fail(&limits, -32600, "refused");
        server.read_end().await;
    };
    let (talked, ()) = block_on(join(talk(stdin, stdout, &ManualTimer::new()), script));
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
    let (talked, ()) = block_on(join(talk(stdin, stdout, &ManualTimer::new()), script));
    assert!(
        matches!(talked, Talked::Failed(AppServerError::Closed)),
        "{talked:?}"
    );
}

/// Plays a recorded app-server stdout line by line: a response goes out only once the reader has
/// sent the request with its id, as the app-server answers, and the other lines go out at once.
async fn replay(server: &mut Server, recorded: &str) {
    for line in recorded.lines() {
        let message: Value = serde_json::from_str(line).unwrap();
        if message.get("result").is_some() {
            loop {
                let sent = server.read().await;
                if sent["id"] == message["id"] {
                    break;
                }
            }
        }
        server
            .stdout
            .unbounded_send(format!("{line}\n").into_bytes())
            .unwrap();
    }
    server.read_end().await;
}

#[test]
fn a_recorded_plus_session_decodes_to_plus_limits() {
    let recorded = include_str!("../../../../fixtures/recorded/codex/plus.jsonl");
    let (stdin, stdout, mut server) = pipes();
    let timer = ManualTimer::new();
    let (talked, ()) = block_on(join(
        talk(stdin, stdout, &timer),
        replay(&mut server, recorded),
    ));
    let ReadOutcome::Usage {
        limits,
        reached: None,
        account_read: AccountRead::Read,
    } = answered(talked)
    else {
        panic!("not usage with no reached type");
    };
    assert_eq!(limits.account, AccountRef("test@example.com".to_owned()));
    assert_eq!(limits.plan, Some(PlanLabel("ChatGPT Plus".to_owned())));
    assert_eq!(limits.billing, Billing::Plan);
    let shape: Vec<(WindowKind, Option<u32>)> = limits
        .windows
        .iter()
        .map(|window| (window.kind, window.duration_mins))
        .collect();
    assert_eq!(
        shape,
        [
            (WindowKind::Session, Some(300)),
            (WindowKind::Weekly, Some(10080))
        ]
    );
}
