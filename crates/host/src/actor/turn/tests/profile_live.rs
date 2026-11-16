//! A launch profile change reaching a live agent (`idle.rs`'s profile refresh) with a real
//! `fake_agent`. The mark has no grace of its own: a marked idle agent is not stopped a
//! millisecond before the idle limit, and is stopped at it. A Stop in the turn and a question to
//! the lawyer keep the agent until they end, and the next prompt relaunches it. A prompt soon
//! after a background command goes to the agent as it is; one past the window relaunches it.
//!
//! The test moves the hook's generation from 0 to 1 and hands the change to the actor as the
//! hook's wait posts it (`idle::profile_changed`). The relaunch takes `session/load` and answers
//! "Relaunched.", so a prompt the old agent answered shows in the log.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use clauseo_acp::PendingKey;
use clauseo_profile::PacksWritten;
use clauseo_protocol::{AgentId, ElicitationId, HostCommand, ThreadId};
use serde_json::{Map, Value, json};

use super::agents::send_prompt;
use super::idle::{MS, advance, idle_stops, live, stops_after_a_full_limit, until_idle};
use super::idle_busy::kept_past_the_limit;
use super::rig::Rig;
use super::tools_live::{one_answer, rig_adjusted, turns_ended};
use crate::ProfileHook;
use crate::actor::Actor;
use crate::actor::turn::agents::Agent;
use crate::actor::turn::idle::{Pending, Relaunch, profile_changed};
use crate::actor::turn::{TurnPhase, idle_tick};
use crate::config::CAP_MIN_IDLE;

/// The relaunch's branch: `session/load` of `sess-1`, then one prompt answered "Relaunched.".
fn relaunch() -> Value {
    json!([
        {"expect":{"method":"session/load","as":"load2","params":{"sessionId":"sess-1"}}},
        {"respond":{"to":"load2","result":{}}},
        {"expect":{"method":"session/prompt","as":"p-r","params":{"sessionId":"sess-1"}}},
        {"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-r",
            "content":{"type":"text","text":"Relaunched."}}}},
        {"respond":{"to":"p-r","result":{"stopReason":"end_turn"}}}
    ])
}

/// A profile whose generation the test moves on; it signals no change itself.
#[derive(Clone, Default)]
struct Stepped(Arc<AtomicU64>);

impl ProfileHook for Stepped {
    fn session_meta(&self, _: &AgentId, _: Option<&PacksWritten>) -> Option<Map<String, Value>> {
        None
    }

    fn launch_env(&self, _: &AgentId) -> BTreeMap<OsString, OsString> {
        BTreeMap::new()
    }

    fn generation(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// A rig with the idle stop at its limit and the [`Stepped`] hook, whose first launch plays
/// `steps`, the relaunch [`relaunch`].
fn rig(test: &str, steps: &[Value]) -> (Rig, Stepped) {
    let hook = Stepped::default();
    let given = hook.clone();
    let rig = rig_adjusted(test, steps, &relaunch(), true, move |config| {
        config.with_profile_hook(Box::new(given))
    });
    (rig, hook)
}

/// The new launch profile lands (generation 1), with the tick after that input.
fn profile_lands(rig: &mut Rig, hook: &Stepped) {
    hook.0.store(1, Ordering::SeqCst);
    profile_changed(&mut rig.actor, 1);
    idle_tick(&mut rig.actor);
}

/// `thread`'s live agent.
fn agent(rig: &mut Rig, thread: ThreadId) -> &mut Agent {
    let state = &mut rig.actor.threads.get_mut(&thread).unwrap().turn;
    state.agent.as_deref_mut().unwrap()
}

/// Whether `thread`'s agent is marked for the profile change.
fn marked(actor: &Actor, thread: ThreadId) -> bool {
    let agent = actor.threads[&thread].turn.agent.as_deref().unwrap();
    agent.idle.relaunch == Some(Relaunch::ProfileChanged)
}

/// Handles inputs until `thread`'s turn has ended and nothing is in flight, its agent there.
fn until_answered(rig: &mut Rig, thread: ThreadId) {
    rig.run_until("the turn's end", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none()
            && state.closing.is_none()
            && state.agent.is_some()
            && actor.work.commits == 0
    });
}

/// The next prompt, command `n`, relaunches the agent: its answer is the relaunch's.
fn relaunches(rig: &mut Rig, n: u128, thread: ThreadId) {
    let before = turns_ended(rig, thread);
    send_prompt(rig, n, thread);
    until_answered(rig, thread);
    assert_eq!(turns_ended(rig, thread), before + 1);
    let log = format!("{:?}", rig.log(thread));
    assert!(log.contains("Relaunched."), "not the relaunch's answer");
}

#[test]
fn a_marked_idle_agent_stops_at_the_idle_limit_not_before() {
    let (mut rig, hook) = rig("profile-live-idle", &one_answer());
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    profile_lands(&mut rig, &hook);
    assert!(marked(&rig.actor, thread));
    assert!(live(&rig.actor, thread), "stopped at the change");
    stops_after_a_full_limit(&mut rig, thread);
    relaunches(&mut rig, 11, thread);
    rig.finish();
}

#[test]
fn a_stop_in_the_turn_keeps_the_agent_and_the_next_prompt_relaunches_it() {
    let steps = vec![
        json!({"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}),
        json!({"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-1",
            "content":{"type":"text","text":"Working."}}}}),
        json!({"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}),
        json!({"respond":{"to":"p1","result":{"stopReason":"cancelled"}}}),
    ];
    let (mut rig, hook) = rig("profile-live-stop", &steps);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    rig.run_until("the prompt out", |actor| {
        actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|current| {
                current.phase == TurnPhase::Prompting && current.first_words.is_some()
            })
    });
    profile_lands(&mut rig, &hook);
    assert!(marked(&rig.actor, thread));
    // The lawyer presses Stop: the turn ends cancelled, the agent stays.
    rig.dispatch(11, HostCommand::Cancel { thread }).unwrap();
    until_answered(&mut rig, thread);
    assert!(live(&rig.actor, thread), "stopped with the turn");
    assert!(marked(&rig.actor, thread));
    assert_eq!(idle_stops(&mut rig, thread), 0);
    relaunches(&mut rig, 12, thread);
    rig.finish();
}

#[test]
fn a_question_to_the_lawyer_keeps_the_agent_until_it_is_answered() {
    let (mut rig, hook) = rig("profile-live-question", &one_answer());
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    // A form question waits for the lawyer, put in place by hand as `idle_checks.rs` does.
    let now = rig.actor.now().wall;
    let id: ElicitationId = rig.actor.ids.mint(now).unwrap();
    agent(&mut rig, thread)
        .elicitations
        .insert(id, PendingKey(8));
    idle_tick(&mut rig.actor);
    profile_lands(&mut rig, &hook);
    assert!(marked(&rig.actor, thread));
    kept_past_the_limit(&mut rig, thread, Pending::Ask);
    agent(&mut rig, thread).elicitations.clear();
    idle_tick(&mut rig.actor);
    relaunches(&mut rig, 11, thread);
    assert_eq!(idle_stops(&mut rig, thread), 0);
    rig.finish();
}

#[test]
fn a_prompt_soon_after_a_background_command_goes_to_the_agent_as_it_is() {
    let mut steps = one_answer();
    steps.extend([
        json!({"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-1"}}}),
        json!({"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-2",
            "content":{"type":"text","text":"Second."}}}}),
        json!({"respond":{"to":"p2","result":{"stopReason":"end_turn"}}}),
    ]);
    let (mut rig, hook) = rig("profile-live-background", &steps);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    // That turn closed after a steer during a tool call, as `queue/steer.rs` notes it: a
    // command Claude put in the background may still run.
    let now = rig.actor.now().mono;
    agent(&mut rig, thread).idle.steered_end = Some(now);
    profile_lands(&mut rig, &hook);
    assert!(marked(&rig.actor, thread));
    send_prompt(&mut rig, 11, thread);
    until_answered(&mut rig, thread);
    let log = format!("{:?}", rig.log(thread));
    assert!(log.contains("Second."), "not the first agent's answer");
    assert!(
        !log.contains("Relaunched."),
        "relaunched with the command running"
    );
    assert!(live(&rig.actor, thread));
    // Past the window the command has had its time: the next prompt relaunches the agent.
    advance(&mut rig, CAP_MIN_IDLE + MS);
    assert!(live(&rig.actor, thread), "stopped before the idle limit");
    relaunches(&mut rig, 12, thread);
    rig.finish();
}
