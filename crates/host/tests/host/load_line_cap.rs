//! Documents what the host does TODAY when an agent sends a line over the ACP reader's 8 MiB cap
//! (`clauseo_acp`'s `MAX_LINE_BYTES`) while it replays a conversation on `session/load`. This
//! test pins current behaviour; it is not a statement that the behaviour is right.
//!
//! Today: the line ends the connection (`Error::Protocol`, which only the host's debug log
//! shows). The pending `session/load` comes back `Closed`, and the host's fallback
//! `session/new` goes to the same dead connection and fails `Closed` too, so the launch fails
//! and the prompt that reopened the conversation ends `Failed` with `agent.exited` ("the
//! agent connection is closed"), not `agent.protocol_error`. No session attaches and no
//! detach is logged. There is no relaunch loop: one launch per prompt, no retry within the turn,
//! none after it. But the thread keeps its stored session id, so every later prompt launches the
//! agent and sends the same `session/load` again, which fails the same way: the conversation
//! can never reopen. That is the bug this test shows; an automatic relaunch loop would be a
//! worse one, and the launch cap here would stop it.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher};
use clauseo_protocol::{DomainEvent, ErrorCode, StopReason, StreamId, TurnId};
use serde_json::json;

use crate::observe::Events;
use crate::support::{Dir, FAKE, Scenario};
use crate::write;

/// The ACP reader's cap on one inbound line, newline excluded (`crates/acp/src/line.rs`).
const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

/// Launches past this many start a program that does not exist, so a relaunch loop, were
/// there one, would end instead of hanging the test.
const LAUNCH_CAP: usize = 6;

/// A launcher that counts launches and refuses to start the agent after [`LAUNCH_CAP`].
struct Counting {
    inner: PassthroughLauncher,
    launches: AtomicUsize,
}

impl Counting {
    fn launches(&self) -> usize {
        self.launches.load(Ordering::SeqCst)
    }
}

impl Launcher for Counting {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        let n = self.launches.fetch_add(1, Ordering::SeqCst) + 1;
        if n <= LAUNCH_CAP {
            return self.inner.launch(spec);
        }
        let program = spec.cwd().join("no-such-agent");
        let args: Vec<OsString> = Vec::new();
        let spec = LaunchSpec::new(program, args, spec.env().clone(), spec.cwd().to_path_buf())?;
        self.inner.launch(&spec)
    }
}

/// The scenario: on `session/new` one turn that ends well; on `session/load` of `sess-1` the
/// replay of the lawyer's first prompt as one `session/update` line over the cap, never
/// answered. Written at run time: the line is over 8 MiB.
fn scenario() -> String {
    let header = json!({"scenario": "load-line-cap", "description": "One turn on session/new; \
        on session/load a replayed user prompt over 8 MiB on one line", "on_stdin_close": "exit"});
    let init = json!({"expect": {"method": "initialize", "as": "init",
        "params": {"protocolVersion": 1}}});
    let init_answer = json!({"respond": {"to": "init", "result": {"protocolVersion": 1,
        "agentCapabilities": {"loadSession": true}, "authMethods": [],
        "agentInfo": {"name": "fake_agent", "version": "0.0.0"}}}});
    let huge = json!({"jsonrpc": "2.0", "method": "session/update", "params": {
        "sessionId": "sess-1", "update": {"sessionUpdate": "user_message_chunk",
        "content": {"type": "text", "text": "x".repeat(MAX_LINE_BYTES + 1)}}}})
    .to_string();
    let branches = json!({"one_of": [
        [
            {"expect": {"method": "session/new", "as": "new", "params": {"cwd": "{{any}}"}}},
            {"respond": {"to": "new", "result": {"sessionId": "sess-1"}}},
            {"expect": {"method": "session/prompt", "as": "p1",
                "params": {"sessionId": "sess-1"}}},
            {"update": {"update": {"sessionUpdate": "agent_message_chunk",
                "messageId": "m-1", "content": {"type": "text", "text": "First answer."}}}},
            {"respond": {"to": "p1", "result": {"stopReason": "end_turn"}}},
        ],
        [
            {"expect": {"method": "session/load", "as": "load",
                "params": {"sessionId": "sess-1", "cwd": "{{any}}"}}},
            {"send_line": huge},
        ],
    ]});
    let mut text = String::new();
    for line in [header, init, init_answer, branches] {
        text.push_str(&line.to_string());
        text.push('\n');
    }
    text
}

/// `turn`'s `TurnFailed` code, if logged.
fn failed_code(events: &Events, turn: TurnId) -> Option<ErrorCode> {
    events.events().find_map(|event| match event {
        DomainEvent::TurnFailed(failed) if failed.turn == turn => Some(failed.code),
        _ => None,
    })
}

/// `turn`'s `TurnFailed` detail, or empty.
fn failed_detail(events: &Events, turn: TurnId) -> String {
    events
        .events()
        .find_map(|event| match event {
            DomainEvent::TurnFailed(failed) if failed.turn == turn => failed.detail.clone(),
            _ => None,
        })
        .unwrap_or_default()
}

/// How many `AgentSessionAttached` the stream holds.
fn attached(events: &Events) -> usize {
    events.count(|event| matches!(event, DomainEvent::AgentSessionAttached(_)))
}

#[test]
fn a_replayed_line_over_the_cap_fails_each_reopening_prompt_without_a_loop() {
    let dir = Dir::new("load_line_cap", &[]);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let file: PathBuf = folder.with_file_name("load-line-cap.jsonl");
    write::write(&file, scenario().as_bytes());
    dir.set_agents(&[(FAKE, Scenario::Written(file))]);
    let launcher = Arc::new(Counting {
        inner: dir.launcher(),
        launches: AtomicUsize::new(0),
    });

    // Run 0: the conversation exists, its session `sess-1` stored.
    let host = dir.start_launching(0, Arc::clone(&launcher) as Arc<dyn Launcher>, |config| {
        config
    });
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "First question.");
    let ended = events.until_turn_ended(first);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(launcher.launches(), 1);
    host.shutdown();

    // Run 1: the host restarts and the lawyer asks again, which reopens with `session/load`.
    let host = dir.start_launching(1, Arc::clone(&launcher) as Arc<dyn Launcher>, |config| {
        config
    });
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let attached_before = attached(&events);
    for (n, prompt) in ["Second question.", "Third question."].iter().enumerate() {
        let (turn, _) = host.prompt(thread, prompt);
        let ended = events.until_turn_ended(turn);
        host.settle();
        events.drain_ready();
        events.assert_no_mismatch();
        // Each prompt ends failed as if the agent exited, after exactly one launch.
        assert_eq!(ended.stop, StopReason::Failed, "{}", events.dump());
        assert_eq!(
            failed_code(&events, turn),
            Some(ErrorCode::AgentExited),
            "{}",
            events.dump()
        );
        assert_eq!(
            failed_detail(&events, turn),
            "acp: the agent connection is closed"
        );
        assert_eq!(
            launcher.launches(),
            2 + n,
            "launches after prompt {}",
            n + 2
        );
    }
    // No session ever attached on run 1 (neither the load nor the fallback `session/new`), so
    // no detach is logged either.
    assert_eq!(attached(&events), attached_before, "{}", events.dump());
    let detached = |events: &Events| {
        events.count(|event| matches!(event, DomainEvent::AgentSessionDetached(_)))
    };
    assert_eq!(detached(&events), 1, "only run 0's: {}", events.dump());
    host.shutdown();
    assert_eq!(launcher.launches(), 3, "no launch after the last prompt");
}
