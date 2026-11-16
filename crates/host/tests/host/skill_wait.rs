//! An agent launch while the profile's skills load in the background (`ProfileHook::launch_wait`,
//! desktop's stetdoc try after an install or an update): the launch's prep waits for the try in
//! flight, at most `LAUNCH_WAIT_LIMIT` (20 s) on the host clock, with the turn "starting"; a try
//! that answers in time brings the skill, one that does not leaves the launch with what it read
//! at its start. A launch with no try in flight does not wait. The try is the profile crate's
//! own `Tries`, which desktop holds; the test ends it by dropping its `InFlight`, so nothing
//! hangs on the wall clock.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_host::{LAUNCH_WAIT_LIMIT, LaunchSkills, LaunchWait, ProfileHook};
use clauseo_profile::skills::stetdoc::start::Tries;
use clauseo_profile::{ClauseoProfile, SkillWrite};
use clauseo_protocol::{
    AgentId, AgentKind, MatterId, StopReason, StreamId, ThreadId, ThreadStatus,
};
use serde_json::{Map, Value, json};

use crate::profile_rig::{CLAUDE, Wired, agent_lines, agents, settings, template};
use crate::support::{Dir, Running, wait};

/// A hook whose skills writes are what `writes` holds when asked, and whose launch wait is the
/// try in flight in `tries`, ending with the writes as they are then, as desktop's does.
#[derive(Clone, Default)]
struct Loading {
    writes: Arc<Mutex<Vec<SkillWrite>>>,
    tries: Tries,
}

impl ProfileHook for Loading {
    fn session_meta(
        &self,
        _agent: &AgentId,
        _packs: Option<&clauseo_profile::PacksWritten>,
    ) -> Option<Map<String, Value>> {
        None
    }

    fn launch_env(&self, _agent: &AgentId) -> BTreeMap<OsString, OsString> {
        BTreeMap::new()
    }

    fn launch_writes(&self, _agent: &AgentId) -> Vec<SkillWrite> {
        self.writes.lock().unwrap().clone()
    }

    fn launch_wait(&self, _agent: &AgentId) -> Option<LaunchWait> {
        let end = self.tries.wait()?;
        let writes = Arc::clone(&self.writes);
        Some(Box::pin(async move {
            end.await;
            LaunchSkills {
                generation: 0,
                writes: writes.lock().unwrap().clone(),
            }
        }))
    }
}

/// A Claude agent under `dir` whose host has `hook`, a matter and a thread with it.
fn claude_with(dir: &Dir, name: &str, hook: &Loading) -> (Running, MatterId, ThreadId) {
    let claude = template(
        dir,
        name,
        &agent_lines(name, "session/new", &json!({}), &[]),
    );
    settings(dir, agents(&[(CLAUDE, &claude, json!({}))]), json!({}));
    let hook = hook.clone();
    let host = dir.start_launching(0, Wired::new(dir), move |config| {
        config.with_profile_hook(Box::new(hook))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    (host, matter, thread)
}

/// The skills writes of a profile under `dir`'s data with stetdoc's skill, and its `SKILL.md`.
fn skill_writes(dir: &Dir) -> (Vec<SkillWrite>, PathBuf) {
    let profile: ClauseoProfile = crate::tools::profile(dir.data());
    let skill_md = dir
        .data()
        .join("profile")
        .join("claude-plugin")
        .join("skills")
        .join("stetdoc")
        .join("SKILL.md");
    (profile.launch_writes(Some(AgentKind::Claude)), skill_md)
}

/// Waits until the host has armed a timer of [`LAUNCH_WAIT_LIMIT`] (a launch began its wait).
fn until_wait_armed(host: &Running) {
    for _ in 0..256 {
        if wait("a timer armed", host.clock.next_armed()) == LAUNCH_WAIT_LIMIT {
            return;
        }
    }
    panic!("no launch waited for the skills");
}

/// A launch while a slow try is in flight waits, its turn "starting" and no skill written, and
/// goes on with the skill the try brought once the try ends.
#[test]
fn a_launch_during_a_slow_try_waits_and_gets_the_skill() {
    let dir = Dir::new("skill_wait_slow", &[]);
    let hook = Loading::default();
    let (writes, skill_md) = skill_writes(&dir);
    let held = hook.tries.begin();
    let (host, matter, thread) = claude_with(&dir, "skill-wait-slow", &hook);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Go.");
    until_wait_armed(&host);
    host.settle();
    events.drain_ready();
    assert!(events.turn_ended(turn).is_none(), "{}", events.dump());
    assert_eq!(host.status(matter, thread), ThreadStatus::Starting);
    assert!(!skill_md.exists(), "nothing is written while the try runs");

    *hook.writes.lock().unwrap() = writes;
    drop(held);
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    assert!(
        skill_md.is_file(),
        "the launch wrote the skill the try brought"
    );
}

/// A launch while a try is in flight that never ends in time goes on at the limit, 20 s from the
/// launch on the host clock, with the writes it read at its start (no skill), and not before.
#[test]
fn a_launch_during_a_try_that_times_out_goes_on_without_the_skill_at_20_s() {
    let dir = Dir::new("skill_wait_timeout", &[]);
    let hook = Loading::default();
    let (_, skill_md) = skill_writes(&dir);
    let held = hook.tries.begin();
    let (host, matter, thread) = claude_with(&dir, "skill-wait-timeout", &hook);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Go.");
    until_wait_armed(&host);
    host.clock
        .advance(LAUNCH_WAIT_LIMIT.saturating_sub(Duration::from_millis(1)));
    host.settle();
    events.drain_ready();
    assert!(events.turn_ended(turn).is_none(), "{}", events.dump());
    assert_eq!(host.status(matter, thread), ThreadStatus::Starting);

    host.clock.advance(Duration::from_millis(1));
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    assert!(!skill_md.exists(), "the launch went on without the skill");
    drop(held);
}

/// A launch with no try in flight does not wait: its turn ends with the host clock never moved,
/// and the skill it read is written.
#[test]
fn a_launch_with_no_try_in_flight_does_not_wait() {
    let dir = Dir::new("skill_wait_none", &[]);
    let hook = Loading::default();
    let (writes, skill_md) = skill_writes(&dir);
    *hook.writes.lock().unwrap() = writes;
    drop(hook.tries.begin());
    let (host, _, thread) = claude_with(&dir, "skill-wait-none", &hook);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Go.");
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    assert!(skill_md.is_file());
}
