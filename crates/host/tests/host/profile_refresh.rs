//! The profile refresh (`turn/idle.rs`): when the launch profile changes
//! (`ProfileHook::changed`, desktop's stetdoc try putting a new profile in place), every live
//! agent launched on an older generation is marked to relaunch: the conversation's next
//! question relaunches it on the new profile, resuming its session, and with no question it
//! stops at the idle limit (`IDLE_LIMIT`), as any idle agent does, never mid-turn. The hook
//! here hands the generation to each launch in its environment, so a launch shows which
//! profile it got. Nothing hangs on the wall clock: the test changes the profile itself and
//! waits for the host to take the change (its next `changed` wait).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_host::{IDLE_LIMIT, ProfileChange, ProfileHook};
use clauseo_protocol::{
    AgentChoiceId, AgentId, DomainEvent, HostCommand, StopReason, StreamId, ThreadStatus,
};
use futures::channel::oneshot;
use serde_json::{Map, Value};

use crate::codex::Recording;
use crate::idle_stop::idle_stopped;
use crate::permissions::until_first_request;
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};

/// The variable each launch gets with the profile generation it launched on.
const PROFILE: &str = "CLAUSEO_TEST_PROFILE";

/// The step one millisecond makes.
const MS: Duration = Duration::from_millis(1);

/// A profile whose generation the test moves on ([`Refreshing::put`]). Each `changed` wait the
/// host asks for is told on `watched` with the generation it waits past.
#[derive(Clone)]
struct Refreshing(Arc<Mutex<Held>>);

struct Held {
    generation: u64,
    waits: Vec<oneshot::Sender<u64>>,
    watched: Sender<u64>,
}

impl Refreshing {
    fn new() -> (Self, Receiver<u64>) {
        let (watched, receiver) = mpsc::channel();
        let held = Held {
            generation: 0,
            waits: Vec::new(),
            watched,
        };
        (Self(Arc::new(Mutex::new(held))), receiver)
    }

    /// Puts the next generation in place and ends the host's wait for it.
    fn put(&self) {
        let mut held = self.0.lock().unwrap();
        held.generation += 1;
        let generation = held.generation;
        for wait in held.waits.drain(..) {
            let _gone = wait.send(generation);
        }
    }
}

impl ProfileHook for Refreshing {
    fn session_meta(
        &self,
        _agent: &AgentId,
        _packs: Option<&clauseo_profile::PacksWritten>,
    ) -> Option<Map<String, Value>> {
        None
    }

    fn launch_env(&self, _agent: &AgentId) -> BTreeMap<OsString, OsString> {
        let generation = self.0.lock().unwrap().generation;
        BTreeMap::from([(
            OsString::from(PROFILE),
            OsString::from(generation.to_string()),
        )])
    }

    fn generation(&self) -> u64 {
        self.0.lock().unwrap().generation
    }

    fn changed(&self, seen: u64) -> Option<ProfileChange> {
        let mut held = self.0.lock().unwrap();
        let _told = held.watched.send(seen);
        if held.generation > seen {
            let generation = held.generation;
            return Some(Box::pin(async move { generation }));
        }
        let (sender, receiver) = oneshot::channel();
        held.waits.push(sender);
        Some(Box::pin(async move {
            // The test holds the hook, so its senders outlive the host.
            receiver.await.unwrap_or(seen)
        }))
    }
}

/// A host whose fake agent plays `scenario`, with the [`Refreshing`] hook and the idle stop at
/// its limit; the launches'
/// environments; and the host's waits, after its start's (generation 0) was seen.
struct Rig {
    dir: Dir,
    host: Running,
    hook: Refreshing,
    watched: Receiver<u64>,
    launches: Arc<Mutex<Vec<BTreeMap<OsString, OsString>>>>,
}

fn rig(test: &str, scenario: &'static str) -> Rig {
    let dir = Dir::new(test, &[(FAKE, Scenario::Local(scenario))]);
    let recording = Recording::new(&dir);
    let launches = Arc::clone(&recording.envs);
    let (hook, watched) = Refreshing::new();
    let given = hook.clone();
    let host = dir.start_launching(0, Arc::new(recording), move |config| {
        config
            .with_profile_hook(Box::new(given))
            .with_idle_limit(IDLE_LIMIT)
    });
    let rig = Rig {
        dir,
        host,
        hook,
        watched,
        launches,
    };
    rig.until_watching(0);
    rig
}

impl Rig {
    /// Waits until the host waits for a change past `generation`: it has taken that one.
    fn until_watching(&self, generation: u64) {
        loop {
            let seen = self
                .watched
                .recv_timeout(Duration::from_secs(30))
                .expect("the host waits for the next profile change");
            if seen == generation {
                return;
            }
        }
    }

    /// The profile generation each launch got, in order.
    fn launched_on(&self) -> Vec<String> {
        self.launches
            .lock()
            .unwrap()
            .iter()
            .map(|env| env[&OsString::from(PROFILE)].to_string_lossy().into_owned())
            .collect()
    }
}

/// An idle agent on the old profile is not stopped a millisecond before the idle limit, and is
/// stopped at it, its thread `IdleStopped`; the next question relaunches it on the new profile
/// and resumes the session, which still knows the first turn.
#[test]
fn an_idle_agent_on_the_old_profile_stops_and_the_next_question_gets_the_new_one() {
    let rig = rig("profile_refresh_idle", "idle-stop-then-resume");
    let (dir, host) = (&rig.dir, &rig.host);
    let (matter, thread, _) = matter_with_thread(dir, host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Remember the word heron.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);

    rig.hook.put();
    rig.until_watching(1);
    host.settle();
    host.clock.advance(IDLE_LIMIT.checked_sub(MS).unwrap());
    host.settle();
    events.drain_ready();
    assert_eq!(
        events.count(idle_stopped),
        0,
        "before the limit: {}",
        events.dump()
    );
    host.clock.advance(MS);
    events.until("the idle stop", |events| {
        events.position(idle_stopped).is_some()
    });
    host.settle();
    assert_eq!(host.status(matter, thread), ThreadStatus::IdleStopped);

    let (second, _) = host.prompt(thread, "What was the word?");
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    assert!(
        events.turn_text(second).contains("heron"),
        "{}",
        events.dump()
    );
    assert_eq!(rig.launched_on(), ["0", "1"]);
    assert_eq!(events.count(idle_stopped), 1, "{}", events.dump());
}

/// A question after the new profile lands does not wait for a stop: the agent is relaunched for
/// it on the new profile, and the session still knows the first turn.
#[test]
fn a_question_after_the_change_relaunches_on_the_new_profile() {
    let rig = rig("profile_refresh_prompt", "idle-stop-then-resume");
    let (dir, host) = (&rig.dir, &rig.host);
    let (_, thread, _) = matter_with_thread(dir, host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Remember the word heron.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);

    rig.hook.put();
    rig.until_watching(1);
    host.settle();
    let (second, _) = host.prompt(thread, "What was the word?");
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    assert!(
        events.turn_text(second).contains("heron"),
        "{}",
        events.dump()
    );
    assert_eq!(rig.launched_on(), ["0", "1"]);
}

/// An agent mid-turn when the new profile lands is not interrupted: twice the idle limit passes
/// with its permission card open and the card stays; the turn ends `EndTurn` once the lawyer
/// answers, the agent stops at the idle limit after it, and the next question relaunches it on
/// the new profile.
#[test]
fn a_busy_agent_finishes_its_turn_and_stops_after_it() {
    let rig = rig("profile_refresh_busy", "profile-refresh-mid-turn");
    let (dir, host) = (&rig.dir, &rig.host);
    let (matter, thread, _) = matter_with_thread(dir, host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit clause 1.");
    until_first_request(&mut events);

    rig.hook.put();
    rig.until_watching(1);
    host.settle();
    host.clock.advance(IDLE_LIMIT * 2);
    host.settle();
    events.drain_ready();
    assert!(events.turn_ended(turn).is_none(), "{}", events.dump());
    assert_eq!(events.count(idle_stopped), 0, "mid-turn: {}", events.dump());
    assert_eq!(host.status(matter, thread), ThreadStatus::NeedsYou);
    assert_eq!(events.permission_requests().len(), 1, "{}", events.dump());

    let request = events.permission_requests()[0];
    host.ok(HostCommand::AnswerPermission {
        thread,
        request,
        option_id: AgentChoiceId::new("allow-once").unwrap(),
    });
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    host.settle();
    host.clock.advance(IDLE_LIMIT);
    events.until("the idle stop", |events| {
        events.position(idle_stopped).is_some()
    });
    let ended = events.position(|event| matches!(event, DomainEvent::TurnEnded(_)));
    assert!(ended < events.position(idle_stopped), "{}", events.dump());

    let (second, _) = host.prompt(thread, "Now clause 2.");
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    assert_eq!(rig.launched_on(), ["0", "1"]);
    assert_eq!(events.count(idle_stopped), 1, "{}", events.dump());
}

/// Two questions to the agent launched on `generation`'s profile: the second goes to that same
/// agent, and once the idle limit passes after it the agent stops, once, as any idle agent does.
fn two_questions_on_one_agent(rig: &Rig, generation: &str) {
    let (dir, host) = (&rig.dir, &rig.host);
    let (_, thread, _) = matter_with_thread(dir, host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Read the brief.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    host.settle();
    let (second, _) = host.prompt(thread, "And the second clause?");
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    assert!(
        events.turn_text(second).contains("Second."),
        "{}",
        events.dump()
    );
    assert_eq!(rig.launched_on(), [generation]);
    // The deadline's input: the stop at the idle limit, the only one.
    host.settle();
    host.clock.advance(IDLE_LIMIT);
    events.until("the idle stop", |events| {
        events.position(idle_stopped).is_some()
    });
    host.settle();
    events.drain_ready();
    assert_eq!(events.count(idle_stopped), 1, "{}", events.dump());
    assert_eq!(rig.launched_on(), [generation]);
}

/// An agent launched once the host knows the new profile is on it, and is left alone.
#[test]
fn an_agent_launched_on_the_new_profile_is_left_alone() {
    let rig = rig("profile_refresh_new", "profile-refresh-two-questions");
    rig.hook.put();
    rig.until_watching(1);
    two_questions_on_one_agent(&rig, "1");
}

/// With no profile change, an agent is left alone.
#[test]
fn no_profile_change_stops_nothing() {
    let rig = rig("profile_refresh_none", "profile-refresh-two-questions");
    two_questions_on_one_agent(&rig, "0");
}
