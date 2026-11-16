//! The pickers against options the agent reports while a session starts (`actor/turn/picks.rs`):
//! an agent may send a `config_option_update` at its defaults before it answers `session/new`
//! or `session/resume`, as Claude and `ChatGPT` may. A new thread's `CreateThread::config` (model
//! and effort, for Claude's shape and Codex's), a resumed thread's own values, and a pick made
//! while the session starts all still reach the agent before the first prompt, and the options
//! the thread logs (the ones its header and pickers show) are the picks. Each scenario takes
//! the prompt only after the picks, so a pick left out is a mismatch. The early-update ones run
//! paced ([`paced`]), so the options update is durable in the thread's log before the answer
//! comes, as with an agent slow to answer (stage C saw it with a cold Codex app-server).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher};
use clauseo_protocol::{
    AgentId, CommandEnvelope, CommandOutcome, ConfigPicks, HostCommand, HostError, MatterId,
    StopReason, StreamId, ThreadId,
};
use futures::FutureExt;

use crate::codex::CODEX;
use crate::matters::summary;
use crate::pickers::{
    CLAUDE, claude_prefs, create_thread, current, logged, picks, replaced, set, turn,
};
use crate::support::{Dir, Running, Scenario, command_id, send_prompt, wait};
use crate::write;

/// The pause `--pace-ms` puts between the options update and the update after it, which the
/// session's answer follows: ample for the host to log the first.
const PACE_MS: &str = "500";

/// Writes `settings.json` so `agent` plays bundled `scenario` paced by [`PACE_MS`]; read at
/// the next start.
fn paced(dir: &Dir, agent: &str, scenario: &str) {
    let bin = clauseo_fake_agent::bin_path()
        .unwrap_or_else(|e| panic!("{e} (run cargo build -p clauseo-fake_agent)"));
    let mut agents = serde_json::Map::new();
    agents.insert(
        agent.to_owned(),
        serde_json::json!({
            "command": bin.display().to_string(),
            "args": ["--scenario", scenario, "--pace-ms", PACE_MS],
        }),
    );
    let settings = serde_json::json!({ "agents": agents });
    write::write(
        &dir.data().join("settings.json"),
        serde_json::to_string_pretty(&settings).unwrap().as_bytes(),
    );
}

/// A Codex thread in `matter` with `config`.
fn codex_thread(host: &Running, matter: MatterId, config: ConfigPicks) -> ThreadId {
    let outcome = host.ok(HostCommand::CreateThread {
        matter,
        agent: AgentId::new(CODEX).unwrap(),
        config: Some(config),
    });
    let CommandOutcome::ThreadCreated { thread } = outcome else {
        panic!("CreateThread answered {outcome:?}");
    };
    thread
}

/// (a) Claude's shape: model haiku and effort low from `CreateThread::config` are set before
/// the first prompt over the defaults the agent reported first, and the one logged list is the
/// attach's, with both picks: the update sent before the answer is older than it.
#[test]
fn create_thread_config_wins_over_an_options_update_before_the_session() {
    let dir = Dir::new(
        "pickers_early",
        &[(CLAUDE, Scenario::Bundled("pickers-early"))],
    );
    paced(&dir, CLAUDE, "pickers-early");
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let config = picks(&[("model", "haiku"), ("effort", "low")]);
    let thread = create_thread(&host, matter, Some(config));
    let events = turn(&host, thread, "Summarise clause 4.");
    let lists = replaced(&events);
    assert_eq!(lists.len(), 1, "{}", events.dump());
    assert_eq!(current(&lists[0], "model").as_deref(), Some("haiku"));
    assert_eq!(current(&lists[0], "effort").as_deref(), Some("low"));
    host.shutdown();
}

/// (b) Codex's shape (its own option ids, no Default rows, the ask mode switched first): model
/// gpt-6-luna and effort low are set before the first prompt over the defaults it reported
/// first, and the thread logs them.
#[test]
fn codex_create_thread_config_wins_over_an_options_update_before_the_session() {
    let dir = Dir::new(
        "pickers_codex_early",
        &[(CODEX, Scenario::Bundled("pickers-codex-early"))],
    );
    paced(&dir, CODEX, "pickers-codex-early");
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let config = picks(&[("model", "gpt-6-luna"), ("reasoning_effort", "low")]);
    let thread = codex_thread(&host, matter, config);
    let events = turn(&host, thread, "Summarise clause 4.");
    let lists = replaced(&events);
    assert_eq!(lists.len(), 1, "{}", events.dump());
    assert_eq!(current(&lists[0], "model").as_deref(), Some("gpt-6-luna"));
    assert_eq!(
        current(&lists[0], "reasoning_effort").as_deref(),
        Some("low")
    );
    host.shutdown();
}

/// (c) A conversation reopened with `session/resume` gets its own model and effort back even
/// when the agent reports its defaults before answering.
#[test]
fn a_resumed_session_gets_the_threads_values_back_over_an_early_update() {
    let dir = Dir::new(
        "pickers_resume",
        &[(CLAUDE, Scenario::Bundled("pickers-picked"))],
    );
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let config = picks(&[("model", "opus"), ("effort", "high")]);
    let thread = create_thread(&host, matter, Some(config));
    drop(turn(&host, thread, "Summarise clause 4."));
    host.shutdown();

    paced(&dir, CLAUDE, "pickers-resume");
    let host = dir.start(1);
    let events = turn(&host, thread, "And clause 5?");
    let options = logged(&events);
    assert_eq!(current(&options, "model").as_deref(), Some("opus"));
    assert_eq!(current(&options, "effort").as_deref(), Some("high"));
    host.shutdown();
}

/// A launcher that holds each launch until [`Gate::open`], so a command can wait for the
/// session before the agent exists.
struct Gate {
    inner: PassthroughLauncher,
    open: Arc<(Mutex<bool>, Condvar)>,
}

impl Gate {
    fn open(open: &(Mutex<bool>, Condvar)) {
        *open.0.lock().unwrap() = true;
        open.1.notify_all();
    }
}

impl Launcher for Gate {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        let (lock, opened) = &*self.open;
        let held = lock.lock().unwrap();
        let wait = opened.wait_timeout_while(held, Duration::from_secs(60), |open| !*open);
        drop(wait.unwrap());
        self.inner.launch(spec)
    }
}

/// Polls `command`'s dispatch once, which sends it to the host, and returns it to await later.
fn sent(
    host: &Running,
    command: HostCommand,
) -> Pin<Box<impl Future<Output = Result<CommandOutcome, HostError>> + '_>> {
    let envelope = CommandEnvelope {
        command_id: command_id(),
        command,
    };
    let mut dispatch = Box::pin(host.handle.dispatch(envelope));
    let mut cx = Context::from_waker(futures::task::noop_waker_ref());
    assert!(matches!(dispatch.poll_unpin(&mut cx), Poll::Pending));
    dispatch
}

/// (d) A pick made after the first send, while the session starts: the lawyer's model opus
/// replaces the draft's haiku and is set before the first prompt with the draft's effort low,
/// its command answers once the session holds it, and the matter's prefs keep it as a pick
/// answered later does. The launch is held until the pick has been sent to the host.
#[test]
fn a_pick_made_while_the_session_starts_is_set_before_the_first_prompt() {
    let dir = Dir::new(
        "pickers_starting",
        &[(CLAUDE, Scenario::Bundled("pickers-starting"))],
    );
    let open = Arc::new((Mutex::new(true), Condvar::new()));
    let gate = Gate {
        inner: dir.launcher(),
        open: Arc::clone(&open),
    };
    let host = dir.start_launching(0, Arc::new(gate), |config| config);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let config = picks(&[("model", "haiku"), ("effort", "low")]);
    let thread = create_thread(&host, matter, Some(config));
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    *open.0.lock().unwrap() = false;

    let prompt = sent(&host, send_prompt(thread, "Summarise clause 4."));
    let pick = sent(&host, set(thread, "model", "opus"));
    Gate::open(&open);
    let CommandOutcome::PromptAccepted { turn, .. } = wait("the prompt", prompt).unwrap() else {
        panic!("SendPrompt answered another outcome");
    };
    assert_eq!(wait("the pick", pick).unwrap(), CommandOutcome::Done);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let options = logged(&events);
    assert_eq!(current(&options, "model").as_deref(), Some("opus"));
    assert_eq!(current(&options, "effort").as_deref(), Some("low"));
    assert_eq!(
        summary(&host, matter).agent_prefs,
        Some(claude_prefs(&[("model", "opus")]))
    );
    host.shutdown();
}
