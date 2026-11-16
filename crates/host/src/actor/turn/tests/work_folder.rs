//! The work folder is the conversation's (`launch_checks::folders::work_dir`): each relaunch of
//! its agent (a Tools edit, a sign-in, the idle stop or the live-agents cap and then a prompt,
//! an agent that died, a host restart) gets the same one, with what the agent wrote there, and
//! a run folder of its own; the host's fence (`asks::fence_check`) lets the agent reach its own
//! work folder and refuses another conversation's. A moved matter's relaunch is in
//! `relocate.rs`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_protocol::{AgentId, ThreadId};
use futures::channel::oneshot;

use super::agents::send_prompt;
use super::idle::{TWO_ANSWERS, idle_stops, rig, start_idle, until_ended, until_idle};
use super::rig::Rig;
use super::tools_live::{one_answer, relaunches_with_notes, rig_with, turn_on, with_notes};
use crate::actor::Actor;
use crate::actor::input::{Request, SignInRequest};
use crate::agent::data_fence::ShellKind;

/// The agent answers the prompt, then exits while idle, as one that died.
const EXIT_AFTER_ANSWER: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
{"exit":{"code":1}}
"#;

/// `thread`'s live launch: its run folder and its work folder.
pub(super) fn launch(actor: &Actor, thread: ThreadId) -> (PathBuf, PathBuf) {
    let run = actor.threads[&thread].turn.run.as_ref().expect("a launch");
    (run.root.clone(), run.work.clone())
}

/// Whether the host's fence refuses `thread`'s agent a request that names `path`.
fn refused(actor: &Actor, thread: ThreadId, path: &Path) -> bool {
    let paths = [path.to_owned()];
    super::super::asks::fence_check(actor, thread, &paths, None, ShellKind::Unknown)
        .is_some_and(|check| check.touches_data())
}

/// `thread`'s launch, with `draft.md` written in its work folder as its agent writes it, which
/// the fence lets through.
pub(super) fn wrote(actor: &Actor, thread: ThreadId) -> (PathBuf, PathBuf) {
    let launch = launch(actor, thread);
    let draft = launch.1.join("draft.md");
    crate::write::write_file(&draft, b"draft").unwrap();
    assert!(!refused(actor, thread, &draft), "the own work folder");
    launch
}

/// `thread`'s launch after `before`'s: a run folder of its own, the same work folder, under
/// `<data>/agent-state/<matter>/work/<thread>`, with the draft written there before; the fence
/// lets the agent read it.
pub(super) fn same_work(actor: &Actor, thread: ThreadId, before: &(PathBuf, PathBuf)) {
    let (root, work) = launch(actor, thread);
    assert_ne!(root, before.0, "a run folder per launch");
    assert_eq!(work, before.1, "the conversation's work folder");
    let state = actor.effective.locations().data_dir().join("agent-state");
    let matter = actor.threads[&thread].row.matter.to_string();
    let want = state.join(matter).join("work").join(thread.to_string());
    assert_eq!(work, want);
    let draft = work.join("draft.md");
    assert_eq!(std::fs::read(&draft).unwrap(), b"draft");
    assert!(
        !refused(actor, thread, &draft),
        "the own work folder after the relaunch"
    );
}

#[test]
fn the_idle_stop_then_a_prompt_keeps_the_work_folder() {
    let mut rig = rig("work-idle", TWO_ANSWERS, "exit");
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    let before = wrote(&rig.actor, thread);
    rig.clock.advance(crate::config::IDLE_LIMIT);
    until_ended(&mut rig, thread);
    assert_eq!(idle_stops(&mut rig, thread), 1);
    assert!(
        before.1.join("draft.md").exists(),
        "kept when the agent stops"
    );
    send_prompt(&mut rig, 11, thread);
    until_idle(&mut rig, thread);
    same_work(&rig.actor, thread, &before);
    rig.finish();
}

/// Three conversations of one matter under a cap of two: each has its own work folder, refused
/// to the others' agents; the one the cap stopped gets its own back at its next prompt.
#[test]
fn the_live_agents_cap_then_a_prompt_keeps_the_work_folder() {
    let mut rig = start_idle("work-cap", TWO_ANSWERS, "exit", |config| {
        config
            .with_idle_limit(Duration::MAX)
            .with_live_agents(2)
            .with_cap_min_idle(Duration::from_secs(5))
    });
    let [first, second, third] = [rig.thread_by(1), rig.thread_by(2), rig.thread_by(3)];
    send_prompt(&mut rig, 10, first);
    until_idle(&mut rig, first);
    let before = wrote(&rig.actor, first);
    rig.clock.advance(Duration::from_secs(10));
    send_prompt(&mut rig, 11, second);
    until_idle(&mut rig, second);
    let other = launch(&rig.actor, second).1;
    assert_ne!(other, before.1);
    assert!(refused(&rig.actor, first, &other.join("draft.md")));
    assert!(refused(&rig.actor, second, &before.1.join("draft.md")));
    rig.clock.advance(Duration::from_secs(10));
    send_prompt(&mut rig, 12, third);
    until_ended(&mut rig, first);
    until_idle(&mut rig, third);
    assert_eq!(idle_stops(&mut rig, first), 1, "the cap stopped the first");
    rig.clock.advance(Duration::from_secs(10));
    send_prompt(&mut rig, 13, first);
    until_idle(&mut rig, first);
    same_work(&rig.actor, first, &before);
    assert!(refused(&rig.actor, first, &other.join("draft.md")));
    rig.finish();
}

#[test]
fn a_tools_edit_relaunch_keeps_the_work_folder() {
    let mut rig = rig_with("work-tools", &one_answer(), &with_notes(), false);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    let before = wrote(&rig.actor, thread);
    turn_on(&mut rig, 11, "notes");
    relaunches_with_notes(&mut rig, 12, thread);
    same_work(&rig.actor, thread, &before);
    rig.finish();
}

/// `restart_agents` after a sign-in stops the idle agent; the next prompt launches a new one.
#[test]
fn a_sign_in_restart_keeps_the_work_folder() {
    let mut rig = Rig::start("work-sign-in", TWO_ANSWERS);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    let before = wrote(&rig.actor, thread);
    let (reply, _answer) = oneshot::channel();
    let agent = AgentId::new("fake-agent").unwrap();
    let restart = SignInRequest::Restart { agent, reply };
    rig.actor.on_request(Request::SignIn(restart));
    until_ended(&mut rig, thread);
    send_prompt(&mut rig, 11, thread);
    until_idle(&mut rig, thread);
    same_work(&rig.actor, thread, &before);
    rig.finish();
}

/// An agent that exits on its own: the next prompt launches a new one.
#[test]
fn an_agent_that_died_keeps_the_work_folder() {
    let mut rig = Rig::start("work-died", EXIT_AFTER_ANSWER);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    rig.run_until("the launch", |actor| {
        actor.threads[&thread].turn.run.is_some()
    });
    let before = wrote(&rig.actor, thread);
    until_ended(&mut rig, thread);
    send_prompt(&mut rig, 11, thread);
    rig.run_until("the relaunch", |actor| {
        actor.threads[&thread].turn.run.is_some()
    });
    same_work(&rig.actor, thread, &before);
    rig.finish();
}

/// A host restart: the run folder goes with the old host, the work folder stays.
#[test]
fn a_host_restart_keeps_the_work_folder() {
    let mut rig = Rig::start("work-restart", TWO_ANSWERS);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    let before = wrote(&rig.actor, thread);
    let mut rig = rig.restart(1);
    assert!(!before.0.exists(), "the run folder went at the shutdown");
    assert!(before.1.join("draft.md").exists(), "the work folder stays");
    send_prompt(&mut rig, 20, thread);
    until_idle(&mut rig, thread);
    same_work(&rig.actor, thread, &before);
    rig.finish();
}
