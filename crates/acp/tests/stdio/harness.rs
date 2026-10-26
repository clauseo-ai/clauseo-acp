//! Starts `fake_agent` on a scenario over real pipes, connects `clauseo-acp` to it, and drives
//! both on one thread with `futures::executor::block_on`.
//!
//! A failing test prints the agent's stderr, so a `fake_agent: <file>:<line>:` mismatch shows
//! in its output.

use std::path::PathBuf;

use async_process::{Child, ChildStderr, Command, Stdio};
use clauseo_acp::{
    AcpClient, AcpEvent, AgentUpdate, ConnectOptions, Error, NewSession, Origin, SessionStart,
};
use clauseo_protocol::{AgentSessionId, ContentBlock, TurnId};
use futures::AsyncReadExt;

/// What a test body gets.
pub(crate) struct Ctx {
    pub(crate) client: AcpClient,
    pub(crate) events: async_channel::Receiver<AcpEvent>,
    /// The folder passed as the session's `cwd`.
    pub(crate) cwd: PathBuf,
}

impl Ctx {
    /// `initialize` then `session/new` in [`Ctx::cwd`], as `common/handshake.jsonl` expects.
    pub(crate) async fn handshake(&self) -> SessionStart {
        self.client.initialize().await.expect("initialize");
        self.client
            .new_session(self.new_session())
            .await
            .expect("session/new")
    }

    pub(crate) fn new_session(&self) -> NewSession {
        NewSession {
            cwd: self.cwd.clone(),
            mcp_servers: Vec::new(),
            meta: None,
        }
    }

    /// Every event received so far.
    pub(crate) fn drain(&self) -> Vec<AcpEvent> {
        std::iter::from_fn(|| self.events.try_recv().ok()).collect()
    }

    /// Every update received so far, with its origin; any other event fails the test.
    pub(crate) fn updates(&self) -> Vec<(Origin, AgentUpdate)> {
        self.drain()
            .into_iter()
            .map(|event| match event {
                AcpEvent::Update { origin, update, .. } => (origin, update),
                other => panic!("expected only updates, got {other:?}"),
            })
            .collect()
    }

    /// The next event, waiting for it.
    pub(crate) async fn next(&self) -> AcpEvent {
        self.events.recv().await.expect("the event channel closed")
    }
}

/// How the run ended once the test body returned and dropped its client.
#[derive(Debug)]
pub(crate) struct Outcome {
    pub(crate) driver: Result<(), Error>,
    pub(crate) exit: Option<i32>,
}

impl Outcome {
    /// The driver ended `Ok` and the agent exited 0: no mismatch.
    pub(crate) fn assert_clean(&self) {
        assert_eq!(self.driver, Ok(()), "driver result");
        assert_eq!(self.exit, Some(0), "fake_agent exit code");
    }
}

/// A bundled scenario's name, or the path of one of this crate's own.
pub(crate) fn local(name: &str) -> String {
    format!(
        "{}/tests/scenarios/{name}.jsonl",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// Plays `scenario` against the client the body drives.
pub(crate) fn run(scenario: &str, body: impl AsyncFnOnce(Ctx)) -> Outcome {
    let cwd = std::env::temp_dir();
    let bin = clauseo_fake_agent::bin_path().expect("fake_agent binary");
    let mut child = Command::new(bin)
        .args(["--scenario", scenario])
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("start fake_agent");
    let stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut guard = StderrOnPanic {
        stderr: child.stderr.take(),
        child: Some(child),
    };
    let (client, events, driver) = clauseo_acp::connect(stdin, stdout, ConnectOptions::default());
    let ctx = Ctx {
        client,
        events,
        cwd,
    };
    futures::executor::block_on(async {
        let (driver, ()) = futures::join!(driver, body(ctx));
        let mut child = guard.child.take().expect("child");
        let status = child.status().await.expect("fake_agent status");
        Outcome {
            driver,
            exit: status.code(),
        }
    })
}

/// Prints the agent's stderr when the test panics.
struct StderrOnPanic {
    stderr: Option<ChildStderr>,
    child: Option<Child>,
}

impl Drop for StderrOnPanic {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            return;
        }
        if let Some(child) = &mut self.child {
            drop(child.kill());
        }
        if let Some(stderr) = &mut self.stderr {
            let mut text = String::new();
            drop(futures::executor::block_on(
                stderr.read_to_string(&mut text),
            ));
            eprintln!("fake_agent stderr:\n{text}");
        }
    }
}

/// A fresh turn id: a fixed v7 uuid with `n` in its last bits.
pub(crate) fn turn(n: u128) -> TurnId {
    TurnId::new(uuid::Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0000 | n,
    ))
    .unwrap()
}

/// A text prompt.
pub(crate) fn text(text: &str) -> Vec<ContentBlock> {
    vec![ContentBlock::Text {
        text: text.to_owned(),
    }]
}

/// `sess-1`, the session every bundled scenario's handshake starts.
pub(crate) fn sess1() -> AgentSessionId {
    AgentSessionId::new("sess-1").unwrap()
}

/// The text of an answer, thought or user chunk.
pub(crate) fn chunk_text(
    update: &AgentUpdate,
) -> Option<(clauseo_acp::MessageRole, &str, Option<&str>)> {
    match update {
        AgentUpdate::MessageChunk {
            role,
            acp_message_id,
            content: ContentBlock::Text { text },
        } => Some((*role, text.as_str(), acp_message_id.as_deref())),
        _ => None,
    }
}
