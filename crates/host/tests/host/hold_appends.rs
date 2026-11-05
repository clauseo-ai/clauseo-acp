//! Prompts sent while a hold for the sign-in applies (decision record 0048, P4b and P4d): while
//! `stop_agents` holds the agent, or the queue is held for the sign-in, every prompt sent while
//! nothing runs queues at the end, held, whatever its delivery (queue or send now), so nothing
//! launches and nothing fails; a second prompt keeps the queue held and marked, and
//! `restart_agents` sends them all once, in order (the host code owner's review of #461).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::{Arc, Mutex};

use clauseo_host::AgentsStopped;
use clauseo_protocol::{
    AgentId, DeliveryRequest, DomainEvent, MatterId, StopReason, StreamId, ThreadId, TurnId,
};

use crate::codex::Recording;
use crate::observe::Events;
use crate::queue::{held, send, settle, starts, until_queued};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, text, wait};

/// The first prompt.
const ASKED: &str = "Look at the matter.";

/// The fake agent's id.
fn fake() -> AgentId {
    AgentId::new(FAKE).unwrap()
}

/// A host whose agent answered one prompt and was stopped idle, so `stop_agents` holds it.
struct Stopped {
    dir: Dir,
    host: Running,
    /// Each launch's environment, as the recording launcher kept it.
    launches: Arc<Mutex<Vec<BTreeMap<OsString, OsString>>>>,
    matter: MatterId,
    thread: ThreadId,
    events: Events,
}

impl Stopped {
    fn new(test: &str) -> Self {
        let dir = Dir::new(test, &[(FAKE, Scenario::Local("stop-agents-hold-three"))]);
        let recording = Recording::new(&dir);
        let launches = Arc::clone(&recording.envs);
        let host = dir.start_launching(0, Arc::new(recording), |config| config);
        let (matter, thread, _) =
            matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
        let mut events = host.events(StreamId::Thread(thread));
        let (first, _) = host.prompt(thread, ASKED);
        assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
        host.settle();
        let stopped = wait("stop_agents", host.handle.stop_agents(fake())).unwrap();
        assert_eq!(stopped, AgentsStopped::Exited);
        Self {
            dir,
            host,
            launches,
            matter,
            thread,
            events,
        }
    }
}

/// Where a prompt goes: the host, its thread's events, the matter and the thread, and how many
/// turns failed before.
type At<'a> = (&'a Running, &'a mut Events, MatterId, ThreadId, usize);

/// Sends `prompt` with `delivery` while a hold applies: it queues, the queue shows held, and
/// nothing of it starts or fails.
fn send_held(at: At<'_>, prompt: &str, delivery: DeliveryRequest) -> TurnId {
    let (host, events, matter, thread, failed) = at;
    let (turn, _) = send(host, thread, text(prompt), delivery);
    until_queued(events, turn);
    settle(host, events);
    assert!(held(host, matter, thread), "{prompt}: {}", events.dump());
    assert_eq!(starts(events, turn), 0, "{prompt}: {}", events.dump());
    assert_eq!(failures(events), failed, "{prompt}: {}", events.dump());
    turn
}

/// How many `TurnFailed` were received.
fn failures(events: &Events) -> usize {
    events.count(|event| matches!(event, DomainEvent::TurnFailed(_)))
}

#[test]
fn a_second_prompt_queued_while_stop_agents_holds_keeps_the_queue_held() {
    let Stopped {
        dir: _dir,
        host,
        launches,
        matter,
        thread,
        mut events,
    } = Stopped::new("hold_appends_second");
    send_held(
        (&host, &mut events, matter, thread, 0),
        "One",
        DeliveryRequest::Queue,
    );
    send_held(
        (&host, &mut events, matter, thread, 0),
        "Two",
        DeliveryRequest::Queue,
    );
    assert_eq!(launches.lock().unwrap().len(), 1, "no launch while held");
    host.shutdown();
}
