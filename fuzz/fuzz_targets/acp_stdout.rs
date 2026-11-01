//! acp: the agent's stdout bytes, read by `clauseo_acp::connect` through its line reader and its
//! JSON-RPC decoding (crates/acp/src: inbound.rs, map.rs and driver.rs), into `AcpEvent`s and
//! call results.
//!
//! Items driven: `connect` with `ConnectOptions::default()`, and the driver future it returns;
//! `AcpClient::initialize`, `new_session`, `load_session`, `prompt`, `cancel`,
//! `answer_permission` and `answer_elicitation`; and the `AcpEvent` stream.
//!
//! Input: byte 0 = mode, byte 1 = read chunk size (1 to 256; 0 writes each segment whole), rest =
//! the agent's stdout.
//!   mode & 1 == 0 ("turn"): a scripted agent answers `initialize` and `session/new` (session
//!     "sess-1"). The client then sends `session/prompt`, and with mode & 2 also
//!     `session/cancel`. The fuzz bytes are what the agent writes from then on.
//!   mode & 1 == 1 ("handshake"): the client calls `initialize`, then `session/new` and
//!     `session/load` if those succeed. The fuzz bytes answer the handshake itself.
//! The SDK mints random UUID request ids, so the stdout bytes are split into segments at
//! "\n@@\n". Segment i is written after the client's i-th request, and each `"@ID"` in it is
//! replaced by that request's JSON-RPC id. When the client sends nothing within 256 polls, the
//! remaining segments are written at once. Then stdout ends (EOF). Every permission request is
//! answered with its first option, and every elicitation with Cancel.
//!
//! Invariants: no panic and no hang (a hang shows as a libFuzzer timeout). The driver ends at
//! EOF. An agent text chunk is at most 1 MiB, the cap the crate documents. Inputs are at most
//! 64 KiB (`-max_len`), so that cap cannot be reached here.
#![no_main]

use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll};

use clauseo_acp::{
    AcpClient, AcpEvent, AgentUpdate, ConnectOptions, LoadSession, NewSession, connect,
};
use clauseo_protocol::{AgentChoiceId, AgentSessionId, ContentBlock, ElicitationResponse, TurnId};
use futures::channel::mpsc;
use futures::{AsyncWrite, StreamExt, TryStreamExt};
use libfuzzer_sys::fuzz_target;

/// The cap on one text chunk that the crate documents (a text chunk is at most 1 MiB).
const MAX_TEXT_CHUNK: usize = 1024 * 1024;

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

fn send_chunked(out: &mpsc::UnboundedSender<io::Result<Vec<u8>>>, bytes: &[u8], step: usize) {
    if bytes.is_empty() {
        return;
    }
    let step = if step == 0 { bytes.len() } else { step };
    for chunk in bytes.chunks(step) {
        let _ = out.unbounded_send(Ok(chunk.to_vec()));
    }
}

fn respond(out: &mpsc::UnboundedSender<io::Result<Vec<u8>>>, request: &[u8], result: &str) {
    let request: serde_json::Value = serde_json::from_slice(request).expect("client wrote JSON");
    let id = serde_json::to_string(&request["id"]).unwrap();
    let line = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{result}}}\n");
    let _ = out.unbounded_send(Ok(line.into_bytes()));
}

/// Splits the agent's stdout into segments at each `\n@@\n`: segment i is written after the
/// client's i-th request (counting from the one the mode waits for).
fn split_segments(bytes: &[u8]) -> Vec<&[u8]> {
    const SEP: &[u8] = b"\n@@\n";
    let mut out = Vec::new();
    let mut rest = bytes;
    while let Some(at) = rest.windows(SEP.len()).position(|w| w == SEP) {
        out.push(&rest[..at + 1]);
        rest = &rest[at + SEP.len()..];
    }
    out.push(rest);
    out
}

/// Replaces each `"@ID"` with the JSON id of the client's latest request (the SDK mints
/// random UUID ids, so raw bytes could never answer a request otherwise).
fn substitute(segment: &[u8], id: &[u8]) -> Vec<u8> {
    const TOKEN: &[u8] = b"\"@ID\"";
    let mut out = Vec::with_capacity(segment.len());
    let mut rest = segment;
    while let Some(at) = rest.windows(TOKEN.len()).position(|w| w == TOKEN) {
        out.extend_from_slice(&rest[..at]);
        out.extend_from_slice(id);
        rest = &rest[at + TOKEN.len()..];
    }
    out.extend_from_slice(rest);
    out
}

fn request_id(line: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(line).ok()?;
    let id = value.get("id")?;
    value.get("method")?;
    serde_json::to_string(id).ok()
}

struct YieldNow(bool);

impl std::future::Future for YieldNow {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

async fn answer(client: &AcpClient, event: &AcpEvent) {
    match event {
        AcpEvent::PermissionRequested { key, options, .. } => {
            if let Some(first) = options.first()
                && let Ok(id) = AgentChoiceId::new(first.option_id.as_str())
            {
                let _ = client.answer_permission(*key, id).await;
            }
        }
        AcpEvent::ElicitationRequested { key, .. } => {
            let _ = client
                .answer_elicitation(*key, ElicitationResponse::Cancel)
                .await;
        }
        _ => {}
    }
}

fn new_session() -> NewSession {
    NewSession {
        cwd: PathBuf::from("/matters/fuzz"),
        mcp_servers: Vec::new(),
        meta: None,
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }
    let mode = data[0];
    let step = usize::from(data[1]);
    let stdout_bytes = &data[2..];

    let (written_tx, mut written) = mpsc::unbounded::<Vec<u8>>();
    let (stdout, stdout_rx) = mpsc::unbounded::<io::Result<Vec<u8>>>();
    let (client, events, driver) = connect(
        Stdin(written_tx),
        stdout_rx.into_async_read(),
        ConnectOptions::default(),
    );
    let client = &client;
    let session = AgentSessionId::new("sess-1").unwrap();
    let turn: TurnId = "01900000-0000-7000-8000-000000000001".parse().unwrap();

    let turn_mode = mode & 1 == 0;

    let calls = async {
        if turn_mode {
            if client.initialize().await.is_err() {
                return;
            }
            if client.new_session(new_session()).await.is_err() {
                return;
            }
            let _ = client.prompt(&session, turn, Vec::new()).await;
            if mode & 2 != 0 {
                let _ = client.cancel(&session).await;
            }
        } else {
            if client.initialize().await.is_err() {
                return;
            }
            if let Ok(start) = client.new_session(new_session()).await {
                let _ = client
                    .load_session(LoadSession {
                        session: start.session,
                        cwd: PathBuf::from("/matters/fuzz"),
                        mcp_servers: Vec::new(),
                    })
                    .await;
            }
        }
    };

    let script = async {
        let mut current: Option<Vec<u8>> = if turn_mode {
            let Some(init) = written.next().await else {
                return;
            };
            respond(
                &stdout,
                &init,
                r#"{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}"#,
            );
            let Some(new) = written.next().await else {
                return;
            };
            respond(&stdout, &new, r#"{"sessionId":"sess-1"}"#);
            // Wait for the prompt to be sent before the agent writes.
            written.next().await
        } else {
            written.next().await
        };
        let mut segments = split_segments(stdout_bytes).into_iter();
        while let Some(segment) = segments.next() {
            let id = current
                .as_deref()
                .and_then(request_id)
                .unwrap_or_else(|| "null".to_owned());
            send_chunked(&stdout, &substitute(segment, id.as_bytes()), step);
            // Give the driver and the calls a bounded number of polls to send their next
            // request; deterministic in this single-threaded executor.
            current = None;
            for _ in 0..256 {
                match written.try_recv() {
                    Ok(line) => {
                        current = Some(line);
                        break;
                    }
                    Err(e) if e.is_closed() => break,
                    Err(_) => YieldNow(false).await,
                }
            }
            if current.is_none() {
                for rest in segments.by_ref() {
                    send_chunked(&stdout, &substitute(rest, id.as_bytes()), step);
                }
            }
        }
        stdout.close_channel();
        // Keep draining what the client writes so nothing blocks.
        while written.next().await.is_some() {}
    };

    let drain = async {
        while let Ok(event) = events.recv().await {
            if let AcpEvent::Update {
                update:
                    AgentUpdate::MessageChunk {
                        content: ContentBlock::Text { text },
                        ..
                    },
                ..
            } = &event
            {
                assert!(
                    text.len() <= MAX_TEXT_CHUNK,
                    "text chunk over 1 MiB: {}",
                    text.len()
                );
            }
            answer(client, &event).await;
        }
    };

    let (_result, (), (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script, drain) });
});
