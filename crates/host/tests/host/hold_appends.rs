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
    AgentId, CommandOutcome, DeliveryRequest, DomainEvent, HostCommand, MatterId, QueueHoldReason,
    StopReason, StreamHeader, StreamId, ThreadId, TurnId, ViewPatchBody,
};

use crate::codex::Recording;
use crate::observe::Events;
use crate::queue::{held, hold_reason, send, settle, starts, until_queued};
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

/// Whether a fresh view of `thread` shows its header's `queue_held`.
fn view_held(host: &Running, thread: ThreadId) -> bool {
    let mut view = host.view(StreamId::Thread(thread), None);
    view.until("the view's reset", |view| view.patches().next().is_some());
    let Some(ViewPatchBody::Reset { view_window }) = view.patches().next().map(|patch| &patch.body)
    else {
        panic!("the view did not start with a reset: {}", view.dump());
    };
    matches!(&view_window.header, Some(StreamHeader::Thread(header)) if header.queue_held)
}

/// How many `TurnFailed` were received.
fn failures(events: &Events) -> usize {
    events.count(|event| matches!(event, DomainEvent::TurnFailed(_)))
}

/// Where `turn`'s `TurnStarted` is.
fn started_at(events: &Events, turn: TurnId) -> usize {
    events
        .position(
            |event| matches!(event, DomainEvent::TurnStarted(started) if started.turn == turn),
        )
        .unwrap_or_else(|| panic!("{turn} never started: {}", events.dump()))
}

/// `restart_agents` sends `turns` once each, in order, and a second call sends nothing.
fn restart_sends_once(host: &Running, events: &mut Events, turns: &[TurnId]) {
    wait("restart_agents", host.handle.restart_agents(fake())).unwrap();
    for turn in turns {
        assert_eq!(events.until_turn_ended(*turn).stop, StopReason::EndTurn);
    }
    settle(host, events);
    wait("restart_agents", host.handle.restart_agents(fake())).unwrap();
    settle(host, events);
    for pair in turns.windows(2) {
        assert!(
            started_at(events, pair[0]) < started_at(events, pair[1]),
            "{}",
            events.dump()
        );
    }
    for turn in turns {
        assert_eq!(starts(events, *turn), 1, "{}", events.dump());
    }
}

#[test]
fn a_send_now_behind_a_prompt_queued_while_stop_agents_holds_queues_and_does_not_fail() {
    let Stopped {
        dir: _dir,
        host,
        launches,
        matter,
        thread,
        mut events,
    } = Stopped::new("hold_appends_send_now");
    send_held(
        (&host, &mut events, matter, thread, 0),
        "One",
        DeliveryRequest::Queue,
    );
    send_held(
        (&host, &mut events, matter, thread, 0),
        "Two",
        DeliveryRequest::SendNow,
    );
    let ended = events.count(|event| matches!(event, DomainEvent::TurnEnded(_)));
    assert_eq!(ended, 1, "only the first turn ended: {}", events.dump());
    assert_eq!(launches.lock().unwrap().len(), 1, "no launch while held");
    host.shutdown();
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

#[test]
fn three_prompts_sent_while_stop_agents_holds_run_in_order_after_one_restart() {
    let Stopped {
        dir: _dir,
        host,
        launches,
        matter,
        thread,
        mut events,
    } = Stopped::new("hold_appends_stop_three");
    let one = send_held(
        (&host, &mut events, matter, thread, 0),
        "One",
        DeliveryRequest::Queue,
    );
    let two = send_held(
        (&host, &mut events, matter, thread, 0),
        "Two",
        DeliveryRequest::SendNow,
    );
    let three = send_held(
        (&host, &mut events, matter, thread, 0),
        "Three",
        DeliveryRequest::Queue,
    );
    // The stop's hold, with the sign-in's mark on the queue, shows the login switching.
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::Switching)
    );
    restart_sends_once(&host, &mut events, &[one, two, three]);
    assert!(!held(&host, matter, thread));
    assert_eq!(hold_reason(&host, matter, thread), None);
    assert_eq!(failures(&events), 0, "{}", events.dump());
    let started = events.count(|event| matches!(event, DomainEvent::TurnStarted(_)));
    assert_eq!(started, 4, "{}", events.dump());
    assert_eq!(launches.lock().unwrap().len(), 2, "one relaunch");
    host.shutdown();
}

#[test]
fn prompts_sent_while_a_turn_is_held_for_the_sign_in_run_after_it_after_one_restart() {
    let dir = Dir::new(
        "hold_appends_sign_in",
        &[(FAKE, Scenario::Local("sign-in-hold-three"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, ASKED);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Failed);
    let head = |event: &DomainEvent| matches!(event, DomainEvent::TurnQueued(queued) if queued.is_at_head());
    events.until("the turn put back", |events| {
        events.position(head).is_some()
    });
    let again = events
        .events()
        .find_map(|event| match event {
            DomainEvent::TurnQueued(queued) if queued.is_at_head() => Some(queued.turn()),
            _ => None,
        })
        .unwrap();
    settle(&host, &mut events);
    assert!(held(&host, matter, thread), "{}", events.dump());
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::SignIn)
    );
    assert_eq!(failures(&events), 1);

    // Two more prompts, one sent now: both queue behind the held turn, which stays held.
    let two = send_held(
        (&host, &mut events, matter, thread, 1),
        "Two",
        DeliveryRequest::SendNow,
    );
    let three = send_held(
        (&host, &mut events, matter, thread, 1),
        "Three",
        DeliveryRequest::Queue,
    );
    assert_eq!(starts(&events, again), 0, "{}", events.dump());
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::SignIn)
    );

    restart_sends_once(&host, &mut events, &[again, two, three]);
    assert!(!held(&host, matter, thread));
    assert_eq!(hold_reason(&host, matter, thread), None);
    assert_eq!(failures(&events), 1, "{}", events.dump());
    host.shutdown();
}

/// `ResumeQueue` or `SendQueuedNow` while `stop_agents` holds the agent changes nothing the lawyer
/// can see: the view and `ListThreads` still show the queue held, and nothing starts or fails.
/// `restart_agents` then runs each turn once, in the order they were sent.
fn command_changes_nothing_while_held(
    dir_test: &str,
    command: impl Fn(ThreadId, TurnId) -> HostCommand,
    pick: usize,
) {
    let Stopped {
        dir: _dir,
        host,
        launches,
        matter,
        thread,
        mut events,
    } = Stopped::new(dir_test);
    let mut turns = Vec::new();
    for prompt in ["One", "Two", "Three"] {
        turns.push(send_held(
            (&host, &mut events, matter, thread, 0),
            prompt,
            DeliveryRequest::Queue,
        ));
    }
    let target = turns[pick];
    assert_eq!(host.ok(command(thread, target)), CommandOutcome::Done);
    settle(&host, &mut events);
    assert!(view_held(&host, thread), "the view: {}", events.dump());
    assert!(held(&host, matter, thread), "{}", events.dump());
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::Switching)
    );
    for turn in &turns {
        assert_eq!(starts(&events, *turn), 0, "{}", events.dump());
    }
    assert_eq!(failures(&events), 0, "{}", events.dump());
    assert_eq!(launches.lock().unwrap().len(), 1, "no launch while held");

    restart_sends_once(&host, &mut events, &turns);
    assert!(!held(&host, matter, thread), "{}", events.dump());
    assert!(!view_held(&host, thread), "{}", events.dump());
    assert_eq!(failures(&events), 0, "{}", events.dump());
    assert_eq!(launches.lock().unwrap().len(), 2, "one relaunch");
    host.shutdown();
}

#[test]
fn resume_queue_while_stop_agents_holds_changes_nothing_until_one_restart() {
    command_changes_nothing_while_held(
        "hold_appends_resume",
        |thread, _| HostCommand::ResumeQueue { thread },
        0,
    );
}

#[test]
fn send_queued_now_while_stop_agents_holds_changes_nothing_until_one_restart() {
    command_changes_nothing_while_held(
        "hold_appends_send_queued_now",
        |thread, turn| HostCommand::SendQueuedNow { thread, turn },
        2,
    );
}
