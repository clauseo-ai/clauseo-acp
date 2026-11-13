//! What the host does when an agent replays a conversation on `session/load` with a
//! `session/update` line over the ACP reader's 8 MiB cap (`clauseo_acp`'s `MAX_LINE_BYTES`).
//!
//! The reader drops that line up to its newline, logs one WARN with its size and method (never
//! its content) and keeps the connection, so the load's answer still arrives: the session
//! attaches as it was stored, with no fallback `session/new`, and the prompt that reopened the
//! conversation ends `end_turn`, as does the next one, on the same agent. The replayed update
//! is lost, which costs nothing: the host keeps its own transcript. A line over the cap that is
//! not a `session/update` notification (a response, a request) still ends the connection; the
//! acp crate's `line/tests/oversize.rs` covers that.
//!
//! Before the fix the line ended the connection, the load and its fallback failed, every
//! reopening prompt failed `agent.exited`, and since the thread kept its stored session id the
//! conversation could never reopen. The launch cap stays as a guard against a relaunch loop.
//!
//! The helpers here serve the other oversize-line tests too (`oversize_live.rs`,
//! `max_prompt_reopen.rs`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher};
use clauseo_protocol::{DomainEvent, StopReason, StreamId};
use serde_json::{Value, json};

use crate::observe::Events;
use crate::support::{Dir, FAKE, Scenario};
use crate::write;

/// The ACP reader's cap on one inbound line, newline excluded (`crates/acp/src/line.rs`).
pub(crate) const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

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

/// A subscriber that keeps the text (message and fields) of every WARN and ERROR event. Set as
/// the global default: nextest runs each test in a process of its own.
pub(crate) struct Warns(pub(crate) Arc<Mutex<Vec<String>>>);

impl Warns {
    /// Sets one as this process's subscriber and returns its lines.
    pub(crate) fn keep() -> Arc<Mutex<Vec<String>>> {
        let lines = Arc::new(Mutex::new(Vec::new()));
        tracing::subscriber::set_global_default(Self(Arc::clone(&lines)))
            .expect("the test's own process sets the subscriber (run it with nextest)");
        lines
    }
}

impl tracing::Subscriber for Warns {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() <= tracing::Level::WARN
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Text(String);
        impl tracing::field::Visit for Text {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                use std::fmt::Write as _;
                let _written = write!(self.0, " {}={value:?}", field.name());
            }
        }
        let mut text = Text(event.metadata().target().to_owned());
        event.record(&mut text);
        self.0.lock().unwrap().push(text.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// The message of the reader's WARN for a skipped line (`crates/acp/src/line.rs`).
pub(crate) const SKIPPED: &str = "skipped a message from the agent over the line cap";

/// The lines of `warns` that report a skipped line.
pub(crate) fn skips(warns: &Mutex<Vec<String>>) -> Vec<String> {
    let lines = warns.lock().unwrap();
    lines
        .iter()
        .filter(|line| line.contains(SKIPPED))
        .cloned()
        .collect()
}

/// The `initialize` exchange of a fake that can load sessions.
pub(crate) fn handshake() -> [Value; 2] {
    [
        json!({"expect": {"method": "initialize", "as": "init",
            "params": {"protocolVersion": 1}}}),
        json!({"respond": {"to": "init", "result": {"protocolVersion": 1,
            "agentCapabilities": {"loadSession": true}, "authMethods": [],
            "agentInfo": {"name": "fake_agent", "version": "0.0.0"}}}}),
    ]
}

/// Writes a scenario named `name` (header, `initialize`, then `steps`) next to `folder` and
/// makes it the fake's. Written at run time: its lines can be megabytes long.
pub(crate) fn write_scenario(dir: &Dir, folder: &Path, name: &str, steps: Vec<Value>) {
    let header = json!({"scenario": name, "description": "an oversize-line test's agent",
        "on_stdin_close": "exit"});
    let mut text = String::new();
    for line in [header].into_iter().chain(handshake()).chain(steps) {
        text.push_str(&line.to_string());
        text.push('\n');
    }
    let file: PathBuf = folder.with_file_name(format!("{name}.jsonl"));
    write::write(&file, text.as_bytes());
    dir.set_agents(&[(FAKE, Scenario::Written(file))]);
}

/// The agent sessions attached, in order.
pub(crate) fn attached(events: &Events) -> Vec<String> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::AgentSessionAttached(attached) => {
                Some(attached.session.as_str().to_owned())
            }
            _ => None,
        })
        .collect()
}

/// How many `TurnFailed` the stream holds.
pub(crate) fn failures(events: &Events) -> usize {
    events.count(|event| matches!(event, DomainEvent::TurnFailed(_)))
}

/// The steps of the `session/new` branch: one prompt answered `First answer.`.
pub(crate) fn first_session() -> Vec<Value> {
    vec![
        json!({"expect": {"method": "session/new", "as": "new", "params": {"cwd": "{{any}}"}}}),
        json!({"respond": {"to": "new", "result": {"sessionId": "sess-1"}}}),
        json!({"expect": {"method": "session/prompt", "as": "p1",
            "params": {"sessionId": "sess-1"}}}),
        json!({"update": {"update": {"sessionUpdate": "agent_message_chunk",
            "messageId": "m-1", "content": {"type": "text", "text": "First answer."}}}}),
        json!({"respond": {"to": "p1", "result": {"stopReason": "end_turn"}}}),
    ]
}

/// A `session/prompt` labelled `label` on `sess-1`, answered `text` and `end_turn`.
pub(crate) fn answered(label: &str, text: &str) -> [Value; 3] {
    [
        json!({"expect": {"method": "session/prompt", "as": label,
            "params": {"sessionId": "sess-1"}}}),
        json!({"update": {"update": {"sessionUpdate": "agent_message_chunk",
            "messageId": format!("m-{label}"), "content": {"type": "text", "text": text}}}}),
        json!({"respond": {"to": label, "result": {"stopReason": "end_turn"}}}),
    ]
}

/// The scenario: on `session/new` one turn that ends well; on `session/load` of `sess-1` the
/// replay of the lawyer's first prompt as one `session/update` line over the cap, then the
/// load's answer and two more prompts.
fn steps() -> Vec<Value> {
    let huge = json!({"jsonrpc": "2.0", "method": "session/update", "params": {
        "sessionId": "sess-1", "update": {"sessionUpdate": "user_message_chunk",
        "content": {"type": "text", "text": "x".repeat(MAX_LINE_BYTES + 1)}}}})
    .to_string();
    let mut load = vec![
        json!({"expect": {"method": "session/load", "as": "load",
            "params": {"sessionId": "sess-1", "cwd": "{{any}}"}}}),
        json!({"send_line": huge}),
        json!({"respond": {"to": "load", "result": {}}}),
    ];
    load.extend(answered("p2", "Second answer."));
    load.extend(answered("p3", "Third answer."));
    vec![json!({"one_of": [first_session(), load]})]
}

#[test]
fn a_replayed_update_over_the_cap_is_skipped_and_the_conversation_reopens() {
    let warns = Warns::keep();
    let dir = Dir::new("load_line_cap", &[]);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    write_scenario(&dir, &folder, "load-line-cap", steps());
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
    assert_eq!(skips(&warns), Vec::<String>::new());

    // Run 1: the host restarts and the lawyer asks again, which reopens with `session/load`.
    let host = dir.start_launching(1, Arc::clone(&launcher) as Arc<dyn Launcher>, |config| {
        config
    });
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    assert_eq!(attached(&events), ["sess-1"]);
    for (prompt, answer) in [
        ("Second question.", "Second answer."),
        ("Third question.", "Third answer."),
    ] {
        let (turn, _) = host.prompt(thread, prompt);
        let ended = events.until_turn_ended(turn);
        host.settle();
        events.drain_ready();
        events.assert_no_mismatch();
        assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
        assert!(events.turn_text(turn).contains(answer), "{}", events.dump());
    }
    // The load attached the stored session (the scenario has no `session/new` after a load),
    // once, and both prompts went to that one agent.
    assert_eq!(attached(&events), ["sess-1", "sess-1"], "{}", events.dump());
    assert_eq!(failures(&events), 0, "{}", events.dump());
    assert_eq!(launcher.launches(), 2);
    host.shutdown();
    assert_eq!(launcher.launches(), 2, "no launch after the last prompt");

    // One WARN, with the line's size and method and none of its content.
    let skipped = skips(&warns);
    assert_eq!(skipped.len(), 1, "{skipped:#?}");
    assert!(
        skipped[0].contains("method=\"session/update\""),
        "{}",
        skipped[0]
    );
    assert!(skipped[0].contains("bytes="), "{}", skipped[0]);
    let lines = warns.lock().unwrap();
    assert!(
        lines.iter().all(|line| !line.contains("xxxxxxxx")),
        "content logged"
    );
}
