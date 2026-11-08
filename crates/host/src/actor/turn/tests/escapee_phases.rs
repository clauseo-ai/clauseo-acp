//! The Escapee exit at every phase of a turn (R17 PR-E review): before the agent's session
//! exists (the sandbox's first-act fail-closed), while the turn waits for its writer, its
//! before-turn checkpoint or its `TurnStarted` commit, while the host stops the agent, and
//! during the cancel fallback, the turn ends `LeftSafeWorkspace` with no `TurnFailed`; what
//! came while the agent stopped starts nothing; a turn that left is never queued again; and no
//! limit read turns it into a stop at the limit. Each test rewrites the fake agent's held exit
//! to the Escapee's code, as `escapee.rs` does.

use std::cell::Cell;

use clauseo_limits::LimitHit;
use clauseo_protocol::{
    AgentChoiceId, CommandEnvelope, DetachReason, DomainEvent, ErrorCode, HostCommand,
    LimitStopReason, StopReason, ThreadId, TurnId,
};

use super::agents::{send_prompt, until_settled};
use super::escapee::{
    ESCAPEE, EXIT_ON_PROMPT, ended_in, is_end, tier_b, until_waiting_with_a_held_escapee,
};
use super::rig::{Rig, command_id, kind};
use crate::actor::Actor;
use crate::actor::input::{Input, MatterTag, Request};
use crate::actor::turn::agents::{self, Stage};
use crate::actor::turn::close::TurnClose;
use crate::actor::turn::state::LimitMark;
use crate::actor::turn::{Current, End, Holder, TurnPhase, cancel, end, sign_in};
use crate::agent::{AgentInput, Ended};

/// The agent answers nothing after the handshake but its prompt's arrival, and exits when its
/// stdin closes.
const SILENT_PROMPT: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
"#;

/// The agent exits as soon as its session is made.
const EXIT_AFTER_SESSION: &str = r#"{"exit":{"code":1}}"#;

/// Rewrites the held agent's end to the Escapee exit and moves it first, so the actor reads it
/// before what was held with it.
fn escapee_first(rig: &mut Rig) {
    let at = rig
        .held
        .iter()
        .position(is_end)
        .expect("the agent's end held");
    let mut end = rig.held.remove(at);
    let Input::Agent {
        input: AgentInput::Ended(Ended::Exited(report) | Ended::Stopped { report, .. }),
        ..
    } = &mut end
    else {
        panic!("not an end: {end:?}");
    };
    let DetachReason::Exited { code, .. } = report else {
        panic!("not an exit: {report:?}");
    };
    *code = Some(ESCAPEE);
    rig.held.insert(0, end);
}

/// `thread`'s current turn's phase.
fn phase(actor: &Actor, thread: ThreadId) -> Option<TurnPhase> {
    let current = actor.threads[&thread].turn.current.as_deref();
    current.map(|current| current.phase)
}

/// Where the Escapee detach is logged.
#[derive(Debug, Clone, Copy)]
enum Detached {
    /// After `TurnEnded`, as an exit during the turn's steps is.
    AfterTheEnd,
    /// Before it: the cancel fallback's stop ends the turn once the agent is gone.
    BeforeTheEnd,
    /// Nowhere: the agent had no session.
    #[cfg(unix)]
    No,
}

/// Releases what is held, runs to the end, and checks `turn` ended `LeftSafeWorkspace` with no
/// `TurnFailed`, its Escapee detach where `detached` says, and no agent left.
fn assert_left(mut rig: Rig, thread: ThreadId, turn: TurnId, detached: Detached) {
    rig.release_held();
    until_settled(&mut rig, thread);
    let log = rig.log(thread);
    let kinds = || log.iter().map(kind).collect::<Vec<_>>();
    let (stop, failed, ended, detach) = ended_in(&log, turn);
    assert_eq!(stop, Some(StopReason::LeftSafeWorkspace), "{:?}", kinds());
    assert!(!failed, "{:?}", kinds());
    match detached {
        Detached::AfterTheEnd => assert!(detach.is_some() && ended < detach, "{:?}", kinds()),
        Detached::BeforeTheEnd => assert!(detach.is_some() && detach < ended, "{:?}", kinds()),
        #[cfg(unix)]
        Detached::No => assert_eq!(detach, None, "{:?}", kinds()),
    }
    let state = &rig.actor.threads[&thread].turn;
    assert!(state.agent.is_none() && state.preparing.is_none());
    rig.finish();
}

/// The first-act fail-closed: the runner exits before the agent answers `initialize`. The
/// launch's `Closed` answer leaves the turn to the exit report.
#[cfg(unix)]
#[test]
fn an_escapee_before_the_session_ends_the_turn_left_safe_workspace() {
    let mut rig = Rig::start_wrapped("escapee-at-launch", "", "exit 1");
    tier_b(&mut rig);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    let saw_end = Cell::new(false);
    rig.run_holding(
        "the launch's Closed answer, the agent's end held",
        |_, input| {
            let end = is_end(input);
            saw_end.set(saw_end.get() || end);
            end
        },
        |actor| {
            let agent = actor.threads[&thread].turn.agent.as_deref();
            saw_end.get() && agent.is_some_and(|agent| agent.stage == Stage::Stopping)
        },
    );
    // The launch failed, and the turn waits for the exit's own stop.
    assert_eq!(phase(&rig.actor, thread), Some(TurnPhase::Launching));
    escapee_first(&mut rig);
    assert_left(rig, thread, turn, Detached::No);
}

#[test]
fn an_escapee_while_queued_ends_the_turn_left_safe_workspace() {
    let mut rig = Rig::start("escapee-queued", EXIT_AFTER_SESSION);
    tier_b(&mut rig);
    let thread = rig.thread();
    let other = rig.thread_by(2);
    let matter = rig.matter();
    rig.actor.matters.get_mut(&matter).unwrap().turns.holder = Some(Holder::Thread(other));
    let turn = send_prompt(&mut rig, 10, thread);
    rig.run_holding(
        "the queued turn",
        |_, input| is_end(input),
        |actor| phase(actor, thread) == Some(TurnPhase::Queued) && actor.work.commits == 0,
    );
    if rig.held.is_empty() {
        rig.hold_next("the agent's end", |_, input| is_end(input));
    }
    escapee_first(&mut rig);
    rig.release_held();
    rig.settle_commits();
    end::release_writer(&mut rig.actor, matter, other);
    assert_left(rig, thread, turn, Detached::AfterTheEnd);
}

#[test]
fn an_escapee_during_the_before_turn_checkpoint_ends_the_turn_left_safe_workspace() {
    let mut rig = Rig::start("escapee-checkpointing", EXIT_AFTER_SESSION);
    tier_b(&mut rig);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    let (saw_end, saw_checkpoint) = (Cell::new(false), Cell::new(false));
    rig.run_holding(
        "the before-turn checkpoint's answer and the agent's end",
        |actor, input| {
            if is_end(input) {
                saw_end.set(true);
                return true;
            }
            let answer = matches!(
                input,
                Input::Matter {
                    tag: MatterTag::Turn(_),
                    ..
                }
            ) && phase(actor, thread) == Some(TurnPhase::Checkpointing);
            saw_checkpoint.set(saw_checkpoint.get() || answer);
            answer
        },
        |_| saw_end.get() && saw_checkpoint.get(),
    );
    escapee_first(&mut rig);
    assert_left(rig, thread, turn, Detached::AfterTheEnd);
}

#[test]
fn an_escapee_while_turn_started_commits_ends_the_turn_left_safe_workspace() {
    let mut rig = Rig::start("escapee-starting", EXIT_AFTER_SESSION);
    tier_b(&mut rig);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    let (saw_end, saw_started) = (Cell::new(false), Cell::new(false));
    rig.run_holding(
        "the TurnStarted commit and the agent's end",
        |actor, input| {
            if is_end(input) {
                saw_end.set(true);
                return true;
            }
            let started = matches!(input, Input::Committed { ticket, .. }
                if actor.pipeline.events(*ticket).iter()
                    .any(|e| matches!(e.event, DomainEvent::TurnStarted(_))));
            saw_started.set(saw_started.get() || started);
            started
        },
        |_| saw_end.get() && saw_started.get(),
    );
    assert_eq!(phase(&rig.actor, thread), Some(TurnPhase::Starting));
    escapee_first(&mut rig);
    assert_left(rig, thread, turn, Detached::AfterTheEnd);
}

/// Runs `rig`'s prompt until it is in flight with the silent agent.
fn until_prompting(rig: &mut Rig, thread: ThreadId) {
    rig.run_until("the prompt in flight", |actor| {
        let current = actor.threads[&thread].turn.current.as_deref();
        current.is_some_and(|current| current.live() && current.prompt_call.is_some())
            && actor.work.commits == 0
    });
}

/// The host stops the agent (a close, a sign-in hold) as it exits Escapee: the exit is the
/// turn's stop, not the host's close.
#[test]
fn an_escapee_while_the_host_stops_the_agent_ends_the_turn_left_safe_workspace() {
    let mut rig = Rig::start("escapee-host-stop", SILENT_PROMPT);
    tier_b(&mut rig);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    until_prompting(&mut rig, thread);
    agents::stop(&mut rig.actor, thread);
    let saw_end = Cell::new(false);
    rig.run_holding(
        "the agent's end while the turn ends",
        |actor, input| {
            if is_end(input) {
                saw_end.set(true);
                return true;
            }
            let answer = matches!(
                input,
                Input::Matter {
                    tag: MatterTag::Turn(_),
                    ..
                }
            );
            answer && phase(actor, thread) == Some(TurnPhase::Ending)
        },
        |actor| saw_end.get() && phase(actor, thread) == Some(TurnPhase::Ending),
    );
    escapee_first(&mut rig);
    assert_left(rig, thread, turn, Detached::AfterTheEnd);
}

/// The cancel fallback stops an agent that did not answer its cancel, and it exits Escapee.
#[test]
fn an_escapee_during_the_cancel_fallback_ends_the_turn_left_safe_workspace() {
    let mut rig = Rig::start("escapee-fallback", SILENT_PROMPT);
    tier_b(&mut rig);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    until_prompting(&mut rig, thread);
    let current = rig
        .actor
        .threads
        .get_mut(&thread)
        .unwrap()
        .turn
        .current
        .as_mut();
    current.unwrap().cancelled = true;
    cancel::on_cancel_timer(&mut rig.actor, thread, turn);
    assert_eq!(phase(&rig.actor, thread), Some(TurnPhase::FallbackStop));
    rig.hold_next("the agent's end", |_, input| is_end(input));
    escapee_first(&mut rig);
    assert_left(rig, thread, turn, Detached::BeforeTheEnd);
}

/// A mode change sent while the agent's pipes were closed waits for a new agent; on an
/// Escapee exit none starts, and the change fails.
#[test]
fn an_escapee_starts_no_agent_for_what_came_while_it_stopped() {
    let mut rig = Rig::start("escapee-after-stop", EXIT_ON_PROMPT);
    tier_b(&mut rig);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    until_waiting_with_a_held_escapee(&mut rig, thread);
    let (reply, mut answer) = futures::channel::oneshot::channel();
    let command = CommandEnvelope {
        command_id: command_id(11),
        command: HostCommand::SetMode {
            thread,
            mode_id: AgentChoiceId::new("plan").unwrap(),
        },
    };
    rig.actor.on_request(Request::Dispatch { command, reply });
    // The change waits for a new agent.
    rig.run_holding(
        "the change waiting for a new agent",
        |_, input| is_end(input),
        |actor| {
            let agent = actor.threads[&thread].turn.agent.as_deref();
            agent
                .is_some_and(|agent| agent.stage == Stage::Stopping && !agent.after_stop.is_empty())
        },
    );
    rig.release_held();
    until_settled(&mut rig, thread);
    let Ok(Some(Err(error))) = answer.try_recv() else {
        panic!("the change was not refused");
    };
    assert_eq!(error.code(), ErrorCode::AgentExited);
    let (stop, failed, _, _) = ended_in(&rig.log(thread), turn);
    assert_eq!(stop, Some(StopReason::LeftSafeWorkspace));
    assert!(!failed);
    rig.finish();
}

/// A turn that left the Safe workspace never goes back to the queue, even one the sign-in's
/// `stop_agents` stopped; any other stop of that turn does.
#[test]
fn a_turn_that_left_the_safe_workspace_is_never_requeued() {
    let mut rig = Rig::start("escapee-requeue", "");
    let now = rig.actor.now();
    let turn: TurnId = rig.actor.ids.mint(now.wall).unwrap();
    let message = rig.actor.ids.mint(now.wall).unwrap();
    let mut current = Current::new(turn, message, Vec::new(), now.mono);
    let left = TurnClose::new(turn, StopReason::LeftSafeWorkspace);
    assert!(sign_in::requeue_of(&mut current, Some(turn), &left).is_none());
    let cancelled = TurnClose::new(turn, StopReason::Cancelled);
    assert!(sign_in::requeue_of(&mut current, Some(turn), &cancelled).is_some());
    rig.finish();
}

/// The Codex fallback's limits read at the limit makes a failed turn a stop at the limit, but
/// never one that left the Safe workspace.
#[test]
fn a_limit_read_never_overwrites_left_safe_workspace() {
    let hit = || LimitHit {
        window: Some("five_hour".to_owned()),
        resets_at: None,
        reason: LimitStopReason::PlanWindow,
    };
    let mut left = End::left_safe_workspace(None);
    let mut limit = None;
    let stop = end::close_stop(&mut left, &mut limit, Some(hit()));
    assert_eq!(stop, StopReason::LeftSafeWorkspace);
    assert!(limit.is_none());
    let mut marked = End::left_safe_workspace(None);
    let mut limit = Some(LimitMark {
        window: None,
        resets_at: None,
        reason: LimitStopReason::PlanWindow,
    });
    let stop = end::close_stop(&mut marked, &mut limit, None);
    assert_eq!(stop, StopReason::LeftSafeWorkspace);
    let mut failed = End::failed(ErrorCode::AgentProtocolError, None, None);
    let mut limit = None;
    let stop = end::close_stop(&mut failed, &mut limit, Some(hit()));
    assert_eq!(stop, StopReason::AtLimit);
}
