//! `stop_agents` waits and holds (decision record 0048, P4d): it answers once the agent id's
//! processes have exited, or once its bounded wait has run out, and says which. Until
//! `restart_agents`, nothing of that id launches: a prompt that would start at once queues held,
//! marked for the sign-in as P4b's turn put back is (#408's hold), and `restart_agents` lifts the
//! hold and sends it once.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_host::AgentsStopped;
use clauseo_protocol::{AgentId, DomainEvent, MatterId, StopReason, StreamId, ThreadId, TurnId};
use futures::future::{Either, select};

use crate::codex::Recording;
use crate::observe::Events;
use crate::queue::{held, starts};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, wait};

/// The first prompt; the agent answers it and ends the turn.
const ASKED: &str = "Look at the matter.";

/// The prompt sent while the agent is held.
const WHILE_HELD: &str = "Sent while held.";

/// `stop_agents`' bounded wait (`STOP_WAIT` in the host's `turn/sign_in/stops.rs`).
const STOP_WAIT: Duration = Duration::from_secs(10);

/// The fake agent's id.
fn fake() -> AgentId {
    AgentId::new(FAKE).unwrap()
}

/// A host whose fake agent answers one prompt, is stopped idle, and, relaunched, loads its
/// session and answers the prompt sent while held; the launches are counted.
struct Held {
    dir: Dir,
    host: Running,
    /// Each launch's environment, as the recording launcher kept it.
    launches: Arc<Mutex<Vec<BTreeMap<OsString, OsString>>>>,
    matter: MatterId,
    thread: ThreadId,
    events: Events,
    /// The turn of the prompt sent while held.
    queued: TurnId,
}

/// One prompt answered, `stop_agents` (the idle agent exits when its stdin closes), then a
/// prompt sent while held: it is accepted and queued, held, and nothing launches.
fn held_prompt(test: &str) -> Held {
    let dir = Dir::new(test, &[(FAKE, Scenario::Local("stop-agents-hold"))]);
    let recording = Recording::new(&dir);
    let launches = Arc::clone(&recording.envs);
    let host = dir.start_launching(0, Arc::new(recording), |config| config);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, ASKED);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    host.settle();

    let stopped = wait("stop_agents", host.handle.stop_agents(fake())).unwrap();
    assert_eq!(stopped, AgentsStopped::Exited);
    let (queued, _) = host.prompt(thread, WHILE_HELD);
    events.until("the prompt queued", |events| {
        events
            .position(|event| matches!(event, DomainEvent::TurnQueued(q) if q.turn() == queued))
            .is_some()
    });
    host.settle();
    events.drain_ready();
    Held {
        dir,
        host,
        launches,
        matter,
        thread,
        events,
        queued,
    }
}

#[test]
fn a_prompt_sent_while_stop_agents_holds_stays_queued_and_nothing_launches() {
    let Held {
        dir: _dir,
        host,
        launches,
        matter,
        thread,
        events,
        queued,
    } = held_prompt("stop_hold_queued");
    assert!(held(&host, matter, thread), "{}", events.dump());
    assert_eq!(starts(&events, queued), 0, "{}", events.dump());
    assert_eq!(launches.lock().unwrap().len(), 1, "no launch while held");
    events.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn restart_agents_lifts_the_hold_and_sends_the_held_prompt_once() {
    let Held {
        dir: _dir,
        host,
        launches,
        matter,
        thread,
        mut events,
        queued,
    } = held_prompt("stop_hold_restart");
    wait("restart_agents", host.handle.restart_agents(fake())).unwrap();
    assert_eq!(events.until_turn_ended(queued).stop, StopReason::EndTurn);
    host.settle();
    assert!(!held(&host, matter, thread));

    // A second restart resumes nothing.
    wait("restart_agents", host.handle.restart_agents(fake())).unwrap();
    host.settle();
    events.drain_ready();
    assert_eq!(starts(&events, queued), 1, "{}", events.dump());
    let started = events.count(|event| matches!(event, DomainEvent::TurnStarted(_)));
    assert_eq!(started, 2, "{}", events.dump());
    assert_eq!(launches.lock().unwrap().len(), 2, "one relaunch");
    events.assert_no_mismatch();
    host.shutdown();
}

/// Settings mapping the fake agent's id to a shell that ignores SIGTERM and then runs the fake
/// agent (an ignored signal stays ignored across `exec`), playing `stop-agents-stubborn`, which
/// also ignores its stdin closing: only the kill ends it.
#[cfg(unix)]
fn stubborn(dir: &Dir) {
    let bin = clauseo_fake_agent::bin_path()
        .unwrap_or_else(|e| panic!("{e} (run cargo build -p clauseo-fake_agent)"));
    let scenario = Scenario::Local("stop-agents-stubborn").arg();
    let settings = serde_json::json!({
        "agents": {
            FAKE: {
                "command": "/bin/sh",
                "args": [
                    "-c",
                    "trap '' TERM; exec \"$0\" \"$@\"",
                    bin.display().to_string(),
                    "--scenario",
                    scenario,
                ],
            },
        },
    });
    crate::write::write(
        &dir.data().join("settings.json"),
        serde_json::to_string_pretty(&settings).unwrap().as_bytes(),
    );
}

#[cfg(unix)]
#[test]
fn a_stubborn_agent_still_gets_stop_agents_its_answer_once_the_wait_runs_out() {
    let dir = Dir::new("stop_hold_stubborn", &[]);
    stubborn(&dir);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, ASKED);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    host.settle();

    // The agent ignores its stdin closing and SIGTERM. One move of the clock past the wait
    // fires it and the ladder's first step only: the ladder's kill is 3 s after its SIGTERM.
    let clock = host.clock.clone();
    let mover = async move {
        while clock.next_armed().await != STOP_WAIT {}
        clock.advance(STOP_WAIT);
    };
    let stopped = wait("stop_agents", async {
        let stop = pin!(host.handle.stop_agents(fake()));
        match select(stop, pin!(mover)).await {
            Either::Left((stopped, _mover)) => stopped,
            Either::Right(((), stop)) => stop.await,
        }
    })
    .unwrap();
    assert_eq!(stopped, AgentsStopped::TimedOut);

    // The ladder goes on to the kill, which ends the agent.
    for _ in 0..10 {
        host.clock.advance(Duration::from_secs(1));
        host.settle();
    }
    events.until("the agent ends", |events| {
        events
            .position(|event| matches!(event, DomainEvent::AgentSessionDetached(_)))
            .is_some()
    });
    host.shutdown();
}
